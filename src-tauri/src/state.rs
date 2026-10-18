//! Shared app state: paths, world library, model supervisor, LLM clients, one
//! background curator per character, and the current language.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use i18n::{Lang, Settings, tr};
use memory_core::layout::{APP_CONFIG, LOGS_DIR, MODELS_CONFIG, WORLDS_DIR};
use memory_core::conversation::Conversation;
use memory_core::llm::{Backend, ConsolidationWorker, LlmClient, LlmEndpoint};
use memory_core::collab::Collab;
use memory_core::world::{Library, World};
use memory_core::{GraphId, HandlerConfig, MemoryHandler};
use model_hub::{HubConfig, Slot, Supervisor};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub type Res<T> = Result<T, String>;

/// Language of the app (`Lang::index`), readable from anywhere without the state.
static LANG: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    Lang::from_index(LANG.load(Ordering::Relaxed) as usize).unwrap_or_default()
}

pub struct AppState {
    pub home: PathBuf,
    pub lib: Library,
    pub sup: Arc<Supervisor>,
    worlds: Mutex<HashMap<String, Arc<World>>>,
    /// Like the worlds, never emptied (by slug).
    collabs: Mutex<HashMap<String, Arc<Collab>>>,
    clients: tokio::sync::Mutex<Option<(LlmClient, LlmClient)>>,
    workers: Mutex<HashMap<GraphId, ConsolidationWorker>>,
    /// A single curator at a time: the actor always keeps a slot of the server.
    gate: Arc<tokio::sync::Semaphore>,
    /// One actor turn at a time (there is one GPU).
    pub turn_lock: tokio::sync::Mutex<()>,
    /// Last context sent to the actor in each conversation (for the debug panel).
    pub last_context: Mutex<HashMap<GraphId, String>>,
}

/// Finds the project folder: `CHATS_HOME`, or the first folder (walking up from the
/// current directory and from the executable) that has `worlds/`, `models/` or `config/`
/// (or the old `mundos/`).
pub fn find_home() -> PathBuf {
    if let Some(h) = std::env::var_os("CHATS_HOME") {
        return PathBuf::from(h);
    }
    let marker = |d: &Path| [WORLDS_DIR, "mundos", "models", "config"].iter().any(|m| d.join(m).is_dir());
    let starts = [std::env::current_dir().ok(), std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))];
    for start in starts.into_iter().flatten() {
        let mut dir: Option<&Path> = Some(&start);
        while let Some(d) = dir {
            if marker(d) {
                return d.to_path_buf();
            }
            dir = d.parent();
        }
    }
    std::env::current_dir().unwrap_or_default()
}

#[derive(Clone, Serialize)]
pub struct StatusEvent {
    pub text: String,
    pub busy: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEvent {
    pub world: String,
    pub graph: String,
    pub name: String,
    pub applied: usize,
    pub rejected: usize,
    pub error: Option<String>,
}

pub fn emit_status(app: &AppHandle, text: impl Into<String>, busy: bool) {
    let _ = app.emit("status", StatusEvent { text: text.into(), busy });
}

impl AppState {
    pub fn new(home: PathBuf) -> Res<Self> {
        // Folders with the old Spanish names are renamed before anything reads them.
        for (from, to) in memory_core::layout::migrate_legacy(&home) {
            eprintln!("moved {} → {}", from.display(), to.display());
        }
        let language = i18n::resolve(&home.join(APP_CONFIG));
        LANG.store(language.index() as u8, Ordering::Relaxed);
        let mut cfg = HubConfig::load_or_create(&home.join(MODELS_CONFIG)).map_err(|e| e.to_string())?;
        if cfg.models_dir.is_relative() {
            cfg.models_dir = home.join(&cfg.models_dir);
        }
        let sup = Arc::new(Supervisor::new(cfg, home.join(LOGS_DIR)));
        Ok(Self {
            lib: Library::new(home.join(WORLDS_DIR), HandlerConfig::with_lang(language)),
            home,
            sup,
            worlds: Mutex::new(HashMap::new()),
            collabs: Mutex::new(HashMap::new()),
            clients: tokio::sync::Mutex::new(None),
            workers: Mutex::new(HashMap::new()),
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
            turn_lock: tokio::sync::Mutex::new(()),
            last_context: Mutex::new(HashMap::new()),
        })
    }

