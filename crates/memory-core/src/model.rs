//! Basic graph types: nodes, edges and character metadata.
//!
//! Everything that is persisted derives `rkyv` (the binary `.mem` file) and `serde`
//! (only for IPC with the frontend and to read the curator model's JSON output; never
//! for the main persistence).

use std::time::{SystemTime, UNIX_EPOCH};

use i18n::Lang;

/// Node identifier: `petgraph`'s native index (`NodeIndex<u32>`), never a string.
pub type NodeId = u32;

/// Current Unix seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------- story time
//
// Two clocks live in a graph. Unix time (`created_at`, `last_accessed`…) says when the
// app learned something and drives forgetting. **Story time** says when it is true inside
// the world: in-world days, so a world's timeline can put Babel in 1093 and Londinium in
// 1098. A chat happens at one moment of the timeline and only sees what exists then.

/// In-world days (`year * DAYS_PER_YEAR + day`).
pub type StoryTime = i64;
pub const DAYS_PER_YEAR: i64 = 365;
/// A node or edge that has been true since forever (seeds without dates, old files).
pub const STORY_ALWAYS: StoryTime = i64::MIN;
/// An edge that is still valid.
pub const STORY_OPEN: StoryTime = i64::MAX;
/// The "now" of a world without a timeline: after everything.
pub const STORY_PRESENT: StoryTime = i64::MAX - 1;

/// Story time of a year (and day of that year).
pub fn story_of_year(year: i64, day: i64) -> StoryTime {
    year.saturating_mul(DAYS_PER_YEAR).saturating_add(day.clamp(0, DAYS_PER_YEAR - 1))
}

/// The year a story time falls in (`None` for the open ends).
pub fn story_year(t: StoryTime) -> Option<i64> {
    (t != STORY_ALWAYS && t < STORY_PRESENT).then(|| t.div_euclid(DAYS_PER_YEAR))
}

/// The date written on what is learned at story time `t`: in a world without a timeline
/// (`STORY_PRESENT`) new memories are timeless.
pub fn written_at(t: StoryTime) -> StoryTime {
    if t >= STORY_PRESENT { STORY_ALWAYS } else { t }
}

/// A named point of a world's timeline.
#[derive(
    rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq,
)]
pub struct Moment {
    /// Stable id inside the world (chats and characters refer to it).
    pub id: u32,
    pub name: String,
    pub at: StoryTime,
    /// What is going on in the world at this point (for the actor).
    pub summary: String,
}

/// A moment of the story a character belongs to, with what they are doing then.
#[derive(
    rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq,
)]
pub struct CharacterMoment {
    pub moment: u32,
    /// Replaces the character's scenario in chats set at this moment (empty = keep it).
    pub scenario: String,
}

/// Node kind. 1 byte in memory, on disk and over IPC (`kind as u8`).
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum NodeKind {
    // --- Adaptive personality tree ---
    /// The character's single root: core identity.
    PersonalityRoot = 0,
    /// Psychological trait (curious, sarcastic…); `weight` = intensity.
    Trait = 1,
    /// Emotional state or tendency; `valence` = affective sign.
    Emotion = 2,
    /// Core value or belief.
    Value = 3,
    /// The character's goal or wish.
    Goal = 4,
    // --- Episodic knowledge graph ---
    /// Memory of something that happened (subject to time decay).
    EpisodicMemory = 5,
    /// Stable fact ("the user is a nurse").
    SemanticFact = 6,
    /// Person, object or organisation.
    Entity = 7,
    Place = 8,
    /// Conversation topic.
    Topic = 9,
    // --- World memory and links between characters ---
    /// A world's single root (the world's "identity": tone, premise).
    WorldRoot = 10,
    /// Another character with their own `.mem` (carries an `ExternalRef`). In a
    /// character's memory its content is the SUBJECTIVE opinion; in the world's, the public sheet.
    Character = 11,
    /// Faction, guild, kingdom, organisation.
    Faction = 12,
    /// World rule (magic, technology, religion); the world's core, as traits are the character's.
    Lore = 13,
    /// Historical fact of the world: it does not fade like a memory.
    Event = 14,
    /// A visual aspect of someone (hair, eyes, outfit…). It hangs (`Composes`) from the
    /// root if it belongs to the character itself, or from another person's `Character`
    /// node: that is how each one remembers how they see the others. In the world graph
    /// it is the canonical appearance.
    Appearance = 15,
    // --- Per-chat memory ---
    /// A conversation's single root: its title, and notes the user keeps for that chat.
    ChatRoot = 16,
    // --- Shared definitions ---
    /// A general definition any character can rely on ("Originium", "Lupo", "Iron"):
    /// the objective baseline, shared by every world of a library.
    Concept = 17,
    /// The single root of a library of shared definitions (universal or a category's).
    LibraryRoot = 18,
}

