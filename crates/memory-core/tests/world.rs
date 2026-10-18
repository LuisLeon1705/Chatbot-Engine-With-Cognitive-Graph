use memory_core::world::{Aspect, slugify};
use memory_core::*;

fn library_in(name: &str, lang: Lang) -> Library {
    let dir = std::env::temp_dir().join(format!("memory-core-world-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::new(dir, HandlerConfig::with_lang(lang))
}

fn library(name: &str) -> Library {
    library_in(name, Lang::En)
}

fn p(content: &str, affection: f32, closeness: f32) -> Perception {
    Perception { content: content.into(), affection, closeness }
}

/// Test world: Lyra and Kael know each other (with different opinions); Mira does not know Kael.
fn aerendal(lib: &Library) -> (World, GraphId, GraphId, GraphId) {
    let w = lib
        .create_world("Fantasy", "Aerendal", "A kingdom in ruins after the Ash War.", "Melancholic tone, sober prose.")
        .unwrap();
    w.memory.write(|g| {
        let root = g.root();
        let lore = g.add_node(MemoryNode::new(NodeKind::Lore, "Ash magic", "Magic consumes the memories of whoever uses it", 0)).unwrap();
        g.connect(root, lore, EdgeKind::Composes, 0.9, 0).unwrap();
        g.add_node(MemoryNode::new(NodeKind::Place, "Vors", "Fortress city razed in the siege", 0)).unwrap();
    });
    let lyra = w
        .create_character("Lyra", "Elven librarian, reserved", "Keeper of the library of Vors", "elf, silver hair, blue tunic")
        .unwrap()
        .id();
    let kael = w.create_character("Kael", "Weathered mercenary", "Veteran mercenary of the siege of Vors", "human, scar, grey cloak").unwrap().id();
    let mira = w.create_character("Mira", "Curious apprentice", "Lyra's apprentice", "girl, curly hair").unwrap().id();
    w.relate(lyra, kael, &p("He saved her life in Vors; she owes him everything", 0.8, 0.8), &p("An odd elf who keeps staring at him", -0.1, 0.3))
        .unwrap();
    (w, lyra, kael, mira)
}

#[test]
fn slugs_are_filesystem_safe() {
    assert_eq!(slugify("  Ciencia Ficción / Ópera Espacial! "), "ciencia-ficcion-opera-espacial");
    assert_eq!(slugify("Kael"), "kael");
    assert_eq!(slugify("¿?"), "");
}

#[test]
fn worlds_are_grouped_by_category_and_reload_with_characters() {
    let lib = library("reload");
    let (w, lyra, kael, _) = aerendal(&lib);
    lib.create_world("Science fiction", "Falcon Station", "A mining station adrift.", "").unwrap();
    w.save_all().unwrap();

    assert_eq!(lib.categories().unwrap(), ["fantasy", "science-fiction"]);
    let listing = lib.worlds().unwrap();
    assert!(listing.broken.is_empty());
    let names: Vec<_> = listing.items.iter().map(|e| (e.category.as_str(), e.meta.name.as_str())).collect();
    assert_eq!(names, [("fantasy", "Aerendal"), ("science-fiction", "Falcon Station")]);

    let reopened = lib.open_world("fantasy", "aerendal").unwrap();
    assert_eq!(reopened.id(), w.id());
    assert_eq!(reopened.characters().len(), 3);
    assert!(reopened.dir.join("characters/lyra.mem").is_file());
    assert!(reopened.images_dir().is_dir());
    // External links survive the trip to disk.
    let lyra_h = reopened.character(lyra).unwrap();
    let kael_ref = ExternalRef { scope: Scope::Character, id: kael };
    let node = lyra_h.read(|g| g.find_external(kael_ref)).expect("Lyra remembers Kael");
    assert_eq!(lyra_h.read(|g| g.node(node).unwrap().label.clone()), "Kael");
    assert_eq!(reopened.character(kael).unwrap().meta().appearance, "human, scar, grey cloak");
}

#[test]
fn old_spanish_folders_and_avatar_paths_are_migrated_on_open() {
    let lib = library("legacy");
    let (w, lyra, ..) = aerendal(&lib);
    w.character(lyra).unwrap().write(|g| g.meta.avatar = Some("imagenes/lyra-1.png".into()));
    w.save_all().unwrap();
    let dir = w.dir.clone();
    drop(w);
    std::fs::rename(dir.join("world.mem"), dir.join("mundo.mem")).unwrap();
    std::fs::rename(dir.join("characters"), dir.join("personajes")).unwrap();
    std::fs::rename(dir.join("images"), dir.join("imagenes")).unwrap();

    let reopened = lib.open_world("fantasy", "aerendal").unwrap();
    assert!(dir.join("world.mem").is_file() && dir.join("characters").is_dir() && dir.join("images").is_dir());
    assert_eq!(reopened.characters().len(), 3);
    assert_eq!(reopened.character(lyra).unwrap().meta().avatar.as_deref(), Some("images/lyra-1.png"));
}

#[test]
fn perceptions_are_asymmetric_and_public_link_is_neutral() {
    let lib = library("asym");
    let (w, lyra, kael, _) = aerendal(&lib);
    let view = |who: GraphId, about: GraphId| {
        let h = w.character(who).unwrap();
        h.read(|g| {
            let n = g.find_external(ExternalRef { scope: Scope::Character, id: about }).unwrap();
            let n = g.node(n).unwrap();
            (n.valence, n.weight)
        })
    };
    assert_eq!(view(lyra, kael), (0.8, 0.8));
    assert_eq!(view(kael, lyra), (-0.1, 0.3));
    // The world only records that they know each other, without the private opinion.
    let public = w.memory.read(|g| {
        let a = g.find_external(ExternalRef { scope: Scope::Character, id: lyra }).unwrap();
        let b = g.find_external(ExternalRef { scope: Scope::Character, id: kael }).unwrap();
        g.edge(a, b).map(|e| e.kind)
    });
    assert_eq!(public, Some(EdgeKind::RelatedTo));
}

#[test]
fn scene_recall_mixes_world_and_own_memory_without_leaking_others() {
    let lib = library("scene");
    let (w, lyra, kael, mira) = aerendal(&lib);
    w.character(kael).unwrap().write(|g| {
        g.add_node(MemoryNode::new(NodeKind::SemanticFact, "Kael's secret", "Kael stole the crown of Vors", 0).with_weight(1.0)).unwrap();
    });

    let scene = w.scene_recall(lyra, "Do you remember Vors and the crown?", &[kael, mira]).unwrap();
    let ctx = &scene.context;
    assert!(ctx.starts_with("<world>"), "{ctx}");
    assert!(ctx.contains("Ash magic"), "the world's lore always gets in: {ctx}");
    assert!(ctx.contains("<scene>") && ctx.contains("- Kael: He saved her life in Vors"), "{ctx}");
    assert!(ctx.contains("you are very fond of them"), "{ctx}");
    assert!(ctx.contains("- Mira: you don't know them yet. What people say: Lyra's apprentice"), "{ctx}");
    assert!(ctx.contains("<active_memory>"));
    assert!(!ctx.contains("stole the crown"), "Kael's private memory does not leak to Lyra");
    assert!(ctx.len() <= HandlerConfig::default().budget.max_chars + 400, "{} characters", ctx.len());
}

#[test]
fn a_spanish_world_builds_its_context_in_spanish() {
    let lib = library_in("es", Lang::Es);
    let w = lib.create_world("Fantasía", "Aerendal", "Reino en ruinas.", "").unwrap();
    let a = w.create_character("Lyra", "Bibliotecaria", "Guardiana de la biblioteca", "").unwrap().id();
    let b = w.create_character("Kael", "Mercenario", "Mercenario veterano", "").unwrap().id();
    w.set_perception(a, b, &p("Le salvó la vida", 0.8, 0.8)).unwrap();
    w.set_appearance(a, &[Aspect { label: "Pelo".into(), content: "pelo corto plateado".into(), weight: 0.9 }], true).unwrap();
    let ctx = w.scene_recall(a, "¿Cómo llevas el pelo?", &[a, b]).unwrap().context;
    for want in ["## Contigo en la escena", "le tienes mucho cariño", "Tú · Pelo: pelo corto plateado", "## Quién eres"] {
        assert!(ctx.contains(want), "falta «{want}»:\n{ctx}");
    }
    // The language can change at runtime, for the world and all its characters.
    w.set_lang(Lang::En);
    let ctx = w.scene_recall(a, "How is your hair?", &[a, b]).unwrap().context;
    assert!(ctx.contains("## With you in the scene") && ctx.contains("You · Pelo"), "{ctx}");
}

#[test]
fn curator_character_nodes_link_to_the_roster() {
    let lib = library("roster");
    let (w, lyra, kael, mira) = aerendal(&lib);
    let lyra_h = w.character(lyra).unwrap();
    let raw = r#"{"ops":[
        {"op":"create_node","ref":"$a","kind":"character","label":"kael","content":"duplicate","weight":0.5,"valence":0.5},
        {"op":"create_node","ref":"$b","kind":"character","label":"Mira","content":"Her apprentice, clever and stubborn","weight":0.7,"valence":0.6},
        {"op":"create_node","ref":"$c","kind":"character","label":"Stranger","content":"A nameless traveller","weight":0.2,"valence":0}
    ]}"#;
    let report = lyra_h.apply_ops(&OpBatch::parse_lenient(raw).unwrap());
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    let kael_node = lyra_h.read(|g| g.find_external(ExternalRef { scope: Scope::Character, id: kael }));
    assert_eq!(report.created[0].1, kael_node.unwrap(), "it merges with the existing perception");
    let mira_node = lyra_h.read(|g| g.find_external(ExternalRef { scope: Scope::Character, id: mira }));
    assert_eq!(report.created[1].1, mira_node.expect("Mira ends up linked to her .mem"));
    assert!(lyra_h.read(|g| g.external(report.created[2].1)).is_none(), "an NPC without a sheet is not linked");
}

