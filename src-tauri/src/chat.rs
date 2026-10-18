//! Commands for worlds, conversations (one-to-one and groups), models and images.

use base64::Engine;
use i18n::tr;
use memory_core::conversation::{Conversation, list_conversations};
use memory_core::llm::{SceneTurn, TurnRecord, scene_turn};
use memory_core::text::fold;
use memory_core::world::World;
use memory_core::{GraphId, unix_now};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, COLLAB_PREFIX, Place, Res, emit_status, lang, parse_id};
use crate::timeline::{CharacterMomentDto, MomentDto};

/// Recent messages the actor sees besides its memory.
const WINDOW: usize = 8;

// ------------------------------------------------------------------ DTOs (u64 ids → string)

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterDto {
    id: String,
    name: String,
    tagline: String,
    appearance: String,
    avatar: Option<String>,
    nodes: usize,
    /// Their usual scenario.
    scenario: String,
    /// Moments of the story they belong to (empty = any).
    moments: Vec<CharacterMomentDto>,
    /// In a Collab, the key of the world they come from, and its name.
    world: Option<String>,
    world_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldDto {
    key: String,
    category: String,
    name: String,
    premise: String,
    nodes: usize,
    characters: Vec<CharacterDto>,
    timeline: Vec<MomentDto>,
    /// How the world's chats look.
    style: memory_core::WorldStyle,
    /// Its libraries of shared definitions (category's, universal), existing or not.
    definitions: Vec<LibraryDto>,
    /// For a Collab (`key` = `collab:<slug>`), the worlds it links and at which moment.
    links: Option<Vec<memory_core::collab::CollabLink>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDto {
    /// Graph key in the Memory view ("lib:category", "lib:universal").
    key: String,
    name: String,
    /// Definitions in it (0 if it does not exist yet).
    nodes: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDto {
    speaker: String,
    speaker_id: Option<String>,
    text: String,
    at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummaryDto {
    id: String,
    title: String,
    participants: Vec<String>,
    user_name: String,
    updated_at: i64,
    message_count: usize,
    last_speaker: Option<String>,
    last_text: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDto {
    id: String,
    title: String,
    participants: Vec<String>,
    user_name: String,
    messages: Vec<MessageDto>,
    /// Whether each character's own memory also learns from this chat.
    characters_remember: bool,
    /// Moment of the world's timeline the chat is set at (`None` = the latest).
    moment: Option<u32>,
}

impl From<&Conversation> for ConversationDto {
    fn from(c: &Conversation) -> Self {
        Self {
            id: c.id.to_string(),
            title: c.title.clone(),
            participants: c.participants.iter().map(|p| p.to_string()).collect(),
            user_name: c.user_name.clone(),
            messages: c
                .messages
                .iter()
                .map(|m| MessageDto { speaker: m.speaker.clone(), speaker_id: m.speaker_id.map(|i| i.to_string()), text: m.text.clone(), at: m.at })
                .collect(),
            characters_remember: c.characters_remember,
            moment: c.moment,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenEvent {
    conversation: String,
    speaker_id: String,
    speaker: String,
    token: String,
}

// ------------------------------------------------------------------ worlds

fn avatar_data_url(world: &World, rel: &Option<String>) -> Option<String> {
    let rel = rel.as_ref()?;
    let bytes = std::fs::read(world.dir.join(rel)).ok()?;
    let mime = if rel.ends_with(".jpg") || rel.ends_with(".jpeg") { "image/jpeg" } else { "image/png" };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command]
pub fn list_worlds(state: State<'_, AppState>) -> Res<Vec<WorldDto>> {
    // Careful: the world cache is NEVER emptied. If a world were reopened, the curator and
    // the editor would hold different copies of the same character and one would
    // overwrite the other when saving.
    let listing = state.lib.worlds().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for e in listing.items {
        let key = format!("{}/{}", e.category, e.slug);
        let w = state.world(&key)?;
        let characters = w.characters().iter().map(|h| character_dto(&w, h, None)).collect();
        let premise = w.memory.read(|g| g.node(g.root()).map(|n| n.content.clone()).unwrap_or_default());
        let timeline = w.timeline().iter().map(Into::into).collect();
        let definitions = [("category", memory_core::world::DefinitionLevel::Category), ("universal", memory_core::world::DefinitionLevel::Universal)]
            .into_iter()
            .map(|(k, level)| {
                let h = w.definition_library(level, false).ok().flatten();
                let name = h.as_ref().map(|h| h.meta().name).unwrap_or_else(|| match level {
                    memory_core::world::DefinitionLevel::Universal => i18n::t(lang(), "library.universal").to_owned(),
                    memory_core::world::DefinitionLevel::Category => e.category.clone(),
                });
                // The root is not a definition.
                LibraryDto { key: format!("lib:{k}"), name, nodes: h.map_or(0, |h| h.read(|g| g.node_count().saturating_sub(1))) }
            })
            .collect();
        out.push(WorldDto {
            key,
            category: e.category,
            name: e.meta.name,
            premise,
            nodes: e.nodes,
            characters,
            timeline,
            style: w.style(),
            definitions,
            links: None,
        });
    }
    Ok(out)
}

fn character_dto(w: &World, h: &memory_core::MemoryHandler, world: Option<String>) -> CharacterDto {
    let m = h.meta();
    CharacterDto {
        id: m.id.to_string(),
        avatar: avatar_data_url(w, &m.avatar),
        name: m.name,
        tagline: m.tagline,
        appearance: m.appearance,
        nodes: h.read(|g| g.node_count()),
        scenario: m.scenario,
        moments: m.moments.iter().map(Into::into).collect(),
        world_name: world.as_ref().map(|_| w.memory.meta().name),
        world,
    }
}

/// A Collab, shaped like a world: its characters are those of the linked worlds who
/// belong to the moment it sets for each.
pub fn collab_dto(state: &AppState, slug: &str) -> Res<WorldDto> {
    let key = format!("{COLLAB_PREFIX}{slug}");
    let Place::Collab(c, worlds) = state.place(&key)? else { unreachable!("a collab key") };
    let mut characters = Vec::new();
    for (wkey, w) in &worlds {
        let moment = w.resolve_moment(c.moment_of(wkey)).map(|m| m.id);
        for h in w.characters() {
            let fits = moment.is_none_or(|id| h.read(|g| g.meta.moments.is_empty() || g.meta.moments.iter().any(|m| m.moment == id)));
            if fits {
                characters.push(character_dto(w, &h, Some(wkey.clone())));
            }
        }
    }
    let (name, premise, nodes) = c.memory.read(|g| (g.meta.name.clone(), g.node(g.root()).map(|n| n.content.clone()).unwrap_or_default(), g.node_count()));
    Ok(WorldDto {
        key,
        category: String::new(),
        name,
        premise,
        nodes,
        characters,
        timeline: Vec::new(),
        style: c.style(),
        definitions: Vec::new(),
        links: Some(c.links()),
    })
}

// ------------------------------------------------------------------ conversations

#[tauri::command]
pub fn list_chats(state: State<'_, AppState>, world: String) -> Res<Vec<ConversationSummaryDto>> {
    let place = state.place(&world)?;
    Ok(list_conversations(place.dir())
        .into_iter()
        .filter_map(Result::ok)
        .map(|s| ConversationSummaryDto {
            id: s.id.to_string(),
            title: s.title,
            participants: s.participants.iter().map(|p| p.to_string()).collect(),
            user_name: s.user_name,
            updated_at: s.updated_at,
            message_count: s.message_count,
            last_speaker: s.last_message.as_ref().map(|(w, _)| w.clone()),
            last_text: s.last_message.map(|(_, t)| t),
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewChat {
    world: String,
    participants: Vec<String>,
    title: Option<String>,
    user_name: String,
    /// Default: true.
    characters_remember: Option<bool>,
    /// Moment of the timeline (`None` = the latest).
    moment: Option<u32>,
}

#[tauri::command]
pub fn create_chat(state: State<'_, AppState>, req: NewChat) -> Res<ConversationDto> {
    let place = state.place(&req.world)?;
    if req.participants.is_empty() {
        return Err(tr!(lang(), "app.pick_character"));
    }
    let ids: Vec<GraphId> = req.participants.iter().map(|p| parse_id(p)).collect::<Res<_>>()?;
    let names: Vec<String> = ids.iter().map(|&id| place.character(id).map(|h| h.meta().name)).collect::<Res<_>>()?;
    let title = req.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| names.join(", "));
    let user = if req.user_name.trim().is_empty() { tr!(lang(), "common.user") } else { req.user_name.trim().to_owned() };
    let mut conv = Conversation::new(title.trim(), ids, user);
    conv.characters_remember = req.characters_remember.unwrap_or(true);
    // In a Collab the moments are the ones it sets for each world.
    if let (Place::World(w), Some(m)) = (&place, req.moment) {
        w.moment(m).ok_or_else(|| memory_core::WorldError::NoSuchMoment(m).to_string())?;
        conv.moment = Some(m);
    }
    conv.save(place.dir()).map_err(|e| e.to_string())?;
    // The chat's memory starts right away, so it can be edited before the first message.
    place.chat_memory(&conv)?;
    Ok((&conv).into())
}

#[tauri::command]
pub fn get_chat(state: State<'_, AppState>, world: String, id: String) -> Res<ConversationDto> {
    let place = state.place(&world)?;
    let c = Conversation::load_id(place.dir(), parse_id(&id)?).map_err(|e| e.to_string())?;
    Ok((&c).into())
}

#[tauri::command]
pub fn rename_chat(state: State<'_, AppState>, world: String, id: String, title: String) -> Res<()> {
    let place = state.place(&world)?;
    let mut c = Conversation::load_id(place.dir(), parse_id(&id)?).map_err(|e| e.to_string())?;
    c.title = title.trim().to_owned();
    c.save(place.dir()).map_err(|e| e.to_string())?;
    place.rename_chat_memory(c.id, &c.title)
}

/// Whether the characters' own memories also learn from this chat (off = isolated storyline).
#[tauri::command]
pub fn set_chat_memory_mode(state: State<'_, AppState>, world: String, id: String, characters_remember: bool) -> Res<ConversationDto> {
    let place = state.place(&world)?;
    let mut c = Conversation::load_id(place.dir(), parse_id(&id)?).map_err(|e| e.to_string())?;
    c.characters_remember = characters_remember;
    c.save(place.dir()).map_err(|e| e.to_string())?;
    Ok((&c).into())
}

/// Moves a chat to another moment of the timeline (`None` = the latest).
#[tauri::command]
pub fn set_chat_moment(state: State<'_, AppState>, world: String, id: String, moment: Option<u32>) -> Res<ConversationDto> {
    let w = state.world(&world)?;
    if let Some(m) = moment {
        w.moment(m).ok_or_else(|| memory_core::WorldError::NoSuchMoment(m).to_string())?;
    }
    let mut c = Conversation::load_id(&w.dir, parse_id(&id)?).map_err(|e| e.to_string())?;
    c.moment = moment;
    c.save(&w.dir).map_err(|e| e.to_string())?;
    Ok((&c).into())
}

#[tauri::command]
pub fn delete_chat(state: State<'_, AppState>, world: String, id: String) -> Res<()> {
    let place = state.place(&world)?;
    let id = parse_id(&id)?;
    Conversation::delete(place.dir(), id).map_err(|e| e.to_string())?;
    place.delete_chat_memory(id)
}

/// Who answers: the chosen one; otherwise someone named in the message; otherwise, in a
/// group, the next one in order after whoever spoke last.
fn pick_speaker(place: &Place, c: &Conversation, text: &str, explicit: Option<GraphId>) -> GraphId {
    if let Some(id) = explicit.filter(|id| c.participants.contains(id)) {
        return id;
    }
    if c.participants.len() == 1 {
        return c.participants[0];
    }
    let t = fold(text);
    if let Some(&id) = c.participants.iter().find(|&&id| place.character(id).is_ok_and(|h| t.contains(&fold(&h.meta().name)))) {
        return id;
    }
    let last = c.messages.iter().rev().find_map(|m| m.speaker_id);
    match last.and_then(|l| c.participants.iter().position(|&p| p == l)) {
        Some(i) => c.participants[(i + 1) % c.participants.len()],
        None => c.participants[0],
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendReq {
    world: String,
    id: String,
    /// Empty = the character carries on the scene without the user speaking.
    text: String,
    /// `None` = automatic.
    speaker: Option<String>,
}

#[tauri::command]
pub async fn send_message(app: AppHandle, state: State<'_, AppState>, req: SendReq) -> Res<MessageDto> {
    let _turn = state.turn_lock.lock().await;
    let place = state.place(&req.world)?;
    let conv_id = parse_id(&req.id)?;
    let mut conv = Conversation::load_id(place.dir(), conv_id).map_err(|e| e.to_string())?;
    let explicit = req.speaker.as_deref().filter(|s| !s.is_empty()).map(parse_id).transpose()?;
    let speaker = pick_speaker(&place, &conv, &req.text, explicit);
    let speaker_name = place.character(speaker)?.meta().name;

    let history: Vec<(String, String)> = conv.messages.iter().map(|m| (m.speaker.clone(), m.text.clone())).collect();
    let text = req.text.trim().to_owned();
    if !text.is_empty() {
        conv.push(conv.user_name.clone(), None, text.clone());
        conv.save(place.dir()).map_err(|e| e.to_string())?;
    }

    let chat_memory = place.chat_memory(&conv)?;
    let (actor, curator) = state.clients(&app).await?;
    emit_status(&app, tr!(lang(), "chat.typing", name = speaker_name), true);
    let token_app = app.clone();
    let (conv_s, speaker_s, name_s) = (req.id.clone(), speaker.to_string(), speaker_name.clone());
    let on_token = move |t: &str| {
        let _ = token_app.emit(
            "chat-token",
            TokenEvent { conversation: conv_s.clone(), speaker_id: speaker_s.clone(), speaker: name_s.clone(), token: t.to_owned() },
        );
    };
    let worlds = place.world_refs();
    let collab = match &place {
        Place::Collab(c, _) => Some((c.as_ref(), worlds.as_slice())),
        Place::World(_) => None,
    };
    let turn = SceneTurn {
        world: place.home_of(speaker)?,
        speaker,
        present: &conv.participants,
        user_name: &conv.user_name,
        history: &history,
        text: &text,
        window: WINDOW,
        chat: Some(&chat_memory),
        moment: conv.moment,
        collab,
    };
    let result = scene_turn(&actor, turn, on_token).await;
    let (reply, recall) = match result {
        Ok(r) => r,
        Err(e) => {
            emit_status(&app, tr!(lang(), "chat.actor_error", error = e), false);
            return Err(e.to_string());
        }
    };
    state.last_context.lock().insert(conv_id, recall.context);
    conv.push(speaker_name.clone(), Some(speaker), reply.clone());
    conv.save(place.dir()).map_err(|e| e.to_string())?;
    emit_status(&app, "", false);

    // The chat's memory records the turn; if the chat allows it, everyone present also
    // stores it in their copy of their memory, from their own point of view.
    let mut lines = Vec::new();
    if !text.is_empty() {
        lines.push((conv.user_name.clone(), text));
    }
    lines.push((speaker_name.clone(), reply.clone()));
    // What is learned is dated at the chat's moment of the story (in a Collab, the chat's
    // memory is timeless and each copy is dated at its own world's moment).
    let turn = TurnRecord { lines, story: place.chat_story(conv.moment) };
    state.submit_to_curator(&app, &req.world, &format!("chat:{conv_id}"), &chat_memory, &curator, turn.clone());
    if conv.characters_remember {
        for &p in &conv.participants {
            if let Ok(h) = place.chat_character(conv.id, p) {
                let turn = TurnRecord { story: place.story_of(p, conv.moment), ..turn.clone() };
                state.submit_to_curator(&app, &req.world, &format!("chat:{conv_id}:{p}"), &h, &curator, turn);
            }
        }
    }
    Ok(MessageDto { speaker: speaker_name, speaker_id: Some(speaker.to_string()), text: reply, at: unix_now() })
}

#[tauri::command]
pub fn last_context(state: State<'_, AppState>, id: String) -> Res<Option<String>> {
    Ok(state.last_context.lock().get(&parse_id(&id)?).cloned())
}

// ------------------------------------------------------------------ models and images

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusDto {
    slots: Vec<model_hub::SlotStatus>,
    home: String,
    image_ready: bool,
    image_info: String,
    /// If the mmproj did not load and the actor works without vision, the reason.
    vision_error: Option<String>,
}

#[tauri::command]
pub async fn model_status(state: State<'_, AppState>) -> Res<ModelStatusDto> {
    let reg = state.sup.registry();
    let kit = reg.diffusion_kit(&state.sup.config().image.model);
    let l = lang();
    let (image_ready, image_info) = match (&kit, state.sup.config().sd_cli()) {
        (_, None) => (false, tr!(l, "models.no_sd")),
        (None, _) => (false, tr!(l, "models.no_image_model")),
        (Some(k), _) if !k.is_complete() => {
            let list: Vec<&str> = k.missing().iter().map(|m| i18n::t(l, &format!("kit.missing.{m}"))).collect();
            (false, tr!(l, "models.image_missing", list = list.join(", ")))
        }
        (Some(k), _) => (true, k.model.file_name.clone()),
    };
    Ok(ModelStatusDto {
        slots: state.sup.status().await,
        home: state.home.display().to_string(),
        image_ready,
        image_info,
        vision_error: state.sup.vision_error(model_hub::Slot::Actor),
    })
}

#[tauri::command]
pub async fn stop_models(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    state.sup.stop_all().await;
    emit_status(&app, tr!(lang(), "models.stopped"), false);
    Ok(())
}
