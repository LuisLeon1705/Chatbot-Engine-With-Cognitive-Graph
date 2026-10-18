//! Story time: links that stop holding (`valid_until`), nodes that exist from a point of
//! the story (`since`), world timelines and the moments each character belongs to.

use memory_core::persist::v2;
use memory_core::prompts::scene_actor_prompt;
use memory_core::*;

const NOW: i64 = 1_700_000_000;

fn year(y: i64) -> StoryTime {
    story_of_year(y, 0)
}

fn meta(name: &str) -> GraphMeta {
    GraphMeta::new(Scope::Character, name, NOW)
}

/// Lyra; Kael belongs to the Guild of Vors (the link closes in the year 1000).
fn lyra() -> (MemoryGraph, NodeId, NodeId) {
    let mut g = MemoryGraph::new(meta("Lyra"), "Elven librarian", NOW);
    let root = g.root();
    let kael = g.add_node(MemoryNode::new(NodeKind::Entity, "Kael", "A weathered mercenary", NOW).with_weight(0.8)).unwrap();
    let guild = g.add_node(MemoryNode::new(NodeKind::Faction, "Guild of Vors", "The mercenaries of the fortress", NOW).with_weight(0.7)).unwrap();
    g.connect(root, kael, EdgeKind::Knows, 0.8, NOW).unwrap();
    g.connect(kael, guild, EdgeKind::RelatedTo, 0.9, NOW).unwrap();
    (g, kael, guild)
}

fn ids(active: &[Activated]) -> Vec<NodeId> {
    active.iter().map(|a| a.id).collect()
}

#[test]
fn a_closed_link_stops_spreading_and_is_told_as_the_past() {
    let (mut g, kael, guild) = lyra();
    let p = ActivationParams::default();
    assert!(ids(&g.retrieve("Kael", NOW, &p)).contains(&guild), "while it holds, Kael brings up his guild");

    assert!(g.close_edge(kael, guild, year(1000)).unwrap());
    assert!(g.edge(kael, guild).is_none(), "no open link is left");
    assert_eq!(g.edges_from_to(kael, guild).count(), 1, "the link is kept, closed");

    // After it closed: it no longer spreads, and the actor reads it as the past.
    let later = p.at(year(1001));
    let active = g.retrieve("Kael", NOW, &later);
    assert!(!ids(&active).contains(&guild));
    let block = g.actor_block_at(&active, NOW, year(1001), ContextBudget::default(), Lang::En);
    assert!(block.contains("[PAST / NO LONGER VALID] Kael related Guild of Vors"), "{block}");
    let es = g.actor_block_at(&active, NOW, year(1001), ContextBudget::default(), Lang::Es);
    assert!(es.contains("[PASADO / YA NO VIGENTE] Kael relacionado Guild of Vors"), "{es}");

    // Before it closed, it still holds.
    let before = p.at(year(999));
    let active = g.retrieve("Kael", NOW, &before);
    assert!(ids(&active).contains(&guild));
    assert!(!g.actor_block_at(&active, NOW, year(999), ContextBudget::default(), Lang::En).contains("PAST"));

    // A new link between the same pair is a new open edge; the old one stays as history.
    g.connect_from(kael, guild, EdgeKind::Contradicts, 0.6, NOW, year(1002)).unwrap();
    assert_eq!(g.edges_from_to(kael, guild).count(), 2);
    assert_eq!(g.edge(kael, guild).unwrap().kind, EdgeKind::Contradicts);
    assert_eq!(g.close_edge(kael, guild, year(1001)), Err(GraphError::BadInterval), "it cannot end before it starts");
}