    /// Changes the language of the app, the prompts and the open worlds, and saves it
    /// in `config/app.toml`.
    pub fn set_lang(&self, language: Lang) -> Res<()> {
        LANG.store(language.index() as u8, Ordering::Relaxed);
        let path = self.home.join(APP_CONFIG);
        let mut settings = Settings::load(&path);
        settings.language = language;
        settings.save(&path).map_err(|e| e.to_string())?;
        for w in self.worlds.lock().values() {
            w.set_lang(language);
        }
        for c in self.collabs.lock().values() {
            c.set_lang(language);
        }
        Ok(())
    }

    /// Opens (or returns from the cache) the world `category/slug`.
    pub fn world(&self, key: &str) -> Res<Arc<World>> {
        if let Some(w) = self.worlds.lock().get(key) {
            return Ok(w.clone());
        }
        let (cat, slug) = key.split_once('/').ok_or_else(|| tr!(lang(), "app.bad_world", key = key))?;
        let w = Arc::new(self.lib.open_world(cat, slug).map_err(|e| e.to_string())?);
        w.set_lang(lang());
        self.worlds.lock().insert(key.to_owned(), w.clone());
        Ok(w)
    }

    /// Opens (or returns from the cache) the Collab `slug`.
    pub fn collab(&self, slug: &str) -> Res<Arc<Collab>> {
        if let Some(c) = self.collabs.lock().get(slug) {
            return Ok(c.clone());
        }
        let c = Arc::new(self.lib.open_collab(slug).map_err(|e| e.to_string())?);
        c.set_lang(lang());
        self.collabs.lock().insert(slug.to_owned(), c.clone());
        Ok(c)
    }

    /// Adds a new Collab to the cache.
    pub fn insert_collab(&self, c: Collab) -> Arc<Collab> {
        let c = Arc::new(c);
        c.set_lang(lang());
        self.collabs.lock().insert(c.slug.clone(), c.clone());
        c
    }

    /// Where chats happen: a world (`category/slug`) or a Collab (`collab:<slug>`).
    pub fn place(&self, key: &str) -> Res<Place> {
        match key.strip_prefix(COLLAB_PREFIX) {
            Some(slug) => {
                let c = self.collab(slug)?;
                // A linked world that no longer exists is left out.
                let worlds = c.links().into_iter().filter_map(|l| self.world(&l.world).ok().map(|w| (l.world, w))).collect();
                Ok(Place::Collab(c, worlds))
            }
            None => Ok(Place::World(self.world(key)?)),
        }
    }

    /// The graph `graph` of a place. In a Collab, `"world"` is its bridge memory and a
    /// chat's character key is their Carbon Copy there.
    pub fn place_graph(&self, place: &Place, graph: &str) -> Res<MemoryHandler> {
        let c = match place {
            Place::World(w) => return self.graph(w, graph),
            Place::Collab(c, _) => c,
        };
        let refs = place.world_refs();
        if graph == "world" {
            Ok(c.memory.clone())
        } else if let Some(rest) = graph.strip_prefix("chat:") {
            if let Some((chat_s, char_s)) = rest.split_once(':') {
                c.speaker_copy(parse_id(chat_s)?, parse_id(char_s)?, &refs).map_err(|e| e.to_string())
            } else {
                let conv = Conversation::load_id(&c.dir, parse_id(rest)?).map_err(|e| e.to_string())?;
                c.chat_memory(&conv, &refs).map_err(|e| e.to_string())
            }
        } else if graph.starts_with("lib:") {
            Err(tr!(lang(), "app.no_library"))
        } else {
            place.character(parse_id(graph)?)
        }
    }

