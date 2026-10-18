//! Dual inference pipeline over llama-server or Ollama (`llm` feature).
//!
//! ```text
//!  user ─▶ MemoryHandler::recall ─▶ actor (7B–27B, mmproj) ─▶ reply to the UI
//!                                                 │
//!                                   TurnRecord ───┘ (channel, does not block)
//!                                                 ▼
//!                       ConsolidationWorker: curator (1B–4B, GBNF) ─▶ apply_ops ─▶ .mem
//! ```
//!
//! The worker processes turns one at a time: two curator writes never compete, and if
//! the curator is slow the turns queue up instead of getting lost.

use std::time::Duration;

use i18n::{Lang, Prompt, tr};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::handler::{MemoryHandler, Recall};
use crate::model::GraphId;
use crate::ops::{ApplyReport, OpBatch};
use crate::prompts::{
    CURATOR_GBNF, CURATOR_SCHEMA, actor_system_prompt, chat_curator_system_prompt, curator_system_prompt, curator_user_message, scene_actor_prompt,
};
use crate::world::{SceneRecall, World};

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected server response: {0}")]
    Shape(String),
    #[error("the model returned invalid JSON: {0}")]
    BadJson(#[from] serde_json::Error),
    #[error("save: {0}")]
    Persist(#[from] crate::persist::PersistError),
    #[error("world: {0}")]
    World(#[from] crate::world::WorldError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    /// Base64 images (without the `data:` prefix), JPEG or PNG. They need the actor's
    /// model to be launched with its `--mmproj`.
    pub images: Vec<String>,
}

impl ChatMessage {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self { role, content: content.into(), images: Vec::new() }
    }
    pub fn system(c: impl Into<String>) -> Self {
        Self::new(Role::System, c)
    }
    pub fn user(c: impl Into<String>) -> Self {
        Self::new(Role::User, c)
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Self::new(Role::Assistant, c)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `llama-server` (OpenAI endpoint `/v1/chat/completions` + `grammar` field).
    LlamaServer,
    /// Ollama (`/api/chat` + `format` with a JSON Schema).
    Ollama,
}

#[derive(Debug, Clone)]
pub struct LlmEndpoint {
    pub backend: Backend,
    /// e.g. `http://127.0.0.1:8080`
    pub base_url: String,
    /// Model name (Ollama requires it; llama-server ignores it when it serves only one).
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    /// Turns off reasoning mode in hybrid models (Qwen3.5…).
    pub disable_thinking: bool,
    pub timeout: Duration,
}

impl LlmEndpoint {
    /// Preset for the curator: deterministic, short output, no reasoning.
    pub fn curator(backend: Backend, base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            backend,
            base_url: base_url.into(),
            model: model.into(),
            temperature: 0.2,
            max_tokens: 900,
            disable_thinking: true,
            timeout: Duration::from_secs(120),
        }
    }

    /// Preset for the roleplay actor.
    pub fn actor(backend: Backend, base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            backend,
            base_url: base_url.into(),
            model: model.into(),
            temperature: 0.85,
            max_tokens: 700,
            disable_thinking: true,
            timeout: Duration::from_secs(300),
        }
    }
}

/// Output constraint.
#[derive(Debug, Clone, Copy)]
pub enum Constraint<'a> {
    None,
    Grammar(&'a str),
    JsonSchema(&'a Value),
}

#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    pub endpoint: LlmEndpoint,
}

