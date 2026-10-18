# Engram: Cognitive Graph Memory Engine for Local Characters
*(Binary executable & CLI: `chats`)*

**Engram** is a local, offline cognitive memory engine and desktop application to interact with autonomous characters powered by **cognitive memory graphs**. 

In cognitive science and neurobiology, an *engram* is the biological trace etched into neural substrates through lived experience and recalled through associative reactivation. Rather than relying on monolithic, ever-growing chat logs that saturate the model's context window, **Engram** equips each character with an evolving, bi-temporal memory graph. On every conversation turn, only the memories, traits, and facts that matter are activated through Cognitive GraphRAG (Personalized PageRank, temporal gating, and Hebbian reinforcement).

Characters inhabit **worlds** (grouped by category) with shared public lore, while keeping **their own subjective view** of each other.

What there is so far:
- the cognitive memory core (`memory-core`): dual-subgraph architecture (Personality Tree + Episodic Memory Graph), spreading activation, bi-temporal links, and `rkyv` zero-copy persistence;
- model and image management (`model-hub`): local model supervisor, dynamic VRAM sharing between LLM and Stable Diffusion, and vision reference reader;
- bilingual English/Spanish dictionary and prompt system (`i18n`);
- CLI inspection and interaction tool (`chats`);
- desktop application (Tauri v2 + React 19 + Tailwind): interactive 1:1 and group chats with real-time streaming, and 2D/3D WebGL memory constellation visualizers.

```
Chats/
├─ Cargo.toml                      workspace
├─ config/models.toml              which model is the actor, curator, reader and painter (created by itself; not in git)
├─ config/app.toml                 preferences: language (not in git)
├─ models/                         llm/ · vision/ · mmproj/ · diffusion/{vae,encoders,loras}/ · bin/ (optional)
├─ worlds/<category>/<world>/      world.mem · characters/*.mem · images/ · references/ · chats/<id>.{chat,mem}   (not in git)
├─ seeds/                          world seeds for local testing (not in git)
├─ crates/i18n/                    dictionary.toml (en/es) + prompts/<lang>/*.md
├─ crates/memory-core/             graph, activation, rkyv persistence, worlds, actor/curator pipeline
│  ├─ src/model.rs                 NodeKind / EdgeKind / Scope (#[repr(u8)]), GraphMeta, ExternalRef
│  ├─ src/graph.rs                 MemoryGraph (character or world) on petgraph::StableGraph + CRUD
│  ├─ src/activation.rs            spreading activation + forgetting + ranking with core slots
│  ├─ src/context.rs               <active_memory>, <world> blocks and the #id block (curator)
│  ├─ src/ops.rs                   curator GraphOps + guardrails + character resolution
│  ├─ src/persist.rs               .mem format v2 (rkyv) + zero-copy peek_meta
│  ├─ src/world.rs                 Library (categories), World, relationships, scene recall
│  ├─ src/layout.rs                folder names of a home + move from the old Spanish names
│  ├─ src/handler.rs               MemoryHandler: Arc + RwLock, dirty flag, atomic save, language
│  ├─ src/prompts.rs, src/llm.rs   prompts, llama-server/Ollama client, consolidation worker
│  └─ assets/                      memory_ops.gbnf, memory_ops.schema.json
├─ crates/model-hub/               models/ registry, mmproj pairing, process supervisor, images
│  ├─ src/gguf.rs, safetensors.rs  header reading without loading weights
│  ├─ src/registry.rs              classification, checked mmproj↔LLM pairing, diffusion families
│  ├─ src/supervisor.rs            llama-server on demand + VRAM sharing with sd-cli
│  └─ src/image.rs                 stable-diffusion.cpp arguments per family, portrait prompts
├─ crates/chats-cli/               `chats`: models, worlds, seed, context, memory, chat, image…
├─ src-tauri/                      desktop app: IPC commands for chats, groups, models, memory editing, language
├─ ui/                             React + TypeScript + Tailwind + react-force-graph-2d frontend
├─ chats.bat                       launcher with a menu (option A = the app, L = language)
├─ AGENTS.md                       guide for AI coding assistants (imported by CLAUDE.md and GEMINI.md)
└─ docs/
   ├─ PIPELINE.md                  flow, prompts, grammar, operations, a real run
   ├─ MODELS.md                    models per role + download commands
   ├─ SEEDS.md                     .toml format to create worlds and characters
   └─ example-seed.toml            Aerendal: Lyra, Kael and Mira
```

