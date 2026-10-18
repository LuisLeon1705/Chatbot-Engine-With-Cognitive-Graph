//! Binary persistence with `rkyv` (the `.mem` format).
//!
//! File layout:
//!
//! ```text
//! 0..4    magic  b"CMEM"
//! 4..6    format version (u16 LE)
//! 6..8    reserved
//! 8..16   payload length (u64 LE)
//! 16..    rkyv payload of `GraphFile` (validated with bytecheck when read)
//! ```
//!
//! On disk the graph is stored "flattened" (vectors of nodes and edges with their u32
//! indices); on load the `StableGraph` is rebuilt keeping the holes of deleted nodes,
//! so `NodeId`s do not change between sessions.

use std::fs;
use std::io::Write;
use std::path::Path;

use petgraph::stable_graph::NodeIndex;
use petgraph::visit::NodeIndexable;
use rkyv::rancor::Error as RkyvError;
use rkyv::util::AlignedVec;

use crate::graph::{MemoryGraph, Graph};
use crate::model::{ExternalRef, GraphMeta, MemoryEdge, MemoryNode, NodeId, NodeKind};

pub const MAGIC: [u8; 4] = *b"CMEM";
/// v2: scope (character/world), stable id, appearance and the external links table.
/// v3: story time (node `since`, edge `valid_from`/`valid_until`) and timelines.
/// Version 2 files are still read.
pub const FORMAT_VERSION: u16 = 3;
const HEADER_LEN: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a memory file (.mem)")]
    BadMagic,
    #[error("format version {0} is not supported (this build reads 2 to {FORMAT_VERSION})")]
    UnsupportedVersion(u16),
    #[error("truncated or damaged file: {0}")]
    Corrupt(String),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub struct NodeRecord {
    pub id: NodeId,
    pub node: MemoryNode,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub struct EdgeRecord {
    pub source: NodeId,
    pub target: NodeId,
    pub edge: MemoryEdge,
}

/// What is actually serialised.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub struct GraphFile {
    pub meta: GraphMeta,
    pub root: NodeId,
    /// Upper bound of indices (includes holes of deleted nodes).
    pub node_bound: u32,
    pub nodes: Vec<NodeRecord>,
    pub edges: Vec<EdgeRecord>,
    /// Links to other graphs (sparse table).
    pub externals: Vec<(NodeId, ExternalRef)>,
}

impl MemoryGraph {
    /// Flattened copy ready for `rkyv`.
    pub fn to_file(&self) -> GraphFile {
        GraphFile {
            meta: self.meta.clone(),
            root: self.root(),
            node_bound: self.graph.node_bound() as u32,
            nodes: self.nodes().map(|(id, n)| NodeRecord { id, node: n.clone() }).collect(),
            edges: self.edges().map(|(s, t, e)| EdgeRecord { source: s, target: t, edge: *e }).collect(),
            externals: { let mut v: Vec<_> = self.externals().collect(); v.sort_unstable_by_key(|(id, _)| *id); v },
        }
    }

