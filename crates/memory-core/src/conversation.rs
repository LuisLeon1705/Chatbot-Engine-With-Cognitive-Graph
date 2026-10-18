//! Conversations (one-to-one chats and groups) stored next to the world:
//! `worlds/<category>/<world>/chats/<id>.chat`, in binary rkyv like the `.mem` files.
//!
//! A conversation is the record of what was said (to show it and for the actor's short
//! context window). What is *remembered* lives in graphs: each character's own memory
//! and the chat's memory (`chats/<id>.mem`, see [`crate::world::World::chat_memory`]).

use std::path::{Path, PathBuf};

use rkyv::rancor::Error as RkyvError;
use rkyv::util::AlignedVec;

use crate::model::{GraphId, new_graph_id, unix_now};
use crate::persist::{PersistError, write_atomic};

pub const CHATS_DIR: &str = "chats";
const MAGIC: [u8; 4] = *b"CCHT";
/// v2: `characters_remember`. v3: `moment`. Versions 1 and 2 are still read.
const VERSION: u16 = 3;
const HEADER: usize = 16;

/// One utterance. `speaker_id` is `None` for the user and for the narrator.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
pub struct Utterance {
    pub speaker: String,
    pub speaker_id: Option<GraphId>,
    pub text: String,
    pub at: i64,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
pub struct Conversation {
    pub id: GraphId,
    pub title: String,
    /// Characters of the world taking part (1 = one-to-one chat, 2+ = group).
    pub participants: Vec<GraphId>,
    /// Name the user goes by in the scene.
    pub user_name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<Utterance>,
    /// Whether each character's own memory also learns from this chat. Off = an isolated
    /// storyline: only the chat's memory records it.
    pub characters_remember: bool,
    /// Moment of the world's timeline the chat is set at (`None` = the latest).
    pub moment: Option<u32>,
}

/// Layout of version 2 (before timelines), only to read old files.
#[derive(rkyv::Archive, rkyv::Deserialize)]
struct ConversationV2 {
    id: GraphId,
    title: String,
    participants: Vec<GraphId>,
    user_name: String,
    created_at: i64,
    updated_at: i64,
    messages: Vec<Utterance>,
    characters_remember: bool,
}

impl From<ConversationV2> for Conversation {
    fn from(c: ConversationV2) -> Self {
        Self {
            id: c.id,
            title: c.title,
            participants: c.participants,
            user_name: c.user_name,
            created_at: c.created_at,
            updated_at: c.updated_at,
            messages: c.messages,
            characters_remember: c.characters_remember,
            moment: None,
        }
    }
}

/// Layout of version 1 (before per-chat memory), only to read old files.
#[derive(rkyv::Archive, rkyv::Deserialize)]
struct ConversationV1 {
    id: GraphId,
    title: String,
    participants: Vec<GraphId>,
    user_name: String,
    created_at: i64,
    updated_at: i64,
    messages: Vec<Utterance>,
}

impl From<ConversationV1> for Conversation {
    fn from(c: ConversationV1) -> Self {
        Self {
            id: c.id,
            title: c.title,
            participants: c.participants,
            user_name: c.user_name,
            created_at: c.created_at,
            updated_at: c.updated_at,
            messages: c.messages,
            characters_remember: true,
            moment: None,
        }
    }
}

/// Summary for the side list (read without deserialising the messages).
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationSummary {
    pub id: GraphId,
    pub title: String,
    pub participants: Vec<GraphId>,
    pub user_name: String,
    pub updated_at: i64,
    pub message_count: usize,
    pub last_message: Option<(String, String)>,
}

impl Conversation {
    pub fn new(title: impl Into<String>, participants: Vec<GraphId>, user_name: impl Into<String>) -> Self {
        let now = unix_now();
        Self {
            id: new_graph_id(),
            title: title.into(),
            participants,
            user_name: user_name.into(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            characters_remember: true,
            moment: None,
        }
    }

    pub fn is_group(&self) -> bool {
        self.participants.len() > 1
    }

    pub fn push(&mut self, speaker: impl Into<String>, speaker_id: Option<GraphId>, text: impl Into<String>) {
        let now = unix_now();
        self.messages.push(Utterance { speaker: speaker.into(), speaker_id, text: text.into(), at: now });
        self.updated_at = now;
    }

    pub fn file_name(id: GraphId) -> String {
        format!("{id:016x}.chat")
    }

    /// Path of the chat's memory graph (`chats/<id>.mem`).
    pub fn memory_path(world_dir: &Path, id: GraphId) -> PathBuf {
        world_dir.join(CHATS_DIR).join(format!("{id:016x}.mem"))
    }

    /// Path of a character's carbon-copy memory inside a chat (`chats/<chat_id>-<char_id>.mem`).
    pub fn character_memory_path(world_dir: &Path, chat_id: GraphId, char_id: GraphId) -> PathBuf {
        world_dir.join(CHATS_DIR).join(format!("{chat_id:016x}-{char_id:016x}.mem"))
    }

    pub fn path_in(&self, world_dir: &Path) -> PathBuf {
        world_dir.join(CHATS_DIR).join(Self::file_name(self.id))
    }

    pub fn save(&self, world_dir: &Path) -> Result<(), PersistError> {
        let payload = rkyv::to_bytes::<RkyvError>(self).map_err(|e| PersistError::Corrupt(e.to_string()))?;
        let mut out = Vec::with_capacity(HEADER + payload.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&payload);
        write_atomic(&self.path_in(world_dir), &out)
    }

    pub fn load(path: &Path) -> Result<Self, PersistError> {
        let (version, aligned) = payload(&std::fs::read(path)?)?;
        let corrupt = |e: RkyvError| PersistError::Corrupt(e.to_string());
        if version == 1 {
            let archived = rkyv::access::<ArchivedConversationV1, RkyvError>(&aligned).map_err(corrupt)?;
            return rkyv::deserialize::<ConversationV1, RkyvError>(archived).map(Into::into).map_err(corrupt);
        }
        if version == 2 {
            let archived = rkyv::access::<ArchivedConversationV2, RkyvError>(&aligned).map_err(corrupt)?;
            return rkyv::deserialize::<ConversationV2, RkyvError>(archived).map(Into::into).map_err(corrupt);
        }
        let archived = rkyv::access::<ArchivedConversation, RkyvError>(&aligned).map_err(corrupt)?;
        rkyv::deserialize::<Conversation, RkyvError>(archived).map_err(corrupt)
    }

    pub fn load_id(world_dir: &Path, id: GraphId) -> Result<Self, PersistError> {
        Self::load(&world_dir.join(CHATS_DIR).join(Self::file_name(id)))
    }

    pub fn delete(world_dir: &Path, id: GraphId) -> std::io::Result<()> {
        std::fs::remove_file(world_dir.join(CHATS_DIR).join(Self::file_name(id)))
    }
}

/// Checks the header and returns the format version with the aligned payload.
fn payload(bytes: &[u8]) -> Result<(u16, AlignedVec), PersistError> {
    if bytes.len() < HEADER || bytes[0..4] != MAGIC {
        return Err(PersistError::BadMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if !(1..=VERSION).contains(&version) {
        return Err(PersistError::UnsupportedVersion(version));
    }
    let len = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let body = bytes
        .get(HEADER..HEADER.saturating_add(len))
        .ok_or_else(|| PersistError::Corrupt(format!("expected {len} bytes")))?;
    let mut aligned = AlignedVec::with_capacity(body.len());
    aligned.extend_from_slice(body);
    Ok((version, aligned))
}

/// Builds the summary from either archived layout (their fields have the same names).
macro_rules! summary {
    ($a:expr) => {{
        let a = $a;
        ConversationSummary {
            id: a.id.to_native(),
            title: a.title.as_str().to_owned(),
            participants: a.participants.iter().map(|p| p.to_native()).collect(),
            user_name: a.user_name.as_str().to_owned(),
            updated_at: a.updated_at.to_native(),
            message_count: a.messages.len(),
            last_message: a.messages.last().map(|m| (m.speaker.as_str().to_owned(), m.text.as_str().to_owned())),
        }
    }};
}

/// A world's conversations, newest first. Each file is read with zero-copy access and
/// only the summary is copied.
pub fn list_conversations(world_dir: &Path) -> Vec<Result<ConversationSummary, (PathBuf, String)>> {
    let dir = world_dir.join(CHATS_DIR);
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<Result<ConversationSummary, (PathBuf, String)>> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "chat"))
        .map(|path| {
            let read = || -> Result<ConversationSummary, PersistError> {
                let (version, aligned) = payload(&std::fs::read(&path)?)?;
                let corrupt = |e: RkyvError| PersistError::Corrupt(e.to_string());
                Ok(if version == 1 {
                    summary!(rkyv::access::<ArchivedConversationV1, RkyvError>(&aligned).map_err(corrupt)?)
                } else if version == 2 {
                    summary!(rkyv::access::<ArchivedConversationV2, RkyvError>(&aligned).map_err(corrupt)?)
                } else {
                    summary!(rkyv::access::<ArchivedConversation, RkyvError>(&aligned).map_err(corrupt)?)
                })
            };
            read().map_err(|e| (path.clone(), e.to_string()))
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.as_ref().map(|s| s.updated_at).unwrap_or(i64::MAX)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_listing() {
        let dir = std::env::temp_dir().join(format!("memory-core-chat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut c = Conversation::new("Night watch", vec![1, 2], "Tomás");
        c.push("Tomás", None, "Hello");
        c.push("Kal'tsit", Some(1), "*Nods.*");
        c.characters_remember = false;
        c.moment = Some(2);
        c.save(&dir).unwrap();
        assert_eq!(Conversation::load_id(&dir, c.id).unwrap(), c);
        let list = list_conversations(&dir);
        let s = list[0].as_ref().unwrap();
        assert_eq!((s.title.as_str(), s.message_count, s.participants.clone()), ("Night watch", 2, vec![1, 2]));
        assert_eq!(s.last_message, Some(("Kal'tsit".into(), "*Nods.*".into())));
        Conversation::delete(&dir, c.id).unwrap();
        assert!(list_conversations(&dir).is_empty());
    }

    /// A version 1 file (written before per-chat memory) still loads and lists.
    #[test]
    fn version_1_files_still_load() {
        #[derive(rkyv::Archive, rkyv::Serialize)]
        struct V1 {
            id: GraphId,
            title: String,
            participants: Vec<GraphId>,
            user_name: String,
            created_at: i64,
            updated_at: i64,
            messages: Vec<Utterance>,
        }
        let dir = std::env::temp_dir().join(format!("memory-core-chat-v1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let old = V1 {
            id: 7,
            title: "Old chat".into(),
            participants: vec![1],
            user_name: "Tomás".into(),
            created_at: 1,
            updated_at: 2,
            messages: vec![Utterance { speaker: "Tomás".into(), speaker_id: None, text: "Hi".into(), at: 2 }],
        };
        let payload = rkyv::to_bytes::<RkyvError>(&old).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&payload);
        std::fs::create_dir_all(dir.join(CHATS_DIR)).unwrap();
        std::fs::write(dir.join(CHATS_DIR).join(Conversation::file_name(7)), bytes).unwrap();

        let c = Conversation::load_id(&dir, 7).unwrap();
        assert_eq!((c.title.as_str(), c.messages.len(), c.characters_remember), ("Old chat", 1, true));
        assert_eq!(list_conversations(&dir)[0].as_ref().unwrap().title, "Old chat");
        // Saving it again writes the current version.
        c.save(&dir).unwrap();
        assert_eq!(Conversation::load_id(&dir, 7).unwrap(), c);
    }
}
