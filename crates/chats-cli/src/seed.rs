//! Seeding a whole world from a readable `.toml` (lore, places, characters, traits,
//! memories and relationships). Format and example in `docs/SEEDS.md`.
//!
//! The keys are English; the old Spanish keys (`mundo`, `personajes`, `recuerdos`…) are
//! still accepted as aliases so older seeds keep working.

use std::collections::HashMap;

use memory_core::text::fold;
use memory_core::world::{Aspect, Library, Perception, World};
use memory_core::{CharacterMoment, EdgeKind, MemoryNode, Moment, NodeId, NodeKind, STORY_ALWAYS, StoryTime, story_of_year, unix_now};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Seed {
    #[serde(alias = "mundo")]
    pub world: WorldSeed,
    #[serde(default, alias = "personajes")]
    pub characters: Vec<CharacterSeed>,
    #[serde(default, alias = "relaciones")]
    pub relations: Vec<RelationSeed>,
    /// How one character sees another's looks.
    #[serde(default, alias = "percepciones_visuales")]
    pub visual_perceptions: Vec<VisualSeed>,
    /// Shared definitions (universal or of the world's category).
    #[serde(default, alias = "definiciones")]
    pub definitions: Vec<DefinitionSeed>,
}

/// A definition shared by every world of a library: the objective baseline any character
/// can rely on. Each world's own rules and each character's knowledge still take precedence.
#[derive(Debug, Deserialize)]
pub struct DefinitionSeed {
    /// `universal` or `category` (the default).
    #[serde(default, alias = "nivel")]
    pub level: memory_core::world::DefinitionLevel,
    #[serde(alias = "nombre")]
    pub name: String,
    #[serde(alias = "texto")]
    pub text: String,
    #[serde(default = "half", alias = "importancia")]
    pub importance: f32,
    /// Like `characters.knows`; `concept` by default.
    #[serde(default, alias = "tipo")]
    pub kind: Option<String>,
    /// Other definitions of the same library it is associated with.
    #[serde(default, alias = "relacionado")]
    pub related: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct VisualSeed {
    #[serde(alias = "de")]
    pub from: String,
    #[serde(alias = "a")]
    pub to: String,
    /// Aspect ("Eyes", "Outfit"…).
    #[serde(alias = "nombre")]
    pub name: String,
    #[serde(alias = "texto")]
    pub text: String,
    #[serde(default = "half", alias = "importancia")]
    pub importance: f32,
}

#[derive(Debug, Deserialize)]
pub struct WorldSeed {
    #[serde(alias = "categoria")]
    pub category: String,
    #[serde(alias = "nombre")]
    pub name: String,
    /// Premise: the world's root.
    #[serde(alias = "premisa")]
    pub premise: String,
    /// Narration style for the actor.
    #[serde(default, alias = "estilo")]
    pub style: String,
    #[serde(default, alias = "reglas")]
    pub rules: Vec<Item>,
    #[serde(default, alias = "lugares")]
    pub places: Vec<Item>,
    #[serde(default, alias = "facciones")]
    pub factions: Vec<Item>,
    #[serde(default, alias = "historia")]
    pub history: Vec<Item>,
    /// People without a sheet of their own (NPCs): historical figures, allies, enemies.
    #[serde(default, alias = "personas")]
    pub people: Vec<Item>,
    /// Points of the story chats can be set at, in any order.
    #[serde(default, alias = "linea_temporal")]
    pub timeline: Vec<MomentSeed>,
    /// Name of the moment the characters' `days_ago` count back from (unless a character
    /// has a `moment` of their own).
    #[serde(default, alias = "presente")]
    pub present: Option<String>,
    /// How the world's chats look (accent, mode, font, texture); written to `style.toml`.
    #[serde(default, alias = "estilo_visual")]
    pub look: Option<memory_core::WorldStyle>,
}

#[derive(Debug, Deserialize)]
pub struct MomentSeed {
    #[serde(alias = "nombre")]
    pub name: String,
    #[serde(alias = "anio", alias = "año")]
    pub year: i64,
    /// Day of that year (0–364).
    #[serde(default, alias = "dia")]
    pub day: i64,
    #[serde(default, alias = "resumen")]
    pub summary: String,
}

/// Another moment of the story a character belongs to.
#[derive(Debug, Deserialize)]
pub struct CharacterMomentSeed {
    #[serde(alias = "momento")]
    pub moment: String,
    #[serde(default, alias = "escenario")]
    pub scenario: String,
}

#[derive(Debug, Deserialize)]
pub struct Item {
    #[serde(alias = "nombre")]
    pub name: String,
    #[serde(alias = "texto")]
    pub text: String,
    #[serde(default = "half", alias = "importancia")]
    pub importance: f32,
    /// Only in `characters.knows`: fact (default), person, place, faction, topic, rule, history.
    #[serde(default, alias = "tipo")]
    pub kind: Option<String>,
    /// Names of other nodes in the same graph it is associated with (`related_to` edge).
    #[serde(default, alias = "relacionado")]
    pub related: Vec<String>,
    /// Nodes this one contradicts (inhibitory `contradicts` edge).
    #[serde(default, alias = "contradice")]
    pub contradicts: Vec<String>,
    /// Nodes this one causes (`causes` edge).
    #[serde(default, alias = "causa")]
    pub causes: Vec<String>,
    /// Year of the story from which it exists (without it: always).
    #[serde(default, alias = "anio", alias = "año")]
    pub year: Option<i64>,
}

/// Links declared by name in a seed element.
struct Links<'a> {
    name: &'a str,
    related: &'a [String],
    contradicts: &'a [String],
    causes: &'a [String],
}