#[test]
fn duplicate_names_are_rejected() {
    let lib = library("dupes");
    let (w, ..) = aerendal(&lib);
    assert!(matches!(w.create_character("lyra", "x", "x", "x"), Err(WorldError::AlreadyExists(_))));
    assert!(matches!(lib.create_world("fantasy", "Aerendal", "x", ""), Err(WorldError::AlreadyExists(_))));
}

// ---------------------------------------------------------------- per-chat memory

fn chat_with(w: &World, title: &str, participants: Vec<GraphId>, user: &str) -> memory_core::conversation::Conversation {
    let c = memory_core::conversation::Conversation::new(title, participants, user);
    c.save(&w.dir).unwrap();
    c
}

#[test]
fn chat_memory_is_shared_by_the_scene_and_kept_apart_from_characters() {
    let lib = library("chatmem");
    let (w, lyra, kael, _) = aerendal(&lib);
    let conv = chat_with(&w, "Night watch", vec![lyra, kael], "Tomás");
    let chat = w.chat_memory(&conv).unwrap();
    // Root + one linked node per participant + the user.
    assert_eq!(chat.read(|g| g.node_count()), 4);
    assert_eq!(chat.meta().scope, Scope::Chat);
    assert!(chat.read(|g| g.find_external(ExternalRef { scope: Scope::Character, id: kael })).is_some());
    let now = unix_now();
    chat.write(|g| {
        let root = g.root();
        let rule = g.add_node(MemoryNode::new(NodeKind::Lore, "No magic tonight", "In this chat nobody can cast spells", now).with_weight(0.9)).unwrap();
        g.connect(root, rule, EdgeKind::Composes, 0.9, now).unwrap();
        g.add_node(MemoryNode::new(NodeKind::EpisodicMemory, "The broken lantern", "Kael broke the lantern at the door of the tower", now)).unwrap();
    });

    let q = "What happened with the lantern?";
    for speaker in [lyra, kael] {
        let ctx = w.scene_recall_in(speaker, q, &[lyra, kael], Some(&chat)).unwrap().context;
        assert!(ctx.contains("<chat_memory>") && ctx.contains("## Rules of this conversation") && ctx.contains("No magic tonight"), "{ctx}");
        assert!(ctx.contains("The broken lantern"), "{ctx}");
    }
    let plain = w.scene_recall(lyra, q, &[lyra, kael]).unwrap().context;
    assert!(!plain.contains("<chat_memory>") && !plain.contains("lantern"), "a scene without the chat does not see it");
    // The characters' own memories did not get the chat's nodes.
    assert!(w.character(lyra).unwrap().read(|g| g.find_by_label("The broken lantern", None)).is_none());

    // It survives reopening the world, follows renames and goes away with the chat.
    w.save_all().unwrap();
    let reopened = lib.open_world("fantasy", "aerendal").unwrap();
    let again = reopened.chat_memory(&conv).unwrap();
    assert_eq!(again.read(|g| g.node_count()), 6);
    reopened.rename_chat_memory(conv.id, "Long night").unwrap();
    assert_eq!(again.read(|g| g.node(g.root()).unwrap().label.clone()), "Long night");
    reopened.delete_chat_memory(conv.id).unwrap();
    assert!(!memory_core::conversation::Conversation::memory_path(&reopened.dir, conv.id).exists());
}

