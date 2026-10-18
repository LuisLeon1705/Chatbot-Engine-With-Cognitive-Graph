//! Stress tests of the memory engine: scale, recall precision under noise, invariants
//! under random curator operations, and concurrency.
//!
//! The time limits are loose because `cargo test` builds without optimisations; the
//! real figures come from `chats stress` (release).

use std::collections::HashSet;
use std::time::Instant;

use memory_core::synth::{Rng, SynthSpec, synth_character};
use memory_core::world::Aspect;
use memory_core::*;

const NOW: i64 = 1_790_000_000;

fn ids(active: &[Activated]) -> Vec<NodeId> {
    active.iter().map(|a| a.id).collect()
}

/// Checks that the graph is consistent: intact root, values in range, external links to
/// live nodes and edges between live nodes.
fn assert_invariants(g: &MemoryGraph) {
    let root = g.node(g.root()).expect("the root exists");
    assert!(root.kind.is_root());
    for (id, n) in g.nodes() {
        assert!(n.weight.is_finite() && (0.0..=1.0).contains(&n.weight), "#{id} weight {}", n.weight);
        assert!(n.valence.is_finite() && (-1.0..=1.0).contains(&n.valence), "#{id} valence {}", n.valence);
        if id != g.root() {
            assert!(!n.kind.is_root(), "#{id} is a second root");
        }
    }
    for (a, b, e) in g.edges() {
        assert!(g.contains(a) && g.contains(b));
        assert!(e.weight.is_finite() && (0.0..=1.0).contains(&e.weight));
    }
    for (id, _) in g.externals() {
        assert!(g.contains(id), "orphan external link of #{id}");
    }
}

#[test]
fn needle_in_a_haystack_of_5000_nodes() {
    let spec = SynthSpec { nodes: 5000, seed: 7, now: NOW, ..Default::default() };
    let mut s = synth_character(&spec);
    let g = &mut s.graph;
    // The needle: an old, unimportant memory with unique terms.
    let (owner, owner_name) = s.entities[s.entities.len() - 1].clone();
    let mut needle = MemoryNode::new(
        NodeKind::EpisodicMemory,
        "The obsidian reliquary",
        format!("{owner_name} hid an obsidian reliquary under the third tile of the lighthouse."),
        NOW - 2000 * 86_400,
    )
    .with_weight(0.3);
    needle.last_accessed = NOW - 2000 * 86_400;
    let needle_id = g.add_memory(needle, &[owner], &[], NOW).unwrap();
    assert!(g.node_count() > 5000);

    let p = ActivationParams::default();
    let started = Instant::now();
    let hit = g.retrieve("Where did the obsidian reliquary end up?", NOW, &p);
    let elapsed = started.elapsed();
    assert_eq!(hit.first().map(|a| a.id), Some(needle_id), "the needle must come first: {:?}", &ids(&hit)[..5.min(hit.len())]);
    assert!(elapsed.as_millis() < 2000, "recall took {elapsed:?} in debug");

    // Through the owning entity (no words of the needle): the needle must still show up.
    let by_owner = g.retrieve(&format!("Tell me about {owner_name}"), NOW, &p);
    assert!(ids(&by_owner).contains(&needle_id), "spreading from the entity to the needle");

    // The personality core always gets a slot, however big the haystack.
    assert!(hit.iter().filter(|a| a.kind.is_personality()).count() >= 1);
    assert!(hit.len() <= p.top_n);
}

#[test]
fn hubs_do_not_flood_the_context() {
    let spec = SynthSpec { nodes: 3000, seed: 11, now: NOW, ..Default::default() };
    let s = synth_character(&spec);
    let g = &s.graph;
    // The most connected entity (Zipf) is a hub: asking about it must not return only its
    // filler memories; the core and some variety must survive.
    let (hub, hub_name) = &s.entities[0];
    assert!(g.degree(*hub) > 100, "the generator produces hubs ({} edges)", g.degree(*hub));
    let hit = g.retrieve(hub_name, NOW, &ActivationParams::default());
    let kinds: HashSet<NodeKind> = hit.iter().map(|a| a.kind).collect();
    assert!(kinds.len() >= 2, "variety of kinds: {kinds:?}");
    let block = g.actor_block(&hit, NOW, ContextBudget::default(), Lang::En);
    assert!(block.len() <= ContextBudget::default().max_chars + 64);
}