    pub fn from_file(file: GraphFile) -> Result<Self, PersistError> {
        let bound = file.node_bound as usize;
        let mut slots: Vec<Option<MemoryNode>> = (0..bound).map(|_| None).collect();
        for rec in file.nodes {
            let slot = slots
                .get_mut(rec.id as usize)
                .ok_or_else(|| PersistError::Corrupt(format!("node #{} out of range", rec.id)))?;
            *slot = Some(rec.node);
        }
        if !matches!(slots.get(file.root as usize), Some(Some(n)) if n.kind.is_root() && n.kind == file.meta.scope.root_kind()) {
            return Err(PersistError::Corrupt("the graph's root is missing".into()));
        }

        // Every index is inserted in order and then the hole markers are removed: that
        // way each node gets exactly its original index back.
        let mut graph = Graph::with_capacity(bound, file.edges.len());
        let mut holes = Vec::new();
        for (i, slot) in slots.into_iter().enumerate() {
            let is_hole = slot.is_none();
            let n = graph.add_node(slot.unwrap_or_else(|| MemoryNode::new(NodeKind::Topic, "", "", 0)));
            debug_assert_eq!(n.index(), i);
            if is_hole {
                holes.push(n);
            }
        }
        for h in holes {
            graph.remove_node(h);
        }

        for rec in file.edges {
            let (s, t) = (NodeIndex::new(rec.source as usize), NodeIndex::new(rec.target as usize));
            if !graph.contains_node(s) || !graph.contains_node(t) {
                return Err(PersistError::Corrupt(format!("orphan edge #{}→#{}", rec.source, rec.target)));
            }
            graph.add_edge(s, t, rec.edge);
        }
        let mut externals = std::collections::HashMap::with_capacity(file.externals.len());
        for (id, r) in file.externals {
            if !graph.contains_node(NodeIndex::new(id as usize)) {
                return Err(PersistError::Corrupt(format!("orphan external link of #{id}")));
            }
            externals.insert(id, r);
        }
        Ok(MemoryGraph::from_parts(file.meta, graph, file.root, externals))
    }