impl NodeKind {
    pub const ALL: [NodeKind; 19] = [
        NodeKind::PersonalityRoot,
        NodeKind::Trait,
        NodeKind::Emotion,
        NodeKind::Value,
        NodeKind::Goal,
        NodeKind::EpisodicMemory,
        NodeKind::SemanticFact,
        NodeKind::Entity,
        NodeKind::Place,
        NodeKind::Topic,
        NodeKind::WorldRoot,
        NodeKind::Character,
        NodeKind::Faction,
        NodeKind::Lore,
        NodeKind::Event,
        NodeKind::Appearance,
        NodeKind::ChatRoot,
        NodeKind::Concept,
        NodeKind::LibraryRoot,
    ];

    /// Graph roots (operations never create or delete them).
    pub fn is_root(self) -> bool {
        matches!(self, NodeKind::PersonalityRoot | NodeKind::WorldRoot | NodeKind::ChatRoot | NodeKind::LibraryRoot)
    }

    /// Core that always gets a slot in the context: personality in a character,
    /// rules (lore) in a world or a chat.
    pub fn is_core(self) -> bool {
        self.is_personality() || matches!(self, NodeKind::WorldRoot | NodeKind::ChatRoot | NodeKind::LibraryRoot | NodeKind::Lore)
    }

    /// Does it belong to the personality tree?
    pub fn is_personality(self) -> bool {
        matches!(
            self,
            NodeKind::PersonalityRoot | NodeKind::Trait | NodeKind::Emotion | NodeKind::Value | NodeKind::Goal
        )
    }

    /// Only episodic memories fade over time; personality and stable facts do not.
    pub fn decays(self) -> bool {
        matches!(self, NodeKind::EpisodicMemory)
    }

    /// Episodic memory or historical event: what it involved or evoked when it happened
    /// is part of the record and does not expire later.
    pub fn is_memory(self) -> bool {
        matches!(self, NodeKind::EpisodicMemory | NodeKind::Event)
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }

    /// The `snake_case` name used in JSON, the GBNF grammar and the dictionary.
    pub fn key(self) -> &'static str {
        match self {
            NodeKind::PersonalityRoot => "personality_root",
            NodeKind::Trait => "trait",
            NodeKind::Emotion => "emotion",
            NodeKind::Value => "value",
            NodeKind::Goal => "goal",
            NodeKind::EpisodicMemory => "episodic_memory",
            NodeKind::SemanticFact => "semantic_fact",
            NodeKind::Entity => "entity",
            NodeKind::Place => "place",
            NodeKind::Topic => "topic",
            NodeKind::WorldRoot => "world_root",
            NodeKind::Character => "character",
            NodeKind::Faction => "faction",
            NodeKind::Lore => "lore",
            NodeKind::Event => "event",
            NodeKind::Appearance => "appearance",
            NodeKind::ChatRoot => "chat_root",
            NodeKind::Concept => "concept",
            NodeKind::LibraryRoot => "library_root",
        }
    }

    /// Short lowercase label for the actor's context block ("memory", "world rule"…).
    pub fn context_label(self, lang: Lang) -> &'static str {
        i18n::t(lang, &format!("ctx.kind.{}", self.key()))
    }

    /// Label shown in the app and the CLI ("Memory", "Rule"…).
    pub fn label(self, lang: Lang) -> &'static str {
        i18n::t(lang, &format!("kind.{}", self.key()))
    }
}

/// Edge kind. 1 byte.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum EdgeKind {
    /// Branch of the personality tree: root/trait → sub-trait, emotion, value, goal.
    Composes = 0,
    /// Memory → entity / place / topic that appears in it.
    Involves = 1,
    /// Generic association.
    RelatedTo = 2,
    /// Causality.
    Causes = 3,
    /// Temporal sequence between memories.
    Precedes = 4,
    /// Memory → emotion / trait it caused or shaped (joins both graphs).
    Evokes = 5,
    /// Root → fact or entity the character knows.
    Knows = 6,
    /// Inhibitory connection: it subtracts activation from its target.
    Contradicts = 7,
}