#[test]
fn twenty_thousand_nodes_roundtrip_and_stay_compact() {
    let spec = SynthSpec { nodes: 20_000, seed: 3, now: NOW, ..Default::default() };
    let s = synth_character(&spec);
    let g = &s.graph;
    let started = Instant::now();
    let bytes = g.to_bytes().unwrap();
    let ser = started.elapsed();
    let started = Instant::now();
    let back = MemoryGraph::from_bytes(&bytes).unwrap();
    let de = started.elapsed();
    assert_eq!(back.node_count(), g.node_count());
    assert_eq!(back.edge_count(), g.edge_count());
    for (id, n) in g.nodes().step_by(97) {
        assert_eq!(back.node(id), Some(n));
    }
    let text: usize = g.nodes().map(|(_, n)| n.label.len() + n.content.len()).sum();
    let overhead = (bytes.len() - text) as f32 / (g.node_count() + g.edge_count()) as f32;
    assert!(overhead < 64.0, "{overhead:.1} bytes of structure per element");
    assert!(ser.as_secs_f32() < 5.0 && de.as_secs_f32() < 5.0, "ser {ser:?} / de {de:?}");
    assert_invariants(&back);
}

/// Random operations (valid, invalid and extreme) against a graph: nothing may break the
/// invariants or saving.
#[test]
fn curator_fuzz_keeps_invariants() {
    let spec = SynthSpec { nodes: 600, seed: 99, now: NOW, ..Default::default() };
    let mut g = synth_character(&spec).graph;
    let mut rng = Rng::new(1234);
    let kinds: Vec<NodeKind> = NodeKind::ALL.to_vec();
    let edges: Vec<EdgeKind> = EdgeKind::ALL.to_vec();
    let weird = [f32::NAN, f32::INFINITY, -f32::INFINITY, -5.0, 5.0, 0.0, 1.0, 0.5];
    let limits = ApplyLimits::default();
    let mut applied = 0;
    let mut rejected = 0;
    for round in 0..400 {
        let live: Vec<NodeId> = g.nodes().map(|(id, _)| id).collect();
        let node_ref = |rng: &mut Rng| -> NodeRef {
            match rng.below(10) {
                0 => NodeRef::New(format!("$x{}", rng.below(4))),
                1 => NodeRef::Id(900_000 + rng.below(10) as u32),
                2 => NodeRef::Id(g.root()),
                _ => NodeRef::Id(*rng.pick(&live)),
            }
        };
        let mut ops = Vec::new();
        for _ in 0..rng.below(20) {
            let op = match rng.below(5) {
                0 => GraphOp::CreateNode {
                    reference: format!("$x{}", rng.below(4)),
                    kind: *rng.pick(&kinds),
                    label: if rng.below(8) == 0 { String::new() } else { format!("Node {}", rng.below(50)) },
                    content: "test content".into(),
                    weight: *rng.pick(&weird),
                    valence: *rng.pick(&weird),
                },
                1 => GraphOp::UpdateNode {
                    target: node_ref(&mut rng),
                    label: (rng.below(3) == 0).then(|| format!("Renamed {}", rng.below(9))),
                    content: (rng.below(2) == 0).then(|| "new content".into()),
                    weight_delta: Some(*rng.pick(&weird)),
                    valence_delta: Some(*rng.pick(&weird)),
                },
                2 => GraphOp::Link { source: node_ref(&mut rng), target: node_ref(&mut rng), kind: *rng.pick(&edges), weight: *rng.pick(&weird) },
                3 => GraphOp::Reinforce { source: node_ref(&mut rng), target: node_ref(&mut rng), delta: *rng.pick(&weird) },
                _ => GraphOp::Forget { target: node_ref(&mut rng) },
            };
            ops.push(op);
        }
        let report = g.apply_ops(&OpBatch { ops }, &limits, NOW + round);
        applied += report.applied;
        rejected += report.rejected.len();
        if round % 50 == 0 {
            assert_invariants(&g);
            let back = MemoryGraph::from_bytes(&g.to_bytes().unwrap()).unwrap();
            assert_eq!(back.node_count(), g.node_count());
            assert_invariants(&back);
        }
    }
    assert_invariants(&g);
    assert!(applied > 500 && rejected > 100, "the fuzz exercises both paths ({applied} applied, {rejected} rejected)");
}

