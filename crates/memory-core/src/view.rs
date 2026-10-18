//! Bridge to the frontend (React + react-force-graph).
//!
//! Only what is needed to draw travels: id, name, kind (u8), size and, if any, the
//! activation heat of the last turn. A node's full content is requested separately
//! (`NodeDetail`) on click. Field names follow the format react-force-graph expects
//! (`nodes[].id`, `links[].source/target`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::MemoryGraph;
use crate::model::{NodeId, NodeKind, STORY_ALWAYS, STORY_OPEN, STORY_PRESENT, StoryTime};

/// A story time over IPC. The open ends are `i64::MIN`/`MAX`, which JavaScript numbers
/// cannot hold, so they travel as words: `"always"`, `"open"`, `"present"` or a day.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Story {
    Day(i64),
    Mark(StoryMark),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoryMark {
    Always,
    Open,
    Present,
}

impl From<StoryTime> for Story {
    fn from(t: StoryTime) -> Self {
        match t {
            STORY_ALWAYS => Story::Mark(StoryMark::Always),
            STORY_OPEN => Story::Mark(StoryMark::Open),
            STORY_PRESENT => Story::Mark(StoryMark::Present),
            d => Story::Day(d),
        }
    }
}

impl From<Story> for StoryTime {
    fn from(s: Story) -> Self {
        match s {
            Story::Mark(StoryMark::Always) => STORY_ALWAYS,
            Story::Mark(StoryMark::Open) => STORY_OPEN,
            Story::Mark(StoryMark::Present) => STORY_PRESENT,
            // A day can never collide with the marks.
            Story::Day(d) => d.clamp(STORY_ALWAYS + 1, STORY_PRESENT - 1),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewNode {
    pub id: NodeId,
    pub name: String,
    pub kind: u8,
    /// Relative radius of the sphere (`nodeVal`).
    pub val: f32,
    pub valence: f32,
    /// Activation of the last recall in [0, 1]; absent if it did not take part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat: Option<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewLink {
    pub source: NodeId,
    pub target: NodeId,
    pub kind: u8,
    pub weight: f32,
    /// The link no longer holds from some point of the story (drawn as the past).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphView {
    pub root: NodeId,
    pub nodes: Vec<ViewNode>,
    pub links: Vec<ViewLink>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetail {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: String,
    pub content: String,
    pub weight: f32,
    pub valence: f32,
    pub created_at: i64,
    pub last_accessed: i64,
    pub access_count: u32,
    pub degree: usize,
    pub since: Story,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EdgeDetail {
    pub other: NodeId,
    pub other_label: String,
    pub kind: crate::model::EdgeKind,
    pub weight: f32,
    /// `true` if the edge goes from this node to `other`.
    pub outgoing: bool,
    pub valid_from: Story,
    /// `"open"` while it still holds.
    pub valid_until: Story,
}

impl MemoryGraph {
    pub fn view(&self, heat: &HashMap<NodeId, f32>) -> GraphView {
        GraphView {
            root: self.root(),
            nodes: self
                .nodes()
                .map(|(id, n)| ViewNode {
                    id,
                    name: n.label.clone(),
                    kind: n.kind as u8,
                    val: if n.kind.is_root() { 8.0 } else { 1.0 + 4.0 * n.weight },
                    valence: n.valence,
                    heat: heat.get(&id).copied().filter(|h| *h > 0.0).map(|h| h.min(1.0)),
                })
                .collect(),
            links: self
                .edges()
                .map(|(s, t, e)| ViewLink { source: s, target: t, kind: e.kind as u8, weight: e.weight, closed: !e.is_open() })
                .collect(),
        }
    }

    /// A node's edges with the label of the other end (for the memory editor).
    pub fn node_edges(&self, id: NodeId) -> Vec<EdgeDetail> {
        let mut out: Vec<EdgeDetail> = self
            .neighbors(id)
            .filter_map(|(other, e, outgoing)| {
                Some(EdgeDetail {
                    other,
                    other_label: self.node(other)?.label.clone(),
                    kind: e.kind,
                    weight: e.weight,
                    outgoing,
                    valid_from: e.valid_from.into(),
                    valid_until: e.valid_until.into(),
                })
            })
            .collect();
        // Open links first, then the past (most recently closed first).
        out.sort_by(|a, b| {
            let until = |d: &EdgeDetail| StoryTime::from(d.valid_until);
            until(b).cmp(&until(a)).then(b.weight.total_cmp(&a.weight))
        });
        out
    }

    pub fn detail(&self, id: NodeId) -> Option<NodeDetail> {
        let n = self.node(id)?;
        Some(NodeDetail {
            id,
            kind: n.kind,
            label: n.label.clone(),
            content: n.content.clone(),
            weight: n.weight,
            valence: n.valence,
            created_at: n.created_at,
            last_accessed: n.last_accessed,
            access_count: n.access_count,
            degree: self.degree(id),
            since: n.since.into(),
        })
    }
}