impl EdgeKind {
    pub const ALL: [EdgeKind; 8] = [
        EdgeKind::Composes,
        EdgeKind::Involves,
        EdgeKind::RelatedTo,
        EdgeKind::Causes,
        EdgeKind::Precedes,
        EdgeKind::Evokes,
        EdgeKind::Knows,
        EdgeKind::Contradicts,
    ];

    /// Sign with which the edge passes on activation.
    pub fn sign(self) -> f32 {
        if self == EdgeKind::Contradicts { -1.0 } else { 1.0 }
    }

    /// Structural links of a memory (`Involves`, `Evokes`): an event that already happened
    /// cannot stop involving who took part in it or evoking what it evoked then.
    pub fn is_immutable_on_memory(self) -> bool {
        matches!(self, EdgeKind::Involves | EdgeKind::Evokes)
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }

    /// The `snake_case` name used in JSON, the GBNF grammar and the dictionary.
    pub fn key(self) -> &'static str {
        match self {
            EdgeKind::Composes => "composes",
            EdgeKind::Involves => "involves",
            EdgeKind::RelatedTo => "related_to",
            EdgeKind::Causes => "causes",
            EdgeKind::Precedes => "precedes",
            EdgeKind::Evokes => "evokes",
            EdgeKind::Knows => "knows",
            EdgeKind::Contradicts => "contradicts",
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        i18n::t(lang, &format!("edge.{}", self.key()))
    }
}

const _: () = assert!(std::mem::size_of::<NodeKind>() == 1);
const _: () = assert!(std::mem::size_of::<EdgeKind>() == 1);
const _: () = assert!(std::mem::size_of::<Scope>() == 1);

/// The `petgraph` node weight.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
pub struct MemoryNode {
    pub kind: NodeKind,
    /// Short name: what is drawn on the sphere and the deduplication key.
    pub label: String,
    /// Text injected into the prompt.
    pub content: String,
    /// Importance / intensity in [0, 1].
    pub weight: f32,
    /// Emotional charge in [-1, 1].
    pub valence: f32,
    pub created_at: i64,
    pub last_accessed: i64,
    /// Times the node made it into the context (rehearsal effect).
    pub access_count: u32,
    /// Story time from which it exists (`STORY_ALWAYS` = timeless). A chat set earlier
    /// does not see it.
    pub since: StoryTime,
}

impl MemoryNode {
    pub fn new(kind: NodeKind, label: impl Into<String>, content: impl Into<String>, now: i64) -> Self {
        Self {
            kind,
            label: label.into(),
            content: content.into(),
            weight: 0.5,
            valence: 0.0,
            created_at: now,
            last_accessed: now,
            access_count: 0,
            since: STORY_ALWAYS,
        }
    }

    pub fn since(mut self, t: StoryTime) -> Self {
        self.since = t;
        self
    }

    /// Does it exist at story time `t`?
    pub fn exists_at(&self, t: StoryTime) -> bool {
        self.since <= t
    }

    pub fn with_weight(mut self, w: f32) -> Self {
        self.weight = clamp_unit(w);
        self
    }

    pub fn with_valence(mut self, v: f32) -> Self {
        self.valence = clamp_signed(v);
        self
    }
}

/// The `petgraph` edge weight.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct MemoryEdge {
    pub kind: EdgeKind,
    /// Strength of the connection in [0, 1] (`kind` gives the sign).
    pub weight: f32,
    pub created_at: i64,
    pub last_reinforced: i64,
    /// Story time from which the connection holds (`STORY_ALWAYS` = since forever).
    pub valid_from: StoryTime,
    /// Story time at which it stopped holding (`STORY_OPEN` = still valid). A closed edge
    /// is not deleted: from then on it is the past ("they were friends").
    pub valid_until: StoryTime,
}

/// Where an edge stands at a given story time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    /// It does not exist yet.
    Future,
    Active,
    /// It held once and no longer does.
    Past,
}

impl MemoryEdge {
    pub fn new(kind: EdgeKind, weight: f32, now: i64) -> Self {
        Self { kind, weight: clamp_unit(weight), created_at: now, last_reinforced: now, valid_from: STORY_ALWAYS, valid_until: STORY_OPEN }
    }