impl LlmClient {
    pub fn new(endpoint: LlmEndpoint) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder().timeout(endpoint.timeout).build()?;
        Ok(Self { http, endpoint })
    }

    /// Request body (kept apart so it can be tested without a network).
    pub fn request_body(&self, messages: &[ChatMessage], constraint: Constraint<'_>) -> (String, Value) {
        let ep = &self.endpoint;
        let base = ep.base_url.trim_end_matches('/');
        match ep.backend {
            Backend::LlamaServer => {
                let msgs: Vec<Value> = messages
                    .iter()
                    .map(|m| {
                        if m.images.is_empty() {
                            json!({ "role": m.role, "content": m.content })
                        } else {
                            let mut parts = vec![json!({ "type": "text", "text": m.content })];
                            parts.extend(m.images.iter().map(|b64| {
                                json!({ "type": "image_url", "image_url": { "url": format!("data:{};base64,{b64}", sniff_mime(b64)) } })
                            }));
                            json!({ "role": m.role, "content": parts })
                        }
                    })
                    .collect();
                let mut body = json!({
                    "model": ep.model,
                    "messages": msgs,
                    "temperature": ep.temperature,
                    "max_tokens": ep.max_tokens,
                    "stream": false,
                });
                if ep.disable_thinking {
                    body["chat_template_kwargs"] = json!({ "enable_thinking": false });
                }
                match constraint {
                    Constraint::None => {}
                    Constraint::Grammar(g) => body["grammar"] = json!(g),
                    Constraint::JsonSchema(s) => {
                        body["response_format"] = json!({ "type": "json_schema", "json_schema": { "schema": s } })
                    }
                }
                (format!("{base}/v1/chat/completions"), body)
            }
            Backend::Ollama => {
                let msgs: Vec<Value> = messages
                    .iter()
                    .map(|m| {
                        let mut v = json!({ "role": m.role, "content": m.content });
                        if !m.images.is_empty() {
                            v["images"] = json!(m.images);
                        }
                        v
                    })
                    .collect();
                let mut body = json!({
                    "model": ep.model,
                    "messages": msgs,
                    "stream": false,
                    "options": { "temperature": ep.temperature, "num_predict": ep.max_tokens },
                });
                if ep.disable_thinking {
                    body["think"] = json!(false);
                }
                match constraint {
                    Constraint::None => {}
                    // Ollama does not accept GBNF: the caller should pass the JSON Schema.
                    Constraint::Grammar(_) => body["format"] = json!("json"),
                    Constraint::JsonSchema(s) => body["format"] = s.clone(),
                }
                (format!("{base}/api/chat"), body)
            }
        }
    }

    pub async fn chat(&self, messages: &[ChatMessage], constraint: Constraint<'_>) -> Result<String, LlmError> {
        let (url, body) = self.request_body(messages, constraint);
        let resp: Value = self.http.post(url).json(&body).send().await?.error_for_status()?.json().await?;
        let content = match self.endpoint.backend {
            Backend::LlamaServer => resp.pointer("/choices/0/message/content"),
            Backend::Ollama => resp.pointer("/message/content"),
        };
        content
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| LlmError::Shape(resp.to_string().chars().take(300).collect()))
    }

    /// Like `chat`, but hands the text in pieces to `on_token` as it is generated
    /// (llama-server's SSE). With Ollama it is delivered all at once at the end.
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        constraint: Constraint<'_>,
        mut on_token: impl FnMut(&str),
    ) -> Result<String, LlmError> {
        if self.endpoint.backend != Backend::LlamaServer {
            let full = self.chat(messages, constraint).await?;
            on_token(&full);
            return Ok(full);
        }
        let (url, mut body) = self.request_body(messages, constraint);
        body["stream"] = json!(true);
        let mut resp = self.http.post(url).json(&body).send().await?.error_for_status()?;
        let mut pending: Vec<u8> = Vec::new();
        let mut full = String::new();
        while let Some(chunk) = resp.chunk().await? {
            pending.extend_from_slice(&chunk);
            while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = pending.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let Some(data) = line.trim().strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    return Ok(full);
                }
                if let Ok(v) = serde_json::from_str::<Value>(data)
                    && let Some(t) = v.pointer("/choices/0/delta/content").and_then(Value::as_str)
                    && !t.is_empty()
                {
                    full.push_str(t);
                    on_token(t);
                }
            }
        }
        Ok(full)
    }

    /// The right constraint for the curator on this backend.
    fn curator_constraint(&self, schema: &'static Value) -> Constraint<'static> {
        match self.endpoint.backend {
            Backend::LlamaServer => Constraint::Grammar(CURATOR_GBNF),
            Backend::Ollama => Constraint::JsonSchema(schema),
        }
    }
}

fn sniff_mime(b64: &str) -> &'static str {
    if b64.starts_with("iVBOR") { "image/png" } else if b64.starts_with("UklGR") { "image/webp" } else { "image/jpeg" }
}

fn curator_schema() -> &'static Value {
    static SCHEMA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    SCHEMA.get_or_init(|| serde_json::from_str(CURATOR_SCHEMA).expect("memory_ops.schema.json is valid JSON"))
}

// ---------------------------------------------------------------------- actor turn

