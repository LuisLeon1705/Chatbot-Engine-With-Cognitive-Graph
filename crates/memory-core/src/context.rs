//! Building the context blocks from the active subgraph.
//!
//! - `actor_block`: clean text for the roleplay model (no internal ids).
//! - `curator_block`: the same memory with `#id` and kind, so the curator model can
//!   refer to existing nodes in its operations.
//!
//! Section titles and labels come from the dictionary in the handler's language; the
//! XML-like tags (`<world>`, `<scene>`, `<active_memory>`) are structural and always English.

use std::fmt::Write;

use i18n::{Lang, tr};

use crate::activation::Activated;
use crate::graph::MemoryGraph;
use crate::model::{DAYS_PER_YEAR, Moment, NodeId, NodeKind, STORY_ALWAYS, STORY_OPEN, STORY_PRESENT, Scope, StoryTime, Validity};

/// Most closed relationships written into a block (the most recently closed first).
const MAX_PAST_LINES: usize = 3;
/// Most links between the listed nodes shown to the curator.
const MAX_CURATOR_EDGES: usize = 10;

/// Approximate size of the block (≈4 characters per token).
#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    pub max_chars: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self { max_chars: 4800 }
    }
}

/// "3 days ago", "2 h ago"…
pub fn relative_time(lang: Lang, then: i64, now: i64) -> String {
    let d = (now - then).max(0);
    match d {
        0..60 => tr!(lang, "time.ago.now"),
        60..3600 => tr!(lang, "time.ago.minutes", n = d / 60),
        3600..86_400 => tr!(lang, "time.ago.hours", n = d / 3600),
        86_400..172_800 => tr!(lang, "time.ago.yesterday"),
        172_800..2_592_000 => tr!(lang, "time.ago.days", n = d / 86_400),
        2_592_000..5_184_000 => tr!(lang, "time.ago.month"),
        5_184_000..31_536_000 => tr!(lang, "time.ago.months", n = d / 2_592_000),
        31_536_000..63_072_000 => tr!(lang, "time.ago.year"),
        _ => tr!(lang, "time.ago.years", n = d / 31_536_000),
    }
}

/// A story time in words: the timeline's moment when it falls on one, else the year.
pub fn story_label(lang: Lang, t: StoryTime, timeline: &[Moment]) -> String {
    match t {
        STORY_ALWAYS => i18n::t(lang, "story.always").to_owned(),
        STORY_OPEN => i18n::t(lang, "story.open").to_owned(),
        STORY_PRESENT => i18n::t(lang, "story.present").to_owned(),
        _ => match timeline.iter().find(|m| m.at == t) {
            Some(m) => m.name.clone(),
            None => {
                let (year, day) = (t.div_euclid(DAYS_PER_YEAR), t.rem_euclid(DAYS_PER_YEAR));
                if day == 0 { tr!(lang, "story.year", year = year) } else { tr!(lang, "story.year_day", year = year, day = day) }
            }
        },
    }
}

/// Puts the valence (affection) and weight (closeness) of a relationship into words for the actor.
pub fn relationship(lang: Lang, valence: f32, closeness: f32) -> String {
    let affect = match valence {
        v if v >= 0.6 => "ctx.relation.love",
        v if v >= 0.2 => "ctx.relation.like",
        v if v > -0.2 => "ctx.relation.neutral",
        v if v > -0.6 => "ctx.relation.distrust",
        _ => "ctx.relation.hate",
    };
    let close = match closeness {
        c if c >= 0.7 => "ctx.relation.close",
        c if c >= 0.4 => "ctx.relation.known",
        _ => "ctx.relation.far",
    };
    format!("{}; {}", i18n::t(lang, affect), i18n::t(lang, close))
}

/// "Label: content", or just the label when there is no content.
fn lc(label: &str, content: &str) -> String {
    if content.trim().is_empty() { label.to_owned() } else { format!("{label}: {content}") }
}