#[test]
fn chat_curator_example_applies_and_personality_stays_out() {
    for lang in Lang::ALL {
        let lib = library_in(&format!("chatcur-{lang}"), lang);
        let w = lib.create_world("Tests", "Library", "A library.", "").unwrap();
        let lyra = w.create_character("Lyra", "Librarian", "Keeper of the library of Vors", "").unwrap().id();
        let conv = chat_with(&w, "Night watch", vec![lyra], "User");
        let chat = w.chat_memory(&conv).unwrap();

        let prompt = memory_core::prompts::chat_curator_system_prompt(lang, "Night watch", 0);
        let marker = format!("{}\n", i18n::t(lang, "curator.output"));
        let examples: Vec<OpBatch> = prompt.split(marker.as_str()).skip(1).map(|json| OpBatch::parse_lenient(json).unwrap()).collect();
        assert_eq!(examples.len(), 2, "{lang}");
        let report = chat.apply_ops(&examples[0]);
        assert!(report.rejected.is_empty() && report.applied == 7, "{lang}: {:?}", report.rejected);
        chat.read(|g| {
            assert_eq!(g.node(2).unwrap().label, "Tomás");
            let rule = report.created.iter().find(|(r, _)| r == "$a").unwrap().1;
            assert!(g.edge(g.root(), rule).is_some(), "the chat's rules hang from its root");
        });

        // Example 2: the key passes from Lyra (#1) to the user (#2). The old fact (#3) is
        // kept, its link closes at the chat's moment and a new fact contradicts it.
        let conv = chat_with(&w, "Archive", vec![lyra], "Tomás");
        let chat2 = w.chat_memory(&conv).unwrap();
        let at = story_of_year(1093, 0);
        let old = "Lyra carries the key of the sealed archive";
        chat2.write(|g| {
            let fact = g.add_node(MemoryNode::new(NodeKind::SemanticFact, "Lyra has the key", old, 0)).unwrap();
            assert_eq!(fact, 3, "{lang}: the example refers to this id");
            g.connect(fact, 1, EdgeKind::RelatedTo, 0.8, 0).unwrap();
        });
        let report = chat2.apply_ops_at(&examples[1], at);
        assert!(report.rejected.is_empty() && report.applied == 7, "{lang}: {:?}", report.rejected);
        let new = report.created.iter().find(|(r, _)| r == "$a").unwrap().1;
        chat2.read(|g| {
            assert!(g.edge(3, 1).is_none() && g.edges_from_to(3, 1).next().unwrap().valid_until == at, "{lang}");
            assert_eq!(g.node(3).unwrap().content, old, "{lang}: the old fact is not rewritten");
            assert_eq!(g.edge(new, 2).unwrap().valid_from, at);
            assert_eq!(g.edge(new, 3).unwrap().kind, EdgeKind::Contradicts);
        });

        let trait_op = r#"{"ops":[{"op":"create_node","ref":"$t","kind":"trait","label":"Brave","content":"x","weight":0.5,"valence":0}]}"#;
        let r = chat.apply_ops(&OpBatch::parse_lenient(trait_op).unwrap());
        assert_eq!((r.applied, r.rejected.len()), (0, 1), "personality belongs to the characters");
    }
}