/// A full actor turn. `recent` is a SHORT window (e.g. the last 4–6 messages) for local
/// coherence; the long-term continuity comes from memory.
pub async fn actor_turn(
    handler: &MemoryHandler,
    actor: &LlmClient,
    recent: &[ChatMessage],
    user: ChatMessage,
) -> Result<(String, Recall), LlmError> {
    let recall = handler.recall(&user.content);
    let system = handler.read(|g| actor_system_prompt(handler.lang(), &g.meta, &recall.context));
    let mut messages = Vec::with_capacity(recent.len() + 2);
    messages.push(ChatMessage::system(system));
    messages.extend(recent.iter().cloned());
    messages.push(user);
    let reply = actor.chat(&messages, Constraint::None).await?;
    Ok((reply, recall))
}

// ---------------------------------------------------------------------- scene turn

/// Data for a turn inside a world (one-to-one or group chat).
pub struct SceneTurn<'a> {
    pub world: &'a World,
    /// Who answers.
    pub speaker: GraphId,
    /// Every character in the conversation (including `speaker`).
    pub present: &'a [GraphId],
    /// The user's name in the scene.
    pub user_name: &'a str,
    /// Previous conversation as `(speaker, text)`, in order.
    pub history: &'a [(String, String)],
    /// The user's message; empty = the character carries on the scene by itself.
    pub text: &'a str,
    /// Recent messages the actor sees besides its memory.
    pub window: usize,
    /// The conversation's memory (`World::chat_memory`), if the scene is a saved chat.
    pub chat: Option<&'a MemoryHandler>,
    /// Moment of the world's timeline the scene is set at (`None` = the latest).
    pub moment: Option<u32>,
    /// The scene happens in a Collab, linking these worlds (`category/world` keys). Then
    /// `world` and `moment` are ignored: each speaker stays in their own world, at the
    /// moment the Collab sets for it.
    pub collab: Option<(&'a crate::collab::Collab, &'a [(&'a str, &'a World)])>,
}

/// Scene recall → prompt → actor (streaming). Returns the cleaned reply and the recall
/// used (to show the context). It does not consolidate: the caller does that, sending
/// the `TurnRecord` to the curators of the people present.
pub async fn scene_turn(actor: &LlmClient, t: SceneTurn<'_>, on_token: impl FnMut(&str)) -> Result<(String, SceneRecall), LlmError> {
    let query = if t.text.trim().is_empty() {
        t.history.iter().rev().take(2).map(|(_, x)| x.as_str()).collect::<Vec<_>>().join("\n")
    } else {
        t.text.to_owned()
    };
    // The speaker's own world and moment (in a Collab, the ones it sets for that world).
    let (home, moment_id) = match t.collab {
        Some((c, worlds)) => {
            let (w, key) = crate::collab::Collab::world_of(worlds, t.speaker).ok_or(crate::world::WorldError::NoSuchCharacter(t.speaker))?;
            (w, c.moment_of(&key))
        }
        None => (t.world, t.moment),
    };
    let lang = home.lang();
    let me = home.character(t.speaker)?.meta();
    let moment = home.resolve_moment(moment_id);
    let recall = match t.collab {
        Some((c, worlds)) => c.scene_recall(worlds, t.speaker, &query, t.present, t.chat)?,
        None => {
            let story = moment.as_ref().map_or(crate::model::STORY_PRESENT, |m| m.at);
            home.scene_recall_at(t.speaker, &query, t.present, t.chat, story)?
        }
    };
    let name_of = |id: GraphId| match t.collab {
        Some((_, worlds)) => crate::collab::Collab::world_of(worlds, id).and_then(|(w, _)| w.character(id).ok()).map(|h| h.meta().name),
        None => home.character(id).ok().map(|h| h.meta().name),
    };
    let others: Vec<String> = t.present.iter().filter(|&&id| id != t.speaker).filter_map(|&id| name_of(id)).collect();
    let system = scene_actor_prompt(lang, &me, &home.memory.meta(), &recall.context, &others, t.user_name, moment.as_ref());

    let mut messages = vec![ChatMessage::system(system)];
    let start = t.history.len().saturating_sub(t.window);
    for (who, text) in &t.history[start..] {
        messages.push(if *who == me.name { ChatMessage::assistant(text.clone()) } else { ChatMessage::user(format!("{who}: {text}")) });
    }
    if t.text.trim().is_empty() {
        messages.push(ChatMessage::user(tr!(lang, "prompt.scene.continue", name = me.name)));
    } else {
        messages.push(ChatMessage::user(format!("{}: {}", t.user_name, t.text.trim())));
    }
    let raw = actor.chat_stream(&messages, Constraint::None, on_token).await?;
    Ok((clean_reply(&raw, &me.name), recall))
}

