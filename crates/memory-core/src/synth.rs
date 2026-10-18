//! Deterministic generator of synthetic memories for stress tests and benchmarks.
//!
//! It produces a character graph shaped like a real one: a personality core, entities
//! acting as hubs (a few very connected, many almost loose), places, episodic memories
//! spread over time with varied importance and valence, and facts. The text is built
//! from an English vocabulary so the lexical index works with realistic terms (and
//! collisions).

use crate::graph::MemoryGraph;
use crate::model::{EdgeKind, GraphMeta, MemoryNode, NodeId, NodeKind, Scope};

/// xorshift64* PRNG: no dependencies and reproducible.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
    /// Index with an approximate Zipf bias: the first ones come up far more often.
    pub fn zipf(&mut self, n: usize) -> usize {
        let u = self.unit().max(1e-6);
        ((n as f32).powf(u) - 1.0).floor().clamp(0.0, (n - 1) as f32) as usize
    }
}

const SYLLABLES: &[&str] = &[
    "ka", "ri", "mo", "sel", "an", "to", "vi", "lu", "dor", "el", "na", "ser", "gal", "ba", "qui", "ron", "te", "mi", "zar", "os",
];
const OBJECTS: &[&str] = &[
    "a burnt book", "a letter", "an old map", "a broken sword", "a medicine flask", "a pocket watch", "a supply crate",
    "a ring", "a scarf", "a medical report", "a radio", "some seeds", "a notebook", "a compass", "an Arts catalyst",
    "a photograph", "a key", "an Originium sample",
];
const VERBS: &[&str] = &["found", "lost", "gave away", "hid", "repaired", "broke", "returned", "studied", "sold", "stole", "looked after", "forgot"];
const PLACES: &[&str] = &[
    "the hangar", "the infirmary", "the canteen", "the bridge", "the garden", "the slums", "the market", "the station",
    "the wasteland", "the library", "the laboratory", "the mines", "the wall", "the harbour",
];
const MOODS: &[&str] = &[
    "in the rain", "before dawn", "in silence", "laughing", "in a hurry", "after an argument", "during the evacuation",
    "in the middle of a storm", "without a word", "with trembling hands",
];
const TRAITS: &[(&str, NodeKind)] = &[
    ("Patient", NodeKind::Trait),
    ("Distrustful", NodeKind::Trait),
    ("Curious", NodeKind::Trait),
    ("Loyal", NodeKind::Trait),
    ("Ironic", NodeKind::Trait),
    ("Brave", NodeKind::Trait),
    ("Melancholy", NodeKind::Emotion),
    ("Held-back anger", NodeKind::Emotion),
    ("Hope", NodeKind::Emotion),
    ("Fear of forgetting", NodeKind::Emotion),
    ("Justice", NodeKind::Value),
    ("Protect their own", NodeKind::Goal),
];

/// Invented proper name ("Kariselmo", "Vilugal"…).
pub fn name(rng: &mut Rng) -> String {
    let n = 2 + rng.below(3);
    let mut s: String = (0..n).map(|_| *rng.pick(SYLLABLES)).collect();
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}

#[derive(Debug, Clone)]
pub struct SynthSpec {
    /// Approximate total nodes (not counting the root).
    pub nodes: usize,
    /// Fraction of entities/people (the rest: memories, facts and places).
    pub entity_ratio: f32,
    pub seed: u64,
    /// "Now" (memories are spread up to 10 years back).
    pub now: i64,
}

impl Default for SynthSpec {
    fn default() -> Self {
        Self { nodes: 1000, entity_ratio: 0.08, seed: 42, now: 1_790_000_000 }
    }
}

/// Result: the graph and the ids of its entities (to plant needles and run queries).
pub struct Synth {
    pub graph: MemoryGraph,
    pub entities: Vec<(NodeId, String)>,
    pub places: Vec<NodeId>,
}

