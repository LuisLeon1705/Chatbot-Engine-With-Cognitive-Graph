//! Collabs: several worlds meeting in a bridge memory, always isolated from them.

use memory_core::collab::CollabLink;
use memory_core::conversation::Conversation;
use memory_core::world::DefinitionLevel;
use memory_core::*;

fn year(y: i64) -> StoryTime {
    story_of_year(y, 0)
}

struct Fixture {
    lib: Library,
    terra: World,
    talos: World,
    kal: GraphId,
    perlica: GraphId,
    babel: u32,
}

/// Terra (Kal'tsit, a timeline with Babel and Londinium) and Talos-II (Perlica, whose lore
/// knows Kal'tsit as a legend), both in the Arknights category.
fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("memory-core-collab-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::new(dir, HandlerConfig::with_lang(Lang::En));
    let terra = lib.create_world("Arknights", "Terra", "Nomadic cities.", "").unwrap();
    let talos = lib.create_world("Arknights", "Talos II", "A frontier moon.", "").unwrap();
    let moments = terra
        .set_timeline(vec![
            Moment { id: 0, name: "Babel".into(), at: year(1093), summary: String::new() },
            Moment { id: 0, name: "Londinium".into(), at: year(1098), summary: String::new() },
        ])
        .unwrap();
    terra.memory.write(|g| {
        let root = g.root();
        let death = g.add_node(MemoryNode::new(NodeKind::Event, "Death of Theresa", "Theresa died in the fall of Babel", 0).since(year(1094))).unwrap();
        g.connect(root, death, EdgeKind::Knows, 0.9, 0).unwrap();
    });
    talos.memory.write(|g| {
        let root = g.root();
        let legend = g.add_node(MemoryNode::new(NodeKind::Entity, "Kal'tsit", "The ageless doctor of the old Rhodes Island, a legend of the colony's founding", 0)).unwrap();
        g.connect(root, legend, EdgeKind::Knows, 0.6, 0).unwrap();
    });
    let kal = terra.create_character("Kal'tsit", "Head of the medical department", "Doctor of Rhodes Island", "").unwrap().id();
    let perlica = talos.create_character("Perlica", "Supervisor of Endfield", "Endfield's supervisor", "").unwrap().id();
    Fixture { lib, terra, talos, kal, perlica, babel: moments[0].id }
}

#[test]
fn people_from_other_worlds_are_known_only_through_what_each_side_knows() {
    let f = fixture("know");
    let collab = f
        .lib
        .create_collab("The Rift", "A rift opens between Terra and Talos-II.", vec![
            CollabLink { world: "arknights/terra".into(), moment: None },
            CollabLink { world: "arknights/talos-ii".into(), moment: None },
        ])
        .unwrap();
    let worlds = [("arknights/terra", &f.terra), ("arknights/talos-ii", &f.talos)];
    let present = [f.kal, f.perlica];

    // Talos-II's lore knows Kal'tsit: to Perlica she is a legend.
    let p = collab.scene_recall(&worlds, f.perlica, "Who are you?", &present, None).unwrap().context;
    assert!(p.contains("Kal'tsit: you have never met them, but in your world they are known: The ageless doctor"), "{p}");
    // Terra knows nothing of Talos-II: to Kal'tsit, Perlica is a stranger.
    let k = collab.scene_recall(&worlds, f.kal, "Who are you?", &present, None).unwrap().context;
    assert!(k.contains("Perlica: a stranger from another world"), "{k}");
    assert!(!k.contains("Endfield's supervisor"), "no rumours across worlds:\n{k}");

    // Once the Collab's memory records their meeting, that is what is known here.
    collab.memory.write(|g| {
        let root = g.root();
        let n = g.add_node(MemoryNode::new(NodeKind::Character, "Perlica", "Came through the rift with a team from Endfield", 0)).unwrap();
        g.set_external(n, Some(ExternalRef { scope: Scope::Character, id: f.perlica })).unwrap();
        g.connect(root, n, EdgeKind::Knows, 0.6, 0).unwrap();
    });
    let k = collab.scene_recall(&worlds, f.kal, "Who are you?", &present, None).unwrap().context;
    assert!(k.contains("Perlica: from another world. What is known of them here: Came through the rift"), "{k}");
}

