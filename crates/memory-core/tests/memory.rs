use std::collections::HashSet;
use std::path::PathBuf;

use memory_core::model::{ArchivedEdgeKind, ArchivedNodeKind};
use memory_core::*;

const DAY: i64 = 86_400;
const NOW: i64 = 1_790_000_000;

fn meta(name: &str) -> GraphMeta {
    let mut m = GraphMeta::new(Scope::Character, name, NOW);
    m.tagline = "elven librarian".into();
    m.scenario = "A library in ruins after a fire.".into();
    m
}

/// Fixture with the same ids as the example in the curator prompt (#0, #3, #7).
fn lyra() -> MemoryGraph {
    let mut g = MemoryGraph::new(meta("Lyra"), "Elven librarian, reserved and scholarly", NOW - 90 * DAY);
    let root = g.root();
    let t = NOW - 90 * DAY;
    let curious = g.add_trait(root, NodeKind::Trait, "Curious", "She loves old books", 0.8, t).unwrap(); // 1
    let melancholy = g.add_trait(root, NodeKind::Emotion, "Melancholy", "She misses her mother", 0.6, t).unwrap(); // 2
    let distrustful = g.add_trait(root, NodeKind::Trait, "Distrustful", "Finds it hard to trust strangers", 0.7, t).unwrap(); // 3
    let garden = g.add_node(MemoryNode::new(NodeKind::Place, "Garden", "The library's garden", t)).unwrap(); // 4
    let marta = g.add_node(MemoryNode::new(NodeKind::Entity, "Marta", "Lyra's former apprentice", t).with_weight(0.6)).unwrap(); // 5
    let fire = g
        .add_memory(
            MemoryNode::new(NodeKind::EpisodicMemory, "The fire", "The library burned and Lyra lost her mother's books", t)
                .with_weight(0.9)
                .with_valence(-0.9),
            &[],
            &[melancholy],
            t,
        )
        .unwrap(); // 6
    let user = g.add_node(MemoryNode::new(NodeKind::Entity, "User", "The person who visits the library", t).with_weight(0.8)).unwrap(); // 7
    assert_eq!((curious, distrustful, user), (1, 3, 7));
    let _ = g.add_memory(
        MemoryNode::new(NodeKind::EpisodicMemory, "Marta in the garden", "Marta and Lyra read poetry in the garden", t)
            .with_weight(0.6)
            .with_valence(0.6),
        &[marta, garden],
        &[curious],
        t,
    ); // 8
    g.connect(fire, distrustful, EdgeKind::Evokes, 0.6, t).unwrap();
    g
}

