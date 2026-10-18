//! Worlds, categories and relationships between characters.
//!
//! ```text
//! worlds/
//! └─ <category>/                      e.g. "fantasy", "science-fiction"
//!    └─ <world>/
//!       ├─ world.mem                  the world's memory (Scope::World): rules, places,
//!       │                             factions, history and each character's PUBLIC sheet
//!       ├─ characters/<name>.mem      each character's private memory (Scope::Character)
//!       ├─ images/                    generated portraits and scenes
//!       ├─ references/                reference images for each character's appearance
//!       └─ chats/<id>.chat, <id>.mem  conversations and each chat's own memory (Scope::Chat)
//! ```
//!
//! Each character keeps their **own view** of the others (a `Character` node with an
//! `ExternalRef`): Lyra can adore Kael while Kael distrusts her. Private memories never
//! mix: in a scene the actor only gets the speaker's memory + what the world knows
//! publicly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use i18n::{Lang, tr};
use parking_lot::RwLock;

use crate::context::ContextBudget;
use crate::graph::{GraphError, MemoryGraph};
use crate::handler::{HandlerConfig, MemoryHandler, Recall};
use crate::conversation::Conversation;
use crate::model::{
    CharacterMoment, EdgeKind, ExternalRef, GraphId, GraphMeta, MemoryNode, Moment, NodeId, NodeKind, STORY_PRESENT, Scope, StoryTime, unix_now,
};
use crate::ops::Roster;
use crate::persist::{PersistError, peek_meta};
use crate::text::fold;

pub const WORLD_FILE: &str = "world.mem";
/// Definitions shared by every world (`worlds/_universal.mem`).
pub const UNIVERSAL_FILE: &str = "_universal.mem";
/// Definitions shared by the worlds of one category (`worlds/<category>/_category.mem`).
pub const CATEGORY_FILE: &str = "_category.mem";

/// Which library of shared definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionLevel {
    /// For every world: colours, materials, basic ideas.
    Universal,
    /// For the worlds of one category (Terra and Talos-II share "arknights").
    #[default]
    Category,
}
pub const CHARACTERS_DIR: &str = "characters";
pub const IMAGES_DIR: &str = "images";
pub const REFERENCES_DIR: &str = "references";

/// Activation seeds: (node, strength).
type Seeds = Vec<(NodeId, f32)>;

/// A visual aspect: "Hair" → "short, white with green highlights".
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Aspect {
    pub label: String,
    pub content: String,
    pub weight: f32,
}

/// A character's canonical appearance and how the others see them.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppearanceReport {
    pub canonical: Vec<Aspect>,
    pub seen_by: Vec<(String, Vec<Aspect>)>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    #[error(transparent)]
    Persist(#[from] PersistError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error("character {0:016x} does not exist in this world")]
    NoSuchCharacter(GraphId),
    #[error("{0} already exists")]
    AlreadyExists(String),
    #[error("empty or invalid name")]
    BadName,
    #[error("moment {0} is not in this world's timeline")]
    NoSuchMoment(u32),
}

/// Cuts at the last space before `max` characters and adds "…".
fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches([',', ';', '.', ':']))
}

/// Minimum importance for an aspect to show up in portraits without the scene asking for it.
pub const PORTRAIT_MIN_WEIGHT: f32 = 0.5;