/// A chat stays at one moment: a link learned in one turn and undone in the next starts
/// and ends at the same story time. It must close (as the past), and coming back in that
/// same moment reopens it instead of piling up copies.
#[test]
fn a_link_can_close_in_the_moment_it_was_learned() {
    let (mut g, kael, guild) = lyra();
    let babel = year(1093);
    let raw = |op: &str| OpBatch::parse_lenient(&format!(r#"{{"ops":[{{"op":"{op}","source":{kael},"target":{guild}{}}}]}}"#, if op == "link" { r#","kind":"related_to","weight":0.8"# } else { "" })).unwrap();
    let apply = |g: &mut MemoryGraph, op: &str| g.apply_ops_at(&raw(op), &ApplyLimits::default(), &Roster::new(), NOW, babel);
    g.disconnect(kael, guild);

    // Turn 1: Kael joins the guild. Turn 2, same moment: he leaves it.
    assert_eq!(apply(&mut g, "link").applied, 1);
    let r = apply(&mut g, "close_edge");
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
    let e = *g.edges_from_to(kael, guild).next().unwrap();
    assert_eq!((e.valid_from, e.valid_until, e.validity_at(babel)), (babel, babel, Validity::Past));
    let block = g.actor_block_at(&g.retrieve("Kael", NOW, &ActivationParams::default().at(babel)), NOW, babel, ContextBudget::default(), Lang::En);
    assert!(block.contains("[PAST / NO LONGER VALID] Kael related Guild of Vors"), "{block}");

    // Turn 3, same moment: he comes back. The same link reopens; no second copy.
    assert_eq!(apply(&mut g, "link").applied, 1);
    assert_eq!(g.edges_from_to(kael, guild).count(), 1);
    assert_eq!(g.edge(kael, guild).unwrap().validity_at(babel), Validity::Active);
}

#[test]
fn nodes_from_later_in_the_story_are_not_recalled_earlier() {
    let (mut g, kael, _) = lyra();
    let siege = g
        .add_memory(MemoryNode::new(NodeKind::EpisodicMemory, "The siege", "Kael carried her out of the burning library", NOW).since(year(1098)), &[kael], &[], NOW)
        .unwrap();
    let p = ActivationParams::default();
    assert!(ids(&g.retrieve("the siege of the library", NOW, &p.at(year(1098)))).contains(&siege));
    assert!(!ids(&g.retrieve("the siege of the library", NOW, &p.at(year(1093)))).contains(&siege), "it has not happened yet");
    // Without a timeline (the present) everything is there.
    assert!(ids(&g.retrieve("the siege of the library", NOW, &p)).contains(&siege));
}

#[test]
fn curator_ops_are_dated_at_the_turn_and_close_edges() {
    let (mut g, kael, guild) = lyra();
    let raw = format!(
        r#"{{"ops":[
        {{"op":"create_node","ref":"$a","kind":"faction","label":"Free Company","content":"Kael's new band","weight":0.6,"valence":0.2}},
        {{"op":"close_edge","source":{kael},"target":{guild}}},
        {{"op":"link","source":{kael},"target":"$a","kind":"related_to","weight":0.8}},
        {{"op":"close_edge","source":{kael},"target":{guild}}}
        ]}}"#
    );
    let batch = OpBatch::parse_lenient(&raw).unwrap();
    let r = g.apply_ops_at(&batch, &ApplyLimits::default(), &Roster::new(), NOW, year(1094));
    assert_eq!(r.applied, 3, "{:?}", r.rejected);
    assert_eq!(r.rejected.len(), 1, "the second close finds nothing open");
    let company = r.created[0].1;
    assert_eq!(g.node(company).unwrap().since, year(1094));
    assert_eq!(g.edges_from_to(kael, guild).next().unwrap().valid_until, year(1094));
    assert_eq!(g.edge(kael, company).unwrap().valid_from, year(1094));

    // In a world without a timeline, what is learned is timeless.
    let r = g.apply_ops_at(
        &OpBatch::parse_lenient(r#"{"ops":[{"op":"create_node","ref":"$b","kind":"place","label":"Harbour","content":"Where the band waits","weight":0.5,"valence":0}]}"#).unwrap(),
        &ApplyLimits::default(),
        &Roster::new(),
        NOW,
        STORY_PRESENT,
    );
    assert_eq!(g.node(r.created[0].1).unwrap().since, STORY_ALWAYS);

    // The curator sees the links that hold, by id.
    let active = g.retrieve("Kael Free Company", NOW, &ActivationParams::default().at(year(1094)));
    let block = g.curator_block_at(&active, year(1094), Lang::En);
    assert!(block.contains(&format!("#{kael} -related_to-> #{company}")), "{block}");
    assert!(!block.contains(&format!("-> #{guild}")), "a closed link is not offered:\n{block}");
}

