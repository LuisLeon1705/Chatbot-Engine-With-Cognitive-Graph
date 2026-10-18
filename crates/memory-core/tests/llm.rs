#![cfg(feature = "llm")]
//! The dual pipeline against a fake llama-server/Ollama (no real network).

use memory_core::llm::*;
use memory_core::*;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Minimal HTTP server: answers one request with `reply` and returns the JSON body it got.
async fn fake_server(reply: Value) -> (String, tokio::task::JoinHandle<Value>) {
    fake_raw(reply.to_string(), "application/json").await
}

/// Same, but sends `payload` as it is with the given `content-type` (e.g. SSE).
async fn fake_raw(payload: String, content_type: &'static str) -> (String, tokio::task::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        let body = loop {
            let mut chunk = [0u8; 4096];
            let n = sock.read(&mut chunk).await.unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf);
            if let Some(h) = text.find("\r\n\r\n") {
                let len: usize = text[..h]
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                    .unwrap_or(0);
                if buf.len() >= h + 4 + len {
                    break serde_json::from_slice::<Value>(&buf[h + 4..h + 4 + len]).unwrap();
                }
            }
        };
        let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}", payload.len());
        sock.write_all(resp.as_bytes()).await.unwrap();
        body
    });
    (url, task)
}

fn character() -> MemoryHandler {
    let meta = GraphMeta::new(Scope::Character, "Lyra", 0);
    let mut g = MemoryGraph::new(meta, "Elven librarian", unix_now());
    let root = g.root();
    g.add_trait(root, NodeKind::Trait, "Distrustful", "Finds it hard to trust", 0.7, unix_now()).unwrap();
    let path = std::env::temp_dir().join(format!("memory-core-llm-{}.mem", std::process::id()));
    MemoryHandler::new(g, path, HandlerConfig::default())
}

#[tokio::test]
async fn curator_on_llama_server_uses_grammar_and_applies_ops() {
    let ops = r#"{"ops":[{"op":"create_node","ref":"$a","kind":"entity","label":"Tomás","content":"Visitor who brought the atlas.","weight":0.8,"valence":0.5},{"op":"update_node","target":1,"weight_delta":-0.05}]}"#;
    let (url, server) = fake_server(json!({ "choices": [{ "message": { "role": "assistant", "content": ops } }] })).await;
    let h = character();
    let curator = LlmClient::new(LlmEndpoint::curator(Backend::LlamaServer, url, "curator")).unwrap();

    let turn = TurnRecord::dialog("User", "My name is Tomás", "Lyra", "Nice to meet you, Tomás.");
    let report = consolidate(&h, &curator, &turn).await.unwrap();

    let body = server.await.unwrap();
    assert!(body["grammar"].as_str().unwrap().contains("create_node"));
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    let user_msg = body["messages"][1]["content"].as_str().unwrap();
    assert!(user_msg.contains("#1 [trait] Distrustful") && user_msg.contains("User: My name is Tomás"), "{user_msg}");
    assert!(user_msg.starts_with("ACTIVE MEMORY:") && user_msg.ends_with("OUTPUT:"));

    assert_eq!(report.applied, 2, "{:?}", report.rejected);
    assert!(h.read(|g| g.find_by_label("tomas", None)).is_some());
    assert!(!h.is_dirty(), "consolidate saves to disk");
    assert!(MemoryGraph::load(h.path()).unwrap().find_by_label("Tomás", Some(NodeKind::Entity)).is_some());
}

#[tokio::test]
async fn actor_on_ollama_sends_memory_and_images() {
    let (url, server) = fake_server(json!({ "message": { "role": "assistant", "content": "*looks up* Who are you?" } })).await;
    let h = character();
    let actor = LlmClient::new(LlmEndpoint::actor(Backend::Ollama, url, "gemma4-12b")).unwrap();
    let mut user = ChatMessage::user("Look at this photo, do you trust me?");
    user.images.push("iVBORw0KGgo=".into());

    let (reply, recall) = actor_turn(&h, &actor, &[], user).await.unwrap();
    let body = server.await.unwrap();
    assert_eq!(reply, "*looks up* Who are you?");
    assert_eq!(body["model"], "gemma4-12b");
    assert!(body["messages"][0]["content"].as_str().unwrap().contains("<active_memory>"));
    assert_eq!(body["messages"][1]["images"][0], "iVBORw0KGgo=");
    assert!(recall.active.iter().any(|a| a.id == 1), "the personality always gets in");
}