fn tmp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("memory-core-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

type Snapshot = (Vec<(NodeId, MemoryNode)>, HashSet<(NodeId, NodeId, u8, u32)>);

fn snapshot(g: &MemoryGraph) -> Snapshot {
    let nodes = g.nodes().map(|(id, n)| (id, n.clone())).collect();
    let edges = g.edges().map(|(a, b, e)| (a, b, e.kind as u8, e.weight.to_bits())).collect();
    (nodes, edges)
}

// ---------------------------------------------------------------- types and persistence

#[test]
fn kinds_take_one_byte_in_memory_and_on_disk() {
    assert_eq!(std::mem::size_of::<NodeKind>(), 1);
    assert_eq!(std::mem::size_of::<EdgeKind>(), 1);
    assert_eq!(std::mem::size_of::<ArchivedNodeKind>(), 1);
    assert_eq!(std::mem::size_of::<ArchivedEdgeKind>(), 1);
    for k in NodeKind::ALL {
        assert_eq!(NodeKind::from_u8(k as u8), Some(k));
    }
}

#[test]
fn roundtrip_keeps_ids_across_deleted_holes() {
    let mut g = lyra();
    g.remove_node(4).unwrap(); // a hole in the middle
    g.remove_node(2).unwrap();
    let path = tmp_path("roundtrip.mem");
    g.save(&path).unwrap();

    let loaded = MemoryGraph::load(&path).unwrap();
    assert_eq!(snapshot(&loaded), snapshot(&g));
    assert_eq!(loaded.root(), 0);
    assert!(!loaded.contains(4) && loaded.contains(5) && loaded.contains(8));
    assert_eq!(loaded.meta, g.meta);

    let (m, nodes, edges) = peek_meta(&path).unwrap();
    assert_eq!((m.name.as_str(), nodes, edges), ("Lyra", g.node_count(), g.edge_count()));
}

#[test]
fn corrupt_files_are_errors_not_panics() {
    let g = lyra();
    let bytes = g.to_bytes().unwrap();
    assert!(matches!(MemoryGraph::from_bytes(b"JSON{}"), Err(PersistError::BadMagic)));
    assert!(matches!(MemoryGraph::from_bytes(&bytes[..bytes.len() / 2]), Err(PersistError::Corrupt(_))));
    let mut wrong_version = bytes.clone();
    wrong_version[4] = 99;
    assert!(matches!(MemoryGraph::from_bytes(&wrong_version), Err(PersistError::UnsupportedVersion(99))));
    let mut flipped = bytes.clone();
    let n = flipped.len();
    for b in &mut flipped[n - 40..] {
        *b ^= 0xFF;
    }
    assert!(MemoryGraph::from_bytes(&flipped).is_err());
}

#[test]
fn binary_is_compact() {
    let g = lyra();
    let bytes = g.to_bytes().unwrap();
    let text: usize = g.nodes().map(|(_, n)| n.label.len() + n.content.len()).sum();
    // Structural overhead per node/edge well below an equivalent JSON (story time adds
    // 8 bytes per node and 16 per edge).
    assert!(bytes.len() < text + 88 * g.node_count() + 56 * g.edge_count(), "{} bytes", bytes.len());
}

// ---------------------------------------------------------------- recall

#[test]
fn spreading_reaches_memories_through_entities() {
    let g = lyra();
    let p = ActivationParams::default();
    let active = g.retrieve("Do you remember Marta?", NOW, &p);
    let ids: Vec<NodeId> = active.iter().map(|a| a.id).collect();
    assert!(ids.contains(&5), "the Marta seed: {ids:?}");
    assert!(ids.contains(&8), "memory reached by spreading: {ids:?}");
    let pos = |id| ids.iter().position(|&x| x == id).unwrap();
    // Marta's memory beats unrelated memories (if they show up).
    if let Some(i) = ids.iter().position(|&x| x == 6) {
        assert!(pos(8) < i);
    }
    // The personality never leaves the context.
    assert!(active.iter().any(|a| a.kind.is_personality()));
}

#[test]
fn old_memories_fade_unless_rehearsed() {
    let mut g = MemoryGraph::new(meta("Lyra"), "Librarian", NOW);
    let roses = g.add_node(MemoryNode::new(NodeKind::Topic, "Roses", "Flowers of the garden", NOW)).unwrap();
    let mk = |label: &str, t: i64| MemoryNode::new(NodeKind::EpisodicMemory, label, "They pruned the roses together", t).with_weight(0.5);
    let old = g.add_memory(mk("Old pruning", NOW - 60 * DAY), &[roses], &[], NOW).unwrap();
    let new = g.add_memory(mk("Recent pruning", NOW - DAY), &[roses], &[], NOW).unwrap();

    let p = ActivationParams::default();
    let score = |g: &MemoryGraph, id| g.retrieve("roses", NOW, &p).iter().find(|a| a.id == id).unwrap().score;
    assert!(score(&g, new) > score(&g, old));
    assert!(score(&g, old) > 0.0, "the recency floor prevents total forgetting");

    g.touch(&[old], NOW);
    assert!(score(&g, old) >= score(&g, new), "rehearsing a memory brings it back to the present");
}

#[test]
fn contradicts_inhibits_activation() {
    let mut g = MemoryGraph::new(meta("Lyra"), "x", NOW);
    let a = g.add_node(MemoryNode::new(NodeKind::SemanticFact, "Tomás lies", "Tomás made up the story", NOW)).unwrap();
    let b = g.add_node(MemoryNode::new(NodeKind::SemanticFact, "Tomás is honest", "Tomás always tells the truth", NOW)).unwrap();
    let c = g.add_node(MemoryNode::new(NodeKind::Entity, "Tomás", "A visitor", NOW)).unwrap();
    g.connect(c, a, EdgeKind::RelatedTo, 0.8, NOW).unwrap();
    g.connect(c, b, EdgeKind::RelatedTo, 0.8, NOW).unwrap();
    let p = ActivationParams::default();
    let before = g.spread(&[(c, 1.0)], &p)[&b];
    g.connect(a, b, EdgeKind::Contradicts, 1.0, NOW).unwrap();
    let after = g.spread(&[(c, 1.0)], &p)[&b];
    assert!(after < before, "{after} < {before}");
}

#[test]
fn actor_block_respects_budget_hides_ids_and_speaks_the_language() {
    let g = lyra();
    let active = g.retrieve("Marta garden fire", NOW, &ActivationParams::default());
    let block = g.actor_block(&active, NOW, ContextBudget::default(), Lang::En);
    assert!(block.starts_with("<active_memory>") && block.ends_with("</active_memory>"));
    assert!(block.contains("Marta in the garden") && block.contains("## Who you are"), "{block}");
    assert!(!(0..10).any(|d| block.contains(&format!("#{d}"))), "no internal ids");
    let tiny = g.actor_block(&active, NOW, ContextBudget { max_chars: 150 }, Lang::En);
    assert!(tiny.len() < block.len());
    assert!(g.curator_block(&active, Lang::En).starts_with("#0 [personality_root] Lyra"));
    assert!(g.curator_block(&active, Lang::En).contains("(weight 1.00, valence +0.00)"));

    let es = g.actor_block(&active, NOW, ContextBudget::default(), Lang::Es);
    assert!(es.contains("## Quién eres") && es.contains("[rasgo "), "{es}");
    assert!(g.curator_block(&active, Lang::Es).contains("(peso 1.00, valencia +0.00)"));
}

// ---------------------------------------------------------------- curator operations

/// The output of every example of the curator's prompt, in order.
fn prompt_examples(lang: Lang) -> Vec<OpBatch> {
    let prompt = memory_core::prompts::curator_system_prompt(lang, "Lyra", 0);
    let marker = format!("{}\n", i18n::t(lang, "curator.output"));
    prompt.split(marker.as_str()).skip(1).map(|json| OpBatch::parse_lenient(json).unwrap_or_else(|e| panic!("{e}: {json}"))).collect()
}

#[test]
fn prompt_example_applies_cleanly_in_every_language() {
    for lang in Lang::ALL {
        let mut g = lyra();
        let examples = prompt_examples(lang);
        assert_eq!(examples.len(), 2, "{lang}");
        let batch = &examples[0];
        assert_eq!(batch.ops.len(), 8, "{lang}");
        let r = g.apply_ops(batch, &ApplyLimits::default(), NOW);
        assert!(r.rejected.is_empty(), "{lang}: {:?}", r.rejected);
        assert_eq!(r.applied, 8);
        assert_eq!(g.node(7).unwrap().label, "Tomás");
        assert!((g.node(3).unwrap().weight - 0.62).abs() < 1e-5);
        let gratitude = r.created.iter().find(|(k, _)| k == "$c").unwrap().1;
        assert!(g.edge(g.root(), gratitude).is_some(), "new emotions hang from the root");
        let memory = r.created.iter().find(|(k, _)| k == "$b").unwrap().1;
        assert_eq!(g.edge(memory, 7).unwrap().kind, EdgeKind::Involves);

        // Example 2: Mira (#2) takes over Kael's (#1) watch (#3). The old fact is kept as it
        // was, its link closes at the turn's story time and a new fact contradicts it.
        let mut g = MemoryGraph::new(meta("Lyra"), "Elven librarian", NOW);
        let kael = g.add_node(MemoryNode::new(NodeKind::Character, "Kael", "Mercenary who saved Lyra", NOW)).unwrap();
        let mira = g.add_node(MemoryNode::new(NodeKind::Character, "Mira", "Lyra's apprentice", NOW)).unwrap();
        let old = "Kael guards the sealed archive at night";
        let watch = g.add_node(MemoryNode::new(NodeKind::SemanticFact, "Archive watch", old, NOW).with_weight(0.7)).unwrap();
        assert_eq!((kael, mira, watch), (1, 2, 3), "{lang}: the example refers to these ids");
        g.connect(watch, kael, EdgeKind::RelatedTo, 0.8, NOW).unwrap();
        let at = story_of_year(1093, 0);
        let r = g.apply_ops_at(&examples[1], &ApplyLimits::default(), &Roster::new(), NOW, at);
        assert!(r.rejected.is_empty() && r.applied == 4, "{lang}: {:?}", r.rejected);
        assert!(g.edge(watch, kael).is_none(), "{lang}: no open link is left");
        assert_eq!(g.edges_from_to(watch, kael).next().unwrap().valid_until, at);
        assert_eq!(g.node(watch).unwrap().content, old, "{lang}: the old fact is not rewritten");
        let new = r.created.iter().find(|(k, _)| k == "$a").unwrap().1;
        assert_eq!(g.node(new).unwrap().since, at);
        assert_eq!(g.edge(new, mira).unwrap().valid_from, at);
        assert_eq!(g.edge(new, watch).unwrap().kind, EdgeKind::Contradicts);

        // Recalling the watch, the new state outranks the old one it contradicts.
        let p = ActivationParams::default().at(at);
        let score = |id| g.retrieve("archive watch Mira Kael", NOW, &p).iter().find(|a| a.id == id).map_or(0.0, |a| a.score);
        assert!(score(new) > score(watch), "{lang}: new {} vs old {}", score(new), score(watch));
    }
}

#[test]
fn guardrails_on_curator_ops() {
    let mut g = lyra();
    let raw = r#"{"ops":[
        {"op":"create_node","ref":"$a","kind":"entity","label":"marta","content":"duplicate","weight":0.5,"valence":0},
        {"op":"update_node","target":1,"weight_delta":0.9},
        {"op":"update_node","target":0,"content":"Now I am someone else"},
        {"op":"create_node","ref":"$r","kind":"personality_root","label":"Other","content":"x","weight":1,"valence":0},
        {"op":"link","source":"$zz","target":1,"kind":"related_to","weight":0.5},
        {"op":"forget","target":3},
        {"op":"forget","target":6}
    ]}"#;
    let batch = OpBatch::parse_lenient(&format!("Sure, here you go:\n{raw}\n")).unwrap();
    let r = g.apply_ops(&batch, &ApplyLimits::default(), NOW);
    assert_eq!(r.created, vec![("$a".to_string(), 5)], "it merges with the existing Marta");
    assert!((g.node(1).unwrap().weight - 0.9).abs() < 1e-5, "personality changes by at most 0.1 per turn");
    assert_eq!(g.node(0).unwrap().content, "Elven librarian, reserved and scholarly");
    assert_eq!(r.rejected.len(), 3, "{:?}", r.rejected);
    assert!(g.contains(3) && (g.node(3).unwrap().weight - 0.6).abs() < 1e-5, "a trait fades, it is not deleted");
    assert!(!g.contains(6), "a memory can be forgotten");
}