    /// Never closed (whatever the story time).
    pub fn is_open(&self) -> bool {
        self.valid_until == STORY_OPEN
    }

    pub fn validity_at(&self, t: StoryTime) -> Validity {
        if self.valid_from > t {
            Validity::Future
        } else if self.valid_until <= t {
            Validity::Past
        } else {
            Validity::Active
        }
    }
}

/// Whose graph is this? 1 byte.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Scope {
    Character = 0,
    World = 1,
    /// One conversation's memory: what happened in it, shared by everyone in the chat.
    Chat = 2,
    /// Shared definitions: the universal library or a category's (`worlds/_universal.mem`,
    /// `worlds/<category>/_category.mem`).
    Library = 3,
    /// A Collab's bridge memory: what happens where several worlds meet
    /// (`worlds/_collabs/<collab>/collab.mem`).
    Collab = 4,
}

impl Scope {
    pub fn root_kind(self) -> NodeKind {
        match self {
            Scope::Character => NodeKind::PersonalityRoot,
            Scope::World => NodeKind::WorldRoot,
            Scope::Chat => NodeKind::ChatRoot,
            Scope::Library => NodeKind::LibraryRoot,
            // Its root is the premise of the meeting, like a world's.
            Scope::Collab => NodeKind::WorldRoot,
        }
    }
}

/// Stable identifier of a graph (character or world) across files.
pub type GraphId = u64;

/// Link from a node to ANOTHER graph (e.g. the "Kael" node in Lyra's memory points to
/// Kael's `.mem`). Kept in a separate table: only the nodes that use it pay for it.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
)]
pub struct ExternalRef {
    pub scope: Scope,
    pub id: GraphId,
}

/// Graph metadata (the file's logical header).
#[derive(
    rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq,
)]
pub struct GraphMeta {
    pub id: GraphId,
    pub scope: Scope,
    pub name: String,
    /// Short description for the gallery.
    pub tagline: String,
    /// Path of the avatar / cover, relative to the world folder.
    pub avatar: Option<String>,
    /// Fixed scenario / style instructions for the actor.
    pub scenario: String,
    /// Stable visual description (fallback prompt for consistent images).
    pub appearance: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// A world's timeline, in story order (empty in characters and chats).
    #[serde(default)]
    pub timeline: Vec<Moment>,
    /// A character's moments of the story. Empty = they fit in any moment.
    #[serde(default)]
    pub moments: Vec<CharacterMoment>,
}

impl GraphMeta {
    pub fn new(scope: Scope, name: impl Into<String>, now: i64) -> Self {
        Self {
            id: new_graph_id(),
            scope,
            name: name.into(),
            tagline: String::new(),
            avatar: None,
            scenario: String::new(),
            appearance: String::new(),
            created_at: now,
            updated_at: now,
            timeline: Vec::new(),
            moments: Vec::new(),
        }
    }

    pub fn moment(&self, id: u32) -> Option<&Moment> {
        self.timeline.iter().find(|m| m.id == id)
    }

    /// Does this character fit in moment `id`? (`None` = a chat with no moment.)
    pub fn fits(&self, moment: Option<u32>) -> bool {
        match moment {
            Some(id) => self.moments.is_empty() || self.moments.iter().any(|m| m.moment == id),
            None => true,
        }
    }

    /// The character's situation at a moment: that moment's scenario, or the usual one.
    pub fn scenario_at(&self, moment: Option<u32>) -> &str {
        moment
            .and_then(|id| self.moments.iter().find(|m| m.moment == id))
            .map(|m| m.scenario.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.scenario)
    }
}

/// Random 64-bit id (time + counter + the process's random seed).
pub fn new_graph_id() -> GraphId {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.finish().max(1)
}

pub(crate) fn clamp_unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

pub(crate) fn clamp_signed(v: f32) -> f32 {
    if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_labels_in_every_language() {
        for k in NodeKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.key(), "serde name = key");
            assert!(i18n::has(&format!("kind.{}", k.key())) && i18n::has(&format!("ctx.kind.{}", k.key())), "{k:?}");
            for lang in Lang::ALL {
                assert!(!k.label(lang).is_empty() && !k.context_label(lang).is_empty());
            }
        }
        for k in EdgeKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.key());
            assert!(i18n::has(&format!("edge.{}", k.key())), "{k:?}");
        }
    }
}