/// Several characters in a world, with threads that do scene recalls, apply operations,
/// change visual perceptions and save at the same time.
#[test]
fn concurrent_world_activity_is_consistent() {
    let dir = std::env::temp_dir().join(format!("memory-core-stress-world-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::new(&dir, HandlerConfig::default());
    let world = lib.create_world("Tests", "City", "A nomadic city in trouble.", "").unwrap();
    let mut ids = Vec::new();
    for i in 0..6 {
        let h = world.create_character(&format!("Person{i}"), "Inhabitant of the city", "Well-known neighbour", "").unwrap();
        ids.push(h.id());
    }
    for (i, &a) in ids.iter().enumerate() {
        let b = ids[(i + 1) % ids.len()];
        world.relate(a, b, &Perception { content: "Neighbour".into(), affection: 0.3, closeness: 0.5 }, &Perception { content: "Neighbour".into(), affection: 0.1, closeness: 0.4 }).unwrap();
    }
    std::thread::scope(|s| {
        for t in 0..ids.len() {
            let (world, ids) = (&world, &ids);
            s.spawn(move || {
                let me = ids[t];
                let other = ids[(t + 2) % ids.len()];
                for j in 0..40 {
                    match j % 4 {
                        0 => {
                            let _ = world.scene_recall(me, "What happened at the market?", &ids[..3]).unwrap();
                        }
                        1 => {
                            let h = world.character(me).unwrap();
                            h.apply_ops(&OpBatch {
                                ops: vec![GraphOp::CreateNode {
                                    reference: "$a".into(),
                                    kind: NodeKind::EpisodicMemory,
                                    label: format!("Event {t}-{j}"),
                                    content: "Something happened at the market".into(),
                                    weight: 0.5,
                                    valence: 0.0,
                                }],
                            });
                        }
                        2 => {
                            let aspect = Aspect { label: "Coat".into(), content: format!("grey coat number {j}"), weight: 0.5 };
                            world.set_visual_perception(me, other, &aspect).unwrap();
                        }
                        _ => world.save_all().unwrap(),
                    }
                }
            });
        }
    });
    world.save_all().unwrap();
    let reopened = lib.open_world("tests", "city").unwrap();
    for &id in &ids {
        let (a, b) = (world.character(id).unwrap(), reopened.character(id).unwrap());
        assert_eq!(a.read(|g| g.node_count()), b.read(|g| g.node_count()));
        b.read(assert_invariants);
        // Each one saved its 10 events.
        assert_eq!(b.read(|g| g.nodes().filter(|(_, n)| n.label.starts_with("Event")).count()), 10);
    }
    // The visual perception was updated in place (a single «Coat» aspect per person seen).
    let h = reopened.character(ids[0]).unwrap();
    assert_eq!(h.read(|g| g.nodes().filter(|(_, n)| n.kind == NodeKind::Appearance).count()), 1);
}

#[test]
fn appearance_canon_and_perceptions_are_separate() {
    let dir = std::env::temp_dir().join(format!("memory-core-stress-look-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::new(&dir, HandlerConfig::default());
    let world = lib.create_world("Tests", "Tower", "A tower.", "").unwrap();
    let a = world.create_character("Ana", "Doctor", "The tower's doctor", "").unwrap().id();
    let b = world.create_character("Bruno", "Guard", "The guard", "").unwrap().id();
    let canon = [
        Aspect { label: "Hair".into(), content: "short silver hair".into(), weight: 0.9 },
        Aspect { label: "Coat".into(), content: "long white lab coat".into(), weight: 0.8 },
    ];
    world.set_appearance(a, &canon, true).unwrap();
    world.set_visual_perception(b, a, &Aspect { label: "Hair".into(), content: "always dishevelled".into(), weight: 0.6 }).unwrap();

    let prompt = world.appearance_prompt(a).unwrap();
    assert_eq!(prompt, "short silver hair, long white lab coat", "the prompt only uses the canon, by importance");
    // The occasional (importance < 0.5) only gets into the portrait if the scene names it.
    world.set_appearance(a, &[Aspect { label: "Cat".into(), content: "a black cat on her shoulder".into(), weight: 0.3 }], false).unwrap();
    assert_eq!(world.appearance_prompt(a).unwrap(), "short silver hair, long white lab coat");
    assert!(world.appearance_prompt_for(a, "reading with her cat").unwrap().ends_with("a black cat on her shoulder"));
    world.set_appearance(a, &canon, true).unwrap();
    let report = world.appearance_report(a).unwrap();
    assert_eq!(report.canonical.len(), 2);
    assert_eq!(report.seen_by, vec![("Bruno".to_string(), vec![Aspect { label: "Hair".into(), content: "always dishevelled".into(), weight: 0.6 }])]);

    // Ana knows her own appearance; Bruno remembers how he sees her when asked about her.
    let ana = world.scene_recall(a, "How is your hair today?", &[a]).unwrap().context;
    assert!(ana.contains("You · Hair: short silver hair"), "{ana}");
    let bruno = world.scene_recall(b, "What does Ana look like? Look at her hair", &[b, a]).unwrap().context;
    assert!(bruno.contains("Ana · Hair: always dishevelled"), "{bruno}");
    assert!(!bruno.contains("You · Hair"), "Bruno has no appearance of his own");

    // Replacing the canon does not touch other people's perceptions; extending does not duplicate.
    world.set_appearance(a, &[Aspect { label: "Hair".into(), content: "long black hair".into(), weight: 0.9 }], false).unwrap();
    let r = world.appearance_report(a).unwrap();
    assert_eq!(r.canonical.iter().filter(|x| x.label == "Hair").count(), 1);
    assert_eq!(r.canonical.len(), 2);
    assert_eq!(r.seen_by[0].1[0].content, "always dishevelled");
}