    /// `"world"` = the world's memory; `"chat:<id>"` = a conversation's memory;
    /// `"chat:<id>:<char_id>"` = a character's Carbon Copy in that conversation; anything
    /// else = a character id.
    pub fn graph(&self, world: &World, graph: &str) -> Res<MemoryHandler> {
        if graph == "world" {
            Ok(world.memory.clone())
        } else if let Some(level) = graph.strip_prefix("lib:") {
            // A library of shared definitions: created (empty) the first time it is opened.
            let level = match level {
                "universal" => memory_core::world::DefinitionLevel::Universal,
                _ => memory_core::world::DefinitionLevel::Category,
            };
            world.definition_library(level, true).map_err(|e| e.to_string())?.ok_or_else(|| tr!(lang(), "app.no_library"))
        } else if let Some(rest) = graph.strip_prefix("chat:") {
            if let Some((chat_s, char_s)) = rest.split_once(':') {
                world.chat_character(parse_id(chat_s)?, parse_id(char_s)?).map_err(|e| e.to_string())
            } else {
                let conv = Conversation::load_id(&world.dir, parse_id(rest)?).map_err(|e| e.to_string())?;
                world.chat_memory(&conv).map_err(|e| e.to_string())
            }
        } else {
            world.character(parse_id(graph)?).map_err(|e| e.to_string())
        }
    }

    /// Loads the models if needed and returns (actor, curator).
    pub async fn clients(&self, app: &AppHandle) -> Res<(LlmClient, LlmClient)> {
        let mut guard = self.clients.lock().await;
        let alive = self.sup.status().await.iter().any(|s| s.slot == Slot::Actor && s.running);
        if let (Some(c), true) = (guard.as_ref(), alive) {
            return Ok(c.clone());
        }
        emit_status(app, tr!(lang(), "models.loading"), true);
        let actor_url = self.sup.ensure(Slot::Actor).await.map_err(|e| e.to_string())?;
        let curator_url = self.sup.ensure(Slot::Curator).await.map_err(|e| e.to_string())?;
        let alias = |s: Slot| self.sup.spec(s).map(|s| s.alias).unwrap_or_default();
        let actor = LlmClient::new(LlmEndpoint::actor(Backend::LlamaServer, actor_url, alias(Slot::Actor))).map_err(|e| e.to_string())?;
        let curator =
            LlmClient::new(LlmEndpoint::curator(Backend::LlamaServer, curator_url, alias(Slot::Curator))).map_err(|e| e.to_string())?;
        *guard = Some((actor.clone(), curator.clone()));
        emit_status(app, tr!(lang(), "models.ready"), false);
        Ok((actor, curator))
    }

    /// The background curator of a character or a chat memory (created the first time).
    pub fn submit_to_curator(
        &self,
        app: &AppHandle,
        world_key: &str,
        graph_key: &str,
        h: &MemoryHandler,
        curator: &LlmClient,
        turn: memory_core::llm::TurnRecord,
    ) {
        let mut workers = self.workers.lock();
        let worker = workers.entry(h.id()).or_insert_with(|| {
            let app = app.clone();
            let graph = graph_key.to_owned();
            let (world, name) = (world_key.to_owned(), h.log_label());
            let log = self.home.join(memory_core::layout::LOGS_DIR).join(memory_core::layout::CURATOR_LOG);
            let (w, _join) = ConsolidationWorker::spawn_gated(h.clone(), curator.clone(), Some(self.gate.clone()), move |r| {
                memory_core::llm::log_curator_result(&log, lang(), &format!("{world} · {name}"), &r);
                let ev = match r {
                    Ok(rep) => MemoryEvent {
                        world: world.clone(),
                        graph: graph.clone(),
                        name: name.clone(),
                        applied: rep.applied,
                        rejected: rep.rejected.len(),
                        error: None,
                    },
                    Err(e) => MemoryEvent {
                        world: world.clone(),
                        graph: graph.clone(),
                        name: name.clone(),
                        applied: 0,
                        rejected: 0,
                        error: Some(e.to_string()),
                    },
                };
                let _ = app.emit("memory-updated", ev);
            });
            w
        });
        if !worker.submit(turn) {
            emit_status(app, tr!(lang(), "chat.queue_full"), false);
        }
    }
}

/// Key prefix of a Collab as a place (`collab:<slug>`).
pub const COLLAB_PREFIX: &str = "collab:";

/// Where a chat happens.
pub enum Place {
    World(Arc<World>),
    /// A Collab and its linked worlds (`category/slug`, world).
    Collab(Arc<Collab>, Vec<(String, Arc<World>)>),
}

impl Place {
    /// The folder its `chats/` live in.
    pub fn dir(&self) -> &Path {
        match self {
            Place::World(w) => &w.dir,
            Place::Collab(c, _) => &c.dir,
        }
    }

