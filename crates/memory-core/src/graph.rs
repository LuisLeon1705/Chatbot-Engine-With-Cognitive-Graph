//! `MemoryGraph`: a memory graph on a single petgraph `StableGraph`, with CRUD and a
//! derived lexical index. It serves two scopes (`Scope`):
//! - character: personality tree + episodic graph joined together;
//! - world: rules (lore), places, factions, history and each character's public sheet.
//!
//! Nodes can point to other graphs (`ExternalRef`): that is how characters are linked
//! to each other and to their world without copying their memories.
//!
//! `StableGraph` keeps the `NodeId`s (u32) valid after deleting nodes: the frontend and
//! the curator model refer to nodes by that number.

use petgraph::Direction;
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;

use std::collections::HashMap;

use crate::model::{
    EdgeKind, ExternalRef, GraphMeta, MemoryEdge, MemoryNode, NodeId, NodeKind, STORY_ALWAYS, STORY_OPEN, Scope, StoryTime, Validity,
    clamp_signed, clamp_unit,
};
use crate::text::{LexicalIndex, fold};

pub type Graph = StableGraph<MemoryNode, MemoryEdge, petgraph::Directed, u32>;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GraphError {
    #[error("node #{0} does not exist")]
    NoSuchNode(NodeId),
    #[error("the graph's root cannot be deleted or duplicated")]
    RootProtected,
    #[error("edges from a node to itself are not allowed (#{0})")]
    SelfLoop(NodeId),
    #[error("there is no edge #{0}→#{1}")]
    NoSuchEdge(NodeId, NodeId),
    #[error("an edge must start before it ends")]
    BadInterval,
    #[error("#{0}→#{1} already has an open edge")]
    AlreadyOpen(NodeId, NodeId),
    #[error("close_edge ignored: structural memory edge (Involves/Evokes) cannot expire")]
    ImmutableMemoryEdge,
}

/// Partial changes to a node (`None` fields are left alone).
#[derive(Debug, Default, Clone)]
pub struct NodePatch {
    /// Kind change (e.g. an entity that turns out to be a character). Roots are not allowed.
    pub kind: Option<NodeKind>,
    pub label: Option<String>,
    pub content: Option<String>,
    pub weight: Option<f32>,
    pub weight_delta: Option<f32>,
    pub valence: Option<f32>,
    pub valence_delta: Option<f32>,
}

#[derive(Clone)]
pub struct MemoryGraph {
    pub meta: GraphMeta,
    pub(crate) graph: Graph,
    root: NodeIndex<u32>,
    index: LexicalIndex,
    externals: HashMap<NodeId, ExternalRef>,
}

#[inline]
fn ix(id: NodeId) -> NodeIndex<u32> {
    NodeIndex::new(id as usize)
}

impl MemoryGraph {
    /// New graph with its root (personality or world, depending on `meta.scope`).
    pub fn new(meta: GraphMeta, root_content: impl Into<String>, now: i64) -> Self {
        let mut graph = Graph::default();
        let root_node = MemoryNode::new(meta.scope.root_kind(), meta.name.clone(), root_content, now).with_weight(1.0);
        let root = graph.add_node(root_node);
        let mut g = Self { meta, graph, root, index: LexicalIndex::default(), externals: HashMap::new() };
        g.index.insert(root.index() as NodeId, &g.graph[root]);
        g
    }

    /// Rebuilt from disk (see `persist`). The lexical index is regenerated here.
    pub(crate) fn from_parts(meta: GraphMeta, graph: Graph, root: NodeId, externals: HashMap<NodeId, ExternalRef>) -> Self {
        let mut index = LexicalIndex::default();
        for n in graph.node_indices() {
            index.insert(n.index() as NodeId, &graph[n]);
        }
        Self { meta, graph, root: ix(root), index, externals }
    }

    pub fn root(&self) -> NodeId {
        self.root.index() as NodeId
    }

    pub fn scope(&self) -> Scope {
        self.meta.scope
    }