/// Drops the "Name:" the model sometimes starts with and the surrounding whitespace.
pub fn clean_reply(raw: &str, name: &str) -> String {
    let t = raw.trim();
    for prefix in [format!("{name}:"), format!("**{name}:**"), format!("**{name}**:")] {
        if let Some(rest) = t.strip_prefix(prefix.as_str()) {
            return rest.trim().to_owned();
        }
    }
    t.to_owned()
}

// ---------------------------------------------------------------------- appearance from images

/// Grammar of the visual description: `{"aspects":[{"aspect":…,"description":…,"importance":…}]}`.
pub const APPEARANCE_GBNF: &str = r#"root    ::= "{" ws "\"aspects\"" ws ":" ws "[" ws item ( ws "," ws item ){2,11} ws "]" ws "}"
item    ::= "{" ws "\"aspect\"" ws ":" ws label ws "," ws "\"description\"" ws ":" ws text ws "," ws "\"importance\"" ws ":" ws unit ws "}"
label   ::= "\"" char{1,32} "\""
text    ::= "\"" char{8,420} "\""
unit    ::= "0." [1-9] [0-9]? | "1" | "1.0"
char    ::= [^"\\\x7F\x00-\x1F] | "\\" ( ["\\/bfnrt] | "u" [0-9a-fA-F]{4} )
ws      ::= | " " | "\n" [ \t]{0,20}
"#;

/// Asks a vision model (the reader, or the actor with its mmproj) for a detailed,
/// structured visual description of `name` from one or more reference images. The
/// aspects come back in `lang`.
pub async fn describe_appearance(
    vision: &LlmClient,
    lang: Lang,
    images_b64: &[String],
    name: &str,
    hint: &str,
) -> Result<Vec<crate::world::Aspect>, LlmError> {
    let mut user = if images_b64.len() > 1 {
        tr!(lang, "vision.many", count = images_b64.len(), name = name)
    } else {
        tr!(lang, "vision.one", name = name)
    };
    if !hint.trim().is_empty() {
        user.push(' ');
        user.push_str(&tr!(lang, "vision.context", hint = hint.trim()));
    }
    let mut msg = ChatMessage::user(user);
    msg.images.extend(images_b64.iter().cloned());
    let mut endpoint = vision.endpoint.clone();
    endpoint.temperature = 0.2;
    endpoint.max_tokens = 2600;
    let client = LlmClient::new(endpoint)?;
    let constraint = match client.endpoint.backend {
        Backend::LlamaServer => Constraint::Grammar(APPEARANCE_GBNF),
        Backend::Ollama => Constraint::None,
    };
    let raw = client.chat(&[ChatMessage::system(i18n::prompt(lang, Prompt::AppearanceReader)), msg], constraint).await?;
    parse_aspects(&raw)
}

/// Reads `{"aspects":[…]}` (tolerating text around it).
pub fn parse_aspects(raw: &str) -> Result<Vec<crate::world::Aspect>, LlmError> {
    let t = raw.trim();
    let json = match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if a < b => &t[a..=b],
        _ => t,
    };
    let v: Value = serde_json::from_str(json)?;
    let items = v["aspects"].as_array().ok_or_else(|| LlmError::Shape("\"aspects\" is missing".into()))?;
    Ok(items
        .iter()
        .filter_map(|i| {
            let label = i["aspect"].as_str()?.trim();
            let content = i["description"].as_str()?.trim();
            (!label.is_empty() && !content.is_empty()).then(|| crate::world::Aspect {
                label: label.to_owned(),
                content: content.to_owned(),
                weight: i["importance"].as_f64().unwrap_or(0.6).clamp(0.1, 1.0) as f32,
            })
        })
        .collect())
}

// ---------------------------------------------------------------------- background curator

/// A scene turn: `(speaker, text)` in order. The curator of each character present
/// gets the same turn and stores it from their own point of view.
#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub lines: Vec<(String, String)>,
    /// Story time of the turn (`World::story_now` of the chat's moment): what the curator
    /// learns is dated then.
    pub story: crate::model::StoryTime,
}

impl TurnRecord {
    /// Classic user ↔ character dialogue, outside any timeline.
    pub fn dialog(user_name: impl Into<String>, user_text: impl Into<String>, char_name: impl Into<String>, reply: impl Into<String>) -> Self {
        Self { lines: vec![(user_name.into(), user_text.into()), (char_name.into(), reply.into())], story: crate::model::STORY_PRESENT }
    }