## Languages and the dictionary

Code, identifiers, comments, commands and docs are English. **Everything a person reads** (app, CLI, status messages) **and every prompt sent to the models** comes from the `i18n` crate, in English or Spanish:

- `crates/i18n/dictionary.toml`: short strings grouped in sections; each leaf has one text per language, with `{name}` placeholders: `typing = { en = "{name} is typing…", es = "{name} está escribiendo…" }` under `[chat]` is the key `chat.typing`.
- `crates/i18n/prompts/<lang>/`: the long prompts (memory curator, vision reader).
- In Rust: `tr!(lang, "chat.typing", name = speaker)`. In the UI: `t("ui.chat.send")` (the app loads the dictionary from the backend at start-up).

The language is chosen with `chats lang es` / `chats lang en`, option **L** of `chats.bat` or the button at the bottom of the app's side bar; it is saved in `config/app.toml` (`CHATS_LANG` overrides it). In the app the change is immediate, prompts included. The **content** of the memories is not translated: seeds and test worlds written in Spanish stay that way; the actor answers in the user's language.

Tests guard the dictionary: every entry exists in every language with the same placeholders, every key used in Rust or in the UI exists, and every node and edge kind has its labels. To add a language: a `Lang` variant, a column in every entry and a `prompts/<code>/` folder; the tests fail until they are complete.

**Old folder names.** Before this change the folders had Spanish names. On start-up the CLI and the app rename them (`mundos/` → `worlds/`, `semillas/` → `seeds/`, `config/modelos.toml` → `config/models.toml`, `models/difusion/` → `models/diffusion/`, and inside each world `mundo.mem`, `personajes/`, `imagenes/`, `referencias/`). Nothing is overwritten, and avatar paths stored in the `.mem` files are fixed too. Old seeds with Spanish keys still load, and every command keeps its Spanish name as an alias.

## Design decisions

**One graph, two subgraphs.** The `PersonalityRoot` holds the personality tree (`Trait`, `Emotion`, `Value`, `Goal`, joined with `Composes`). Memories, facts, entities, places and topics make up the episodic graph. The two cross with `Evokes` edges (memory → emotion/trait). That is why a memory can activate a trait, and activation spreads across both sides.