    /// Label for `logs/curator.log`: the graph's name, or `chat «character»` for a
    /// conversation's memory so it always identifies the associated character/session.
    pub fn log_label(&self) -> String {
        if self.scope() != Scope::Chat {
            return self.meta.name.clone();
        }
        let root = self.root();
        let mut chars: Vec<(NodeId, &str)> = self
            .neighbors(root)
            .filter(|(_, e, out)| *out && e.kind == EdgeKind::Knows)
            .filter_map(|(id, _, _)| self.node(id).filter(|n| n.kind == NodeKind::Character).map(|n| (id, n.label.trim())))
            .filter(|(_, l)| !l.is_empty())
            .collect();
        chars.sort_by_key(|(id, _)| *id);
        let who = if !chars.is_empty() {
            chars.into_iter().map(|(_, l)| l).collect::<Vec<_>>().join(", ")
        } else {
            self.node(root)
                .map(|n| n.label.trim())
                .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("chat"))
                .unwrap_or_else(|| self.meta.name.trim())
                .to_owned()
        };
        if who.is_empty() || who.eq_ignore_ascii_case("chat") {
            "chat".to_owned()
        } else {
            format!("chat «{who}»")
        }
    }

    /// Should a new node of this kind hang from the root? (traits in a character, lore in a
    /// world or a chat)
    pub fn hangs_from_root(&self, kind: NodeKind) -> bool {
        match self.scope() {
            Scope::Character => kind.is_personality() && !kind.is_root(),
            Scope::World | Scope::Chat | Scope::Collab => kind == NodeKind::Lore,
            // Every definition hangs from the library's root.
            Scope::Library => !kind.is_root(),
        }
    }

    // ------------------------------------------------------------------ external links

    pub fn external(&self, id: NodeId) -> Option<ExternalRef> {
        self.externals.get(&id).copied()
    }

    pub fn externals(&self) -> impl Iterator<Item = (NodeId, ExternalRef)> + '_ {
        self.externals.iter().map(|(k, v)| (*k, *v))
    }

    /// Node of this graph that stands for `target` (e.g. "my opinion of Kael").
    pub fn find_external(&self, target: ExternalRef) -> Option<NodeId> {
        self.externals.iter().find(|(_, r)| **r == target).map(|(id, _)| *id)
    }

    pub fn set_external(&mut self, id: NodeId, target: Option<ExternalRef>) -> Result<(), GraphError> {
        if !self.contains(id) {
            return Err(GraphError::NoSuchNode(id));
        }
        match target {
            Some(t) => self.externals.insert(id, t),
            None => self.externals.remove(&id),
        };
        Ok(())
    }

    pub fn node(&self, id: NodeId) -> Option<&MemoryNode> {
        self.graph.node_weight(ix(id))
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.graph.contains_node(ix(id))
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &MemoryNode)> {
        self.graph.node_indices().map(|n| (n.index() as NodeId, &self.graph[n]))
    }

    /// (source, target, edge)
    pub fn edges(&self) -> impl Iterator<Item = (NodeId, NodeId, &MemoryEdge)> {
        self.graph.edge_indices().filter_map(|e| {
            let (a, b) = self.graph.edge_endpoints(e)?;
            Some((a.index() as NodeId, b.index() as NodeId, &self.graph[e]))
        })
    }

    /// Neighbours in both directions: (neighbour, edge, outgoing?)
    pub fn neighbors(&self, id: NodeId) -> impl Iterator<Item = (NodeId, &MemoryEdge, bool)> {
        let out = self.graph.edges_directed(ix(id), Direction::Outgoing).map(|e| (e.target().index() as NodeId, e.weight(), true));
        let inc = self.graph.edges_directed(ix(id), Direction::Incoming).map(|e| (e.source().index() as NodeId, e.weight(), false));
        out.chain(inc)
    }

    pub fn degree(&self, id: NodeId) -> usize {
        self.graph.edges_directed(ix(id), Direction::Outgoing).count()
            + self.graph.edges_directed(ix(id), Direction::Incoming).count()
    }

    // ------------------------------------------------------------------ node CRUD

    pub fn add_node(&mut self, node: MemoryNode) -> Result<NodeId, GraphError> {
        if node.kind.is_root() {
            return Err(GraphError::RootProtected);
        }
        let id = self.graph.add_node(node).index() as NodeId;
        self.index.insert(id, &self.graph[ix(id)]);
        Ok(id)
    }

    pub fn update_node(&mut self, id: NodeId, patch: NodePatch) -> Result<(), GraphError> {
        let n = ix(id);
        let old = self.graph.node_weight(n).ok_or(GraphError::NoSuchNode(id))?.clone();
        if patch.kind.is_some_and(|k| k.is_root()) || (old.kind.is_root() && patch.kind.is_some_and(|k| k != old.kind)) {
            return Err(GraphError::RootProtected);
        }
        let reindex = patch.label.is_some() || patch.content.is_some();
        let node = &mut self.graph[n];
        if let Some(k) = patch.kind {
            node.kind = k;
        }
        if let Some(l) = patch.label {
            node.label = l;
        }
        if let Some(c) = patch.content {
            node.content = c;
        }
        if let Some(w) = patch.weight {
            node.weight = clamp_unit(w);
        }
        if let Some(d) = patch.weight_delta {
            node.weight = clamp_unit(node.weight + d);
        }
        if let Some(v) = patch.valence {
            node.valence = clamp_signed(v);
        }
        if let Some(d) = patch.valence_delta {
            node.valence = clamp_signed(node.valence + d);
        }
        if reindex {
            self.index.remove(id, &old);
            self.index.insert(id, &self.graph[n]);
        }
        Ok(())
    }

    pub fn remove_node(&mut self, id: NodeId) -> Result<MemoryNode, GraphError> {
        if id == self.root() {
            return Err(GraphError::RootProtected);
        }
        let node = self.graph.remove_node(ix(id)).ok_or(GraphError::NoSuchNode(id))?;
        self.index.remove(id, &node);
        self.externals.remove(&id);
        Ok(node)
    }

    /// Finds a node by normalised label (no accents, case-insensitive) and, optionally, kind.
    pub fn find_by_label(&self, label: &str, kind: Option<NodeKind>) -> Option<NodeId> {
        let wanted = fold(label.trim());
        self.nodes()
            .find(|(_, n)| kind.is_none_or(|k| k == n.kind) && fold(n.label.trim()) == wanted)
            .map(|(id, _)| id)
    }

    // ------------------------------------------------------------------ edge CRUD
    //
    // A directed pair has at most one OPEN edge (`valid_until == STORY_OPEN`) and any
    // number of closed ones: the history of a relationship that changed.

    /// Creates or updates the open directed edge `a → b`.
    pub fn connect(&mut self, a: NodeId, b: NodeId, kind: EdgeKind, weight: f32, now: i64) -> Result<(), GraphError> {
        self.connect_from(a, b, kind, weight, now, STORY_ALWAYS)
    }

    /// Like `connect`; a new edge holds from story time `from`. If the pair's link closed
    /// exactly at `from` (it ended in this same moment), that one is reopened instead of
    /// adding a second one.
    pub fn connect_from(&mut self, a: NodeId, b: NodeId, kind: EdgeKind, weight: f32, now: i64, from: StoryTime) -> Result<(), GraphError> {
        self.check_pair(a, b)?;
        let resumed = self.open_edge(a, b).or_else(|| {
            let e = self.edge_ending(a, b, from)?;
            self.graph[e].valid_until = STORY_OPEN;
            Some(e)
        });
        match resumed {
            Some(e) => {
                let edge = &mut self.graph[e];
                edge.kind = kind;
                edge.weight = clamp_unit(edge.weight.max(weight));
                edge.last_reinforced = now;
            }
            None => {
                let mut edge = MemoryEdge::new(kind, weight, now);
                edge.valid_from = from;
                self.graph.add_edge(ix(a), ix(b), edge);
            }
        }
        Ok(())
    }

    /// Adds `delta` to the weight of the open edge between `a` and `b` (in either
    /// direction). Returns `false` if there is none.
    pub fn reinforce(&mut self, a: NodeId, b: NodeId, delta: f32, now: i64) -> Result<bool, GraphError> {
        self.check_pair(a, b)?;
        let Some(e) = self.edge_between(a, b) else { return Ok(false) };
        let edge = &mut self.graph[e];
        edge.weight = clamp_unit(edge.weight + delta);
        edge.last_reinforced = now;
        Ok(true)
    }

    /// Creates or overwrites the open edge `a → b` with the exact kind and weight (manual editing).
    pub fn set_edge(&mut self, a: NodeId, b: NodeId, kind: EdgeKind, weight: f32, now: i64) -> Result<(), GraphError> {
        self.check_pair(a, b)?;
        match self.open_edge(a, b) {
            Some(e) => {
                let edge = &mut self.graph[e];
                edge.kind = kind;
                edge.weight = clamp_unit(weight);
                edge.last_reinforced = now;
            }
            None => {
                self.graph.add_edge(ix(a), ix(b), MemoryEdge::new(kind, weight, now));
            }
        }
        Ok(())
    }

    /// Removes the edge `a → b` that ends at `until` (`STORY_OPEN` = the open one).
    pub fn disconnect_at(&mut self, a: NodeId, b: NodeId, until: StoryTime) -> bool {
        match self.edge_ending(a, b, until) {
            Some(e) => self.graph.remove_edge(e).is_some(),
            None => false,
        }
    }

    /// Removes the open edge `a → b`.
    pub fn disconnect(&mut self, a: NodeId, b: NodeId) -> bool {
        self.disconnect_at(a, b, STORY_OPEN)
    }

    /// The open edge `a → b`.
    pub fn edge(&self, a: NodeId, b: NodeId) -> Option<&MemoryEdge> {
        self.open_edge(a, b).map(|e| &self.graph[e])
    }

    /// Every edge `a → b`, the closed ones included.
    pub fn edges_from_to(&self, a: NodeId, b: NodeId) -> impl Iterator<Item = &MemoryEdge> {
        self.graph.edges_connecting(ix(a), ix(b)).map(|e| e.weight())
    }

    /// Closes the open edge between `a` and `b` (this direction first, then the other) at
    /// story time `at`: from then on it is the past. Returns `false` if there is none.
    ///
    /// Structural links of a memory (`Involves`, `Evokes`) cannot expire and return
    /// [`GraphError::ImmutableMemoryEdge`] without mutating the graph.
    ///
    /// It may close at the very moment it started: a chat stays at one moment, so a link
    /// learned in one turn and undone in the next starts and ends then. Its interval is
    /// empty, so from that moment on it reads as the past ("it held, and no longer does").
    pub fn close_edge(&mut self, a: NodeId, b: NodeId, at: StoryTime) -> Result<bool, GraphError> {
        self.check_pair(a, b)?;
        let Some(e) = self.edge_between(a, b) else { return Ok(false) };
        let touches_memory = [a, b].into_iter().any(|id| self.node(id).is_some_and(|n| n.kind.is_memory()));
        let edge = &mut self.graph[e];
        if edge.kind.is_immutable_on_memory() && touches_memory {
            return Err(GraphError::ImmutableMemoryEdge);
        }
        if at < edge.valid_from {
            return Err(GraphError::BadInterval);
        }
        edge.valid_until = at;
        Ok(true)
    }

    /// Sets when the edge `a → b` that now ends at `until` holds (manual editing).
    pub fn set_edge_validity(&mut self, a: NodeId, b: NodeId, until: StoryTime, new_from: StoryTime, new_until: StoryTime) -> Result<(), GraphError> {
        self.check_pair(a, b)?;
        let e = self.edge_ending(a, b, until).ok_or(GraphError::NoSuchEdge(a, b))?;
        // Equal ends are allowed: a link that held only within one moment.
        if new_from > new_until {
            return Err(GraphError::BadInterval);
        }
        // Reopening is only possible if there is no other open edge for the pair.
        if new_until == STORY_OPEN && until != STORY_OPEN && self.open_edge(a, b).is_some() {
            return Err(GraphError::AlreadyOpen(a, b));
        }
        let edge = &mut self.graph[e];
        edge.valid_from = new_from;
        edge.valid_until = new_until;
        Ok(())
    }

    fn open_edge(&self, a: NodeId, b: NodeId) -> Option<EdgeIndex<u32>> {
        self.edge_ending(a, b, STORY_OPEN)
    }

    fn edge_ending(&self, a: NodeId, b: NodeId, until: StoryTime) -> Option<EdgeIndex<u32>> {
        self.graph.edges_connecting(ix(a), ix(b)).find(|e| e.weight().valid_until == until).map(|e| e.id())
    }

    fn edge_between(&self, a: NodeId, b: NodeId) -> Option<EdgeIndex<u32>> {
        self.open_edge(a, b).or_else(|| self.open_edge(b, a))
    }

    // ------------------------------------------------------------------ story time

    /// Does node `id` exist at story time `t`?
    pub fn exists_at(&self, id: NodeId, t: StoryTime) -> bool {
        self.node(id).is_some_and(|n| n.exists_at(t))
    }

    /// Sets the story time from which a node exists (the root is always there).
    pub fn set_since(&mut self, id: NodeId, since: StoryTime) -> Result<(), GraphError> {
        if id == self.root() {
            return Err(GraphError::RootProtected);
        }
        self.graph.node_weight_mut(ix(id)).ok_or(GraphError::NoSuchNode(id))?.since = since;
        Ok(())
    }

    /// Edges that stopped holding by story time `t` and touch one of `around`, the most
    /// recently closed first: (source, target, edge).
    pub fn past_edges(&self, around: &[NodeId], t: StoryTime) -> Vec<(NodeId, NodeId, &MemoryEdge)> {
        let mut v: Vec<_> = self
            .edges()
            .filter(|(s, d, e)| {
                e.validity_at(t) == Validity::Past
                    && (around.contains(s) || around.contains(d))
                    && self.exists_at(*s, t)
                    && self.exists_at(*d, t)
            })
            .collect();
        v.sort_by(|a, b| b.2.valid_until.cmp(&a.2.valid_until).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
        v
    }

    fn check_pair(&self, a: NodeId, b: NodeId) -> Result<(), GraphError> {
        if a == b {
            return Err(GraphError::SelfLoop(a));
        }
        for id in [a, b] {
            if !self.contains(id) {
                return Err(GraphError::NoSuchNode(id));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ domain shortcuts

    /// New branch of the personality tree hanging from `parent` (the root or another trait).
    pub fn add_trait(
        &mut self,
        parent: NodeId,
        kind: NodeKind,
        label: &str,
        content: &str,
        intensity: f32,
        now: i64,
    ) -> Result<NodeId, GraphError> {
        if !self.contains(parent) {
            return Err(GraphError::NoSuchNode(parent));
        }
        let id = self.add_node(MemoryNode::new(kind, label, content, now).with_weight(intensity))?;
        self.connect(parent, id, EdgeKind::Composes, intensity.max(0.3), now)?;
        Ok(id)
    }

    /// New memory (usually `EpisodicMemory`) connected to what it involves (entities,
    /// places, topics) and to what it evokes (emotions, traits).
    pub fn add_memory(&mut self, memory: MemoryNode, involves: &[NodeId], evokes: &[NodeId], now: i64) -> Result<NodeId, GraphError> {
        for &id in involves.iter().chain(evokes) {
            if !self.contains(id) {
                return Err(GraphError::NoSuchNode(id));
            }
        }
        let evoke_weight = 0.5 + 0.5 * memory.valence.abs();
        let id = self.add_node(memory)?;
        for &e in involves {
            self.connect(id, e, EdgeKind::Involves, 0.7, now)?;
        }
        for &e in evokes {
            self.connect(id, e, EdgeKind::Evokes, evoke_weight, now)?;
        }
        Ok(id)
    }

    /// Marks nodes as recalled (rehearsal effect: refreshes `last_accessed`).
    pub fn touch(&mut self, ids: &[NodeId], now: i64) {
        for &id in ids {
            if let Some(n) = self.graph.node_weight_mut(ix(id)) {
                n.last_accessed = now;
                n.access_count = n.access_count.saturating_add(1);
            }
        }
    }

    pub fn seeds(&self, query: &str) -> Vec<(NodeId, f32)> {
        self.index.seeds(query, self.node_count())
    }
}