    pub fn text(&self) -> String {
        self.lines.iter().map(|(w, t)| format!("{w} {t}")).collect::<Vec<_>>().join("\n")
    }
}

/// Evaluates a turn with the curator and applies the resulting operations.
pub async fn consolidate(handler: &MemoryHandler, curator: &LlmClient, turn: &TurnRecord) -> Result<ApplyReport, LlmError> {
    let lang = handler.lang();
    let block = handler.curator_context_with(&turn.text(), &[], turn.story);
    let (name, root, scope) = handler.read(|g| (g.meta.name.clone(), g.root(), g.scope()));
    // A chat's memory is kept by a neutral chronicler; a character's, from their own eyes.
    let system = if scope == crate::model::Scope::Chat { chat_curator_system_prompt(lang, &name, root) } else { curator_system_prompt(lang, &name, root) };
    let messages = [
        ChatMessage::system(system),
        ChatMessage::user(curator_user_message(lang, &block, &turn.lines)),
    ];
    let raw = curator.chat(&messages, curator.curator_constraint(curator_schema())).await?;
    let batch = OpBatch::parse_lenient(&raw)?;
    let report = handler.apply_ops_at(&batch, turn.story);
    let h = handler.clone();
    tokio::task::spawn_blocking(move || h.save_if_dirty())
        .await
        .map_err(|e| LlmError::Shape(format!("save task: {e}")))??;
    Ok(report)
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) of a Unix time, for logs.
pub fn utc_stamp(unix: i64) -> String {
    let (days, secs) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Appends a curator's result to `path` (`logs/curator.log`) when something went wrong:
/// each rejected operation with the reason, or the error. Clean turns are not written.
/// Best effort: a log that cannot be written never stops the chat.
pub fn log_curator_result(path: &std::path::Path, lang: Lang, graph: &str, result: &Result<ApplyReport, LlmError>) {
    let time = utc_stamp(crate::model::unix_now());
    let mut text = match result {
        Ok(r) if r.rejected.is_empty() => return,
        Ok(r) => {
            let mut s = tr!(lang, "curator.log.rejected", time = time, graph = graph, applied = r.applied, n = r.rejected.len());
            for op in &r.rejected {
                s.push('\n');
                s.push_str(&tr!(lang, "curator.log.op", op = op));
            }
            s
        }
        Err(e) => tr!(lang, "curator.log.error", time = time, graph = graph, error = e),
    };
    text.push('\n');
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Consolidation queue: a single consumer per character.
pub struct ConsolidationWorker {
    tx: mpsc::Sender<TurnRecord>,
}

impl ConsolidationWorker {
    /// `on_result` gets every report (or error) — in Tauri it emits an event to the
    /// frontend so it refreshes the graph. It must run inside a tokio runtime.
    pub fn spawn<F>(handler: MemoryHandler, curator: LlmClient, on_result: F) -> (Self, JoinHandle<()>)
    where
        F: Fn(Result<ApplyReport, LlmError>) + Send + Sync + 'static,
    {
        Self::spawn_gated(handler, curator, None, on_result)
    }

    /// Like `spawn`, but each consolidation waits for a permit from `gate`. With one
    /// `Semaphore(1)` shared by every character of a scene, only one curator uses the
    /// server at a time and the actor always keeps a free slot.
    pub fn spawn_gated<F>(
        handler: MemoryHandler,
        curator: LlmClient,
        gate: Option<std::sync::Arc<tokio::sync::Semaphore>>,
        on_result: F,
    ) -> (Self, JoinHandle<()>)
    where
        F: Fn(Result<ApplyReport, LlmError>) + Send + Sync + 'static,
    {
        let (tx, mut rx) = mpsc::channel::<TurnRecord>(64);
        let task = tokio::spawn(async move {
            while let Some(turn) = rx.recv().await {
                let _permit = match &gate {
                    Some(g) => g.acquire().await.ok(),
                    None => None,
                };
                on_result(consolidate(&handler, &curator, &turn).await);
            }
        });
        (Self { tx }, task)
    }

    /// Queues without blocking. Returns `false` if the queue is full or the worker closed.
    pub fn submit(&self, turn: TurnRecord) -> bool {
        self.tx.try_send(turn).is_ok()
    }
}
