//! `MemoryHandler`: a concurrent facade over `MemoryGraph`.
//!
//! It is `Clone` (everything lives behind an `Arc`), so the same instance is registered
//! as Tauri state and shared with the consolidation worker. Reads (recall, view) take
//! the shared lock; writes (curator ops, rehearsal) the exclusive one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use i18n::Lang;
use parking_lot::{Mutex, RwLock};

use crate::activation::{Activated, ActivationParams};
use crate::context::ContextBudget;
use crate::graph::MemoryGraph;
use crate::model::{GraphId, GraphMeta, NodeId, STORY_PRESENT, StoryTime, unix_now};
use crate::ops::{ApplyLimits, ApplyReport, OpBatch, Roster};
use crate::persist::{PersistError, write_atomic};
use crate::view::{GraphView, NodeDetail};

#[derive(Debug, Clone)]
pub struct HandlerConfig {
    pub activation: ActivationParams,
    pub budget: ContextBudget,
    pub limits: ApplyLimits,
    /// Hebbian reinforcement per recall (0 turns it off).
    pub hebbian_rate: f32,
    /// Initial language of the prompts and context blocks built from this memory
    /// (it can change later with [`MemoryHandler::set_lang`]).
    pub lang: Lang,
}

impl Default for HandlerConfig {
    fn default() -> Self {
        Self {
            activation: ActivationParams::default(),
            budget: ContextBudget::default(),
            limits: ApplyLimits::default(),
            hebbian_rate: 0.02,
            lang: Lang::default(),
        }
    }
}

impl HandlerConfig {
    pub fn with_lang(lang: Lang) -> Self {
        Self { lang, ..Self::default() }
    }
}

/// Result of intercepting a prompt.
#[derive(Debug, Clone)]
pub struct Recall {
    /// Block ready for the actor's system prompt.
    pub context: String,
    pub active: Vec<Activated>,
}

struct Inner {
    graph: RwLock<MemoryGraph>,
    path: PathBuf,
    dirty: AtomicBool,
    /// Serialises saves (two concurrent `save`s must not overwrite the `.tmp`).
    save_lock: Mutex<()>,
    last_heat: RwLock<HashMap<NodeId, f32>>,
    /// Characters of the world it belongs to (to resolve `character` nodes).
    roster: RwLock<Roster>,
    /// Current language (`Lang::index`); it starts as `config.lang`.
    lang: AtomicU8,
    config: HandlerConfig,
}

#[derive(Clone)]
pub struct MemoryHandler(Arc<Inner>);

impl MemoryHandler {
    pub fn new(graph: MemoryGraph, path: impl Into<PathBuf>, config: HandlerConfig) -> Self {
        Self(Arc::new(Inner {
            graph: RwLock::new(graph),
            path: path.into(),
            dirty: AtomicBool::new(false),
            save_lock: Mutex::new(()),
            last_heat: RwLock::new(HashMap::new()),
            roster: RwLock::new(Roster::new()),
            lang: AtomicU8::new(config.lang.index() as u8),
            config,
        }))
    }

    /// Creates a new character and saves it right away.
    pub fn create(path: impl Into<PathBuf>, meta: GraphMeta, root_content: &str, config: HandlerConfig) -> Result<Self, PersistError> {
        let h = Self::new(MemoryGraph::new(meta, root_content, unix_now()), path, config);
        h.save()?;
        Ok(h)
    }

    pub fn open(path: impl Into<PathBuf>, config: HandlerConfig) -> Result<Self, PersistError> {
        let path = path.into();
        let graph = MemoryGraph::load(&path)?;
        Ok(Self::new(graph, path, config))
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }

    pub fn config(&self) -> &HandlerConfig {
        &self.0.config
    }

    pub fn lang(&self) -> Lang {
        Lang::from_index(self.0.lang.load(Ordering::Relaxed) as usize).unwrap_or_default()
    }

    /// Changes the language of the context blocks and prompts built from now on.
    pub fn set_lang(&self, lang: Lang) {
        self.0.lang.store(lang.index() as u8, Ordering::Relaxed);
    }

    pub fn id(&self) -> GraphId {
        self.0.graph.read().meta.id
    }

    pub fn meta(&self) -> GraphMeta {
        self.0.graph.read().meta.clone()
    }

    /// Label for `logs/curator.log` (`chat «character»` for `Scope::Chat`).
    pub fn log_label(&self) -> String {
        self.0.graph.read().log_label()
    }

    pub fn set_roster(&self, roster: Roster) {
        *self.0.roster.write() = roster;
    }

    /// Intercepts the user's prompt: seeds → spreading → active subgraph → context
    /// block. Afterwards it marks the rehearsal and strengthens co-activations.
    pub fn recall(&self, user_prompt: &str) -> Recall {
        self.recall_at(user_prompt, unix_now())
    }

