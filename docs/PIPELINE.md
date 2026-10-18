# Dual inference pipeline

```text
             ┌──────────────── turn N ─────────────────┐
 user ─────▶ │ MemoryHandler::recall(prompt)            │
             │   1. lexical seeds (inverted index)      │
             │   2. spreading activation (3 hops)       │
             │   3. ranking + forgetting + core slots   │
             │   4. <active_memory> block (≤4800 ch)    │
             │   5. rehearsal + Hebbian reinforcement   │
             └──────────────┬──────────────────────────┘
                            ▼
      system = sheet + <active_memory>      short window (4–8 msgs)
                            ▼
                 ACTOR 7B–27B (+mmproj) ─────────────▶ reply to the UI
                            │
                 TurnRecord │ mpsc (does not block)
                            ▼
      ConsolidationWorker (1 consumer per memory: the chat's + each character's, in order)
        curator_context(turn) → CURATOR 1B–4B + GBNF → {"ops":[...]}
        → apply_ops (guardrails) → save_if_dirty (atomic .mem) → "memory-updated" event
```

Code: `crates/memory-core/src/llm.rs` (`actor_turn`, `scene_turn`, `consolidate`, `ConsolidationWorker`) and `src/prompts.rs`. Every text of the prompts comes from the dictionary (`crates/i18n`) in the current language; the XML-like tags are always English.

## 1. The actor's prompt

`prompts::actor_system_prompt(lang, meta, recall.context)` produces (in English):

```text
You are Lyra. Always stay in character and answer as Lyra would, in the user's language.

## Scenario
A library in ruins after a fire.

What follows is what you remember and feel NOW, retrieved from your memory. Use it naturally; do not list it or mention that it is a memory.
<active_memory>
## Who you are
Lyra: Elven librarian, reserved and scholarly
## Personality that matters now
- [trait 80%] Curious: She loves old books
- [emotion 60%] Melancholy: She misses her mother
## Memories that come to mind
- (3 months ago) Marta in the garden: Marta and Lyra read poetry in the garden
## What you know
- [entity] Marta: Lyra's former apprentice
</active_memory>
```

In a world, `scene_actor_prompt` adds who is speaking to the character, who else is present, the world's style and the character's situation, and the context carries `<world>`, `<scene>` and `<active_memory>`.

The actor never sees internal IDs. It only gets this block and a short window of recent turns for local coherence.

## 2. The curator's prompt

Files: [`crates/i18n/prompts/en/curator_system.md`](../crates/i18n/prompts/en/curator_system.md) and its Spanish twin. Key points:

- It is written for 1B–4B models: short numbered rules and **one full example** (small models imitate much better than they obey).
- The active memory reaches it **with IDs** (`#7 [entity] User — …`). That way it refers to existing nodes by number and creates new nodes with `"$a"`.
- It explicitly asks for `{"ops":[]}` when there is nothing to remember, so no noise gets stored.

The user message follows this structure (`prompts::curator_user_message`):

```text
ACTIVE MEMORY:
#0 [personality_root] Kael — … (weight 1.00, valence +0.00)
#4 [entity] User — …
CURRENT LINKS:
#4 -related_to-> #9 (weight 0.70)
TURN:
User: …
Kael: …
OUTPUT:
```