impl MemoryGraph {
    /// Block for the actor. The nodes already come sorted by relevance: if the budget
    /// runs out, the least relevant ones are dropped. A world graph produces
    /// `<world>…</world>`; a chat graph, `<chat_memory>…</chat_memory>`; a character
    /// graph, `<active_memory>…</active_memory>`.
    pub fn actor_block(&self, active: &[Activated], now: i64, budget: ContextBudget, lang: Lang) -> String {
        self.actor_block_at(active, now, STORY_PRESENT, budget, lang)
    }

    /// `actor_block` at a story time: relationships that no longer hold then and touch the
    /// recalled nodes are added as the past, so the actor does not take them as current.
    pub fn actor_block_at(&self, active: &[Activated], now: i64, story: StoryTime, budget: ContextBudget, lang: Lang) -> String {
        if self.scope() == Scope::Chat {
            return self.chronicle_block(false, active, now, story, budget, lang);
        }
        if self.scope() == Scope::Collab {
            return self.chronicle_block(true, active, now, story, budget, lang);
        }
        if self.scope() == Scope::Library {
            return self.definitions_block(active, budget, lang);
        }
        let world = self.scope() == Scope::World;
        // (title, lines) in display order
        let titles: &[&str] = if world {
            &["ctx.world.rules", "ctx.world.history", "ctx.world.people", "ctx.world.looks"]
        } else {
            &["ctx.character.personality", "ctx.character.people", "ctx.character.memories", "ctx.character.knowledge", "ctx.character.looks"]
        };
        let mut sections: Vec<(&str, Vec<String>)> = titles.iter().map(|k| (i18n::t(lang, k), Vec::new())).collect();
        let mut picked: Vec<(usize, String)> = Vec::with_capacity(active.len());
        for a in active {
            let Some(n) = self.node(a.id) else { continue };
            let (slot, line) = match n.kind {
                NodeKind::Appearance => {
                    let owner = self
                        .appearance_owner(a.id)
                        .map_or_else(|| "?".to_owned(), |(o, own)| if own { tr!(lang, "ctx.character.you") } else { o });
                    (if world { 3 } else { 4 }, format!("- {owner} · {}", lc(&n.label, &n.content)))
                }
                NodeKind::EpisodicMemory => {
                    (if world { 1 } else { 2 }, format!("- ({}) {}", relative_time(lang, n.created_at, now), lc(&n.label, &n.content)))
                }
                NodeKind::Character if !world => (1, format!("- {} ({}): {}", n.label, relationship(lang, n.valence, n.weight), n.content)),
                k if k.is_personality() && !world => {
                    (0, format!("- [{} {:.0}%] {}", k.context_label(lang), n.weight * 100.0, lc(&n.label, &n.content)))
                }
                NodeKind::Lore if world => (0, format!("- {}", lc(&n.label, &n.content))),
                NodeKind::Event | NodeKind::SemanticFact if world => (1, format!("- {}", lc(&n.label, &n.content))),
                k if world => (2, format!("- [{}] {}", k.context_label(lang), lc(&n.label, &n.content))),
                k => (3, format!("- [{}] {}", k.context_label(lang), lc(&n.label, &n.content))),
            };
            picked.push((slot, line));
        }
        self.push_past(active, story, lang, &mut sections, &mut picked);

        let (open, close) = if world { ("<world>", "</world>") } else { ("<active_memory>", "</active_memory>") };
        let root = self.node(self.root()).expect("the root always exists");
        let mut out = String::new();
        if world {
            let _ = writeln!(out, "{open}\n## {}\n{}", root.label, root.content);
        } else {
            let _ = writeln!(out, "{open}\n{}\n{}: {}", i18n::t(lang, "ctx.character.who"), root.label, root.content);
        }
        fill_sections(out, sections, picked, close, budget)
    }