    pub fn recall_at(&self, user_prompt: &str, now: i64) -> Recall {
        self.recall_with(user_prompt, &[], &[], self.0.config.budget, now, STORY_PRESENT)
    }

    /// Recall with extra seeds (e.g. the characters present in the scene) and its own
    /// budget (in a scene it is split between character and world). `hide` are nodes
    /// that are activated (and rehearsed) but not written into the block because they
    /// already appear in another section of the prompt. `story` is the moment of the
    /// story the scene happens at (`STORY_PRESENT` = everything).
    pub fn recall_with(
        &self,
        user_prompt: &str,
        extra_seeds: &[(NodeId, f32)],
        hide: &[NodeId],
        budget: ContextBudget,
        now: i64,
        story: StoryTime,
    ) -> Recall {
        let cfg = &self.0.config;
        let params = cfg.activation.at(story);
        let (active, context, heat) = {
            let g = self.0.graph.read();
            let mut seeds = g.seeds(user_prompt);
            seeds.extend_from_slice(extra_seeds);
            let act = g.spread(&seeds, &params);
            let active = g.rank(&act, now, &params);
            let shown: Vec<Activated> = active.iter().filter(|a| !hide.contains(&a.id)).copied().collect();
            let context = g.actor_block_at(&shown, now, story, budget, self.lang());
            (active, context, act)
        };
        {
            let mut g = self.0.graph.write();
            let ids: Vec<NodeId> = active.iter().map(|a| a.id).collect();
            g.touch(&ids, now);
            if cfg.hebbian_rate > 0.0 {
                g.hebbian(&active, cfg.hebbian_rate, now);
            }
        }
        *self.0.last_heat.write() = heat;
        self.mark_dirty();
        Recall { context, active }
    }

    /// Context for the curator, with no side effects (it does not count as rehearsal).
    pub fn curator_context(&self, turn_text: &str) -> String {
        self.curator_context_with(turn_text, &[], STORY_PRESENT)
    }

    /// The curator's context at a story time: what exists then and the links that hold.
    pub fn curator_context_with(&self, turn_text: &str, extra_seeds: &[(NodeId, f32)], story: StoryTime) -> String {
        let params = self.0.config.activation.at(story);
        let g = self.0.graph.read();
        let mut seeds = g.seeds(turn_text);
        seeds.extend_from_slice(extra_seeds);
        let act = g.spread(&seeds, &params);
        let active = g.rank(&act, unix_now(), &params);
        g.curator_block_at(&active, story, self.lang())
    }

    pub fn apply_ops(&self, batch: &OpBatch) -> ApplyReport {
        self.apply_ops_at(batch, STORY_PRESENT)
    }

    /// Applies the curator's operations as learned at story time `story`: new nodes and
    /// links are dated then, and `close_edge` ends a link then.
    pub fn apply_ops_at(&self, batch: &OpBatch, story: StoryTime) -> ApplyReport {
        let roster = self.0.roster.read().clone();
        let report = self.0.graph.write().apply_ops_at(batch, &self.0.config.limits, &roster, unix_now(), story);
        if report.applied > 0 {
            self.mark_dirty();
        }
        report
    }

    /// Arbitrary read access.
    pub fn read<R>(&self, f: impl FnOnce(&MemoryGraph) -> R) -> R {
        f(&self.0.graph.read())
    }

    /// Manual editing (CRUD from the UI). Marks the graph as dirty.
    pub fn write<R>(&self, f: impl FnOnce(&mut MemoryGraph) -> R) -> R {
        let r = f(&mut self.0.graph.write());
        self.mark_dirty();
        r
    }

    pub fn view(&self) -> GraphView {
        let heat = self.0.last_heat.read();
        self.0.graph.read().view(&heat)
    }

    pub fn detail(&self, id: NodeId) -> Option<NodeDetail> {
        self.0.graph.read().detail(id)
    }

    pub fn is_dirty(&self) -> bool {
        self.0.dirty.load(Ordering::Acquire)
    }

    fn mark_dirty(&self) {
        self.0.dirty.store(true, Ordering::Release);
    }

    /// Serialises under the lock (fast: it only copies to bytes) and writes to disk outside it.
    pub fn save(&self) -> Result<(), PersistError> {
        let _guard = self.0.save_lock.lock();
        // Cleared before serialising: if someone writes while we save, it becomes dirty again.
        self.0.dirty.store(false, Ordering::Release);
        let bytes = {
            let mut g = self.0.graph.write();
            g.meta.updated_at = unix_now();
            g.to_bytes()
        };
        let result = bytes.and_then(|b| write_atomic(&self.0.path, &b));
        if result.is_err() {
            self.mark_dirty();
        }
        result
    }

    pub fn save_if_dirty(&self) -> Result<bool, PersistError> {
        if self.is_dirty() {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