#[test]
fn a_collab_chat_is_isolated_and_each_speaker_keeps_their_world_and_moment() {
    let f = fixture("chat");
    let collab = f
        .lib
        .create_collab("The Rift", "A rift opens between Terra and Talos-II.", vec![
            CollabLink { world: "arknights/terra".into(), moment: Some(f.babel) },
            CollabLink { world: "arknights/talos-ii".into(), moment: None },
        ])
        .unwrap();
    collab.memory.write(|g| {
        let root = g.root();
        let rule = g.add_node(MemoryNode::new(NodeKind::Lore, "The rift", "A tear of black crystal that joins both worlds for one night", 0)).unwrap();
        g.connect(root, rule, EdgeKind::Composes, 0.9, 0).unwrap();
    });
    let arknights = f.terra.definition_library(DefinitionLevel::Category, true).unwrap().unwrap();
    arknights.write(|g| {
        let id = g.add_node(MemoryNode::new(NodeKind::Concept, "Originium", "A black crystal that stores energy", 0)).unwrap();
        let root = g.root();
        g.connect(root, id, EdgeKind::Knows, 0.7, 0).unwrap();
    });
    let worlds = [("arknights/terra", &f.terra), ("arknights/talos-ii", &f.talos)];
    let conv = Conversation::new("Night at the rift", vec![f.kal, f.perlica], "User");
    conv.save(&collab.dir).unwrap();
    let chat = collab.chat_memory(&conv, &worlds).unwrap();
    assert_eq!(chat.read(|g| g.nodes().filter(|(_, n)| n.kind == NodeKind::Character).count()), 2, "both worlds' people in the chat");

    // Kal'tsit speaks at Babel (the moment the Collab sets for Terra).
    let k = collab.scene_recall(&worlds, f.kal, "What happened to Theresa? And what is that rift?", &[f.kal, f.perlica], Some(&chat)).unwrap().context;
    assert!(!k.contains("Death of Theresa"), "at Babel it has not happened:\n{k}");
    assert!(k.contains("<collab>") && k.contains("The rift: A tear of black crystal"), "{k}");
    // The libraries of every linked world are there (once).
    let p = collab.scene_recall(&worlds, f.perlica, "Is that Originium?", &[f.kal, f.perlica], Some(&chat)).unwrap().context;
    assert_eq!(p.matches("Originium: A black crystal").count(), 1, "{p}");

    // What Kal'tsit learns here goes to her copy in the Collab, never to Terra.
    let copy = collab.speaker_copy(conv.id, f.kal, &worlds).unwrap();
    assert!(copy.path().starts_with(&collab.dir));
    copy.write(|g| g.add_node(MemoryNode::new(NodeKind::EpisodicMemory, "Met Perlica", "A woman from another world", 0)).unwrap());
    let base = f.terra.character(f.kal).unwrap();
    assert!(base.read(|g| g.find_by_label("Met Perlica", None).is_none()), "Terra's Kal'tsit is untouched");

    collab.delete_chat_memory(conv.id).unwrap();
    assert!(!copy.path().exists() && !chat.path().exists());
}

#[test]
fn links_persist_and_collabs_are_not_a_category() {
    let f = fixture("links");
    let c = f.lib.create_collab("The Rift", "", vec![CollabLink { world: "arknights/terra".into(), moment: None }]).unwrap();
    c.set_links(vec![
        CollabLink { world: "arknights/terra".into(), moment: Some(f.babel) },
        CollabLink { world: "arknights/terra".into(), moment: None },
        CollabLink { world: "arknights/talos-ii".into(), moment: None },
    ])
    .unwrap();
    let again = f.lib.open_collab("the-rift").unwrap();
    assert_eq!(again.links().len(), 2, "duplicates dropped");
    assert_eq!(again.moment_of("arknights/terra"), Some(f.babel));
    assert_eq!(f.lib.collabs().iter().map(|(s, m)| (s.as_str(), m.name.as_str())).collect::<Vec<_>>(), [("the-rift", "The Rift")]);
    assert_eq!(f.lib.categories().unwrap(), ["arknights"]);
    assert_eq!(f.lib.worlds().unwrap().items.len(), 2);
}