#[test]
fn llama_server_body_packs_images_as_data_urls_and_ollama_uses_schema() {
    let llama = LlmClient::new(LlmEndpoint::actor(Backend::LlamaServer, "http://127.0.0.1:8080/", "x")).unwrap();
    let mut m = ChatMessage::user("What do you see?");
    m.images.push("/9j/4AAQ".into());
    let (url, body) = llama.request_body(&[m], Constraint::None);
    assert_eq!(url, "http://127.0.0.1:8080/v1/chat/completions");
    assert_eq!(body["messages"][0]["content"][1]["image_url"]["url"], "data:image/jpeg;base64,/9j/4AAQ");

    let schema: Value = serde_json::from_str(memory_core::prompts::CURATOR_SCHEMA).unwrap();
    let ollama = LlmClient::new(LlmEndpoint::curator(Backend::Ollama, "http://127.0.0.1:11434", "qwen3.5:4b")).unwrap();
    let (url, body) = ollama.request_body(&[ChatMessage::user("hello")], Constraint::JsonSchema(&schema));
    assert_eq!(url, "http://127.0.0.1:11434/api/chat");
    assert_eq!(body["format"]["title"], "OpBatch");
    assert_eq!(body["think"], false);
}

#[tokio::test]
async fn streaming_delivers_tokens_and_scene_turn_cleans_reply() {
    // llama-server SSE split into several events, with the character's name in front.
    let events = ["Lyra:", " *Looks", " up.*", " Hello, Tomás."]
        .iter()
        .map(|t| format!("data: {}\n\n", json!({ "choices": [{ "delta": { "content": t } }] })))
        .collect::<String>()
        + "data: [DONE]\n\n";
    let (url, server) = fake_raw(events, "text/event-stream").await;

    let dir = std::env::temp_dir().join(format!("memory-core-scene-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::new(&dir, HandlerConfig::default());
    let world = lib.create_world("Tests", "Library", "A library in ruins.", "").unwrap();
    let lyra = world.create_character("Lyra", "Elven librarian", "Keeper of the library", "").unwrap().id();

    let actor = LlmClient::new(LlmEndpoint::actor(Backend::LlamaServer, url, "actor")).unwrap();
    let mut tokens = Vec::new();
    let history = vec![("Tomás".to_string(), "Good evening.".to_string())];
    let conv = memory_core::conversation::Conversation::new("Evening", vec![lyra], "Tomás");
    let chat = world.chat_memory(&conv).unwrap();
    let turn = SceneTurn {
        world: &world,
        speaker: lyra,
        present: &[lyra],
        user_name: "Tomás",
        history: &history,
        text: "Can you help me?",
        window: 8,
        chat: Some(&chat),
        moment: None,
        collab: None,
    };
    let (reply, recall) = scene_turn(&actor, turn, |t| tokens.push(t.to_owned())).await.unwrap();

    assert_eq!(tokens.len(), 4, "one event per piece");
    assert_eq!(reply, "*Looks up.* Hello, Tomás.", "the leading «Lyra:» is dropped");
    assert!(recall.context.contains("<active_memory>") && recall.context.contains("<chat_memory>"), "{}", recall.context);
    assert!(recall.chat.is_some());
    let body = server.await.unwrap();
    assert_eq!(body["stream"], true);
    let msgs = body["messages"].as_array().unwrap();
    assert!(msgs[0]["content"].as_str().unwrap().contains("The one talking to you is Tomás"));
    assert_eq!(msgs[1]["content"], "Tomás: Good evening.");
    assert_eq!(msgs.last().unwrap()["content"], "Tomás: Can you help me?");
}

#[test]
fn curator_log_keeps_only_what_failed() {
    assert_eq!(utc_stamp(0), "1970-01-01 00:00:00");
    assert_eq!(utc_stamp(951_782_400), "2000-02-29 00:00:00");
    assert_eq!(utc_stamp(1_790_000_000), "2026-09-21 14:13:20");

    let path = std::env::temp_dir().join(format!("memory-core-curator-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let clean = ApplyReport { applied: 2, ..Default::default() };
    log_curator_result(&path, Lang::En, "Terra · Lyra", &Ok(clean));
    assert!(!path.exists(), "a clean turn writes nothing");

    let bad = ApplyReport { applied: 1, rejected: vec!["CloseEdge: there is no open link to close".into()], ..Default::default() };
    log_curator_result(&path, Lang::En, "Terra · Lyra", &Ok(bad));
    log_curator_result(&path, Lang::Es, "Terra · Kael", &Err(LlmError::Shape("bad json".into())));
    let log = std::fs::read_to_string(&path).unwrap();
    assert!(log.contains("Terra · Lyra: 1 applied, 1 rejected\n    rejected: CloseEdge: there is no open link to close\n"), "{log}");
    assert!(log.contains("Terra · Kael: el curador falló:"), "{log}");

    let dir = std::env::temp_dir().join(format!("memory-core-loglabel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::new(&dir, HandlerConfig::default());
    let world = lib.create_world("Arknights", "Terra", "Terra.", "").unwrap();
    let kal = world.create_character("Kal'tsit", "Doctor of Rhodes Island", "Head of medical", "").unwrap();
    assert_eq!(kal.log_label(), "Kal'tsit");
    let conv = memory_core::conversation::Conversation::new("chat", vec![kal.id()], "Doctor");
    let chat_mem = world.chat_memory(&conv).unwrap();
    assert_eq!(chat_mem.log_label(), "chat «Kal'tsit»");
}