impl<'a> From<&'a Item> for Links<'a> {
    fn from(i: &'a Item) -> Self {
        Links { name: &i.name, related: &i.related, contradicts: &i.contradicts, causes: &i.causes }
    }
}

impl<'a> From<&'a MemorySeed> for Links<'a> {
    fn from(m: &'a MemorySeed) -> Self {
        Links { name: &m.title, related: &m.related, contradicts: &m.contradicts, causes: &m.causes }
    }
}

fn half() -> f32 {
    0.6
}

#[derive(Debug, Deserialize)]
pub struct CharacterSeed {
    #[serde(alias = "nombre")]
    pub name: String,
    /// Personality root: who they are.
    #[serde(alias = "identidad")]
    pub identity: String,
    /// What anyone in the world knows.
    #[serde(alias = "bio_publica")]
    pub public_bio: String,
    /// Stable visual description (fallback for images).
    #[serde(default, alias = "apariencia")]
    pub appearance: String,
    #[serde(default, alias = "escenario")]
    pub scenario: String,
    /// Their own moment of the story: their `days_ago` count back from it, their
    /// `scenario` belongs to it and they only fit in the moments they list.
    #[serde(default, alias = "momento")]
    pub moment: Option<String>,
    /// Other moments they belong to, each with its own scenario.
    #[serde(default, alias = "momentos")]
    pub moments: Vec<CharacterMomentSeed>,
    #[serde(default, alias = "rasgos")]
    pub traits: Vec<TraitSeed>,
    #[serde(default, alias = "recuerdos")]
    pub memories: Vec<MemorySeed>,
    #[serde(default, alias = "sabe")]
    pub knows: Vec<Item>,
    /// Canonical appearance, aspect by aspect (it goes to the world's memory and to theirs).
    #[serde(default, alias = "aspecto")]
    pub aspects: Vec<Item>,
}

#[derive(Debug, Deserialize)]
pub struct TraitSeed {
    #[serde(alias = "nombre")]
    pub name: String,
    #[serde(alias = "texto")]
    pub text: String,
    #[serde(default = "half", alias = "intensidad")]
    pub intensity: f32,
    /// trait | emotion | value | goal
    #[serde(default = "trait_kind", alias = "tipo")]
    pub kind: String,
}

fn trait_kind() -> String {
    "trait".into()
}

