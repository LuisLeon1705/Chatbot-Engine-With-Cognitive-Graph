# Seeds: creating a world with its characters

A world is described in a readable `.toml` file and created with:

```bash
chats seed my-world.toml
```

This creates `worlds/<category>/<world>/` with `world.mem`, `characters/*.mem` and `images/`. From then on the memory grows by itself through conversation: the `.toml` is only the starting point. Seeding the same world twice is an error, so nothing gets overwritten.

If you only improve the appearance (`[[characters.aspects]]` or `[[visual_perceptions]]`), apply it to an already seeded world without losing what was learned or the chats:

```bash
chats seed my-world.toml --appearance-only
```

Examples: [`example-seed.toml`](example-seed.toml) (small) and [`../seeds/terra.toml`](../seeds/terra.toml): Arknights, with Kal'tsit, Amiya and the Doctor, about 330 nodes and secrets meant to test privacy (its content is in Spanish).

The keys are English. Seeds written with the old Spanish keys (`[mundo]`, `[[personajes]]`, `recuerdos`, `hace_dias`…) still load: every field accepts its Spanish name as an alias, and kinds like `rasgo` or `faccion` too.

## `[world]`

| Field | What it is |
|---|---|
| `category` | folder that groups worlds ("Fantasy" → `fantasy`) |
| `name` | name of the world |
| `premise` | the world's root: tone, era, the central conflict. Always in the context |
| `style` | how the actor should narrate (person, length, format of actions) |
| `[[world.rules]]` | the world's rules (magic, technology, religion). They always get a slot in the context |
| `[[world.places]]`, `[[world.factions]]`, `[[world.history]]` | recalled when the conversation touches them; history is never forgotten |
| `[[world.people]]` | characters without a sheet of their own (historical figures, allies, enemies), with their public description |
| `[[world.timeline]]` | moments of the story chats can be set at: `name`, `year`, `day` (0–364, optional) and `summary` (what is going on then; the actor gets it) |
| `present` | name of the moment the characters' `days_ago` count back from |
| `[world.look]` | how the world's chats look: `accent` (`#rrggbb`), `mode` (`dark` / `light`), `font` (`serif` / `sans`), `texture` (`none` / `grain` / `paper`). It is written to `style.toml` in the world's folder and can be changed later in the world's Style tab |

Each element has `name`, `text`, `importance` (0 to 1; 0.6 by default) and, optionally, `year` (it does not exist before that year of the story) and `related = ["Other name", …]`. That creates edges between elements of the same graph so activation passes from one to the next: "Theresa" → "Babel" → "Kazdel". A name that does not exist is an error, because it is almost always a typo.

## `[[characters]]`

| Field | What it is |
|---|---|
| `name` | unique within the world |
| `identity` | the root of their personality: who they are and how they speak |
| `public_bio` | what anyone in the world knows about them (it goes to the world's memory, not theirs) |
| `appearance` | stable visual description, the fallback for their images |
| `scenario` | their current situation (optional) |
| `moment` | their own moment of the timeline (optional): their `days_ago` count back from it, `scenario` belongs to it and they are only offered at the moments they list |
| `[[characters.moments]]` | other moments they belong to: `moment` (its name) and `scenario` (what they do then) |
| `[[characters.traits]]` | `name`, `text`, `intensity` (0–1) and `kind` = `trait` / `emotion` / `value` / `goal` |
| `[[characters.memories]]` | `title`, `text`, `importance`, `valence` (−1 to 1), `days_ago`, `year` (optional), `involves` (names of places, things or characters) and `evokes` (names of their traits or emotions) |
| `[[characters.aspects]]` | canonical appearance, aspect by aspect: `name` ("Hair", "Coat"…), `text` and `importance`. It goes to the world's memory (for everyone) and to theirs ("your appearance"), and it is what their portraits are painted from |
| `[[characters.knows]]` | what they know: `name`, `text`, `importance`, `related` and `kind` = `fact` (default), `person`, `object`, `place`, `faction`, `topic`, `rule` or `history`. The content is **their** version: each character can know different, or false, things about the same subject |

Facts and memories also accept `contradicts = [...]` (inhibitory edge: two beliefs that clash) and `causes = [...]`, besides `related`.

A character's memories chain themselves in chronological order (`precedes` edge), so one evokes the next.

If a memory `involves` another character of the world, that node is linked to their `.mem`. If there is also a declared relationship between the two, the same node is reused.

## Timeline

Without a timeline every chat sees everything. With one, a chat is set at a moment (the latest by default) and only sees what exists then:

```toml
[world]
present = "After the siege"

[[world.timeline]]
name = "The siege"
year = 1201
summary = "Vors is under siege; the library still stands."

[[world.timeline]]
name = "After the siege"
year = 1203
day = 120
summary = "Vors is in ruins and the guild has disbanded."

[[world.history]]
name = "Fall of Vors"
text = "The fortress fell after two years of siege."
year = 1202
```

- A memory's story time is its `year`, or its moment minus `days_ago` (the character's `moment`, else `present`). Without either it is timeless.
- `[[characters.knows]]` and world elements with a `year` do not exist before it; without one, always.
- A character with a `moment` or `[[characters.moments]]` is only offered at those moments; without them, at any.

Terra has two moments: "Babel" (1093, Theresa alive, and she only belongs there) and "Tras Londinium" (winter of 1098, the present).

## `[[definitions]]`

Shared definitions: the objective baseline any character can rely on, so a character from another world is not left in the dark. They are not stored per character: one node per library.

```toml
[[definitions]]
level = "category"          # "category" (default): shared by the worlds of this category
name = "Originium"
text = "A black crystal that stores energy; its dust spreads Oripathy."
importance = 0.8
related = ["Oripathy"]       # other definitions of the same library

[[definitions]]
level = "universal"         # shared by every world
name = "Iron"
text = "A grey metal, strong and common."
kind = "object"             # like characters.knows; "concept" by default
```

- They go to `worlds/<category>/_category.mem` or `worlds/_universal.mem`. A second world of the same category reuses them: a definition that already exists (or that the user edited) is kept as it is.
- A turn only gets the definitions it touches, in a `<definitions>` block after `<world>`. If the speaker, the world or the chat already brought up something with the same name, their own version wins and the definition is not repeated.
- They are edited in the Memory view, section Definitions.

## `[[relations]]`

What **one** character thinks of another. To make it mutual it has to be declared both ways, and it can be asymmetric:

```toml
[[relations]]
from = "Lyra"
to = "Kael"
opinion = "He saved her life in the fire; she owes him everything."
affection = 0.7   # -1 hatred … 1 fondness
closeness = 0.6   # 0 barely knows them … 1 intimate
```

## `[[visual_perceptions]]`

How **one** character sees another's looks. It is stored in the private memory of whoever looks, so each one can notice different things:

```toml
[[visual_perceptions]]
from = "Amiya"
to = "Kal'tsit"
name = "Eyes"
text = "Her green eyes always look tired, though she would never admit it."
importance = 0.7
```

## Checking the result without loading models

```bash
chats context fantasy/aerendal Lyra "Do you remember the fire?" --with Mira,Kael
```

It shows the exact system prompt the actor would get (world, scene and memory) and its approximate size in tokens. `--at <moment>` (its name or year) shows it at another moment of the timeline.