    /// The `<chat_memory>` block (what happened in this conversation, the rules set for it
    /// and what is known in it; everyone in the chat shares it) or, for a Collab, the
    /// `<collab>` block: the same for the place where several worlds meet.
    fn chronicle_block(&self, collab: bool, active: &[Activated], now: i64, story: StoryTime, budget: ContextBudget, lang: Lang) -> String {
        let titles = if collab {
            ["ctx.collab.rules", "ctx.collab.events", "ctx.collab.known", "ctx.collab.looks"]
        } else {
            ["ctx.chat.rules", "ctx.chat.events", "ctx.chat.known", "ctx.chat.looks"]
        };
        let mut sections: Vec<(&str, Vec<String>)> = titles.iter().map(|k| (i18n::t(lang, k), Vec::new())).collect();
        let mut picked: Vec<(usize, String)> = Vec::with_capacity(active.len());
        for a in active {
            let Some(n) = self.node(a.id) else { continue };
            let (slot, line) = match n.kind {
                NodeKind::Lore => (0, format!("- {}", lc(&n.label, &n.content))),
                NodeKind::EpisodicMemory => (1, format!("- ({}) {}", relative_time(lang, n.created_at, now), lc(&n.label, &n.content))),
                NodeKind::Appearance => {
                    let owner = self.appearance_owner(a.id).map_or_else(|| "?".to_owned(), |(o, _)| o);
                    (3, format!("- {owner} · {}", lc(&n.label, &n.content)))
                }
                k => (2, format!("- [{}] {}", k.context_label(lang), lc(&n.label, &n.content))),
            };
            picked.push((slot, line));
        }
        self.push_past(active, story, lang, &mut sections, &mut picked);
        let root = self.node(self.root()).expect("the root always exists");
        let (open, title, close) = if collab {
            ("<collab>", tr!(lang, "ctx.collab.title", name = self.meta.name), "</collab>")
        } else {
            ("<chat_memory>", i18n::t(lang, "ctx.chat.title").to_owned(), "</chat_memory>")
        };
        let mut out = format!("{open}\n{title}\n");
        if !root.content.trim().is_empty() {
            out.push_str(root.content.trim());
            out.push('\n');
        }
        fill_sections(out, sections, picked, close, budget)
    }

    /// The `<definitions>` block: the shared definitions this turn touched, the objective
    /// baseline any character of the library's worlds can rely on. Empty when none was
    /// touched (a library's root says nothing by itself).
    fn definitions_block(&self, active: &[Activated], budget: ContextBudget, lang: Lang) -> String {
        let root = self.root();
        let picked: Vec<(usize, String)> = active
            .iter()
            .filter(|a| a.id != root)
            .filter_map(|a| self.node(a.id))
            .map(|n| (0, format!("- {}", lc(&n.label, &n.content))))
            .collect();
        if picked.is_empty() {
            return String::new();
        }
        let title = tr!(lang, "ctx.definitions.title", name = self.meta.name);
        let sections = vec![(title.as_str(), Vec::new())];
        fill_sections("<definitions>\n".to_owned(), sections, picked, "</definitions>", budget)
    }

    /// Adds a last section with the relationships around `active` that no longer hold at
    /// `story` (lowest priority: they only go in if the budget allows).
    fn push_past(&self, active: &[Activated], story: StoryTime, lang: Lang, sections: &mut Vec<(&str, Vec<String>)>, picked: &mut Vec<(usize, String)>) {
        let around: Vec<NodeId> = active.iter().map(|a| a.id).collect();
        let past = self.past_edges(&around, story);
        if past.is_empty() {
            return;
        }
        let slot = sections.len();
        sections.push((i18n::t(lang, "ctx.past.title"), Vec::new()));
        for (s, d, e) in past.into_iter().take(MAX_PAST_LINES) {
            let (Some(a), Some(b)) = (self.node(s), self.node(d)) else { continue };
            picked.push((slot, tr!(lang, "ctx.past.line", source = a.label, edge = e.kind.label(lang), target = b.label)));
        }
    }
}

