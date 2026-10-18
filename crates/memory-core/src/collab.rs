//! Collabs: places where several worlds meet.
//!
//! ```text
//! worlds/_collabs/<collab>/
//! ├─ collab.mem                 the bridge memory (Scope::Collab): the premise of the
//! │                             meeting, what happens there, who has met whom
//! ├─ collab.toml                which worlds it links and at which moment of each
//! ├─ style.toml                 how its chats look
//! └─ chats/<id>.chat, <id>.mem  its conversations, their memories and the participants'
//!                               Carbon Copies (`<chat>-<character>.mem`)
//! ```
//!
//! A Collab is always isolated: its characters speak and learn from copies of their
//! memories made for each chat, so nothing that happens here reaches their own worlds.
//! Each speaker stays in their own world at the moment the Collab sets for it; people
//! from other worlds are recognised only through what the speaker, the Collab or the
//! speaker's world know of them (so a legend of one world can be a stranger to another).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::conversation::Conversation;
use crate::graph::MemoryGraph;
use crate::handler::{HandlerConfig, MemoryHandler};
use crate::model::{EdgeKind, ExternalRef, GraphId, GraphMeta, MemoryNode, NodeKind, Scope, unix_now};
use crate::ops::Roster;
use crate::style::WorldStyle;
use crate::world::{Bridge, Library, SceneRecall, World, WorldError, slugify};

pub const COLLABS_DIR: &str = "_collabs";
pub const COLLAB_FILE: &str = "collab.mem";
pub const COLLAB_CONFIG: &str = "collab.toml";

/// A world linked by a Collab, and the moment of its timeline the Collab happens at.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CollabLink {
    /// `category/world` (the key of the world in the app).
    pub world: String,
    /// `None` = the latest moment of that world.
    #[serde(default)]
    pub moment: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
struct CollabConfig {
    #[serde(default)]
    links: Vec<CollabLink>,
}

pub struct Collab {
    pub slug: String,
    pub dir: PathBuf,
    /// The bridge memory.
    pub memory: MemoryHandler,
    links: RwLock<Vec<CollabLink>>,
    chats: RwLock<HashMap<GraphId, MemoryHandler>>,
    copies: RwLock<HashMap<(GraphId, GraphId), MemoryHandler>>,
    config: HandlerConfig,
}

impl Library {
    pub fn collabs_dir(&self) -> PathBuf {
        self.root().join(COLLABS_DIR)
    }

    /// Every Collab: (slug, metadata of its bridge memory), by name.
    pub fn collabs(&self) -> Vec<(String, GraphMeta)> {
        let Ok(rd) = std::fs::read_dir(self.collabs_dir()) else { return Vec::new() };
        let mut out: Vec<(String, GraphMeta)> = rd
            .flatten()
            .filter_map(|e| {
                let file = e.path().join(COLLAB_FILE);
                let meta = crate::persist::peek_meta(&file).ok()?.0;
                Some((e.file_name().to_string_lossy().into_owned(), meta))
            })
            .collect();
        out.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        out
    }

    /// Creates `worlds/_collabs/<slug>/` with its bridge memory. `premise` is how the worlds
    /// meet ("a rift opens between Terra and Talos-II").
    pub fn create_collab(&self, name: &str, premise: &str, links: Vec<CollabLink>) -> Result<Collab, WorldError> {
        let slug = slugify(name);
        if slug.is_empty() {
            return Err(WorldError::BadName);
        }
        let dir = self.collabs_dir().join(&slug);
        if dir.join(COLLAB_FILE).exists() {
            return Err(WorldError::AlreadyExists(format!("the collab {slug}")));
        }
        std::fs::create_dir_all(&dir)?;
        let now = unix_now();
        let graph = MemoryGraph::new(GraphMeta::new(Scope::Collab, name.trim(), now), premise.trim(), now);
        let memory = MemoryHandler::new(graph, dir.join(COLLAB_FILE), self.config().clone());
        memory.save()?;
        let collab = Collab::from_parts(slug, dir, memory, Vec::new(), self.config().clone());
        collab.set_links(links)?;
        Ok(collab)
    }