    /// Its worlds, as `Collab` takes them.
    pub fn world_refs(&self) -> Vec<(&str, &World)> {
        match self {
            Place::World(w) => vec![("", w.as_ref())],
            Place::Collab(_, worlds) => worlds.iter().map(|(k, w)| (k.as_str(), w.as_ref())).collect(),
        }
    }

    /// The world a character belongs to here.
    pub fn home_of(&self, id: GraphId) -> Res<&World> {
        match self {
            Place::World(w) => w.character(id).map(|_| w.as_ref()).map_err(|e| e.to_string()),
            Place::Collab(_, worlds) => {
                worlds.iter().map(|(_, w)| w.as_ref()).find(|w| w.character(id).is_ok()).ok_or_else(|| memory_core::WorldError::NoSuchCharacter(id).to_string())
            }
        }
    }

    pub fn character(&self, id: GraphId) -> Res<MemoryHandler> {
        self.home_of(id)?.character(id).map_err(|e| e.to_string())
    }

    pub fn chat_memory(&self, conv: &Conversation) -> Res<MemoryHandler> {
        match self {
            Place::World(w) => w.chat_memory(conv),
            Place::Collab(c, _) => c.chat_memory(conv, &self.world_refs()),
        }
        .map_err(|e| e.to_string())
    }

    /// A participant's Carbon Copy in a chat.
    pub fn chat_character(&self, chat: GraphId, character: GraphId) -> Res<MemoryHandler> {
        match self {
            Place::World(w) => w.chat_character(chat, character),
            Place::Collab(c, _) => c.speaker_copy(chat, character, &self.world_refs()),
        }
        .map_err(|e| e.to_string())
    }

    /// Story time of what a chat's memory learns: the chat's moment, or timeless in a Collab.
    pub fn chat_story(&self, moment: Option<u32>) -> memory_core::StoryTime {
        match self {
            Place::World(w) => w.story_now(moment),
            Place::Collab(..) => memory_core::STORY_PRESENT,
        }
    }

    /// Story time of what a character learns in a chat: in a Collab, the moment it sets
    /// for their world.
    pub fn story_of(&self, character: GraphId, moment: Option<u32>) -> memory_core::StoryTime {
        match self {
            Place::World(w) => w.story_now(moment),
            Place::Collab(c, worlds) => worlds
                .iter()
                .find(|(_, w)| w.character(character).is_ok())
                .map_or(memory_core::STORY_PRESENT, |(k, w)| w.story_now(c.moment_of(k))),
        }
    }

    pub fn rename_chat_memory(&self, chat: GraphId, title: &str) -> Res<()> {
        match self {
            Place::World(w) => w.rename_chat_memory(chat, title),
            Place::Collab(c, _) => c.rename_chat_memory(chat, title),
        }
        .map_err(|e| e.to_string())
    }

    pub fn delete_chat_memory(&self, chat: GraphId) -> Res<()> {
        match self {
            Place::World(w) => w.delete_chat_memory(chat),
            Place::Collab(c, _) => c.delete_chat_memory(chat),
        }
        .map_err(|e| e.to_string())
    }
}

pub fn parse_id(s: &str) -> Res<GraphId> {
    s.trim().parse::<u64>().map_err(|_| tr!(lang(), "app.bad_id", id = s))
}
