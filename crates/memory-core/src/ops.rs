//! CRUD operations emitted by the curator model (JSON forced by a GBNF grammar) and
//! their validated application to the graph.
//!
//! The curator refers to existing nodes by their number (`12`) and to nodes created in
//! the same batch by a temporary reference (`"$a"`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::{MemoryGraph, NodePatch};
use crate::model::{EdgeKind, ExternalRef, MemoryNode, NodeId, NodeKind, STORY_PRESENT, StoryTime, written_at};
use crate::text::fold;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeRef {
    Id(NodeId),
    New(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GraphOp {
    CreateNode {
        #[serde(rename = "ref")]
        reference: String,
        kind: NodeKind,
        label: String,
        content: String,
        weight: f32,
        valence: f32,
    },
    UpdateNode {
        target: NodeRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight_delta: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        valence_delta: Option<f32>,
    },
    Link {
        source: NodeRef,
        target: NodeRef,
        kind: EdgeKind,
        weight: f32,
    },
    Reinforce {
        source: NodeRef,
        target: NodeRef,
        delta: f32,
    },
    Forget {
        target: NodeRef,
    },
    /// A link that stopped being true ("they are no longer friends"): it is not deleted,
    /// it becomes the past from the current moment of the story.
    CloseEdge {
        source: NodeRef,
        target: NodeRef,
    },
}

/// The curator's full output: `{"ops": [...]}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpBatch {
    pub ops: Vec<GraphOp>,
}

impl OpBatch {
    /// Lenient parsing: accepts the `{"ops": [...]}` object even when it is surrounded by
    /// text (useful if the backend did not apply the grammar).
    pub fn parse_lenient(raw: &str) -> Result<Self, serde_json::Error> {
        let trimmed = raw.trim();
        match serde_json::from_str(trimmed) {
            Ok(b) => Ok(b),
            Err(e) => {
                let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) else { return Err(e) };
                if start >= end {
                    return Err(e);
                }
                serde_json::from_str(&trimmed[start..=end])
            }
        }
    }
}

/// Guardrails against a runaway curator.
#[derive(Debug, Clone)]
pub struct ApplyLimits {
    pub max_ops: usize,
    pub max_new_nodes: usize,
    /// Maximum weight/valence change per operation.
    pub max_step: f32,
    /// Maximum change per turn on personality traits (personality moves slowly).
    pub max_personality_step: f32,
    /// Kinds that `forget` may physically delete; the rest only lose weight.
    pub deletable: &'static [NodeKind],
}

impl Default for ApplyLimits {
    fn default() -> Self {
        Self {
            max_ops: 16,
            max_new_nodes: 6,
            max_step: 0.3,
            max_personality_step: 0.1,
            deletable: &[NodeKind::EpisodicMemory, NodeKind::SemanticFact, NodeKind::Topic],
        }
    }
}

#[derive(Clone, Copy)]
struct ApplyCtx<'a> {
    limits: &'a ApplyLimits,
    roster: &'a Roster,
    now: i64,
    /// Story time of the turn.
    story: StoryTime,
}

/// Known characters of the world: normalised name (`text::fold`) → their graph.
/// It lets a `create_node` of kind `character` end up linked to the right `.mem`.
pub type Roster = HashMap<String, ExternalRef>;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ApplyReport {
    /// Temporary reference → final id (includes merges with existing nodes).
    pub created: Vec<(String, NodeId)>,
    /// Nodes touched (to refresh the frontend's view).
    pub touched: Vec<NodeId>,
    pub applied: usize,
    pub rejected: Vec<String>,
}

impl MemoryGraph {
    pub fn apply_ops(&mut self, batch: &OpBatch, limits: &ApplyLimits, now: i64) -> ApplyReport {
        self.apply_ops_with(batch, limits, &Roster::new(), now)
    }

    /// Like `apply_ops`, resolving character names against the world's `roster`.
    pub fn apply_ops_with(&mut self, batch: &OpBatch, limits: &ApplyLimits, roster: &Roster, now: i64) -> ApplyReport {
        self.apply_ops_at(batch, limits, roster, now, STORY_PRESENT)
    }

    /// Like `apply_ops_with`, at story time `story`: what is created is dated then (or
    /// timeless in a world without a timeline) and `close_edge` ends a link then.
    pub fn apply_ops_at(&mut self, batch: &OpBatch, limits: &ApplyLimits, roster: &Roster, now: i64, story: StoryTime) -> ApplyReport {
        let mut report = ApplyReport::default();
        let mut refs: HashMap<String, NodeId> = HashMap::new();
        let mut new_nodes = 0usize;

        if batch.ops.len() > limits.max_ops {
            report.rejected.push(format!("{} extra operations ignored", batch.ops.len() - limits.max_ops));
        }
        for op in batch.ops.iter().take(limits.max_ops) {
            let ctx = ApplyCtx { limits, roster, now, story };
            match self.apply_one(op, &ctx, &mut refs, &mut new_nodes, &mut report) {
                Ok(()) => report.applied += 1,
                Err(why) => report.rejected.push(format!("{op:?}: {why}")),
            }
        }
        report.touched.sort_unstable();
        report.touched.dedup();
        report
    }

    fn resolve(&self, r: &NodeRef, refs: &HashMap<String, NodeId>) -> Result<NodeId, String> {
        match r {
            NodeRef::Id(id) if self.contains(*id) => Ok(*id),
            NodeRef::Id(id) => Err(format!("#{id} does not exist")),
            NodeRef::New(name) => refs.get(name).copied().ok_or_else(|| format!("reference {name} was not created")),
        }
    }