pub fn synth_character(spec: &SynthSpec) -> Synth {
    let mut rng = Rng::new(spec.seed);
    let now = spec.now;
    let mut meta = GraphMeta::new(Scope::Character, "Synthetic", now);
    meta.scenario = "Character generated for stress tests.".into();
    let mut g = MemoryGraph::new(meta, "A survivor with too many memories.", now - 3650 * 86_400);
    let root = g.root();

    let mut core = Vec::new();
    for (label, kind) in TRAITS {
        let id = g
            .add_trait(root, *kind, label, &format!("The synthetic character's trait {label}."), 0.3 + 0.7 * rng.unit(), now)
            .expect("trait");
        core.push(id);
    }

    let places: Vec<NodeId> = PLACES
        .iter()
        .map(|p| g.add_node(MemoryNode::new(NodeKind::Place, p.trim_start_matches("the "), format!("Place: {p}."), now)).expect("place"))
        .collect();

    let budget = spec.nodes.saturating_sub(core.len() + places.len());
    let n_entities = ((budget as f32 * spec.entity_ratio) as usize).max(4);
    let mut entities = Vec::with_capacity(n_entities);
    for _ in 0..n_entities {
        let n = name(&mut rng);
        let kind = if rng.unit() < 0.15 { NodeKind::Character } else { NodeKind::Entity };
        let id = g
            .add_node(MemoryNode::new(kind, &n, format!("{n}, someone from {}.", rng.pick(PLACES)), now).with_weight(rng.unit()).with_valence(rng.unit() * 2.0 - 1.0))
            .expect("entity");
        if rng.unit() < 0.3 {
            g.connect(root, id, EdgeKind::Knows, 0.3 + 0.5 * rng.unit(), now).expect("knows");
        }
        entities.push((id, n));
    }

    let remaining = budget.saturating_sub(n_entities);
    let mut previous: Option<NodeId> = None;
    for i in 0..remaining {
        let when = now - (rng.unit().powi(2) * 3650.0 * 86_400.0) as i64;
        if rng.unit() < 0.15 {
            // Stable fact about an entity.
            let (e, en) = &entities[rng.zipf(entities.len())];
            let text = format!("{en} {} {} in {}.", rng.pick(VERBS), rng.pick(OBJECTS), rng.pick(PLACES));
            let id = g.add_node(MemoryNode::new(NodeKind::SemanticFact, format!("Fact {i}"), text, when).with_weight(0.2 + 0.6 * rng.unit())).expect("fact");
            g.connect(id, *e, EdgeKind::Involves, 0.5, when).expect("involves");
            continue;
        }
        let who: Vec<usize> = (0..1 + rng.below(3)).map(|_| rng.zipf(entities.len())).collect();
        let names: Vec<&str> = who.iter().map(|&k| entities[k].1.as_str()).collect();
        let place = rng.below(places.len());
        let text = format!("{} {} {} in {} {}.", names.join(" and "), rng.pick(VERBS), rng.pick(OBJECTS), PLACES[place], rng.pick(MOODS));
        let mut node = MemoryNode::new(NodeKind::EpisodicMemory, format!("Memory {i}"), text, when)
            .with_weight(rng.unit().powi(2))
            .with_valence(rng.unit() * 2.0 - 1.0);
        node.last_accessed = when + (rng.unit() * (now - when) as f32) as i64;
        node.access_count = rng.below(5) as u32;
        let mut involves: Vec<NodeId> = who.iter().map(|&k| entities[k].0).collect();
        involves.sort_unstable();
        involves.dedup();
        involves.push(places[place]);
        let evokes: Vec<NodeId> = if rng.unit() < 0.3 { vec![*rng.pick(&core)] } else { vec![] };
        let id = g.add_memory(node, &involves, &evokes, when).expect("memory");
        if let Some(p) = previous.filter(|_| rng.unit() < 0.2) {
            g.connect(p, id, EdgeKind::Precedes, 0.25, when).expect("precedes");
        }
        previous = Some(id);
    }
    Synth { graph: g, entities, places }
}
