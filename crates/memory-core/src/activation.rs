//! Spreading activation with time decay.
//!
//! 1. The seeds (lexical matches or, in the future, embeddings) get an initial
//!    activation; the root gets a small baseline (identity is always present).
//! 2. On each pulse, only the nodes whose activation **rose** by at least
//!    `firing_threshold` fire, and they only pass on that increase: cycles are not
//!    counted twice. The pulse is split by `1/sqrt(degree)` (fan effect) so hubs (e.g.
//!    the "user" entity) do not flood the context.
//! 3. `Contradicts` edges subtract activation (inhibition) and do not propagate.
//! 4. The final score mixes activation, importance, rehearsal and, only for episodic
//!    memories, an exponential forgetting curve with a floor.
//! 5. The selection reserves slots for the personality and fills the rest with knowledge.
//!
//! Everything happens at a story time (`ActivationParams::story`): nodes that do not
//! exist yet are not seeded nor reached, and only edges valid at that time spread.

use std::collections::HashMap;

use crate::graph::MemoryGraph;
use crate::model::{NodeId, NodeKind, STORY_PRESENT, StoryTime, Validity};

#[derive(Debug, Clone)]
pub struct ActivationParams {
    /// Maximum hops from the seeds.
    pub max_depth: u8,
    /// Attenuation per hop.
    pub hop_decay: f32,
    /// Minimum increase for a node to fire on the next pulse.
    pub firing_threshold: f32,
    /// Multiplier when spreading against the edge's direction.
    pub reverse_factor: f32,
    /// Baseline activation of the personality root.
    pub root_baseline: f32,
    /// Half-life of a memory that is not rehearsed, in seconds.
    pub recency_half_life_secs: i64,
    /// An old memory never weighs less than this fraction.
    pub recency_floor: f32,
    /// Maximum nodes returned (not counting the root).
    pub top_n: usize,
    /// Slots reserved for personality nodes within `top_n`.
    pub personality_slots: usize,
    /// Slots reserved for rules (lore) in a world or chat graph.
    pub world_core_slots: usize,
    /// Story time of the recall (a chat's moment). `STORY_PRESENT` sees everything.
    pub story: StoryTime,
}

impl ActivationParams {
    /// The same parameters at another story time.
    pub fn at(&self, story: StoryTime) -> Self {
        Self { story, ..self.clone() }
    }
}

