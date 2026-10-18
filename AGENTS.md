# Working on Chats (guide for AI coding assistants)

`CLAUDE.md` (Claude Code) and `GEMINI.md` (Gemini CLI) import this file. Edit it here.

Chats is a local, offline desktop app to chat with characters that have a **graph memory**. Rust workspace + Tauri v2 + React. Read `README.md` for the full picture and `docs/` for details.

## Layout

| Path | What |
|---|---|
| `crates/i18n/` | English/Spanish dictionary (`dictionary.toml`) and long prompts (`prompts/<lang>/*.md`) |
| `crates/memory-core/` | memory graphs (petgraph), spreading activation, rkyv `.mem` files, worlds, chats, actor/curator pipeline |
| `crates/model-hub/` | local model registry, `llama-server` / `sd-cli` supervisor, image generation |
| `crates/chats-cli/` | the `chats` CLI and the seed loader (`seed.rs`) |
| `src-tauri/` | desktop app backend (Tauri commands) |
| `ui/` | React + TypeScript + Tailwind frontend (2D and 3D graph views) |
| `seeds/` | world seeds for local testing (not in git) |
| `docs/` | `MODELS.md`, `PIPELINE.md`, `SEEDS.md`, `example-seed.toml` |

User data and local test data, not in git: `worlds/` (each world may have a `style.toml`: how its chats look), `models/`, `seeds/`, `config/models.toml`, `config/app.toml`, `logs/`.

## Commands

Run from the project folder.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
npx tsc --noEmit -p .
cargo build --release -p chats-cli
npx tauri build --no-bundle
npm run tauri dev
```

- `cargo test` and `tsc` must pass and clippy must stay at zero warnings before calling a change done.
- `npx tauri build` fails with "access denied" if `target/release/chats-app.exe` is running: close the app first.
- The `chats` CLI needs no models for `models`, `worlds`, `context`, `memory`, `appearance`, `stress` and `lang`. `chat`, `image` and `reference` load models on the GPU.

## Rules

**Language.**
- Code, identifiers, comments, commit messages and docs are English.
- Every text a person reads (app, CLI, status messages) and every prompt sent to a model comes from the dictionary in both languages.
  - Rust: `tr!(lang, "section.key", name = value)` or `i18n::t(lang, "section.key")`.
  - UI: `t("ui.section.key", { name })` from `ui/src/i18n.ts`.
  - Placeholders are `{name}` and must be the same in `en` and `es`.
- Never hard-code a visible string. The i18n tests scan the sources and fail on a missing key.
- Do not translate world content. Memories, seeds and model output stay in the language they were written in.
- XML-like prompt tags (`<world>`, `<scene>`, `<chat_memory>`, `<active_memory>`) are structural and always English.

**Data.**
- Persistence is binary rkyv (`.mem`, `.chat`); JSON is not allowed for main persistence.
- IDs are `u32` node indices / `u64` graph ids, enums are `#[repr(u8)]`, times are `i64` Unix seconds and weights are `f32`.
- A new `NodeKind` / `EdgeKind` variant goes **at the end**. It also needs:
  - `key()` and the `kind.*` / `ctx.kind.*` (or `edge.*`) dictionary entries;
  - the GBNF grammar and JSON schema, if the curator may emit it;
  - `ui/src/meta.ts`.
- Changing an rkyv struct changes the file format: bump the version and keep a reader for the old one (see `ConversationV1` in `conversation.rs`).
- Old Spanish folder names are migrated by `memory_core::layout::migrate_legacy`; never overwrite user files.

**Memory model.**
- Scopes:
  - World: public lore plus each character's public sheet.
  - Character: private, subjective memories.
  - Chat: one conversation, shared by everyone in it, editable by the user.
  - Library: shared definitions (`worlds/_universal.mem`, `worlds/<category>/_category.mem`), one handler per file cached in `world::Library`; recalled only when a turn touches them, and never when the speaker, world or chat already has its own node with that name.
  - Collab: a meeting place for several worlds (`worlds/_collabs/<slug>/`: `collab.mem`, `collab.toml` with the linked worlds and one moment each, `style.toml`, `chats/`). Always isolated: its chats use Carbon Copies, never the characters' own memories. Each speaker stays in their own world and moment; a person from another world is known only through the speaker's memory, the Collab's memory or the speaker's world lore (otherwise a stranger). In the app a Collab is a place with the key `collab:<slug>`, accepted by the chat and memory commands.
- Private memories must never leak into another character's context. The test worlds check this: secret nodes start with `SECRETO:`.
- A world's cache in `AppState` is never cleared: two handlers for the same file would overwrite each other.
- Story time (in-world days) is separate from Unix time. Nodes have `since`, edges `valid_from`/`valid_until`; a chat sees its moment of the world's timeline. Close a link (`close_edge`) instead of deleting it when it stops being true. The open ends are `i64::MIN`/`MAX`: over IPC they travel as `"always"`/`"open"`/`"present"` (`view::Story`).

**Characters and content.**
- In test worlds, the principal character acting as the user has no defined gender: use neutral wording.
- Seeds use English keys; the old Spanish keys still load as serde aliases.

**Environment (the author's machine).**
- Windows 11, RTX 3070 8 GB. Ask before loading models on the GPU: the user may be using it.
- After an interrupted model run, check that no `llama-server` / `sd-cli` process is left.
- The git root is the user's home folder (`C:\Users\leonp`). Do not commit unless asked.
- To test the app without touching real data, run it with `CHATS_HOME` pointing to a copy of the project folder.
- Python helper scripts that edit files should use `newline=''` and keep CRLF where the file has it.
- Bash heredocs mangle backslashes and non-ASCII text, so write scripts to a file first.