    fn apply_one(
        &mut self,
        op: &GraphOp,
        ctx: &ApplyCtx<'_>,
        refs: &mut HashMap<String, NodeId>,
        new_nodes: &mut usize,
        report: &mut ApplyReport,
    ) -> Result<(), String> {
        let ApplyCtx { limits, roster, now, story } = *ctx;
        let dated = written_at(story);
        let step = |v: f32, max: f32| if v.is_finite() { v.clamp(-max, max) } else { 0.0 };
        match op {
            GraphOp::CreateNode { reference, kind, label, content, weight, valence } => {
                if kind.is_root() {
                    return Err("cannot create another root".into());
                }
                if matches!(self.scope(), crate::model::Scope::Chat | crate::model::Scope::Library | crate::model::Scope::Collab) && kind.is_personality() {
                    return Err("personality belongs to the characters, not to a chat".into());
                }
                let label = label.trim();
                if label.is_empty() {
                    return Err("empty label".into());
                }
                let external = (*kind == NodeKind::Character).then(|| roster.get(&fold(label)).copied()).flatten();
                // Small models re-create entities that already exist: they are merged.
                // A character of the world is recognised by its link, even under a nickname.
                let existing = external
                    .and_then(|r| self.find_external(r))
                    .or_else(|| self.find_by_label(label, Some(*kind)));
                if let Some(existing) = existing {
                    // Learned now about something dated later: it existed already now.
                    if self.node(existing).is_some_and(|n| n.since > dated) && existing != self.root() {
                        self.set_since(existing, dated).map_err(|e| e.to_string())?;
                    }
                    refs.insert(reference.clone(), existing);
                    report.created.push((reference.clone(), existing));
                    report.touched.push(existing);
                    return Ok(());
                }
                if *new_nodes >= limits.max_new_nodes {
                    return Err("limit of new nodes per turn reached".into());
                }
                let node = MemoryNode::new(*kind, label, content.trim(), now).with_weight(*weight).with_valence(*valence).since(dated);
                let id = self.add_node(node).map_err(|e| e.to_string())?;
                if external.is_some() {
                    self.set_external(id, external).map_err(|e| e.to_string())?;
                }
                // Traits (or a world's rules) hang from the root so they are not orphaned.
                if self.hangs_from_root(*kind) {
                    let root = self.root();
                    self.connect_from(root, id, EdgeKind::Composes, weight.max(0.3), now, dated).map_err(|e| e.to_string())?;
                }
                *new_nodes += 1;
                refs.insert(reference.clone(), id);
                report.created.push((reference.clone(), id));
                report.touched.push(id);
            }
            GraphOp::UpdateNode { target, label, content, weight_delta, valence_delta } => {
                let id = self.resolve(target, refs)?;
                let is_persona = self.node(id).is_some_and(|n| n.kind.is_core());
                let max = if is_persona { limits.max_personality_step } else { limits.max_step };
                if id == self.root() && (content.is_some() || label.is_some()) {
                    return Err("only the user edits the root's identity".into());
                }
                let patch = NodePatch {
                    label: label.as_ref().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()),
                    content: content.as_ref().map(|c| c.trim().to_owned()).filter(|c| !c.is_empty()),
                    weight_delta: weight_delta.map(|d| step(d, max)),
                    valence_delta: valence_delta.map(|d| step(d, max)),
                    ..Default::default()
                };
                self.update_node(id, patch).map_err(|e| e.to_string())?;
                report.touched.push(id);
            }
            GraphOp::Link { source, target, kind, weight } => {
                let (a, b) = (self.resolve(source, refs)?, self.resolve(target, refs)?);
                self.connect_from(a, b, *kind, *weight, now, dated).map_err(|e| e.to_string())?;
                report.touched.extend([a, b]);
            }
            GraphOp::Reinforce { source, target, delta } => {
                let (a, b) = (self.resolve(source, refs)?, self.resolve(target, refs)?);
                let d = step(*delta, limits.max_step);
                if !self.reinforce(a, b, d, now).map_err(|e| e.to_string())? {
                    // Reinforcing something unconnected amounts to associating it.
                    if d > 0.0 {
                        self.connect_from(a, b, EdgeKind::RelatedTo, d, now, dated).map_err(|e| e.to_string())?;
                    } else {
                        return Err("there is no edge to weaken".into());
                    }
                }
                report.touched.extend([a, b]);
            }
            GraphOp::Forget { target } => {
                let id = self.resolve(target, refs)?;
                let kind = self.node(id).map(|n| n.kind).ok_or("node does not exist")?;
                if limits.deletable.contains(&kind) {
                    self.remove_node(id).map_err(|e| e.to_string())?;
                } else {
                    // Personality and entities are not deleted: they fade.
                    let max = if kind.is_core() { limits.max_personality_step } else { limits.max_step };
                    self.update_node(id, NodePatch { weight_delta: Some(-max), ..Default::default() })
                        .map_err(|e| e.to_string())?;
                }
                report.touched.push(id);
            }
            GraphOp::CloseEdge { source, target } => {
                let (a, b) = (self.resolve(source, refs)?, self.resolve(target, refs)?);
                if !self.close_edge(a, b, story).map_err(|e| e.to_string())? {
                    return Err("there is no open link to close".into());
                }
                report.touched.extend([a, b]);
            }
        }
        Ok(())
    }
}