impl Default for ActivationParams {
    fn default() -> Self {
        Self {
            max_depth: 3,
            hop_decay: 0.6,
            firing_threshold: 0.02,
            reverse_factor: 0.5,
            root_baseline: 0.3,
            recency_half_life_secs: 7 * 24 * 3600,
            recency_floor: 0.25,
            top_n: 16,
            personality_slots: 4,
            world_core_slots: 2,
            story: STORY_PRESENT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Activated {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Raw activation after spreading.
    pub activation: f32,
    /// Final score used for sorting.
    pub score: f32,
}

/// Forgetting curve: `floor + (1-floor)·0.5^(Δt/half_life)`.
///
/// Importance slows forgetting ("flashbulb" memories): the half-life is multiplied by
/// `1 + 9·w²` and the floor rises up to half of the remaining margin with `w = 1`.
pub fn recency(last_accessed: i64, now: i64, importance: f32, p: &ActivationParams) -> f32 {
    let w = importance.clamp(0.0, 1.0);
    let dt = (now - last_accessed).max(0) as f32;
    let half = p.recency_half_life_secs.max(1) as f32 * (1.0 + 9.0 * w * w);
    let floor = p.recency_floor + (1.0 - p.recency_floor) * 0.5 * w * w;
    floor + (1.0 - floor) * 0.5f32.powf(dt / half)
}

impl MemoryGraph {
    /// Spreads activation from `seeds` and returns the node → activation map.
    pub fn spread(&self, seeds: &[(NodeId, f32)], p: &ActivationParams) -> HashMap<NodeId, f32> {
        let mut act: HashMap<NodeId, f32> = HashMap::new();
        let mut frontier: Vec<(NodeId, f32)> = Vec::new();

        let root = self.root();
        let mut push_seed = |id: NodeId, a: f32, act: &mut HashMap<NodeId, f32>| {
            let e = act.entry(id).or_default();
            let new = (*e + a).min(1.0);
            let delta = new - *e;
            *e = new;
            if delta > 0.0 {
                frontier.push((id, delta));
            }
        };
        // A library's root is only a container: lighting it would wake every definition.
        if self.contains(root) && p.root_baseline > 0.0 && self.scope() != crate::model::Scope::Library {
            push_seed(root, p.root_baseline, &mut act);
        }
        for &(id, a) in seeds {
            if self.exists_at(id, p.story) && a > 0.0 {
                push_seed(id, a.min(1.0), &mut act);
            }
        }

        let library = self.scope() == crate::model::Scope::Library;
        for _ in 0..p.max_depth {
            let mut pulses: HashMap<NodeId, f32> = HashMap::new();
            for &(u, delta) in &frontier {
                if delta < p.firing_threshold {
                    continue;
                }
                let fan = 1.0 / (self.degree(u).max(1) as f32).sqrt();
                for (v, edge, outgoing) in self.neighbors(u) {
                    if edge.validity_at(p.story) != Validity::Active || !self.exists_at(v, p.story) {
                        continue;
                    }
                    // In a library the root only holds the definitions together: passing
                    // through it would bring up every one of them.
                    if library && v == root {
                        continue;
                    }
                    let dir = if outgoing { 1.0 } else { p.reverse_factor };
                    *pulses.entry(v).or_default() += delta * edge.weight * edge.kind.sign() * p.hop_decay * fan * dir;
                }
            }
            frontier.clear();
            for (v, pulse) in pulses {
                let e = act.entry(v).or_default();
                let new = (*e + pulse).clamp(-1.0, 1.0);
                let delta = new - *e;
                *e = new;
                // Only positive increases keep spreading.
                if delta >= p.firing_threshold {
                    frontier.push((v, delta));
                }
            }
            if frontier.is_empty() {
                break;
            }
        }
        act
    }

    /// Scores and selects the most relevant nodes (excluding the root, which always goes into the context).
    pub fn rank(&self, act: &HashMap<NodeId, f32>, now: i64, p: &ActivationParams) -> Vec<Activated> {
        let root = self.root();
        let mut all: Vec<Activated> = act
            .iter()
            .filter(|(id, a)| **id != root && **a > 0.0)
            .filter_map(|(&id, &a)| {
                let n = self.node(id)?;
                let importance = 0.5 + 0.5 * n.weight;
                let rehearsal = 1.0 + 0.1 * (1.0 + n.access_count as f32).ln();
                // Encoding specificity: a direct cue (high activation) brings back an old
                // memory almost as if it were recent; weak spreading does not.
                let rec = if n.kind.decays() {
                    let r = recency(n.last_accessed, now, n.weight, p);
                    let cue = a.clamp(0.0, 1.0);
                    r + (1.0 - r) * 0.8 * cue * cue
                } else {
                    1.0
                };
                Some(Activated { id, kind: n.kind, activation: a, score: a * importance * rehearsal * rec })
            })
            .collect();
        all.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));

        let (mut persona, knowledge): (Vec<_>, Vec<_>) = all.into_iter().partition(|a| a.kind.is_core());
        // A library reserves nothing: only the definitions the turn touched come up.
        let slots = match self.scope() {
            crate::model::Scope::Character => p.personality_slots,
            crate::model::Scope::Library => 0,
            _ => p.world_core_slots,
        };

        // If the query did not touch the personality, the slots are filled with the
        // root's strongest direct branches (the character never "loses" its character,
        // nor the world its rules).
        if persona.len() < slots {
            let mut fallback: Vec<Activated> = self
                .neighbors(root)
                .filter(|(v, e, out)| {
                    *out && e.validity_at(p.story) == Validity::Active && self.exists_at(*v, p.story) && !persona.iter().any(|a| a.id == *v)
                })
                .filter_map(|(v, e, _)| {
                    let n = self.node(v)?;
                    n.kind.is_core().then_some(Activated { id: v, kind: n.kind, activation: 0.0, score: 0.01 * n.weight * e.weight })
                })
                .collect();
            fallback.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
            persona.extend(fallback.into_iter().take(slots - persona.len()));
        }

        let take_persona = persona.len().min(slots).min(p.top_n);
        let mut out: Vec<Activated> = persona.drain(..take_persona).collect();
        // The rest (knowledge + leftover personality) competes for the free slots.
        let mut rest: Vec<Activated> = knowledge.into_iter().chain(persona).collect();
        rest.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        out.extend(rest.into_iter().take(p.top_n - out.len()));
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        out
    }

    /// Shortcut: lexical seeds of the text → spreading → ranking.
    pub fn retrieve(&self, query: &str, now: i64, p: &ActivationParams) -> Vec<Activated> {
        let seeds = self.seeds(query);
        let act = self.spread(&seeds, p);
        self.rank(&act, now, p)
    }

    /// Hebbian learning: strengthens the existing edges between nodes recalled together.
    /// It does not create new edges (the curator decides that).
    pub fn hebbian(&mut self, active: &[Activated], rate: f32, now: i64) {
        for (i, a) in active.iter().enumerate() {
            for b in &active[i + 1..] {
                let delta = rate * a.score.min(1.0) * b.score.min(1.0);
                if delta > 0.0 {
                    let _ = self.reinforce(a.id, b.id, delta, now);
                }
            }
        }
    }
}