**Compact data.**
- IDs: native `NodeIndex<u32>`, never strings.
- Node and edge kinds: `#[repr(u8)]`. They take 1 byte in memory and in the file (a test also checks rkyv's archived types).
- Times as Unix `i64`; weights, valences and activations as `f32`.
- The lexical index is derived: it is not saved, it is rebuilt on load.

**`rkyv`, not `bincode`.** `bincode` 3.0.0, the latest version published on crates.io, only contains a `compile_error!`: the project stopped being maintained in 2025 (checked in the crate's source). `rkyv` 0.8 is maintained, validates the file when reading it (a damaged `.mem` is an error, not a panic or UB) and allows **reading without deserialising**: `peek_meta()` reads the name and size of hundreds of characters for the gallery without rebuilding any graph.

**The `.mem` format.** A 16-byte header (`CMEM`, version, length) followed by the rkyv payload. The graph is stored flattened and, on load, the `StableGraph` is rebuilt **keeping the holes** of deleted nodes, so IDs do not change between sessions. Saving is atomic: it writes `.mem.tmp`, `fsync`s it and renames it over the original.

**Recall & Cognitive GraphRAG.**
1. **Seeds:** TF-IDF over labels and content, without accents and with light, symmetric stemming for Spanish and English.
2. **HippoRAG with Personalized PageRank (PPR):** Replaces simple neighbor spreading with a stationary probability flow (10–15 power iterations, damping $\alpha = 0.85$). Transition probabilities are weighted by edge weight, destination importance, and affective salience: $T(u \to v) = \text{weight}(e) \times \text{weight}(v) \times (1.0 + |\text{valence}(v)|)$. Multiple associative paths naturally converge into high-scoring memories across multi-hop reasoning.
3. **Bi-Temporal Edges (`valid_until`):** Flat 32-byte memory layout (`valid_until: i64`, where 0 = active). Contradicted or evolved relationships are not deleted: they are sealed with a timestamp and the new active edge is created. In the LLM prompt, recently invalidated edges are explicitly injected with the prefix `[PASADO / YA NO VIGENTE]` to prevent temporal hallucination.
4. **Event Capsules (N-ary Gating):** Hyperedge structures grouping $\ge 3$ member nodes. Complex episodic scenes only trigger into context if at least **2 or more member nodes** are simultaneously active in the turn, eliminating single-token context pollution.
5. **Consolidation Phase ("Sleep Phase"):** Read-only clustering of dense, stale episodic memories with low recall frequency. Prepares an LLM synthesis payload and atomically updates the Personality Tree with the accumulated emotional delta ($\Delta\text{valence}, \Delta\text{strength}$), reconnecting external topology before physical node pruning.
6. **Selection:** Slots reserved for personality traits and core rules ensure characters stay true to their identity even on off-topic queries.
7. **Hebbian Learning:** Co-activated nodes in the active subgraph are reinforced after each turn.

## Worlds and relationships between characters

- **One graph per world** (`Scope::World`, root `WorldRoot`): rules (`Lore`, always in context like the personality), places, factions, history (`Event`, never forgotten) and each character's **public sheet**.
- **One graph per character** (`Scope::Character`): their personality, their memories and a `Character` node for each person they know, with **their** opinion (content), affection (`valence`) and closeness (`weight`). It is asymmetric: Lyra can adore Kael while Kael barely notices her.
- Nodes that stand for another graph carry an `ExternalRef` (scope + 64-bit id), stored in a separate table of the `.mem`: only the nodes that use it pay for it.
- **Scene** (`World::scene_recall`): the speaker gets `<world>` (≈30 % of the budget), `<scene>` (who is there and what they think of each) and `<active_memory>` (≈70 %). **Private memories never mix**: a test checks it (Kael's secret does not reach Lyra's prompt).
- The curator can create `character` nodes. If the name matches someone in the world, it links to their `.mem` by itself (or merges with the existing opinion). Each character present consolidates the turn from their own point of view.

## Per-chat memory

Character memory follows a character into every conversation. **Chat memory** belongs to one conversation (`Scope::Chat`, root `ChatRoot`, stored in `chats/<id>.mem` next to the `.chat` file):

- **What goes in it:** what happened in that chat, the facts the user revealed there, the state of the scene and the rules set for it ("in this story the library never burned"). Rules are `Lore` nodes: they hang from the chat's root and always get a slot in the context.
- **Who writes it:** a neutral chronicler curator (`prompts/<lang>/chat_curator_system.md`) after every turn, and **the user**, who edits it like any other graph in the Memory view. Personality kinds (trait, emotion, value, goal) are rejected: they belong to the characters.
- **How it starts:** with a node for each participant (linked to their sheet) and one for the user, so events can be tied to them from the first turn.
- **Who sees it:** everyone in the chat. Scene recall gives it ≈25 % of the budget in a `<chat_memory>` block, between `<scene>` and the speaker's `<active_memory>` (which drops to ≈50 %, the world keeps ≈25 %).
- **"Characters remember" switch** (per chat, on by default): when it is off, the characters' own memories do not learn from that chat. It is an isolated storyline that only its chat memory keeps.

Renaming a chat renames its memory; deleting it deletes both files. Chats saved before this feature (format v1) still load, and their memory is created the first time it is opened.

## Appearance

- **Canonical:** each character has visual aspects (`appearance` nodes) hanging from their sheet in the world and copied to their own memory as "your appearance". They are the prompt of their portraits, sorted by importance.
- **Subjective:** each one keeps how they see the others (her tired eyes, the mask that hides something…), hanging from their node for that person. When the question is about looks ("how do you see the commander?"), those nodes are seeded directly.
- **References:** `chats reference <category>/<world> <character> [images…]`, or the app's **Appearance** tab, saves the images in `references/` (no duplicates) and the **vision reader** describes them all at once: it separates the permanent traits from the outfit and follows the majority if one image contradicts the others. The output is forced by a grammar into a list of aspects, in the current language. In the app it is a proposal that is not saved until you press Save; in the CLI, `--dry-run` only shows it and `--reader <model>` compares readers.
- **Portraits:** with Z-Image Turbo, `chats image` or the button in the Appearance tab. They use the aspects with importance ≥ 0.5; the occasional ones (a pet, a sword, an unmasked face) only get in if the scene names them. The style is set in `[image] style`.

## Models (`model-hub`)

`chats models` shows what is in `models/` and how the roles would be shared out:
- **Vision pairing:** each `mmproj` is matched with its LLM by normalised name and the match is **checked**: `clip.vision.projection_dim` must equal the model's `embedding_length`. Another model's mmproj is not used even if its name is similar.
- **Extra folders:** `extra_models_dirs` adds other folders with `llm/`, `vision/` and `mmproj/` (e.g. another project's) without copying the models. A repeated file counts once.
- **Vision reader:** `[reader]` is a third slot that only loads to describe references. Loading it unloads the actor (they do not fit together in 8 GB), and the actor comes back with the next message. Empty = the actor itself describes.
- **Actor and curator:** if `curator.model` is empty, the curator shares the actor's `llama-server` (2 slots in parallel), ideal with 8 GB. If you pick a small model, it runs separately on the CPU.
- **VRAM:** the actor starts with `--fit on` (llama-server fits the layers to the free VRAM). Before generating an image, the supervisor estimates what `sd-cli` needs; if it does not fit, it unloads the actor and reloads it on the next turn.
- **Images:** stable-diffusion.cpp (`sd-cli`). The family is detected by name, metadata or tensors (Z-Image, Flux, SD3, SDXL, SD1.5, Chroma, Qwen-Image), with its components (VAE, encoders) and recommended steps/CFG.

## The app

It opens with option **A** of `chats.bat`, or directly with `target\release\chats-app.exe`.

- **Chats:** each world has its conversations, stored in `worlds/<category>/<world>/chats/*.chat` (binary rkyv).
  - The **+** button creates a chat with one character or a **group** with several. That is also where you pick your name in the scene.
  - In a group, whoever you name in your message answers; if you name nobody, they take turns. You can also pick who answers or press **Continue** so the scene goes on without you.
  - Replies stream in. **Turn memory** shows the exact context the actor got.
  - After each turn, the chat's memory records what happened and, if **Characters remember** is on, every character present stores it from their point of view; a notice appears for each memory updated.
  - **Chat memory** opens that conversation's graph in the Memory view.
- **Memory:** the world's memory, each character's private one and each chat's memory.
  - **Views:** interactive graph with colours by kind, or a sortable list. There are filters by kind and search in names and content.
  - **Layout physics** (`ui/src/graphPhysics.ts`, shared by 2D and 3D): the visual topology is decoupled from the data. Structural edges (`composes`, or weight ≥ 0.80) pull hard and short, so a core keeps its branches close. Weak ones (`related_to`, `precedes`, `contradicts`, or weight < 0.70) pull almost nothing (0.012) from far away. Roots, characters and factions repel the most, so each keeps its own "galaxy". Distances grow with the hub's degree, and a gentle gravity keeps loose nodes in orbit. Every edge is still in the memory for the model.
  - **Weak links:** drawn faint until you hover or select one of their nodes; the toolbar button draws them always.
  - **2D / 3D:** the 3D view spreads crowded graphs out in depth. It has orbit controls and floating labels on the important nodes, the selection and its neighbours (the rest show their name on hover), and clicking a node centres the camera on it. The choice is remembered.
  - **Test recall:** lights up what a message would activate, without counting as rehearsal.
  - **Editor:** change name, kind, content, importance and valence; add, edit or remove links; create and delete nodes. Everything is saved to the `.mem` right away.
  - **Appearance:** references, canonical aspects, how the others see the character, and portraits.
- **Models:** status of the actor, curator and reader, image availability and a button to free the GPU. Closing the window frees the models too.
- **Language:** the button at the bottom of the side bar switches between English and Spanish.

## System Requirements

### 1. Hardware

| Component | Minimum (CLI & CPU mode) | Recommended (Local GPU Inference & Desktop App) |
| :--- | :--- | :--- |
| **CPU** | x86_64 or ARM64 with AVX2 support | Modern multi-core CPU (Intel Core i7/i9 12th+ gen, AMD Ryzen 5000+) |
| **RAM** | 8 GB | 16 GB to 32 GB (to host desktop UI, supervisor, and memory caching) |
| **GPU / VRAM** | Integrated graphics / No dedicated GPU | **NVIDIA RTX 3060, 3070 (8 GB VRAM) or higher** with CUDA support |
| **Storage** | 2 GB free SSD space for engine binaries | NVMe SSD with 20–50 GB free space for local GGUF models & diffusion weights |

> [!NOTE]
> **VRAM Management:** With 8 GB of VRAM, the `model-hub` supervisor shares memory dynamically: the LLM Actor runs with `--fit on` to fit layers to available VRAM. When an image portrait is requested, the supervisor evaluates VRAM, temporarily parks the Actor, generates the image using `sd-cli` (Z-Image Turbo or SDXL), and restores the Actor for the next chat turn.

### 2. Software & Toolchains

- **Operating System:** Windows 10/11 (64-bit), Linux (x86_64), or macOS (Apple Silicon).
- **Rust Toolchain:** Rust stable **$\ge 1.88$** (Rust Edition 2024 support is required).
- **Node.js:** Node.js **$\ge 18$** (or 20+ LTS) with `npm` / `npx`.
- **Tauri v2 System Prerequisites:**
  - **Windows:** Microsoft Visual Studio C++ Build Tools (MSVC) + Microsoft Edge WebView2 (pre-installed on Windows 10/11).
  - **Linux:** WebKit2GTK and build tools:
    ```bash
    sudo apt update && sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget libssl-dev libayatana-appindicator3-dev librsvg2-dev
    ```

### 3. Model Roles & Binaries (Optional for CLI tools, required for local inference)

Engram is fully model-agnostic and runs local models without telemetry or external API keys (see [`docs/MODELS.md`](docs/MODELS.md) for full catalogue and download scripts):
- **Actor (LLM):** Qwen 2.5 7B/14B Instruct, Llama 3.1 8B, or Gemma 2 9B (GGUF Q4_K_M or Q5_K_M).
- **Curator (Cognitive Chronicler):** Shares the Actor's `llama-server` process using 2 parallel slots, or runs a lightweight model on CPU (Qwen 2.5 1.5B/3B).
- **Reader (Vision):** Qwen2-VL 7B GGUF with its matching, verified `mmproj` projection file for visual reference ingestion.
- **Painter (Diffusion):** `stable-diffusion.cpp` (`sd-cli`) binary with Z-Image Turbo, SDXL, or SD1.5 checkpoints.

## Development

For development with hot reload:

```bash
npm run tauri dev
```

## Build & Run in Release

To build the entire stack (CLI + Tauri Desktop App) with automated checks and pre-flight protection against running instances, use **`build_release.bat`**:

```bat
build_release.bat
```

Or manually via Cargo and Tauri:

```bash
# Build CLI engine in release
cargo build --release -p chats-cli

# Build Desktop App in release (ensure chats-app.exe is closed)
npx tauri build --no-bundle
```

## Test CLI

The easiest way to interact with the engine from the console is **`chats.bat`**: a menu that builds on demand and lets you chat, inspect graph context, review acquired memories, and switch worlds, characters, companions, or scene user persona.

Commands (from the project folder; each one also accepts its old Spanish name):

| Command | What it does | Loads models? |
|---|---|---|
| `chats models` | inventory of `models/` and roles | no |
| `chats seed docs/example-seed.toml` | creates a world with its characters | no |
| `chats worlds` | gallery by category | no |
| `chats context fantasy/aerendal Lyra "message" --with Mira,Kael` | shows the exact prompt the actor would get | no |
| `chats appearance fantasy/aerendal Lyra` | canonical appearance and how the others see her (with a text, it replaces it) | no |
| `chats reference fantasy/aerendal Lyra a.png b.png --dry-run` | describes all of her references with the vision reader (`--dry-run`: not saved) | yes (vision) |
| `chats seed docs/example-seed.toml --appearance-only` | re-applies the seed's appearance to an existing world, without touching memory or chats | no |
| `chats stress 1000 10000 100000` | engine benchmark with synthetic characters | no |
| `chats memory fantasy/aerendal Lyra 20` | last nodes learned (with their edges) | no |
| `chats chat fantasy/aerendal Lyra --with Mira,Kael --as Tomás` | interactive scene (`/speak`, `/with`, `/memory`, `/image`, `/exit` or Ctrl+C) | yes |
| `chats image fantasy/aerendal Lyra "in the library"` | portrait with her appearance and dominant emotion | yes (image) |
| `chats lang es` | switches the language (without an argument, shows it) | no |

## Usage

```rust
use memory_core::*;

let lib = Library::new("worlds", HandlerConfig::with_lang(Lang::En));
let world = lib.create_world("fantasy", "Aerendal", "A kingdom in ruins…", "Melancholic tone")?;
let h = world.create_character("Lyra", "Elven librarian, reserved", "Keeper of the library", "elf, silver hair")?;
h.write(|g| {
    let root = g.root();
    g.add_trait(root, NodeKind::Trait, "Curious", "She loves old books", 0.8, unix_now())
})?;

let scene = world.scene_recall(h.id(), "Do you remember the fire?", &[])?; // → the actor's system prompt

let chat = conversation::Conversation::new("Night watch", vec![h.id()], "Tomás");
chat.save(&world.dir)?;
let chat_memory = world.chat_memory(&chat)?;                // that conversation's own graph
let scene = world.scene_recall_in(h.id(), "Where were we?", &[], Some(&chat_memory))?;
let view = h.view();                                     // → frontend (JSON over Tauri IPC)
world.set_lang(Lang::Es);                                // later blocks and prompts in Spanish
h.save_if_dirty()?;
```

Tauri's IPC always serialises to JSON. That is why `GraphView` only carries what is needed to draw: `id`, `name`, `kind: u8`, `val`, `valence` and `heat`. A node's full text is requested separately with `detail(id)` on click.

## Tests

```bash
cargo test --workspace
```

There are 59 tests, and `cargo clippy --workspace --all-targets` has no warnings.

- **Dictionary:** complete entries in every language, matching placeholders, every key used in the code exists, labels for every kind.
- **Recall quality in benchmark test worlds** (about 330 nodes, with everyday noise): 20 questions per character with what the context must contain, and a check that no node marked `SECRETO` reaches someone who does not know it.
- **Stress:**
  - a needle in a haystack of 5,000 nodes;
  - hubs do not flood the context;
  - a round trip of 20,000 nodes with under 64 bytes of structure per element;
  - curator fuzzing with extreme values and invariants;
  - concurrent activity in a world of 6 characters;
  - canonical and subjective appearance kept apart.
- **Per-chat memory:** shared by everyone in the chat and kept out of the characters' memories, survives reloads, follows renames and deletes, the chronicler curator's example applies in both languages, personality is rejected, and v1 chat files still load.
- **Core, worlds, models, pipeline and layout:** a Spanish world builds its context in Spanish and can switch at runtime, old folders are moved without overwriting, old Spanish seed keys still load, SSE streaming, and the fallback when the `mmproj` does not load.

Release benchmark (`chats stress`): recall p50 of 0.26 ms with 1,000 nodes, 2.5 ms with 10,000 and 36 ms with 100,000.
