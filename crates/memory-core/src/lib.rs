//! Memory core for characters and worlds: a unified double graph (personality tree +
//! episodic graph) on `petgraph`, world memory by category, relationships between
//! characters, spreading activation with forgetting over time, binary `rkyv`
//! persistence and the dual actor/curator pipeline.

pub mod activation;
pub mod collab;
pub mod context;
pub mod conversation;
pub mod graph;
pub mod handler;
pub mod layout;
#[cfg(feature = "llm")]
pub mod llm;
pub mod model;
pub mod ops;
pub mod persist;
pub mod prompts;
pub mod style;
pub mod synth;
pub mod text;
pub mod view;
pub mod world;

pub use activation::{Activated, ActivationParams};
pub use context::ContextBudget;
pub use graph::{GraphError, MemoryGraph, NodePatch};
pub use handler::{HandlerConfig, MemoryHandler, Recall};
pub use i18n::Lang;
pub use model::{
    CharacterMoment, DAYS_PER_YEAR, EdgeKind, ExternalRef, GraphId, GraphMeta, MemoryEdge, MemoryNode, Moment, NodeId, NodeKind, STORY_ALWAYS, STORY_OPEN,
    STORY_PRESENT, Scope, StoryTime, Validity, story_of_year, story_year, unix_now, written_at,
};
pub use ops::{ApplyLimits, ApplyReport, GraphOp, NodeRef, OpBatch, Roster};
pub use persist::{PersistError, peek_meta};
pub use view::{EdgeDetail, GraphView, NodeDetail, Story, StoryMark, ViewLink, ViewNode};
pub use style::{StyleFont, StyleMode, StyleTexture, WorldStyle};
pub use world::{AppearanceReport, Aspect, Library, Perception, SceneRecall, World, WorldError};