#[test]
fn schema_and_grammar_assets_match_rust_types() {
    let schema: serde_json::Value = serde_json::from_str(memory_core::prompts::CURATOR_SCHEMA).unwrap();
    let kinds: Vec<String> = schema["$defs"]["nodeKind"]["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().into()).collect();
    let edges: Vec<String> = schema["$defs"]["edgeKind"]["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().into()).collect();
    let gbnf = memory_core::prompts::CURATOR_GBNF;
    for k in NodeKind::ALL.into_iter().filter(|k| !k.is_root()) {
        let name = k.key();
        assert!(kinds.iter().any(|x| x == name), "{name} in the schema");
        assert!(gbnf.contains(&format!("\"\\\"{name}\\\"\"")), "{name} in the grammar");
    }
    for k in EdgeKind::ALL {
        let name = k.key();
        assert!(edges.iter().any(|x| x == name), "{name} in the schema");
        assert!(gbnf.contains(&format!("\"\\\"{name}\\\"\"")), "{name} in the grammar");
    }
}

// ---------------------------------------------------------------- handler and view

#[test]
fn handler_recall_view_and_persist() {
    let path = tmp_path("handler.mem");
    let h = MemoryHandler::new(lyra(), &path, HandlerConfig::default());
    let recall = h.recall("Tell me about the fire");
    assert!(recall.context.contains("The fire"));
    assert!(h.is_dirty());

    let view = serde_json::to_value(h.view()).unwrap();
    let node6 = view["nodes"].as_array().unwrap().iter().find(|n| n["id"] == 6).unwrap();
    assert_eq!(node6["name"], "The fire");
    assert_eq!(node6["kind"], 5);
    assert!(node6["heat"].as_f64().unwrap() > 0.0);
    assert!(node6.get("content").is_none(), "the view does not carry the full text");
    assert!(view["links"][0]["source"].is_u64());

    assert!(h.save_if_dirty().unwrap());
    assert!(!h.save_if_dirty().unwrap());
    let reopened = MemoryHandler::open(&path, HandlerConfig::default()).unwrap();
    assert!(reopened.read(|g| g.node(6).unwrap().access_count) >= 1);
}

#[test]
fn handler_is_safe_under_concurrent_recall_and_writes() {
    let h = MemoryHandler::new(lyra(), tmp_path("concurrent.mem"), HandlerConfig::default());
    std::thread::scope(|s| {
        for i in 0..4 {
            let h = h.clone();
            s.spawn(move || {
                for j in 0..50 {
                    if (i + j) % 3 == 0 {
                        let label = format!("Fact {i}-{j}");
                        h.apply_ops(&OpBatch {
                            ops: vec![GraphOp::CreateNode {
                                reference: "$a".into(),
                                kind: NodeKind::SemanticFact,
                                label,
                                content: "Marta visited the garden".into(),
                                weight: 0.5,
                                valence: 0.0,
                            }],
                        });
                    } else {
                        let _ = h.recall("Marta garden");
                    }
                }
            });
        }
        let h = h.clone();
        s.spawn(move || {
            for _ in 0..10 {
                h.save().unwrap();
            }
        });
    });
    let reloaded = MemoryGraph::load(h.path()).unwrap();
    h.save().unwrap();
    assert!(reloaded.node_count() <= h.read(|g| g.node_count()));
}

/// Real output of Gemma 4 E4B (Q4_K_M, llama.cpp) with `memory_ops.gbnf` (a Spanish
/// conversation): it must keep parsing and applying if the types change.
#[test]
fn real_model_output_still_applies() {
    let raw = include_str!("fixtures/curator_gemma4_e4b.json");
    let batch = OpBatch::parse_lenient(raw).unwrap();
    let mut g = MemoryGraph::new(meta("Kael"), "Weathered mercenary", NOW);
    let root = g.root();
    g.add_trait(root, NodeKind::Trait, "Sarcastic", "Irony", 0.75, NOW).unwrap();
    g.add_trait(root, NodeKind::Emotion, "Guilt", "Darian's death", 0.6, NOW).unwrap();
    g.add_node(MemoryNode::new(NodeKind::Topic, "filler", "", NOW)).unwrap();
    g.add_node(MemoryNode::new(NodeKind::Entity, "User", "Traveller", NOW)).unwrap();
    g.add_node(MemoryNode::new(NodeKind::Entity, "Darian", "Kael's brother", NOW)).unwrap();
    let r = g.apply_ops(&batch, &ApplyLimits::default(), NOW);
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
    assert_eq!(r.applied, batch.ops.len());
}