/// Accepts lines by relevance while they fit and writes them grouped by section.
/// `active` comes sorted by score, so lines are taken in that order (counting their
/// section's title the first time); only afterwards are they grouped to be written.
fn fill_sections(mut out: String, mut sections: Vec<(&str, Vec<String>)>, picked: Vec<(usize, String)>, close: &str, budget: ContextBudget) -> String {
    let mut used = out.len() + close.len();
    for (slot, line) in picked {
        let title_cost = if sections[slot].1.is_empty() { sections[slot].0.len() + 1 } else { 0 };
        let cost = line.len() + 1 + title_cost;
        if used + cost > budget.max_chars {
            continue;
        }
        used += cost;
        sections[slot].1.push(line);
    }
    for (title, lines) in sections {
        if lines.is_empty() {
            continue;
        }
        out.push_str(title);
        out.push('\n');
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
    }
    out.push_str(close);
    out
}

impl MemoryGraph {
    /// Whose aspect node this is: `(name, is_the_root)`. It looks for the node it hangs
    /// from (an incoming `Composes` from the root or from a `Character`).
    pub fn appearance_owner(&self, id: NodeId) -> Option<(String, bool)> {
        let root = self.root();
        self.neighbors(id).filter(|(_, _, out)| !*out).find_map(|(src, e, _)| {
            let n = self.node(src)?;
            (e.kind == crate::model::EdgeKind::Composes && (src == root || n.kind == NodeKind::Character)).then(|| (n.label.clone(), src == root))
        })
    }

    /// Aspects hanging from a node (the root or a character), most salient first.
    pub fn appearance_of(&self, owner: NodeId) -> Vec<(NodeId, &crate::model::MemoryNode)> {
        let mut v: Vec<(NodeId, &crate::model::MemoryNode)> = self
            .neighbors(owner)
            .filter(|(_, e, out)| *out && e.kind == crate::model::EdgeKind::Composes)
            .filter_map(|(id, _, _)| self.node(id).filter(|n| n.kind == NodeKind::Appearance).map(|n| (id, n)))
            .collect();
        v.sort_by(|a, b| b.1.weight.total_cmp(&a.1.weight).then(a.0.cmp(&b.0)));
        v
    }

    /// Block for the curator: one line per node with its id, kind and weights.
    pub fn curator_block(&self, active: &[Activated], lang: Lang) -> String {
        self.curator_block_at(active, STORY_PRESENT, lang)
    }

    /// `curator_block` at a story time, followed by the links that hold then between the
    /// listed nodes (so the curator can close one that stopped being true).
    pub fn curator_block_at(&self, active: &[Activated], story: StoryTime, lang: Lang) -> String {
        let mut out = String::new();
        let root = self.root();
        let ids: Vec<NodeId> = std::iter::once(root).chain(active.iter().map(|a| a.id).filter(|&id| id != root)).collect();
        for &id in &ids {
            let Some(n) = self.node(id) else { continue };
            let line = tr!(
                lang,
                "curator.line",
                id = id,
                kind = n.kind.key(),
                label = n.label,
                content = n.content,
                weight = format!("{:.2}", n.weight),
                valence = format!("{:+.2}", n.valence)
            );
            out.push_str(&line);
            out.push('\n');
        }
        let mut links: Vec<_> = self
            .edges()
            .filter(|(s, d, e)| *s != root && *d != root && ids.contains(s) && ids.contains(d) && e.validity_at(story) == Validity::Active)
            .collect();
        if !links.is_empty() {
            links.sort_by(|a, b| b.2.weight.total_cmp(&a.2.weight).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
            out.push_str(i18n::t(lang, "curator.links"));
            out.push('\n');
            for (s, d, e) in links.into_iter().take(MAX_CURATOR_EDGES) {
                out.push_str(&tr!(lang, "curator.link", source = s, kind = e.kind.key(), target = d, weight = format!("{:.2}", e.weight)));
                out.push('\n');
            }
        }
        out
    }
}