    pub fn open_collab(&self, slug: &str) -> Result<Collab, WorldError> {
        let dir = self.collabs_dir().join(slug);
        let memory = MemoryHandler::open(dir.join(COLLAB_FILE), self.config().clone())?;
        let links = std::fs::read_to_string(dir.join(COLLAB_CONFIG))
            .ok()
            .and_then(|s| toml::from_str::<CollabConfig>(&s).ok())
            .unwrap_or_default()
            .links;
        Ok(Collab::from_parts(slug.to_owned(), dir, memory, links, self.config().clone()))
    }
}

impl Collab {
    fn from_parts(slug: String, dir: PathBuf, memory: MemoryHandler, links: Vec<CollabLink>, config: HandlerConfig) -> Self {
        Self { slug, dir, memory, links: RwLock::new(links), chats: RwLock::new(HashMap::new()), copies: RwLock::new(HashMap::new()), config }
    }

    pub fn id(&self) -> GraphId {
        self.memory.id()
    }

    pub fn links(&self) -> Vec<CollabLink> {
        self.links.read().clone()
    }

    /// Which worlds it links, at which moment (`collab.toml`). Duplicates are dropped.
    pub fn set_links(&self, mut links: Vec<CollabLink>) -> Result<(), WorldError> {
        let mut seen = std::collections::HashSet::new();
        links.retain(|l| seen.insert(l.world.clone()));
        let text = toml::to_string(&CollabConfig { links: links.clone() }).map_err(std::io::Error::other)?;
        let tmp = self.dir.join(format!("{COLLAB_CONFIG}.tmp"));
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, self.dir.join(COLLAB_CONFIG))?;
        *self.links.write() = links;
        Ok(())
    }

    pub fn style(&self) -> WorldStyle {
        WorldStyle::load(&self.dir)
    }

    pub fn set_style(&self, style: &WorldStyle) -> Result<(), WorldError> {
        style.save(&self.dir)?;
        Ok(())
    }

    /// The moment the Collab sets for `world` (its key), if any.
    pub fn moment_of(&self, world: &str) -> Option<u32> {
        self.links.read().iter().find(|l| l.world == world).and_then(|l| l.moment)
    }

    /// The linked world `id` belongs to, with its key.
    pub fn world_of<'w>(worlds: &[(&str, &'w World)], id: GraphId) -> Option<(&'w World, String)> {
        worlds.iter().find(|(_, w)| w.character(id).is_ok()).map(|(k, w)| (*w, (*k).to_owned()))
    }

    fn roster(worlds: &[(&str, &World)]) -> Roster {
        worlds.iter().flat_map(|(_, w)| w.roster()).collect()
    }

    /// The memory of one of its conversations: opened, or created with a node for each
    /// participant (linked to their sheet, whatever their world) and one for the user.
    pub fn chat_memory(&self, conv: &Conversation, worlds: &[(&str, &World)]) -> Result<MemoryHandler, WorldError> {
        if let Some(h) = self.chats.read().get(&conv.id) {
            return Ok(h.clone());
        }
        let mut cache = self.chats.write();
        if let Some(h) = cache.get(&conv.id) {
            return Ok(h.clone());
        }
        let path = Conversation::memory_path(&self.dir, conv.id);
        let h = if path.is_file() {
            MemoryHandler::open(&path, self.config.clone())?
        } else {
            let now = unix_now();
            let mut meta = GraphMeta::new(Scope::Chat, conv.title.trim(), now);
            meta.id = conv.id;
            let mut g = MemoryGraph::new(meta, "", now);
            let root = g.root();
            for &p in &conv.participants {
                let Some((w, _)) = Self::world_of(worlds, p) else { continue };
                let m = w.character(p)?.meta();
                let n = g.add_node(MemoryNode::new(NodeKind::Character, m.name, m.tagline, now).with_weight(0.6))?;
                g.set_external(n, Some(ExternalRef { scope: Scope::Character, id: p }))?;
                g.connect(root, n, EdgeKind::Knows, 0.6, now)?;
            }
            let lang = self.memory.lang();
            let user = g.add_node(MemoryNode::new(NodeKind::Entity, conv.user_name.trim(), i18n::t(lang, "ctx.chat.user_node"), now).with_weight(0.6))?;
            g.connect(root, user, EdgeKind::Knows, 0.6, now)?;
            let h = MemoryHandler::new(g, path, self.config.clone());
            h.save()?;
            h
        };
        h.set_lang(self.memory.lang());
        h.set_roster(Self::roster(worlds));
        cache.insert(conv.id, h.clone());
        Ok(h)
    }

    /// A participant's Carbon Copy inside one of its chats: their memory as it was when the
    /// chat started; what they learn here stays here.
    pub fn speaker_copy(&self, chat: GraphId, character: GraphId, worlds: &[(&str, &World)]) -> Result<MemoryHandler, WorldError> {
        let key = (chat, character);
        if let Some(h) = self.copies.read().get(&key) {
            return Ok(h.clone());
        }
        let (w, _) = Self::world_of(worlds, character).ok_or(WorldError::NoSuchCharacter(character))?;
        let mut cache = self.copies.write();
        if let Some(h) = cache.get(&key) {
            return Ok(h.clone());
        }
        let path = Conversation::character_memory_path(&self.dir, chat, character);
        let h = if path.is_file() {
            MemoryHandler::open(&path, self.config.clone())?
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut g = w.character(character)?.read(|g| g.clone());
            // Its own id, so its curator worker never mixes with the original's.
            g.meta.id = chat.rotate_left(29) ^ character ^ self.id();
            let h = MemoryHandler::new(g, path, self.config.clone());
            h.save()?;
            h
        };
        h.set_lang(w.lang());
        h.set_roster(Self::roster(worlds));
        cache.insert(key, h.clone());
        Ok(h)
    }

    /// Scene recall for `speaker` in one of its chats (or outside any, with `chat` = None).
    /// The speaker stays in their own world, at the moment the Collab sets for it.
    pub fn scene_recall(&self, worlds: &[(&str, &World)], speaker: GraphId, prompt: &str, present: &[GraphId], chat: Option<&MemoryHandler>) -> Result<SceneRecall, WorldError> {
        let (home, key) = Self::world_of(worlds, speaker).ok_or(WorldError::NoSuchCharacter(speaker))?;
        let story = home.story_now(self.moment_of(&key));
        let foreign: Vec<(GraphId, String)> = present
            .iter()
            .filter(|&&p| p != speaker && home.character(p).is_err())
            .filter_map(|&p| Self::world_of(worlds, p).and_then(|(w, _)| w.character(p).ok()).map(|h| (p, h.meta().name)))
            .collect();
        let mut libraries: Vec<MemoryHandler> = Vec::new();
        for (_, w) in worlds {
            for l in w.definitions() {
                if !libraries.iter().any(|x| x.path() == l.path()) {
                    libraries.push(l);
                }
            }
        }
        let speaker_memory = match chat {
            Some(c) => self.speaker_copy(c.id(), speaker, worlds)?,
            None => home.character(speaker)?,
        };
        self.memory.set_roster(Self::roster(worlds));
        home.scene_recall_bridged(speaker, prompt, present, chat, story, Some(Bridge { memory: &self.memory, speaker_memory, foreign, libraries }))
    }

    /// Renames a chat's memory (its root follows the conversation's title).
    pub fn rename_chat_memory(&self, chat: GraphId, title: &str) -> Result<(), WorldError> {
        if let Some(h) = self.chats.read().get(&chat).cloned() {
            h.write(|g| -> Result<(), crate::graph::GraphError> {
                g.meta.name = title.trim().to_owned();
                let root = g.root();
                g.update_node(root, crate::graph::NodePatch { label: Some(title.trim().to_owned()), ..Default::default() })
            })?;
            h.save()?;
        }
        Ok(())
    }

    /// Deletes a chat's memory and its Carbon Copies.
    pub fn delete_chat_memory(&self, chat: GraphId) -> Result<(), WorldError> {
        self.chats.write().remove(&chat);
        self.copies.write().retain(|&(c, _), _| c != chat);
        let chats = self.dir.join(crate::conversation::CHATS_DIR);
        let prefix = format!("{chat:016x}");
        if let Ok(rd) = std::fs::read_dir(&chats) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(&prefix) && name.ends_with(".mem") {
                    std::fs::remove_file(e.path())?;
                }
            }
        }
        Ok(())
    }

    pub fn set_lang(&self, lang: i18n::Lang) {
        self.memory.set_lang(lang);
        for h in self.chats.read().values().chain(self.copies.read().values()) {
            h.set_lang(lang);
        }
    }

    pub fn save_all(&self) -> Result<(), WorldError> {
        self.memory.save_if_dirty()?;
        for h in self.chats.read().values().chain(self.copies.read().values()) {
            h.save_if_dirty()?;
        }
        Ok(())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}