fn appearance_prompt_from(g: &MemoryGraph, owner: NodeId, folded_scene: &str) -> String {
    g.appearance_of(owner)
        .iter()
        .filter(|(_, n)| n.weight >= PORTRAIT_MIN_WEIGHT || (!folded_scene.is_empty() && folded_scene.contains(&fold(n.label.trim()))))
        .map(|(_, n)| n.content.trim().trim_end_matches('.').to_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Folder/file name: lowercase, no accents, hyphens.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in fold(name.trim()).chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Gallery entry (read with `peek_meta`, without loading graphs).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    pub category: String,
    pub slug: String,
    pub path: PathBuf,
    pub meta: GraphMeta,
    pub nodes: usize,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Listing {
    pub items: Vec<Entry>,
    /// Files that could not be read (path, reason): the gallery still shows the rest.
    pub broken: Vec<(PathBuf, String)>,
}

/// Root of all the worlds. It also owns the libraries of shared definitions: one handler
/// per file, shared by every clone (two worlds of a category must never hold two handlers
/// of the same `_category.mem`: they would overwrite each other).
#[derive(Clone)]
pub struct Library {
    root: PathBuf,
    config: HandlerConfig,
    shared: std::sync::Arc<RwLock<HashMap<PathBuf, MemoryHandler>>>,
}

impl Library {
    pub fn new(root: impl Into<PathBuf>, config: HandlerConfig) -> Self {
        Self { root: root.into(), config, shared: Default::default() }
    }

    /// A library of shared definitions: the universal one, or `category`'s. Opened once and
    /// cached; with `create`, made (empty) if it does not exist yet. `None` if it does not
    /// exist and `create` is false.
    pub fn definitions(&self, level: DefinitionLevel, category: &str, create: bool) -> Result<Option<MemoryHandler>, WorldError> {
        let path = match level {
            DefinitionLevel::Universal => self.root.join(UNIVERSAL_FILE),
            DefinitionLevel::Category => {
                let cat = slugify(category);
                if cat.is_empty() {
                    return Err(WorldError::BadName);
                }
                self.root.join(cat).join(CATEGORY_FILE)
            }
        };
        if let Some(h) = self.shared.read().get(&path) {
            return Ok(Some(h.clone()));
        }
        let mut cache = self.shared.write();
        if let Some(h) = cache.get(&path) {
            return Ok(Some(h.clone()));
        }
        let h = if path.is_file() {
            MemoryHandler::open(&path, self.config.clone())?
        } else if create {
            let lang = self.config.lang;
            let (name, about) = match level {
                DefinitionLevel::Universal => (i18n::t(lang, "library.universal").to_owned(), i18n::t(lang, "library.universal_about").to_owned()),
                DefinitionLevel::Category => {
                    // The folder is a slug ("arknights"): its readable name is "Arknights".
                    let name = category
                        .split(['-', '_', ' '])
                        .filter(|w| !w.is_empty())
                        .map(|w| {
                            let mut c = w.chars();
                            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    let about = tr!(lang, "library.category_about", category = name);
                    (name, about)
                }
            };
            let now = unix_now();
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let h = MemoryHandler::new(MemoryGraph::new(GraphMeta::new(Scope::Library, name, now), about, now), &path, self.config.clone());
            h.save()?;
            h
        } else {
            return Ok(None);
        };
        cache.insert(path, h.clone());
        Ok(Some(h))
    }

    /// The libraries a world of `category` uses, most specific first (category, universal).
    pub fn definitions_for(&self, category: &str) -> Vec<MemoryHandler> {
        [DefinitionLevel::Category, DefinitionLevel::Universal]
            .into_iter()
            .filter_map(|l| self.definitions(l, category, false).ok().flatten())
            .collect()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn lang(&self) -> Lang {
        self.config.lang
    }

    pub fn config(&self) -> &HandlerConfig {
        &self.config
    }

    /// Category folders (`_collabs` and other `_` names are not categories).
    pub fn categories(&self) -> std::io::Result<Vec<String>> {
        let mut out = Vec::new();
        if self.root.is_dir() {
            for e in std::fs::read_dir(&self.root)? {
                let e = e?;
                let name = e.file_name().to_string_lossy().into_owned();
                if e.file_type()?.is_dir() && !name.starts_with('_') {
                    out.push(name);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Every world of every category.
    pub fn worlds(&self) -> std::io::Result<Listing> {
        let mut listing = Listing::default();
        for category in self.categories()? {
            for e in std::fs::read_dir(self.root.join(&category))? {
                let dir = e?.path();
                let file = dir.join(WORLD_FILE);
                if !file.is_file() {
                    continue;
                }
                match peek_meta(&file) {
                    Ok((meta, nodes, _)) => listing.items.push(Entry {
                        category: category.clone(),
                        slug: dir.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        path: file,
                        meta,
                        nodes,
                    }),
                    Err(err) => listing.broken.push((file, err.to_string())),
                }
            }
        }
        listing.items.sort_by(|a, b| (&a.category, &a.meta.name).cmp(&(&b.category, &b.meta.name)));
        Ok(listing)
    }

    /// Creates `worlds/<category>/<world>/world.mem`. `premise` is the world's root (tone,
    /// era, premise); `scenario`, the style instructions for the actor.
    pub fn create_world(&self, category: &str, name: &str, premise: &str, scenario: &str) -> Result<World, WorldError> {
        let (cat, slug) = (slugify(category), slugify(name));
        if cat.is_empty() || slug.is_empty() {
            return Err(WorldError::BadName);
        }
        let dir = self.root.join(&cat).join(&slug);
        if dir.join(WORLD_FILE).exists() {
            return Err(WorldError::AlreadyExists(format!("the world {cat}/{slug}")));
        }
        std::fs::create_dir_all(dir.join(CHARACTERS_DIR))?;
        std::fs::create_dir_all(dir.join(IMAGES_DIR))?;
        let now = unix_now();
        let mut meta = GraphMeta::new(Scope::World, name.trim(), now);
        meta.scenario = scenario.trim().to_owned();
        let graph = MemoryGraph::new(meta, premise.trim(), now);
        let memory = MemoryHandler::new(graph, dir.join(WORLD_FILE), self.config.clone());
        memory.save()?;
        Ok(World {
            category: cat,
            dir,
            memory,
            characters: RwLock::new(HashMap::new()),
            chats: RwLock::new(HashMap::new()),
            chat_characters: RwLock::new(HashMap::new()),
            config: self.config.clone(),
            library: Some(self.clone()),
        })
    }

    pub fn open_world(&self, category: &str, slug: &str) -> Result<World, WorldError> {
        let mut w = World::open(self.root.join(category).join(slug), category, self.config.clone())?;
        w.library = Some(self.clone());
        Ok(w)
    }
}

/// How one character sees another.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Perception {
    /// First-hand opinion, in the third person ("Kael saved her in Vors; she owes him her life").
    pub content: String,
    /// Affection in [-1, 1] → the node's `valence`.
    pub affection: f32,
    /// Closeness in [0, 1] → the weight of the node and of the edge.
    pub closeness: f32,
}

/// Context of a scene for the character about to speak.
#[derive(Debug, Clone)]
pub struct SceneRecall {
    /// Full block for the system prompt: world + people present + chat memory + own memory.
    pub context: String,
    pub character: Recall,
    pub world: Recall,
    /// The chat's memory, when the scene belongs to a saved conversation.
    pub chat: Option<Recall>,
    /// The shared definitions the turn touched (category's, then universal), one per library.
    pub definitions: Vec<Recall>,
    /// The Collab's bridge memory, when the scene happens in one.
    pub collab: Option<Recall>,
}

/// What a scene inside a Collab adds to the speaker's own world: the bridge memory, the
/// people present from other worlds, the other worlds' definition libraries and the
/// speaker's memory inside the Collab's chat (its Carbon Copy there).
pub struct Bridge<'a> {
    pub memory: &'a MemoryHandler,
    pub speaker_memory: MemoryHandler,
    /// People present from other worlds: (id, name).
    pub foreign: Vec<(GraphId, String)>,
    pub libraries: Vec<MemoryHandler>,
}

pub struct World {
    pub category: String,
    pub dir: PathBuf,
    /// The world's memory.
    pub memory: MemoryHandler,
    characters: RwLock<HashMap<GraphId, MemoryHandler>>,
    /// Memories of the chats opened so far (conversation id → graph).
    chats: RwLock<HashMap<GraphId, MemoryHandler>>,
    /// Per-chat Carbon Copies of participant memories ((conversation id, character id) → graph).
    chat_characters: RwLock<HashMap<(GraphId, GraphId), MemoryHandler>>,
    config: HandlerConfig,
    /// Where its shared definitions live (`None` for a world opened on its own).
    library: Option<Library>,
}

impl World {
    /// Opens the world and all its characters (each `.mem` is small: they load whole).
    /// Folders with the old Spanish names are renamed first.
    pub fn open(dir: impl Into<PathBuf>, category: &str, config: HandlerConfig) -> Result<Self, WorldError> {
        let dir = dir.into();
        crate::layout::migrate_world_dir(&dir);
        let memory = MemoryHandler::open(dir.join(WORLD_FILE), config.clone())?;
        let mut characters = HashMap::new();
        let chars_dir = dir.join(CHARACTERS_DIR);
        if chars_dir.is_dir() {
            for e in std::fs::read_dir(&chars_dir)? {
                let path = e?.path();
                if path.extension().is_some_and(|x| x == "mem") {
                    let h = MemoryHandler::open(&path, config.clone())?;
                    // Avatars saved under the old folder name.
                    let old = h.meta().avatar.filter(|a| a.starts_with(crate::layout::LEGACY_IMAGES_PREFIX));
                    if let Some(old) = old {
                        let new = format!("{IMAGES_DIR}/{}", &old[crate::layout::LEGACY_IMAGES_PREFIX.len()..]);
                        h.write(|g| g.meta.avatar = Some(new));
                        h.save()?;
                    }
                    characters.insert(h.id(), h);
                }
            }
        }
        let world = Self {
            category: category.to_owned(),
            dir,
            memory,
            characters: RwLock::new(characters),
            chats: RwLock::new(HashMap::new()),
            chat_characters: RwLock::new(HashMap::new()),
            config,
            library: None,
        };
        world.refresh_rosters();
        Ok(world)
    }

    pub fn id(&self) -> GraphId {
        self.memory.id()
    }

    pub fn lang(&self) -> Lang {
        self.memory.lang()
    }

    /// The libraries of shared definitions this world uses (category's, then universal).
    pub fn definitions(&self) -> Vec<MemoryHandler> {
        self.library.as_ref().map(|l| l.definitions_for(&self.category)).unwrap_or_default()
    }

    /// One of this world's definition libraries; with `create`, made if missing.
    pub fn definition_library(&self, level: DefinitionLevel, create: bool) -> Result<Option<MemoryHandler>, WorldError> {
        match &self.library {
            Some(l) => l.definitions(level, &self.category, create),
            None => Ok(None),
        }
    }

    /// Changes the language of the world and of every character in it.
    pub fn set_lang(&self, lang: Lang) {
        self.memory.set_lang(lang);
        for h in self.definitions() {
            h.set_lang(lang);
        }
        for h in self
            .characters
            .read()
            .values()
            .chain(self.chats.read().values())
            .chain(self.chat_characters.read().values())
        {
            h.set_lang(lang);
        }
    }

    /// The memory of one conversation (`chats/<id>.mem`): opened from disk, or created
    /// with a node for each participant (linked to their sheet) and one for the user.
    /// Everyone in the chat shares it, and the user edits it like any other graph.
    pub fn chat_memory(&self, conv: &Conversation) -> Result<MemoryHandler, WorldError> {
        let h = if let Some(h) = self.chats.read().get(&conv.id).cloned() {
            h
        } else {
            let mut cache = self.chats.write();
            if let Some(h) = cache.get(&conv.id).cloned() {
                h
            } else {
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
                        let Ok(c) = self.character(p) else { continue };
                        let m = c.meta();
                        let n = g.add_node(MemoryNode::new(NodeKind::Character, m.name, m.tagline, now).with_weight(0.6))?;
                        g.set_external(n, Some(ExternalRef { scope: Scope::Character, id: p }))?;
                        g.connect(root, n, EdgeKind::Knows, 0.6, now)?;
                    }
                    let user = g.add_node(
                        MemoryNode::new(NodeKind::Entity, conv.user_name.trim(), i18n::t(self.lang(), "ctx.chat.user_node"), now).with_weight(0.6),
                    )?;
                    g.connect(root, user, EdgeKind::Knows, 0.6, now)?;
                    let h = MemoryHandler::new(g, path, self.config.clone());
                    h.save()?;
                    h
                };
                h.set_lang(self.lang());
                h.set_roster(self.roster());
                cache.insert(conv.id, h.clone());
                h
            }
        };
        for &p in &conv.participants {
            let _ = self.chat_character(conv.id, p);
        }
        Ok(h)
    }

    /// A character's Carbon Copy inside `chat` (`chats/<chat>-<character>.mem`): opened from
    /// disk, or cloned from the canonical character graph so chat turns never mutate the base
    /// character.
    pub fn chat_character(&self, chat: GraphId, character: GraphId) -> Result<MemoryHandler, WorldError> {
        let key = (chat, character);
        if let Some(h) = self.chat_characters.read().get(&key) {
            return Ok(h.clone());
        }
        let base = self.character(character)?;
        let mut cache = self.chat_characters.write();
        if let Some(h) = cache.get(&key) {
            return Ok(h.clone());
        }
        let path = Conversation::character_memory_path(&self.dir, chat, character);
        let unique_id = chat.rotate_left(13) ^ character;
        let h = if path.is_file() {
            let h = MemoryHandler::open(&path, self.config.clone())?;
            if h.id() != unique_id {
                h.write(|g| g.meta.id = unique_id);
                h.save()?;
            }
            h
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut g = base.read(|g| g.clone());
            g.meta.id = unique_id;
            let h = MemoryHandler::new(g, path, self.config.clone());
            h.save()?;
            h
        };
        h.set_lang(self.lang());
        h.set_roster(self.roster());
        cache.insert(key, h.clone());
        Ok(h)
    }

    /// Renames a chat's memory (its root follows the conversation's title).
    pub fn rename_chat_memory(&self, chat: GraphId, title: &str) -> Result<(), WorldError> {
        let path = Conversation::memory_path(&self.dir, chat);
        let cached = self.chats.read().get(&chat).cloned();
        let h = match cached {
            Some(h) => h,
            None if path.is_file() => MemoryHandler::open(&path, self.config.clone())?,
            None => return Ok(()),
        };
        h.write(|g| -> Result<(), GraphError> {
            g.meta.name = title.trim().to_owned();
            let root = g.root();
            g.update_node(root, crate::graph::NodePatch { label: Some(title.trim().to_owned()), ..Default::default() })
        })?;
        h.save()?;
        Ok(())
    }

    /// Deletes a chat's memory (the `.mem` file, participant Carbon Copies, and cached graphs).
    pub fn delete_chat_memory(&self, chat: GraphId) -> Result<(), WorldError> {
        self.chats.write().remove(&chat);
        self.chat_characters.write().retain(|&(c, _), _| c != chat);
        match std::fs::remove_file(Conversation::memory_path(&self.dir, chat)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let prefix = format!("{chat:016x}-");
        let chats_dir = self.dir.join(crate::conversation::CHATS_DIR);
        if let Ok(rd) = std::fs::read_dir(chats_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with(&prefix) && path.extension().is_some_and(|x| x == "mem") {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        Ok(())
    }

    pub fn images_dir(&self) -> PathBuf {
        self.dir.join(IMAGES_DIR)
    }

    pub fn characters(&self) -> Vec<MemoryHandler> {
        let mut v: Vec<_> = self.characters.read().values().cloned().collect();
        v.sort_by_key(|h| h.meta().name);
        v
    }

    pub fn character(&self, id: GraphId) -> Result<MemoryHandler, WorldError> {
        self.characters.read().get(&id).cloned().ok_or(WorldError::NoSuchCharacter(id))
    }

    pub fn character_by_name(&self, name: &str) -> Option<MemoryHandler> {
        let wanted = fold(name.trim());
        self.characters.read().values().find(|h| fold(&h.meta().name) == wanted).cloned()
    }

    /// Normalised name → character, for the curator.
    pub fn roster(&self) -> Roster {
        self.characters
            .read()
            .values()
            .map(|h| {
                let m = h.meta();
                (fold(&m.name), ExternalRef { scope: Scope::Character, id: m.id })
            })
            .collect()
    }

    fn refresh_rosters(&self) {
        let roster = self.roster();
        self.memory.set_roster(roster.clone());
        for h in self
            .characters
            .read()
            .values()
            .chain(self.chats.read().values())
            .chain(self.chat_characters.read().values())
        {
            h.set_roster(roster.clone());
        }
    }

    /// Creates a character in this world.
    /// - `identity`: personality root (who they are, in the second or third person).
    /// - `public_bio`: what anyone in the world knows about them (it goes to `world.mem`).
    /// - `appearance`: stable visual description to generate their images.
    pub fn create_character(&self, name: &str, identity: &str, public_bio: &str, appearance: &str) -> Result<MemoryHandler, WorldError> {
        let slug = slugify(name);
        if slug.is_empty() {
            return Err(WorldError::BadName);
        }
        if self.character_by_name(name).is_some() {
            return Err(WorldError::AlreadyExists(format!("the character {name}")));
        }
        let path = self.dir.join(CHARACTERS_DIR).join(format!("{slug}.mem"));
        if path.exists() {
            return Err(WorldError::AlreadyExists(path.display().to_string()));
        }
        let now = unix_now();
        let mut meta = GraphMeta::new(Scope::Character, name.trim(), now);
        meta.appearance = appearance.trim().to_owned();
        meta.tagline = short(public_bio.trim(), 120);
        let id = meta.id;
        let handler = MemoryHandler::new(MemoryGraph::new(meta, identity.trim(), now), path, self.config.clone());
        handler.set_lang(self.lang());
        handler.save()?;

        // Public sheet in the world's memory.
        self.memory.write(|w| -> Result<(), GraphError> {
            let node = w.add_node(MemoryNode::new(NodeKind::Character, name.trim(), public_bio.trim(), now).with_weight(0.6))?;
            w.set_external(node, Some(ExternalRef { scope: Scope::Character, id }))?;
            let root = w.root();
            w.connect(root, node, EdgeKind::Knows, 0.6, now)
        })?;
        self.memory.save()?;

        self.characters.write().insert(id, handler.clone());
        self.refresh_rosters();
        Ok(handler)
    }

    /// Writes (or updates) what `who` thinks of `about`. Asymmetric: call it twice for a
    /// relationship in both directions.
    pub fn set_perception(&self, who: GraphId, about: GraphId, p: &Perception) -> Result<NodeId, WorldError> {
        let subject = self.character(who)?;
        let other = self.character(about)?;
        let other_name = other.meta().name;
        let target = ExternalRef { scope: Scope::Character, id: about };
        let now = unix_now();
        let node = subject.write(|g| -> Result<NodeId, GraphError> {
            let node = match g.find_external(target) {
                Some(n) => {
                    g.update_node(
                        n,
                        crate::graph::NodePatch {
                            content: Some(p.content.trim().to_owned()),
                            weight: Some(p.closeness),
                            valence: Some(p.affection),
                            ..Default::default()
                        },
                    )?;
                    n
                }
                None => {
                    let n = g.add_node(
                        MemoryNode::new(NodeKind::Character, other_name.clone(), p.content.trim(), now)
                            .with_weight(p.closeness)
                            .with_valence(p.affection),
                    )?;
                    g.set_external(n, Some(target))?;
                    n
                }
            };
            let root = g.root();
            g.connect(root, node, EdgeKind::Knows, p.closeness.max(0.2), now)?;
            Ok(node)
        })?;
        subject.save()?;

        // The world only records that they know each other (no private opinions).
        self.memory.write(|w| -> Result<(), GraphError> {
            let a = w.find_external(ExternalRef { scope: Scope::Character, id: who });
            let b = w.find_external(target);
            if let (Some(a), Some(b)) = (a, b) {
                w.connect(a, b, EdgeKind::RelatedTo, p.closeness.max(0.2), now)?;
            }
            Ok(())
        })?;
        self.memory.save()?;
        Ok(node)
    }

    /// Relationship in both directions.
    pub fn relate(&self, a: GraphId, b: GraphId, a_about_b: &Perception, b_about_a: &Perception) -> Result<(), WorldError> {
        self.set_perception(a, b, a_about_b)?;
        self.set_perception(b, a, b_about_a)?;
        Ok(())
    }

    /// Scene recall for `speaker`, with `present` = other characters in the scene.
    /// The budget is split ~70 % own memory / ~30 % world.
    pub fn scene_recall(&self, speaker: GraphId, prompt: &str, present: &[GraphId]) -> Result<SceneRecall, WorldError> {
        self.scene_recall_in(speaker, prompt, present, None)
    }

    /// Scene recall inside a saved conversation: besides the world and the speaker's own
    /// memory, the chat's memory gets ~25 % of the budget (own memory ~50 %, world ~25 %).
    pub fn scene_recall_in(&self, speaker: GraphId, prompt: &str, present: &[GraphId], chat: Option<&MemoryHandler>) -> Result<SceneRecall, WorldError> {
        self.scene_recall_at(speaker, prompt, present, chat, self.story_now(None))
    }

    /// `scene_recall_in` at a story time ([`World::story_now`] of the chat's moment): every
    /// memory involved only sees what exists then, and links that ended are the past.
    pub fn scene_recall_at(
        &self,
        speaker: GraphId,
        prompt: &str,
        present: &[GraphId],
        chat: Option<&MemoryHandler>,
        story: StoryTime,
    ) -> Result<SceneRecall, WorldError> {
        self.scene_recall_bridged(speaker, prompt, present, chat, story, None)
    }

    /// `scene_recall_at`, possibly inside a Collab (`bridge`): then the bridge memory gets
    /// its own share, people from other worlds are recognised (or not) by what the speaker,
    /// the Collab or the speaker's world know of them, and the chat's memory is timeless
    /// (the worlds' timelines do not compare).
    pub fn scene_recall_bridged(
        &self,
        speaker: GraphId,
        prompt: &str,
        present: &[GraphId],
        chat: Option<&MemoryHandler>,
        story: StoryTime,
        bridge: Option<Bridge<'_>>,
    ) -> Result<SceneRecall, WorldError> {
        let me = match (&bridge, chat) {
            (Some(b), _) => b.speaker_memory.clone(),
            (None, Some(c)) => self.chat_character(c.id(), speaker)?,
            (None, None) => self.character(speaker)?,
        };
        let lang = self.lang();
        let now = unix_now();
        let total = self.config.budget.max_chars;
        let bridged = bridge.is_some();
        let collab_budget = ContextBudget { max_chars: if bridged { total * 3 / 20 } else { 0 } };
        let char_budget = ContextBudget { max_chars: if bridged { total * 9 / 20 } else if chat.is_some() { total / 2 } else { total * 7 / 10 } };
        let chat_budget = ContextBudget { max_chars: if chat.is_none() { 0 } else if bridged { total / 5 } else { total / 4 } };
        let world_budget = ContextBudget { max_chars: total - char_budget.max_chars - chat_budget.max_chars - collab_budget.max_chars };
        let chat_story = if bridged { crate::model::STORY_PRESENT } else { story };
        // Shared definitions take a third of the world's share (only if there are any). In a
        // Collab, the other worlds' libraries count too (each one once).
        let mut libraries = self.definitions();
        if let Some(b) = &bridge {
            for l in &b.libraries {
                if !libraries.iter().any(|x| x.path() == l.path()) {
                    libraries.push(l.clone());
                }
            }
        }
        let defs_total = if libraries.is_empty() { 0 } else { world_budget.max_chars / 3 };
        let world_budget = ContextBudget { max_chars: world_budget.max_chars - defs_total };
        let chat_handler = chat;
        let others: Vec<GraphId> = present.iter().copied().filter(|&p| p != speaker).collect();
        // When someone's looks are asked about, their aspect nodes are seeded directly
        // (they are two hops away from the speaker and would compete at a disadvantage).
        let visual = crate::text::visual_intent(prompt);

        // The people present activate what the speaker thinks of them…
        let (mut my_seeds, mut lines, my_looks) = me.read(|g| {
            let mut seeds = Vec::new();
            let mut lines = Vec::new();
            let mut looks = Vec::new();
            if visual {
                looks.extend(g.appearance_of(g.root()).into_iter().map(|(n, _)| (n, 0.4)));
            }
            // People from other worlds are recognised below.
            for &p in others.iter().filter(|p| !bridged || self.characters.read().contains_key(p)) {
                let name = self.characters.read().get(&p).map(|h| h.meta().name).unwrap_or_default();
                // Someone they only meet later in the story is still a stranger now.
                let known = g.find_external(ExternalRef { scope: Scope::Character, id: p }).filter(|&n| g.exists_at(n, story));
                match known {
                    Some(n) => {
                        seeds.push((n, 0.8));
                        if visual {
                            looks.extend(g.appearance_of(n).into_iter().map(|(a, _)| (a, 0.75)));
                        }
                        let node = g.node(n).expect("valid link");
                        lines.push(format!("- {}: {} ({})", name, node.content, crate::context::relationship(lang, node.valence, node.weight)));
                    }
                    None => {
                        let mut line = tr!(lang, "ctx.scene.unknown", name = name);
                        if let Some(bio) = self.public_bio(p) {
                            line.push(' ');
                            line.push_str(&tr!(lang, "ctx.scene.rumour", bio = bio));
                        }
                        lines.push(line);
                    }
                }
            }
            (seeds, lines, looks)
        });
        // …and, in the world, their public sheets and the speaker's own.
        let (mut world_seeds, world_looks): (Seeds, Seeds) = self.memory.read(|w| {
            let people: Vec<NodeId> = std::iter::once(speaker)
                .chain(others.iter().copied())
                .filter_map(|id| w.find_external(ExternalRef { scope: Scope::Character, id }))
                .collect();
            let looks = if visual { people.iter().flat_map(|&n| w.appearance_of(n)).map(|(a, _)| (a, 0.4)).collect() } else { Vec::new() };
            (people.into_iter().map(|n| (n, 0.5)).collect(), looks)
        });

        // People from other worlds: what the speaker thinks of them, else what is known of
        // them in the Collab, else what the speaker's world tells (a legend), else nothing.
        if let Some(b) = &bridge {
            let named = |g: &MemoryGraph, folded: &str| g.nodes().find(|(n, node)| fold(&node.label) == folded && g.exists_at(*n, story)).map(|(n, _)| n);
            for (id, name) in b.foreign.iter().filter(|(id, _)| others.contains(id)) {
                let folded = fold(name);
                let own = me.read(|g| named(g, &folded).and_then(|n| g.node(n).map(|x| (n, x.kind, x.content.clone(), x.valence, x.weight))));
                if let Some((n, kind, content, valence, weight)) = own {
                    my_seeds.push((n, 0.8));
                    lines.push(if kind == NodeKind::Character {
                        format!("- {name}: {content} ({})", crate::context::relationship(lang, valence, weight))
                    } else {
                        format!("- {name}: {content}")
                    });
                    continue;
                }
                let met = b.memory.read(|g| {
                    g.find_external(ExternalRef { scope: Scope::Character, id: *id }).and_then(|n| g.node(n)).map(|x| x.content.clone()).filter(|c| !c.trim().is_empty())
                });
                if let Some(text) = met {
                    lines.push(tr!(lang, "ctx.scene.met", name = name, text = text));
                    continue;
                }
                let legend = self.memory.read(|g| named(g, &folded).and_then(|n| g.node(n).map(|x| (n, x.content.clone()))));
                match legend {
                    Some((n, text)) => {
                        world_seeds.push((n, 0.5));
                        lines.push(tr!(lang, "ctx.scene.legend", name = name, text = text));
                    }
                    None => lines.push(tr!(lang, "ctx.scene.stranger", name = name)),
                }
            }
        }

        // The opinions about the people present already go in <scene>, and their public
        // sheets would be redundant in <world>: they are activated but not repeated.
        let my_hidden: Vec<NodeId> = my_seeds.iter().map(|(n, _)| *n).collect();
        let world_hidden: Vec<NodeId> = world_seeds.iter().map(|(n, _)| *n).collect();
        let my_all: Vec<(NodeId, f32)> = my_seeds.iter().chain(&my_looks).copied().collect();
        let world_all: Vec<(NodeId, f32)> = world_seeds.iter().chain(&world_looks).copied().collect();
        let character = me.recall_with(prompt, &my_all, &my_hidden, char_budget, now, story);
        let world = self.memory.recall_with(prompt, &world_all, &world_hidden, world_budget, now, story);
        // In the chat's memory the people present are seeds too (their nodes carry only the
        // public tagline, already in <scene>, so they are not written again).
        let chat = chat.map(|h| {
            let (seeds, looks): (Seeds, Seeds) = h.read(|g| {
                let people: Vec<NodeId> = std::iter::once(speaker)
                    .chain(others.iter().copied())
                    .filter_map(|id| g.find_external(ExternalRef { scope: Scope::Character, id }))
                    .collect();
                let looks = if visual { people.iter().flat_map(|&n| g.appearance_of(n)).map(|(a, _)| (a, 0.5)).collect() } else { Vec::new() };
                (people.into_iter().map(|n| (n, 0.5)).collect(), looks)
            });
            let hidden: Vec<NodeId> = seeds.iter().map(|(n, _)| *n).collect();
            let all: Vec<(NodeId, f32)> = seeds.iter().chain(&looks).copied().collect();
            h.recall_with(prompt, &all, &hidden, chat_budget, now, chat_story)
        });
        // The Collab's bridge memory, seeded by the people present (their nodes there only
        // carry what <scene> already says).
        let collab = bridge.as_ref().map(|b| {
            let seeds: Seeds = b.memory.read(|g| {
                std::iter::once(speaker)
                    .chain(others.iter().copied())
                    .filter_map(|id| g.find_external(ExternalRef { scope: Scope::Character, id }))
                    .map(|n| (n, 0.5))
                    .collect()
            });
            let hidden: Vec<NodeId> = seeds.iter().map(|(n, _)| *n).collect();
            b.memory.recall_with(prompt, &seeds, &hidden, collab_budget, now, crate::model::STORY_PRESENT)
        });

        // Definitions the turn touched, except those the speaker, the world or the chat
        // already brought up by name: their own version is more specific.
        let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut note = |h: &MemoryHandler, r: &Recall| h.read(|g| known.extend(r.active.iter().filter_map(|a| g.node(a.id)).map(|n| fold(&n.label))));
        note(&me, &character);
        note(&self.memory, &world);
        if let (Some(h), Some(r)) = (chat_handler, &chat) {
            note(h, r);
        }
        if let (Some(b), Some(r)) = (&bridge, &collab) {
            note(b.memory, r);
        }
        let per_library = ContextBudget { max_chars: defs_total / libraries.len().max(1) };
        let definitions: Vec<Recall> = libraries
            .iter()
            .map(|h| {
                let hidden: Vec<NodeId> = h.read(|g| g.nodes().filter(|(_, n)| known.contains(&fold(&n.label))).map(|(id, _)| id).collect());
                h.recall_with(prompt, &[], &hidden, per_library, now, story)
            })
            .collect();

        let mut context = world.context.clone();
        context.push('\n');
        for d in definitions.iter().filter(|d| !d.context.is_empty()) {
            context.push_str(&d.context);
            context.push('\n');
        }
        if let Some(c) = &collab {
            context.push_str(&c.context);
            context.push('\n');
        }
        if !lines.is_empty() {
            lines.insert(0, format!("<scene>\n{}", i18n::t(lang, "ctx.scene.title")));
            lines.push("</scene>".to_owned());
            context.push_str(&lines.join("\n"));
            context.push('\n');
        }
        if let Some(c) = &chat {
            context.push_str(&c.context);
            context.push('\n');
        }
        context.push_str(&character.context);
        Ok(SceneRecall { context, character, world, chat, definitions, collab })
    }

    /// A character's canonical appearance: replaces (or extends) the aspects hanging from
    /// their public sheet in the world and copies them to their own memory ("your
    /// appearance"). It also updates `meta.appearance` with the summary (fallback image prompt).
    pub fn set_appearance(&self, id: GraphId, aspects: &[Aspect], replace: bool) -> Result<(), WorldError> {
        let h = self.character(id)?;
        let now = unix_now();
        let target = ExternalRef { scope: Scope::Character, id };
        let write = |g: &mut MemoryGraph, owner: NodeId| -> Result<(), GraphError> {
            let existing: Vec<(NodeId, String)> = g.appearance_of(owner).into_iter().map(|(n, node)| (n, fold(&node.label))).collect();
            if replace {
                for (n, _) in &existing {
                    g.remove_node(*n)?;
                }
            }
            for a in aspects {
                let same = (!replace).then(|| existing.iter().find(|(_, l)| *l == fold(&a.label)).map(|(n, _)| *n)).flatten();
                match same {
                    Some(n) => g.update_node(
                        n,
                        crate::graph::NodePatch { content: Some(a.content.clone()), weight: Some(a.weight), ..Default::default() },
                    )?,
                    None => {
                        let n = g.add_node(MemoryNode::new(NodeKind::Appearance, a.label.trim(), a.content.trim(), now).with_weight(a.weight))?;
                        g.connect(owner, n, EdgeKind::Composes, a.weight.max(0.3), now)?;
                    }
                }
            }
            Ok(())
        };
        self.memory.write(|w| -> Result<(), GraphError> {
            let owner = w.find_external(target).ok_or(GraphError::NoSuchNode(0))?;
            write(w, owner)
        })?;
        self.memory.save()?;
        h.write(|g| -> Result<(), GraphError> {
            let root = g.root();
            write(g, root)?;
            g.meta.appearance = appearance_prompt_from(g, root, "");
            Ok(())
        })?;
        h.save()?;
        Ok(())
    }

    /// How `who` sees `about`: adds (or updates) an aspect in `who`'s memory, hanging from
    /// their node for `about`.
    pub fn set_visual_perception(&self, who: GraphId, about: GraphId, aspect: &Aspect) -> Result<NodeId, WorldError> {
        let subject = self.character(who)?;
        let other_name = self.character(about)?.meta().name;
        let target = ExternalRef { scope: Scope::Character, id: about };
        let now = unix_now();
        let id = subject.write(|g| -> Result<NodeId, GraphError> {
            let owner = match g.find_external(target) {
                Some(n) => n,
                None => {
                    let n = g.add_node(MemoryNode::new(NodeKind::Character, other_name.clone(), "", now).with_weight(0.4))?;
                    g.set_external(n, Some(target))?;
                    let root = g.root();
                    g.connect(root, n, EdgeKind::Knows, 0.4, now)?;
                    n
                }
            };
            let same = g.appearance_of(owner).into_iter().find(|(_, n)| fold(&n.label) == fold(&aspect.label)).map(|(n, _)| n);
            match same {
                Some(n) => {
                    g.update_node(n, crate::graph::NodePatch { content: Some(aspect.content.clone()), weight: Some(aspect.weight), ..Default::default() })?;
                    Ok(n)
                }
                None => {
                    let n = g.add_node(MemoryNode::new(NodeKind::Appearance, aspect.label.trim(), aspect.content.trim(), now).with_weight(aspect.weight))?;
                    g.connect(owner, n, EdgeKind::Composes, aspect.weight.max(0.3), now)?;
                    Ok(n)
                }
            }
        })?;
        subject.save()?;
        Ok(id)
    }

    /// A character's visual prompt: the world's canonical aspects, most salient first.
    /// Falls back to `meta.appearance`.
    pub fn appearance_prompt(&self, id: GraphId) -> Result<String, WorldError> {
        self.appearance_prompt_for(id, "")
    }

    /// Visual prompt for a scene: the aspects with importance ≥ [`PORTRAIT_MIN_WEIGHT`]
    /// plus the secondary ones the scene names (e.g. "with Mon3tr", "without the mask").
    /// That way the occasional ones (a weapon, a companion, the hidden face) do not show
    /// up in every portrait.
    pub fn appearance_prompt_for(&self, id: GraphId, scene: &str) -> Result<String, WorldError> {
        let h = self.character(id)?;
        let scene = fold(scene);
        let from_world = self.memory.read(|w| {
            w.find_external(ExternalRef { scope: Scope::Character, id }).map(|owner| appearance_prompt_from(w, owner, &scene))
        });
        Ok(from_world.filter(|s| !s.is_empty()).unwrap_or_else(|| h.meta().appearance))
    }

    /// Canonical aspects (world) and how each other character sees `id`.
    pub fn appearance_report(&self, id: GraphId) -> Result<AppearanceReport, WorldError> {
        let target = ExternalRef { scope: Scope::Character, id };
        let to_aspects = |g: &MemoryGraph, owner: NodeId| -> Vec<Aspect> {
            g.appearance_of(owner).into_iter().map(|(_, n)| Aspect { label: n.label.clone(), content: n.content.clone(), weight: n.weight }).collect()
        };
        let canonical = self.memory.read(|w| w.find_external(target).map(|o| to_aspects(w, o)).unwrap_or_default());
        let mut seen_by = Vec::new();
        for h in self.characters() {
            if h.id() == id {
                continue;
            }
            let aspects = h.read(|g| g.find_external(target).map(|o| to_aspects(g, o)).unwrap_or_default());
            if !aspects.is_empty() {
                seen_by.push((h.meta().name, aspects));
            }
        }
        Ok(AppearanceReport { canonical, seen_by })
    }

    /// Folder of the characters' reference images.
    pub fn references_dir(&self) -> PathBuf {
        self.dir.join(REFERENCES_DIR)
    }

    /// A character's reference images (`references/<slug>-*.{png,jpg,webp}`).
    pub fn references(&self, id: GraphId) -> Result<Vec<PathBuf>, WorldError> {
        let slug = slugify(&self.character(id)?.meta().name);
        let mut out: Vec<PathBuf> = std::fs::read_dir(self.references_dir())
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let name = p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
                        name.starts_with(&format!("{slug}-")) && [".png", ".jpg", ".jpeg", ".webp"].iter().any(|x| name.ends_with(x))
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        Ok(out)
    }

    /// Copies a reference image into the world folder and returns its new path.
    pub fn add_reference(&self, id: GraphId, bytes: &[u8], ext: &str) -> Result<PathBuf, WorldError> {
        let slug = slugify(&self.character(id)?.meta().name);
        std::fs::create_dir_all(self.references_dir())?;
        let ext = match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => "jpg",
            "webp" => "webp",
            _ => "png",
        };
        // The same image twice adds no information: the existing one is reused.
        for existing in self.references(id)? {
            if std::fs::read(&existing).is_ok_and(|b| b == bytes) {
                return Ok(existing);
            }
        }
        let stamp = unix_now();
        let mut path = self.references_dir().join(format!("{slug}-{stamp}.{ext}"));
        let mut n = 2;
        while path.exists() {
            path = self.references_dir().join(format!("{slug}-{stamp}-{n}.{ext}"));
            n += 1;
        }
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    // ------------------------------------------------------------------ look

    /// How the world's chats look (`style.toml`; the default look without one).
    pub fn style(&self) -> crate::style::WorldStyle {
        crate::style::WorldStyle::load(&self.dir)
    }

    pub fn set_style(&self, style: &crate::style::WorldStyle) -> Result<(), WorldError> {
        style.save(&self.dir)?;
        Ok(())
    }

    // ------------------------------------------------------------------ timeline

    /// The world's timeline, in story order.
    pub fn timeline(&self) -> Vec<Moment> {
        self.memory.meta().timeline
    }

    pub fn moment(&self, id: u32) -> Option<Moment> {
        self.memory.read(|g| g.meta.moment(id).cloned())
    }

    /// Replaces the timeline. Moments with id 0 are new and get a fresh id; the rest keep
    /// theirs, so chats and characters still point to them.
    pub fn set_timeline(&self, moments: Vec<Moment>) -> Result<Vec<Moment>, WorldError> {
        let mut moments: Vec<Moment> = moments
            .into_iter()
            .map(|m| Moment { name: m.name.trim().to_owned(), summary: m.summary.trim().to_owned(), ..m })
            .collect();
        if moments.iter().any(|m| m.name.is_empty()) {
            return Err(WorldError::BadName);
        }
        let first_free = moments.iter().map(|m| m.id).max().unwrap_or(0) + 1;
        for (id, m) in (first_free..).zip(moments.iter_mut().filter(|m| m.id == 0)) {
            m.id = id;
        }
        moments.sort_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
        moments.dedup_by_key(|m| m.id);
        let saved = moments.clone();
        self.memory.write(|g| g.meta.timeline = moments);
        self.memory.save()?;
        // Characters drop the moments that no longer exist.
        for h in self.characters() {
            let before = h.meta().moments;
            let kept: Vec<CharacterMoment> = before.iter().filter(|m| saved.iter().any(|s| s.id == m.moment)).cloned().collect();
            if kept.len() != before.len() {
                h.write(|g| g.meta.moments = kept);
                h.save()?;
            }
        }
        Ok(saved)
    }

    /// The moment a chat set at `moment` happens at: that one or, without one (or if it
    /// was deleted), the timeline's last. `None` in a world without a timeline.
    pub fn resolve_moment(&self, moment: Option<u32>) -> Option<Moment> {
        self.memory.read(|g| {
            let picked = moment.and_then(|id| g.meta.moment(id));
            picked.or_else(|| g.meta.timeline.iter().max_by_key(|m| m.at)).cloned()
        })
    }

    /// The story time of a chat set at `moment` ([`World::resolve_moment`]), or
    /// `STORY_PRESENT` in a world without a timeline (then everything is seen and what is
    /// learned is timeless).
    pub fn story_now(&self, moment: Option<u32>) -> StoryTime {
        self.resolve_moment(moment).map_or(STORY_PRESENT, |m| m.at)
    }

    /// The moments of the story a character belongs to (each with its own scenario).
    pub fn set_character_moments(&self, id: GraphId, moments: Vec<CharacterMoment>) -> Result<(), WorldError> {
        let h = self.character(id)?;
        let timeline = self.timeline();
        if let Some(bad) = moments.iter().find(|m| !timeline.iter().any(|t| t.id == m.moment)) {
            return Err(WorldError::NoSuchMoment(bad.moment));
        }
        let mut moments = moments;
        moments.sort_by_key(|m| timeline.iter().position(|t| t.id == m.moment));
        moments.dedup_by_key(|m| m.moment);
        h.write(|g| g.meta.moments = moments);
        h.save()?;
        Ok(())
    }

    fn public_bio(&self, id: GraphId) -> Option<String> {
        self.memory.read(|w| {
            let n = w.find_external(ExternalRef { scope: Scope::Character, id })?;
            w.node(n).map(|n| n.content.clone()).filter(|c| !c.is_empty())
        })
    }

    pub fn save_all(&self) -> Result<(), WorldError> {
        self.memory.save_if_dirty()?;
        for h in self.definitions() {
            h.save_if_dirty()?;
        }
        for h in self
            .characters
            .read()
            .values()
            .chain(self.chats.read().values())
            .chain(self.chat_characters.read().values())
        {
            h.save_if_dirty()?;
        }
        Ok(())
    }
}