#[derive(Debug, Deserialize)]
pub struct MemorySeed {
    #[serde(alias = "titulo")]
    pub title: String,
    #[serde(alias = "texto")]
    pub text: String,
    #[serde(default = "half", alias = "importancia")]
    pub importance: f32,
    #[serde(default, alias = "valencia")]
    pub valence: f32,
    /// Days ago (so time decay makes sense from the start).
    #[serde(default, alias = "hace_dias")]
    pub days_ago: u32,
    /// Places/entities/characters by name; created if they do not exist.
    #[serde(default, alias = "involucra")]
    pub involves: Vec<String>,
    /// Traits or emotions (by name) this memory evokes.
    #[serde(default, alias = "evoca")]
    pub evokes: Vec<String>,
    #[serde(default, alias = "relacionado")]
    pub related: Vec<String>,
    #[serde(default, alias = "contradice")]
    pub contradicts: Vec<String>,
    #[serde(default, alias = "causa")]
    pub causes: Vec<String>,
    /// Year of the story it happened (overrides `days_ago` for the story time).
    #[serde(default, alias = "anio", alias = "año")]
    pub year: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RelationSeed {
    #[serde(alias = "de")]
    pub from: String,
    #[serde(alias = "a")]
    pub to: String,
    pub opinion: String,
    #[serde(default, alias = "afecto")]
    pub affection: f32,
    #[serde(default = "half", alias = "cercania")]
    pub closeness: f32,
}

fn trait_node_kind(kind: &str) -> Result<NodeKind, String> {
    Ok(match memory_core::text::fold(kind).as_str() {
        "trait" | "rasgo" => NodeKind::Trait,
        "emotion" | "emocion" => NodeKind::Emotion,
        "value" | "belief" | "valor" | "creencia" => NodeKind::Value,
        "goal" | "wish" | "meta" | "deseo" => NodeKind::Goal,
        other => return Err(format!("unknown trait kind: {other}")),
    })
}

fn item_kind(kind: Option<&str>) -> Result<NodeKind, String> {
    Ok(match kind.map(memory_core::text::fold).as_deref() {
        None | Some("fact" | "hecho" | "dato") => NodeKind::SemanticFact,
        Some("person" | "entity" | "object" | "persona" | "entidad" | "objeto") => NodeKind::Entity,
        Some("place" | "lugar") => NodeKind::Place,
        Some("faction" | "organisation" | "organization" | "faccion" | "organizacion") => NodeKind::Faction,
        Some("topic" | "tema") => NodeKind::Topic,
        Some("rule" | "lore" | "regla") => NodeKind::Lore,
        Some("history" | "event" | "historia" | "evento") => NodeKind::Event,
        Some("concept" | "definition" | "concepto" | "definicion") => NodeKind::Concept,
        Some(other) => return Err(format!("unknown kind: {other}")),
    })
}

/// Edges declared by name (`related`, `contradicts`, `causes`). A name that does not
/// exist is an error (almost always a typo in the seed).
fn link_named<'a>(g: &mut memory_core::MemoryGraph, items: impl Iterator<Item = Links<'a>>, now: i64) -> Result<(), String> {
    for it in items {
        let Some(src) = g.find_by_label(it.name, None) else { continue };
        let groups = [(it.related, EdgeKind::RelatedTo, 0.6), (it.contradicts, EdgeKind::Contradicts, 0.7), (it.causes, EdgeKind::Causes, 0.7)];
        for (names, kind, weight) in groups {
            for other in names {
                let dst = g
                    .find_by_label(other, None)
                    .ok_or_else(|| format!("\"{}\" links to \"{other}\", which does not exist in {}", it.name, g.meta.name))?;
                if src != dst {
                    g.connect(src, dst, kind, weight, now).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}

/// Applies the seed's appearance (canonical aspects and visual perceptions) to a world.
/// It also works on a world that was already seeded: it replaces the canonical
/// appearance and updates the perceptions by name, leaving the rest of the memory and
/// the chats alone. Returns how many characters changed.
pub fn apply_appearance(world: &World, seed: &Seed) -> Result<usize, String> {
    let mut changed = 0;
    for c in &seed.characters {
        if c.aspects.is_empty() {
            continue;
        }
        let h = world.character_by_name(&c.name).ok_or_else(|| format!("appearance: {} does not exist", c.name))?;
        let aspects: Vec<Aspect> = c.aspects.iter().map(|a| Aspect { label: a.name.clone(), content: a.text.clone(), weight: a.importance }).collect();
        world.set_appearance(h.id(), &aspects, true).map_err(|e| format!("{}: {e}", c.name))?;
        changed += 1;
    }
    for v in &seed.visual_perceptions {
        let a = world.character_by_name(&v.from).ok_or_else(|| format!("visual perception: {} does not exist", v.from))?;
        let b = world.character_by_name(&v.to).ok_or_else(|| format!("visual perception: {} does not exist", v.to))?;
        let aspect = Aspect { label: v.name.clone(), content: v.text.clone(), weight: v.importance };
        world.set_visual_perception(a.id(), b.id(), &aspect).map_err(|e| e.to_string())?;
    }
    Ok(changed)
}

/// Restores all canonical character `.mem` files in `world` back to their clean seed
/// state while preserving their `GraphId`s, avatars, and existing chats (migrating any
/// existing chat's participant memories to Carbon Copies first).
pub fn restore_characters(world: &World, seed: &Seed) -> Result<usize, String> {
    for summary in memory_core::conversation::list_conversations(&world.dir).into_iter().flatten() {
        for &p in &summary.participants {
            let _ = world.chat_character(summary.id, p);
        }
    }
    let temp_dir = std::env::temp_dir().join(format!("chats-restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let temp_lib = Library::new(&temp_dir, memory_core::HandlerConfig::with_lang(world.lang()));
    let fresh = plant(&temp_lib, seed)?;

    let mut id_map = HashMap::new();
    for c in &seed.characters {
        let f = fresh.character_by_name(&c.name).ok_or_else(|| format!("restore: {} missing in seed", c.name))?;
        let r = world.character_by_name(&c.name).ok_or_else(|| format!("restore: {} does not exist in world", c.name))?;
        id_map.insert(f.id(), r.id());
    }

    let mut restored = 0;
    for c in &seed.characters {
        let f = fresh.character_by_name(&c.name).unwrap();
        let r = world.character_by_name(&c.name).unwrap();
        let real_id = r.id();
        let avatar = r.meta().avatar;
        let mut g = f.read(|g| g.clone());
        g.meta.id = real_id;
        g.meta.avatar = avatar;
        let exts: Vec<(NodeId, memory_core::ExternalRef)> = g.externals().collect();
        for (nid, ext) in exts {
            if ext.scope == memory_core::Scope::Character
                && let Some(&mapped) = id_map.get(&ext.id)
            {
                let _ = g.set_external(nid, Some(memory_core::ExternalRef { scope: memory_core::Scope::Character, id: mapped }));
            }
        }
        r.write(|dst| *dst = g);
        r.save().map_err(|e| e.to_string())?;
        restored += 1;
    }
    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(restored)
}

/// Story time of a `year` field (`STORY_ALWAYS` without one).
fn since_year(year: Option<i64>) -> StoryTime {
    year.map_or(STORY_ALWAYS, |y| story_of_year(y, 0))
}

/// The seed's timeline, saved in the world. Returns it by folded name.
fn plant_timeline(world: &World, s: &WorldSeed) -> Result<HashMap<String, Moment>, String> {
    let moments: Vec<Moment> = s
        .timeline
        .iter()
        .map(|m| Moment { id: 0, name: m.name.clone(), at: story_of_year(m.year, m.day), summary: m.summary.clone() })
        .collect();
    let saved = world.set_timeline(moments).map_err(|e| e.to_string())?;
    let by_name: HashMap<String, Moment> = saved.into_iter().map(|m| (fold(&m.name), m)).collect();
    if by_name.len() != s.timeline.len() {
        return Err("two moments of the timeline have the same name".into());
    }
    Ok(by_name)
}

fn find_moment<'a>(timeline: &'a HashMap<String, Moment>, name: &str, who: &str) -> Result<&'a Moment, String> {
    timeline.get(&fold(name)).ok_or_else(|| format!("{who}: \"{name}\" is not a moment of the timeline"))
}

/// Adds the seed's definitions to the shared libraries. A definition that already exists
/// (seeded by another world of the category, or edited by the user) is kept as it is.
/// Returns how many were added.
pub fn plant_definitions(world: &World, seed: &Seed) -> Result<usize, String> {
    let now = unix_now();
    let mut added = 0;
    let mut touched = Vec::new();
    for d in &seed.definitions {
        let h = world.definition_library(d.level, true).map_err(|e| e.to_string())?.ok_or("no library for definitions")?;
        let kind = match d.kind.as_deref() {
            None => NodeKind::Concept,
            k => item_kind(k)?,
        };
        let new = h.write(|g| -> Result<bool, String> {
            if g.find_by_label(&d.name, None).is_some() {
                return Ok(false);
            }
            let id = g.add_node(MemoryNode::new(kind, &d.name, &d.text, now).with_weight(d.importance)).map_err(|e| e.to_string())?;
            let root = g.root();
            g.connect(root, id, EdgeKind::Knows, d.importance.max(0.3), now).map_err(|e| e.to_string())?;
            Ok(true)
        })?;
        added += usize::from(new);
        if !touched.iter().any(|t: &memory_core::MemoryHandler| t.path() == h.path()) {
            touched.push(h);
        }
    }
    // Associations between definitions of the same library (a name in another library is
    // simply skipped: libraries do not link to each other).
    for d in &seed.definitions {
        let Some(h) = world.definition_library(d.level, false).map_err(|e| e.to_string())? else { continue };
        h.write(|g| {
            let Some(src) = g.find_by_label(&d.name, None) else { return };
            for other in &d.related {
                if let Some(dst) = g.find_by_label(other, None).filter(|&dst| dst != src) {
                    let _ = g.connect(src, dst, EdgeKind::RelatedTo, 0.6, now);
                }
            }
        });
    }
    for h in touched {
        h.save().map_err(|e| e.to_string())?;
    }
    Ok(added)
}

pub fn plant(lib: &Library, seed: &Seed) -> Result<World, String> {
    let s = &seed.world;
    let world = lib.create_world(&s.category, &s.name, &s.premise, &s.style).map_err(|e| e.to_string())?;
    let now = unix_now();
    let timeline = plant_timeline(&world, s)?;
    if let Some(look) = &s.look {
        world.set_style(look).map_err(|e| e.to_string())?;
    }
    plant_definitions(&world, seed)?;
    let present = s.present.as_deref().map(|p| find_moment(&timeline, p, "present")).transpose()?;
    world.memory.write(|g| -> Result<(), String> {
        let root = g.root();
        let sections = [
            (&s.rules, NodeKind::Lore),
            (&s.places, NodeKind::Place),
            (&s.factions, NodeKind::Faction),
            (&s.history, NodeKind::Event),
            (&s.people, NodeKind::Entity),
        ];
        for (items, kind) in sections {
            for it in items {
                let node = MemoryNode::new(kind, &it.name, &it.text, now).with_weight(it.importance).since(since_year(it.year));
                let id = g.add_node(node).map_err(|e| e.to_string())?;
                let edge = if kind == NodeKind::Lore { EdgeKind::Composes } else { EdgeKind::Knows };
                g.connect(root, id, edge, it.importance.max(0.3), now).map_err(|e| e.to_string())?;
            }
        }
        link_named(g, sections.iter().flat_map(|(items, _)| items.iter()).map(Links::from), now)
    })?;

    for c in &seed.characters {
        let h = world.create_character(&c.name, &c.identity, &c.public_bio, &c.appearance).map_err(|e| format!("{}: {e}", c.name))?;
        let home = c.moment.as_deref().map(|m| find_moment(&timeline, m, &c.name)).transpose()?;
        // Their memories' `days_ago` count back from their own moment, or the world's present.
        let anchor = home.or(present).map(|m| m.at);
        let mut moments: Vec<CharacterMoment> = home.map(|m| CharacterMoment { moment: m.id, scenario: c.scenario.clone() }).into_iter().collect();
        for m in &c.moments {
            moments.push(CharacterMoment { moment: find_moment(&timeline, &m.moment, &c.name)?.id, scenario: m.scenario.clone() });
        }
        if !moments.is_empty() {
            world.set_character_moments(h.id(), moments).map_err(|e| format!("{}: {e}", c.name))?;
        }
        h.write(|g| -> Result<(), String> {
            g.meta.scenario = c.scenario.clone();
            let root = g.root();
            let mut by_name: HashMap<String, NodeId> = HashMap::new();
            for t in &c.traits {
                let id = g.add_trait(root, trait_node_kind(&t.kind)?, &t.name, &t.text, t.intensity, now).map_err(|e| e.to_string())?;
                by_name.insert(memory_core::text::fold(&t.name), id);
            }
            for k in &c.knows {
                let kind = item_kind(k.kind.as_deref()).map_err(|e| format!("{}: {e}", c.name))?;
                let node = MemoryNode::new(kind, &k.name, &k.text, now).with_weight(k.importance).since(since_year(k.year));
                let id = g.add_node(node).map_err(|e| e.to_string())?;
                g.connect(root, id, EdgeKind::Knows, k.importance.max(0.3), now).map_err(|e| e.to_string())?;
            }
            let mut timeline: Vec<(i64, NodeId)> = Vec::new();
            for m in &c.memories {
                let when = now - m.days_ago as i64 * 86_400;
                let mut involves = Vec::new();
                for name in &m.involves {
                    let id = match g.find_by_label(name, None) {
                        Some(id) => id,
                        None => g.add_node(MemoryNode::new(NodeKind::Entity, name, "", when)).map_err(|e| e.to_string())?,
                    };
                    involves.push(id);
                }
                let evokes: Vec<NodeId> = m
                    .evokes
                    .iter()
                    .map(|n| by_name.get(&memory_core::text::fold(n)).copied().ok_or_else(|| format!("{}: \"{n}\" is not one of their traits", c.name)))
                    .collect::<Result<_, _>>()?;
                let since = match (m.year, anchor) {
                    (Some(y), _) => story_of_year(y, 0),
                    (None, Some(a)) => a - m.days_ago as i64,
                    (None, None) => STORY_ALWAYS,
                };
                let mut node =
                    MemoryNode::new(NodeKind::EpisodicMemory, &m.title, &m.text, when).with_weight(m.importance).with_valence(m.valence).since(since);
                node.last_accessed = when;
                let id = g.add_memory(node, &involves, &evokes, when).map_err(|e| e.to_string())?;
                timeline.push((when, id));
            }
            // A soft time chain between consecutive memories ("and what happened next?").
            timeline.sort_by_key(|(t, _)| *t);
            for w in timeline.windows(2) {
                g.connect(w[0].1, w[1].1, EdgeKind::Precedes, 0.25, now).map_err(|e| e.to_string())?;
            }
            link_named(g, c.knows.iter().map(Links::from).chain(c.memories.iter().map(Links::from)), now)
        })?;
        h.save().map_err(|e| e.to_string())?;
    }

    // Named characters that appeared in memories as an "entity" become linked character
    // nodes (before the relationships, so these reuse them).
    for h in world.characters() {
        let roster = world.roster();
        h.write(|g| {
            let ids: Vec<NodeId> = g.nodes().filter(|(_, n)| n.kind == NodeKind::Entity).map(|(id, _)| id).collect();
            for id in ids {
                let label = g.node(id).map(|n| memory_core::text::fold(&n.label)).unwrap_or_default();
                if let Some(r) = roster.get(&label).copied().filter(|r| r.id != g.meta.id)
                    && g.find_external(r).is_none()
                {
                    let patch = memory_core::NodePatch { kind: Some(NodeKind::Character), ..Default::default() };
                    if g.update_node(id, patch).is_ok() {
                        let _ = g.set_external(id, Some(r));
                    }
                }
            }
        });
    }
    for r in &seed.relations {
        let a = world.character_by_name(&r.from).ok_or_else(|| format!("relation: {} does not exist", r.from))?;
        let b = world.character_by_name(&r.to).ok_or_else(|| format!("relation: {} does not exist", r.to))?;
        world
            .set_perception(a.id(), b.id(), &Perception { content: r.opinion.clone(), affection: r.affection, closeness: r.closeness })
            .map_err(|e| e.to_string())?;
    }
    apply_appearance(&world, seed)?;
    world.save_all().map_err(|e| e.to_string())?;
    Ok(world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_core::{ExternalRef, HandlerConfig, Lang, Scope};

    fn terra(dir_name: &str) -> World {
        let dir = std::env::temp_dir().join(format!("chats-{dir_name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seed: Seed = toml::from_str(include_str!("../../../seeds/terra.toml")).unwrap();
        plant(&Library::new(&dir, HandlerConfig::with_lang(Lang::Es)), &seed).unwrap()
    }

    #[test]
    fn example_seed_plants_linked_world_without_duplicates() {
        let dir = std::env::temp_dir().join(format!("chats-seed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seed: Seed = toml::from_str(include_str!("../../../docs/example-seed.toml")).unwrap();
        let world = plant(&Library::new(&dir, HandlerConfig::default()), &seed).unwrap();

        let lyra = world.character_by_name("Lyra").unwrap();
        let kael = world.character_by_name("Kael").unwrap();
        let kael_ref = ExternalRef { scope: Scope::Character, id: kael.id() };
        lyra.read(|g| {
            let kaels: Vec<_> = g.nodes().filter(|(_, n)| n.label == "Kael").collect();
            assert_eq!(kaels.len(), 1, "the Kael of the memory and the one of the relationship are the same node");
            assert_eq!(g.find_external(kael_ref), Some(kaels[0].0));
            assert_eq!(kaels[0].1.kind, NodeKind::Character);
            assert!((kaels[0].1.valence - 0.7).abs() < 1e-6);
        });
        assert!(world.memory.read(|g| g.nodes().any(|(_, n)| n.kind == NodeKind::Lore)));
        assert!(plant(&Library::new(&dir, HandlerConfig::default()), &seed).is_err(), "a world is not seeded twice");
    }

    /// Two worlds of a category seeding the same definition share one node; a universal
    /// one goes to the universal library; a user's edit is never overwritten by a seed.
    #[test]
    fn definitions_are_shared_between_worlds_of_a_category() {
        let dir = std::env::temp_dir().join(format!("chats-seed-defs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = Library::new(&dir, HandlerConfig::default());
        let seed = |name: &str, text: &str| -> Seed {
            toml::from_str(&format!(
                r#"
                [world]
                category = "Arknights"
                name = "{name}"
                premise = "A world."
                [[definitions]]
                name = "Originium"
                text = "{text}"
                related = ["Oripathy", "Iron"]
                [[definitions]]
                name = "Oripathy"
                text = "The illness Originium causes."
                [[definitions]]
                level = "universal"
                name = "Iron"
                text = "A grey metal."
                kind = "object"
                "#
            ))
            .unwrap()
        };
        let terra = plant(&lib, &seed("Terra", "A black crystal.")).unwrap();
        let arknights = terra.definition_library(memory_core::world::DefinitionLevel::Category, false).unwrap().unwrap();
        // The user rewrites the definition; the second world's seed must keep it.
        arknights.write(|g| {
            let id = g.find_by_label("Originium", None).unwrap();
            g.update_node(id, memory_core::NodePatch { content: Some("Edited by the user.".into()), ..Default::default() }).unwrap();
        });
        let talos = plant(&lib, &seed("Talos II", "Another text.")).unwrap();
        assert_eq!(talos.definitions().len(), 2);
        arknights.read(|g| {
            let o = g.find_by_label("Originium", None).unwrap();
            assert_eq!(g.nodes().filter(|(_, n)| n.label == "Originium").count(), 1, "one shared node");
            assert_eq!(g.node(o).unwrap().content, "Edited by the user.");
            assert_eq!(g.node(o).unwrap().kind, NodeKind::Concept);
            let ori = g.find_by_label("Oripathy", None).unwrap();
            assert!(g.edge(o, ori).is_some(), "related within the library");
            assert!(g.find_by_label("Iron", None).is_none(), "the universal one lives elsewhere");
        });
        let universal = terra.definition_library(memory_core::world::DefinitionLevel::Universal, false).unwrap().unwrap();
        assert_eq!(universal.read(|g| g.find_by_label("Iron", None).map(|id| g.node(id).unwrap().kind)), Some(NodeKind::Entity));
    }

    #[test]
    fn old_spanish_keys_still_parse() {
        let old = r#"
            [mundo]
            categoria = "pruebas"
            nombre = "Viejo"
            premisa = "Un mundo con claves en español."
            [[personajes]]
            nombre = "Ana"
            identidad = "Médica"
            bio_publica = "La médica"
              [[personajes.rasgos]]
              nombre = "Paciente"
              texto = "Escucha sin prisa"
              tipo = "emocion"
              [[personajes.recuerdos]]
              titulo = "La guardia"
              texto = "Una noche larga"
              hace_dias = 3
              evoca = ["Paciente"]
        "#;
        let seed: Seed = toml::from_str(old).unwrap();
        assert_eq!(seed.world.name, "Viejo");
        assert_eq!(seed.characters[0].memories[0].days_ago, 3);
        assert_eq!(trait_node_kind(&seed.characters[0].traits[0].kind), Ok(NodeKind::Emotion));
    }

    #[test]
    fn terra_secrets_stay_private_in_scenes() {
        let world = terra("terra");
        let id = |n: &str| world.character_by_name(n).unwrap().id();
        let (kal, amiya, doctor) = (id("Kal'tsit"), id("Amiya"), id("Doctor"));
        let q = "¿Qué pasó realmente la noche en que murió Theresa? ¿Dónde estaba el Doctor?";

        let a = world.scene_recall(amiya, q, &[kal, doctor]).unwrap().context;
        assert!(a.contains("muñeco de trapo") && a.contains("Muerte de Theresa"), "{a}");
        let k = world.scene_recall(kal, q, &[amiya, doctor]).unwrap().context;
        assert!(k.contains("La traición del Doctor"), "{k}");
        let d = world.scene_recall(doctor, "¿Qué hice en Babel? ¿Por qué Kal'tsit me odia?", &[kal]).unwrap().context;
        for (who, ctx) in [("Amiya", &a), ("Doctor", &d)] {
            for secret in ["pactó con Theresis", "AMa-10", "Predecesores"] {
                assert!(!ctx.contains(secret), "secret «{secret}» leaked to {who}:\n{ctx}");
            }
        }
        for ctx in [&a, &k, &d] {
            assert!(ctx.len() <= HandlerConfig::default().budget.max_chars + 800, "{} characters", ctx.len());
        }
    }

    /// Terra's timeline: at Babel (1093) Theresa is alive, so nobody remembers her death;
    /// in the present (winter of 1098) everyone does, and the Lone Trail (1099) has not
    /// happened yet. Theresa only belongs to Babel.
    #[test]
    fn terra_timeline_sets_what_each_moment_knows() {
        let world = terra("terra-timeline");
        let timeline = world.timeline();
        assert_eq!(timeline.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Babel", "Tras Londinium"]);
        let (babel, present) = (timeline[0].id, timeline[1].id);
        let kal = world.character_by_name("Kal'tsit").unwrap().id();
        let theresa = world.character_by_name("Theresa").unwrap().meta();
        assert!(theresa.fits(Some(babel)) && !theresa.fits(Some(present)));
        assert!(theresa.scenario_at(Some(babel)).contains("Babel"));

        let q = "¿Qué pasó la noche en que murió Theresa?";
        let at_babel = world.scene_recall_at(kal, q, &[kal], None, world.story_now(Some(babel))).unwrap().context;
        for later in ["Muerte de Theresa", "Lejos cuando mataron a Theresa", "Crisis de Londinium"] {
            assert!(!at_babel.contains(later), "«{later}» has not happened at Babel:\n{at_babel}");
        }
        let now = world.scene_recall_at(kal, q, &[kal], None, world.story_now(None)).unwrap().context;
        assert!(now.contains("Muerte de Theresa") && now.contains("Lejos cuando mataron a Theresa"), "{now}");
        let future = world.scene_recall_at(kal, "Proyecto Lone Trail en Trimounts", &[kal], None, world.story_now(None)).unwrap().context;
        assert!(!future.contains("Proyecto Lone Trail"), "1099 is after the present:\n{future}");
    }

    /// Table of questions about Terra: what each one must remember and what they must not
    /// know. It measures recall quality with ~150 nodes per character and a lot of noise.
    #[test]
    fn terra_recall_quality_table() {
        let world = terra("terra-probes");
        let id = |n: &str| world.character_by_name(n).unwrap().id();
        let (kal, amiya, doc) = (id("Kal'tsit"), id("Amiya"), id("Doctor"));
        // Every secret node in the seed starts with «SECRETO:».
        const SECRETS: &[&str] = &["SECRETO", "pactó con Theresis"];

        // (who speaks, present, question, must contain, may know secrets)
        type Probe<'a> = (u64, &'a [u64], &'a str, &'a [&'a str], bool);
        let probes: &[Probe] = &[
            (kal, &[kal], "¿Qué recuerdas de la noche en que murió Theresa?", &["Lejos cuando mataron a Theresa", "La traición del Doctor"], true),
            (kal, &[kal, doc], "Doctor, ¿qué hiciste en Babel?", &["La traición del Doctor"], true),
            (kal, &[kal], "¿Cuántos anillos le quedan a Amiya?", &["Anillos de Amiya", "nueve"], true),
            (kal, &[kal], "¿Qué es Mon3tr en realidad?", &["Mon3tr"], true),
            (kal, &[kal], "¿Has dormido algo? Llevas ochenta horas en el quirófano", &["Ochenta horas de quirófano"], true),
            (kal, &[kal], "Háblame de la decapitación del rey Yliš en 898", &["La decapitación del rey Yliš"], true),
            (kal, &[kal], "¿Por qué Warfarin quería una muestra de sangre?", &["Muestras de sangre"], true),
            (kal, &[kal, amiya], "¿Qué opinas del aspecto de Amiya últimamente?", &["Amiya · Anillos"], true),
            (amiya, &[amiya], "¿Qué pasó la noche en que murió Theresa?", &["El cuento del muñeco de trapo", "La espada que no dolió"], false),
            (amiya, &[amiya], "Cuéntame lo de Misha", &["Lo que pasó con Misha"], false),
            (amiya, &[amiya], "¿Qué te dijo Patriot antes de morir?", &["Profecía de Patriot"], false),
            (amiya, &[amiya], "¿Cómo conociste al Doctor?", &["La mano del Doctor"], false),
            (amiya, &[amiya], "¿Qué le regalaste al Doctor después de Rim Billiton?", &["El vivario para el Doctor"], false),
            (amiya, &[amiya, kal], "¿Cómo ves a Kal'tsit hoy?", &["Kal'tsit · Ojos"], false),
            (amiya, &[amiya], "¿Qué falta en los suministros?", &["Informe de suministros"], false),
            (doc, &[doc], "¿Qué es lo primero que recuerdas?", &["Despertar"], false),
            (doc, &[doc], "¿Con qué sueñas por las noches?", &["Fragmento: el landship en llamas"], false),
            (doc, &[doc, kal], "¿Kal'tsit te odia?", &["Kal'tsit me desprecia"], false),
            (doc, &[doc], "¿Qué te dijo W en Londinium?", &["W apunta al Doctor"], false),
            (doc, &[doc, amiya], "¿Cómo está Amiya? Mira sus manos", &["Amiya · Anillos"], false),
        ];

        let mut failures = Vec::new();
        for (who, present, q, expected, may_know) in probes {
            let ctx = world.scene_recall(*who, q, present).unwrap().context;
            let name = world.character(*who).unwrap().meta().name;
            for e in *expected {
                if !ctx.contains(e) {
                    failures.push(format!("{name} · «{q}» → missing «{e}»"));
                }
            }
            if !may_know {
                for s in SECRETS {
                    if ctx.contains(s) {
                        failures.push(format!("{name} · «{q}» → LEAKS «{s}»"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{} failures out of {} probes:\n{}", failures.len(), probes.len(), failures.join("\n"));
    }
}