`CURRENT LINKS` lists the links that hold between the listed nodes (not the root's, at most 10), so the curator can close one by id.

### The chat's curator

Each saved conversation also has its own memory (`Scope::Chat`). Its curator gets the same message format but another system prompt ([`chat_curator_system.md`](../crates/i18n/prompts/en/chat_curator_system.md)): a neutral chronicler that stores what happened in that conversation, the facts revealed there, the state of the scene and the rules set for it (`lore`, always in context). It never writes personality: `apply_ops` rejects trait, emotion, value and goal nodes in a chat graph. When the chat's "Characters remember" switch is off, only this curator runs; the characters' own curators skip the turn.

### In a Collab

A Collab's chat runs the same curators, but never on a base memory: the chat's memory (timeless, `STORY_PRESENT`) and each participant's Carbon Copy inside the Collab (`chats/<chat>-<character>.mem`), dated at the moment the Collab sets for that character's world. The Collab's own memory (`collab.mem`) is not curated: like a world's lore, it is written by the user, so the Collab's chats stay independent of each other.

## 3. Forced output

| Backend | Mechanism | File |
|---|---|---|
| llama-server | `grammar` field (GBNF) | [`memory_ops.gbnf`](../crates/memory-core/assets/memory_ops.gbnf) |
| Ollama | `format` = JSON Schema | [`memory_ops.schema.json`](../crates/memory-core/assets/memory_ops.schema.json) |

The grammar enforces: 0 to 8 operations, fields in a fixed order, enumerated node and edge kinds (no `personality_root`), weights in `[0,1]` with 2 decimals, deltas in `(-1,1)`, labels of ≤48 characters, content of ≤240 and bounded whitespace (so it does not loop on `\n`). The test `schema_and_grammar_assets_match_rust_types` checks that the grammar, the schema and the Rust `enum`s stay in sync. The JSON keys are English whatever the language; only the labels and contents follow the conversation's language.

### Operations

| `op` | Fields | Effect |
|---|---|---|
| `create_node` | `ref, kind, label, content, weight, valence` | Creates a node. If one with the same label (without accents, case-insensitive) and the same kind exists, they **merge**. New traits and emotions hang from the root. |
| `update_node` | `target, label?, content?, weight_delta?, valence_delta?` | Modifies a node. The root cannot be touched. |
| `link` | `source, target, kind, weight` | Creates or updates the open edge (one open edge per directed pair). |
| `reinforce` | `source, target, delta` | Adjusts an edge's weight; if there is none and `delta > 0`, creates a `related_to`. |
| `forget` | `target` | Deletes memories, facts and topics. Personality and entities **only fade**. |
| `close_edge` | `source, target` | The link stopped being true. It is **not deleted**: its `valid_until` is set to the turn's story time and it becomes the past. A later `link` between the same pair creates a new open edge. |

### Story time

Every turn happens at a story time: the chat's moment of the world's timeline (`World::story_now`), or `STORY_PRESENT` in a world without one. The curator applies its batch at that time (`apply_ops_at`):

- new nodes get `since` = that time, and new edges `valid_from` = that time (in a world without a timeline, what is learned is timeless);
- `close_edge` sets `valid_until` = that time;
- a node created again that already existed with a later date moves its `since` back: it existed then;
- a link may close at the moment it started (a chat stays at one moment), and linking the pair again in that moment reopens it.

The prompts (rule 7 and example 2) tell the curator how to record a state that stops being true: it never rewrites the old fact. It closes the fact's link, creates a new `semantic_fact` and links it to the old one with `contradicts`, so the old one stays as history but is inhibited in recall. Rejected operations are appended to `logs/curator.log`.

On the reading side, recall at a story time does not seed or reach nodes that do not exist yet, and only edges valid then spread. Edges that closed before it and touch the recalled nodes go to the end of the block as `[PAST / NO LONGER VALID] A knows B` (`ctx.past.line`), at most three, so the actor does not take an old relationship as current.

### Guardrails (`ApplyLimits`)

- At most 16 operations and 6 new nodes per turn.
- Knowledge weights: ±0.3 per operation. **Personality: ±0.1 per turn**, so the character evolves slowly and an odd turn does not rewrite it.
- Invalid references are rejected one by one and end up in `ApplyReport.rejected`; the rest of the batch is applied anyway.
- `OpBatch::parse_lenient` accepts JSON surrounded by text, in case the backend did not apply the grammar.

## 4. A real run

Gemma 4 E4B (Q4_K_M, llama-cpp-python 0.3.34 with Vulkan) on this machine, with the real grammar and a test turn in Spanish (Kael, the mercenary, and a healer who knew his brother Darian):

```json
{"ops":[
{"op":"update_node","target":4,"label":"Curandera","content":"La usuaria se identifica como curandera y atendió heridos en Vors."},
{"op":"create_node","ref":"$a","kind":"semantic_fact","label":"Darian era pelirrojo", ...},
{"op":"create_node","ref":"$b","kind":"episodic_memory","label":"Pregunta sobre el destino de Darian", ...},
{"op":"link","source":"$b","target":4,"kind":"involves","weight":0.8},
{"op":"link","source":"$b","target":"$a","kind":"involves","weight":0.8},
{"op":"link","source":"$b","target":2,"kind":"evokes","weight":0.7},
{"op":"update_node","target":2,"weight_delta":0.05}
]}
```

The JSON was valid on the first try (264 tokens in 29 s). The first version of the prompt created a duplicate "Curandera" entity; after tightening rule 1, it updates the user's node. Mistakes a 4B still makes: it renames the user after their job and attributes "Darian was red-haired" to the user. A 9B curator or an extra example in the prompt would fix it. The output is kept as a regression test in `tests/fixtures/curator_gemma4_e4b.json`.

## 5. Why order matters

`ConsolidationWorker` uses a single consumer per character. If the user types faster than the curator processes, turns queue up (up to 64) instead of running in parallel. That way there are never two batches reading the same memory and writing contradictory decisions. The `recall` of the next turn may run before the consolidation of the previous one finishes; that is acceptable, because the short window of recent messages covers the gap.