    /// Serialises to bytes (header + rkyv payload).
    pub fn to_bytes(&self) -> Result<Vec<u8>, PersistError> {
        let payload = rkyv::to_bytes::<RkyvError>(&self.to_file()).map_err(|e| PersistError::Corrupt(e.to_string()))?;
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PersistError> {
        let (version, aligned) = aligned_payload(bytes)?;
        let corrupt = |e: RkyvError| PersistError::Corrupt(e.to_string());
        let file = if version == 2 {
            let archived = rkyv::access::<v2::ArchivedGraphFile, RkyvError>(&aligned).map_err(corrupt)?;
            rkyv::deserialize::<v2::GraphFile, RkyvError>(archived).map_err(corrupt)?.into()
        } else {
            rkyv::deserialize::<GraphFile, RkyvError>(access(&aligned)?).map_err(corrupt)?
        };
        Self::from_file(file)
    }

    /// Atomic save: writes `<path>.tmp`, fsyncs it and renames it over the original.
    pub fn save(&self, path: &Path) -> Result<(), PersistError> {
        write_atomic(path, &self.to_bytes()?)
    }

    pub fn load(path: &Path) -> Result<Self, PersistError> {
        Self::from_bytes(&fs::read(path)?)
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PersistError> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("mem.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Reads only the metadata without rebuilding the graph: zero-copy access to the
/// archived file and deserialisation of `meta` alone. Meant for the character gallery
/// (hundreds of `.mem` files without loading their graphs).
pub fn peek_meta(path: &Path) -> Result<(GraphMeta, usize, usize), PersistError> {
    let bytes = fs::read(path)?;
    let (version, aligned) = aligned_payload(&bytes)?;
    let corrupt = |e: RkyvError| PersistError::Corrupt(e.to_string());
    if version == 2 {
        let archived = rkyv::access::<v2::ArchivedGraphFile, RkyvError>(&aligned).map_err(corrupt)?;
        let meta = rkyv::deserialize::<v2::GraphMeta, RkyvError>(&archived.meta).map_err(corrupt)?;
        return Ok((meta.into(), archived.nodes.len(), archived.edges.len()));
    }
    let archived = access(&aligned)?;
    let meta = rkyv::deserialize::<GraphMeta, RkyvError>(&archived.meta).map_err(corrupt)?;
    Ok((meta, archived.nodes.len(), archived.edges.len()))
}

fn access(aligned: &AlignedVec) -> Result<&ArchivedGraphFile, PersistError> {
    rkyv::access::<ArchivedGraphFile, RkyvError>(aligned).map_err(|e| PersistError::Corrupt(e.to_string()))
}

/// Checks the header and copies the payload into an aligned buffer (rkyv requires it).
fn aligned_payload(bytes: &[u8]) -> Result<(u16, AlignedVec), PersistError> {
    if bytes.len() < HEADER_LEN || bytes[0..4] != MAGIC {
        return Err(PersistError::BadMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if !(2..=FORMAT_VERSION).contains(&version) {
        return Err(PersistError::UnsupportedVersion(version));
    }
    let len = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let payload = bytes
        .get(HEADER_LEN..HEADER_LEN.saturating_add(len))
        .ok_or_else(|| PersistError::Corrupt(format!("expected {len} bytes of payload")))?;
    let mut aligned = AlignedVec::with_capacity(payload.len());
    aligned.extend_from_slice(payload);
    Ok((version, aligned))
}

/// Layout of version 2 (before story time), only to read old files. Everything in it is
/// timeless: nodes exist since forever and edges are still valid.
pub mod v2 {
    use crate::model::{self, EdgeKind, ExternalRef, GraphId, NodeId, NodeKind, Scope, STORY_ALWAYS, STORY_OPEN};

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
    pub struct MemoryNode {
        pub kind: NodeKind,
        pub label: String,
        pub content: String,
        pub weight: f32,
        pub valence: f32,
        pub created_at: i64,
        pub last_accessed: i64,
        pub access_count: u32,
    }

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
    pub struct MemoryEdge {
        pub kind: EdgeKind,
        pub weight: f32,
        pub created_at: i64,
        pub last_reinforced: i64,
    }

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
    pub struct GraphMeta {
        pub id: GraphId,
        pub scope: Scope,
        pub name: String,
        pub tagline: String,
        pub avatar: Option<String>,
        pub scenario: String,
        pub appearance: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
    pub struct NodeRecord {
        pub id: NodeId,
        pub node: MemoryNode,
    }

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
    pub struct EdgeRecord {
        pub source: NodeId,
        pub target: NodeId,
        pub edge: MemoryEdge,
    }

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
    pub struct GraphFile {
        pub meta: GraphMeta,
        pub root: NodeId,
        pub node_bound: u32,
        pub nodes: Vec<NodeRecord>,
        pub edges: Vec<EdgeRecord>,
        pub externals: Vec<(NodeId, ExternalRef)>,
    }

    impl From<GraphMeta> for model::GraphMeta {
        fn from(m: GraphMeta) -> Self {
            Self {
                id: m.id,
                scope: m.scope,
                name: m.name,
                tagline: m.tagline,
                avatar: m.avatar,
                scenario: m.scenario,
                appearance: m.appearance,
                created_at: m.created_at,
                updated_at: m.updated_at,
                timeline: Vec::new(),
                moments: Vec::new(),
            }
        }
    }

    impl From<GraphFile> for super::GraphFile {
        fn from(f: GraphFile) -> Self {
            Self {
                meta: f.meta.into(),
                root: f.root,
                node_bound: f.node_bound,
                nodes: f
                    .nodes
                    .into_iter()
                    .map(|r| super::NodeRecord {
                        id: r.id,
                        node: model::MemoryNode {
                            kind: r.node.kind,
                            label: r.node.label,
                            content: r.node.content,
                            weight: r.node.weight,
                            valence: r.node.valence,
                            created_at: r.node.created_at,
                            last_accessed: r.node.last_accessed,
                            access_count: r.node.access_count,
                            since: STORY_ALWAYS,
                        },
                    })
                    .collect(),
                edges: f
                    .edges
                    .into_iter()
                    .map(|r| super::EdgeRecord {
                        source: r.source,
                        target: r.target,
                        edge: model::MemoryEdge {
                            kind: r.edge.kind,
                            weight: r.edge.weight,
                            created_at: r.edge.created_at,
                            last_reinforced: r.edge.last_reinforced,
                            valid_from: STORY_ALWAYS,
                            valid_until: STORY_OPEN,
                        },
                    })
                    .collect(),
                externals: f.externals,
            }
        }
    }
}