#[test]
fn chat_character_carbon_copy_isolates_character_mutations_per_chat() {
    let lib = library("carboncopy");
    let (w, lyra, kael, _) = aerendal(&lib);
    let base_nodes = w.character(lyra).unwrap().read(|g| g.node_count());

    let conv = chat_with(&w, "Secret meeting", vec![lyra, kael], "Doctor");
    let chat = w.chat_memory(&conv).unwrap();
    let copy_path = memory_core::conversation::Conversation::character_memory_path(&w.dir, conv.id, lyra);
    assert!(copy_path.is_file(), "Carbon Copy file created with chat");

    let lyra_copy = w.chat_character(conv.id, lyra).unwrap();
    assert_ne!(lyra_copy.id(), lyra, "Carbon Copy gets its own worker GraphId");
    assert_eq!(lyra_copy.read(|g| g.node_count()), base_nodes);

    // Mutate the chat's Carbon Copy of Lyra.
    lyra_copy.write(|g| {
        let root = g.root();
        let m = g.add_node(MemoryNode::new(NodeKind::EpisodicMemory, "Tea in the ruins", "Lyra shared tea with Kael in the ruins", 0).with_weight(0.9)).unwrap();
        g.connect(m, root, EdgeKind::Involves, 0.9, 0).unwrap();
    });
    w.save_all().unwrap();

    // Base character remains untouched.
    assert_eq!(w.character(lyra).unwrap().read(|g| g.node_count()), base_nodes);
    assert!(w.character(lyra).unwrap().read(|g| g.find_by_label("Tea in the ruins", None)).is_none());

    // Scene recall inside the chat reads from the Carbon Copy, while scene recall outside reads from the base character.
    let in_chat = w.scene_recall_in(lyra, "Do you remember the tea in the ruins?", &[lyra, kael], Some(&chat)).unwrap().context;
    assert!(in_chat.contains("Tea in the ruins"), "{in_chat}");
    let out_chat = w.scene_recall(lyra, "Do you remember the tea in the ruins?", &[lyra, kael]).unwrap().context;
    assert!(!out_chat.contains("Tea in the ruins"), "{out_chat}");

    // Deleting the chat removes the Carbon Copy too.
    w.delete_chat_memory(conv.id).unwrap();
    assert!(!copy_path.exists());
}