#[test]
fn version_2_memory_files_load_as_timeless() {
    let old = v2::GraphFile {
        meta: v2::GraphMeta {
            id: 7,
            scope: Scope::Character,
            name: "Old".into(),
            tagline: String::new(),
            avatar: None,
            scenario: "At the library".into(),
            appearance: String::new(),
            created_at: 1,
            updated_at: 2,
        },
        root: 0,
        node_bound: 2,
        nodes: vec![
            v2::NodeRecord { id: 0, node: v2_node(NodeKind::PersonalityRoot, "Old") },
            v2::NodeRecord { id: 1, node: v2_node(NodeKind::Entity, "Kael") },
        ],
        edges: vec![v2::EdgeRecord { source: 0, target: 1, edge: v2::MemoryEdge { kind: EdgeKind::Knows, weight: 0.7, created_at: 1, last_reinforced: 1 } }],
        externals: Vec::new(),
    };
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&old).unwrap();
    let mut bytes = persist::MAGIC.to_vec();
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&payload);

    let g = MemoryGraph::from_bytes(&bytes).unwrap();
    assert_eq!((g.meta.name.as_str(), g.meta.scenario.as_str(), g.node_count()), ("Old", "At the library", 2));
    assert_eq!(g.node(1).unwrap().since, STORY_ALWAYS);
    let e = g.edge(0, 1).unwrap();
    assert!(e.is_open() && e.valid_from == STORY_ALWAYS);
    assert!(g.meta.timeline.is_empty() && g.meta.moments.is_empty());

    let path = std::env::temp_dir().join(format!("memory-core-v2-{}.mem", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(peek_meta(&path).unwrap().0.name, "Old");
    // Saving it writes the current version, which reads back the same.
    g.save(&path).unwrap();
    let back = MemoryGraph::load(&path).unwrap();
    assert_eq!(back.edge(0, 1), g.edge(0, 1));
}

fn v2_node(kind: NodeKind, label: &str) -> v2::MemoryNode {
    v2::MemoryNode { kind, label: label.into(), content: String::new(), weight: 0.6, valence: 0.0, created_at: 1, last_accessed: 1, access_count: 0 }
}

#[test]
fn story_times_cross_ipc_as_words_or_days() {
    for (t, json) in [(STORY_ALWAYS, "\"always\""), (STORY_OPEN, "\"open\""), (STORY_PRESENT, "\"present\""), (year(1098), "400770")] {
        let s: Story = t.into();
        assert_eq!(serde_json::to_string(&s).unwrap(), json);
        assert_eq!(StoryTime::from(serde_json::from_str::<Story>(json).unwrap()), t);
    }
    assert_eq!(context::story_label(Lang::En, year(1098) + 40, &[]), "Year 1098, day 40");
    assert_eq!(context::story_label(Lang::Es, STORY_ALWAYS, &[]), "Siempre");
}

/// Terra in miniature: Theresa only exists at Babel (1093); her death (1094) is not known
/// there, and at Londinium (1098) Kal'tsit's bond with Theresa is the past.
#[test]
fn world_timeline_sets_what_a_scene_can_know() {
    let dir = std::env::temp_dir().join(format!("memory-core-timeline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::new(&dir, HandlerConfig::with_lang(Lang::En));
    let w = lib.create_world("Tests", "Terra", "A world of nomadic cities.", "").unwrap();
    let moments = w
        .set_timeline(vec![
            Moment { id: 0, name: "Londinium".into(), at: year(1098), summary: "After the crisis of Londinium".into() },
            Moment { id: 0, name: "Babel".into(), at: year(1093), summary: "Babel fights the Kazdel civil war".into() },
        ])
        .unwrap();
    assert_eq!(moments.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Babel", "Londinium"], "sorted by story time");
    let (babel, londinium) = (moments[0].id, moments[1].id);
    assert_eq!(w.story_now(None), year(1098), "without a moment, chats are at the latest one");
    assert_eq!(w.story_now(Some(babel)), year(1093));

    w.memory.write(|g| {
        let root = g.root();
        let death = g.add_node(MemoryNode::new(NodeKind::Event, "Death of Theresa", "Theresa died in the fall of Babel", NOW).since(year(1094))).unwrap();
        g.connect(root, death, EdgeKind::Knows, 0.9, NOW).unwrap();
    });
    let kal = w.create_character("Kal'tsit", "Head of the medical department", "Doctor of Rhodes Island", "").unwrap().id();
    let theresa = w.create_character("Theresa", "Queen of the Sarkaz", "Founder of Babel", "").unwrap().id();
    w.set_character_moments(theresa, vec![CharacterMoment { moment: babel, scenario: "On the bridge of Babel".into() }]).unwrap();
    assert!(w.set_character_moments(theresa, vec![CharacterMoment { moment: 99, scenario: String::new() }]).is_err());
    w.relate(kal, theresa, &Perception { content: "Her closest friend".into(), affection: 0.9, closeness: 0.9 }, &Perception { content: "Her wisest ally".into(), affection: 0.8, closeness: 0.8 })
        .unwrap();
    w.character(kal).unwrap().write(|g| {
        let root = g.root();
        let t = g.find_by_label("Theresa", Some(NodeKind::Character)).unwrap();
        g.close_edge(root, t, year(1094)).unwrap();
    });

    let tm = w.character(theresa).unwrap().meta();
    assert!(tm.fits(Some(babel)) && !tm.fits(Some(londinium)) && tm.fits(None));
    assert_eq!(tm.scenario_at(Some(babel)), "On the bridge of Babel");
    assert!(w.character(kal).unwrap().meta().fits(Some(babel)), "no moments = any moment");

    let q = "What happened to Theresa?";
    let at_babel = w.scene_recall_at(kal, q, &[kal], None, year(1093)).unwrap().context;
    assert!(!at_babel.contains("Death of Theresa") && !at_babel.contains("PAST"), "{at_babel}");
    let at_londinium = w.scene_recall_at(kal, q, &[kal], None, year(1098)).unwrap().context;
    assert!(at_londinium.contains("Death of Theresa"), "{at_londinium}");
    assert!(at_londinium.contains("[PAST / NO LONGER VALID] Kal'tsit knows Theresa"), "{at_londinium}");

    let world_meta = w.memory.meta();
    let babel_moment = w.moment(babel).unwrap();
    let prompt = scene_actor_prompt(Lang::En, &tm, &world_meta, "", &[], "Doctor", Some(&babel_moment));
    assert!(prompt.contains("## Moment of the story\nBabel: Babel fights the Kazdel civil war"), "{prompt}");
    assert!(prompt.contains("On the bridge of Babel"), "{prompt}");

    // Deleting a moment takes it out of the characters too; the world reopens with it all.
    w.set_timeline(vec![babel_moment.clone()]).unwrap();
    assert!(w.character(theresa).unwrap().meta().fits(Some(babel)));
    let reopened = lib.open_world("tests", "terra").unwrap();
    assert_eq!(reopened.timeline(), vec![babel_moment]);
    assert_eq!(reopened.character(theresa).unwrap().meta().moments.len(), 1);
}

#[test]
fn structural_memory_edges_cannot_be_closed() {
    assert!(EdgeKind::Involves.is_immutable_on_memory());
    assert!(EdgeKind::Evokes.is_immutable_on_memory());
    assert!(!EdgeKind::RelatedTo.is_immutable_on_memory());
    assert!(!EdgeKind::Knows.is_immutable_on_memory());

    let (mut g, kael, _) = lyra();
    let root = g.root();
    let fear = g.add_trait(root, NodeKind::Emotion, "Fear", "Smoke and fire", 0.7, NOW).unwrap();
    let siege = g
        .add_memory(
            MemoryNode::new(NodeKind::EpisodicMemory, "The siege", "Kael carried her out of the burning library", NOW).since(year(1093)),
            &[kael],
            &[fear],
            NOW,
        )
        .unwrap();

    assert_eq!(g.close_edge(siege, kael, year(1098)), Err(GraphError::ImmutableMemoryEdge));
    assert_eq!(g.close_edge(kael, siege, year(1098)), Err(GraphError::ImmutableMemoryEdge));
    assert_eq!(g.close_edge(siege, fear, year(1098)), Err(GraphError::ImmutableMemoryEdge));
    assert!(g.edge(siege, kael).is_some() && g.edge(siege, fear).is_some(), "graph is not mutated");

    let batch = OpBatch::parse_lenient(&format!(
        r#"{{"ops":[
            {{"op":"close_edge","source":{siege},"target":{kael}}},
            {{"op":"close_edge","source":{siege},"target":{fear}}}
        ]}}"#
    ))
    .unwrap();
    let r = g.apply_ops_at(&batch, &ApplyLimits::default(), &Roster::new(), NOW, year(1098));
    assert_eq!(r.applied, 0);
    assert_eq!(r.rejected.len(), 2);
    for msg in &r.rejected {
        assert!(msg.contains("close_edge ignored: structural memory edge (Involves/Evokes) cannot expire"), "{msg}");
    }
    assert!(g.edge(siege, kael).is_some() && g.edge(siege, fear).is_some());
}

