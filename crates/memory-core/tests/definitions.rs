//! Shared definitions: libraries every world of a category (or every world) relies on,
//! loaded once and recalled only when a turn touches them.

use memory_core::world::DefinitionLevel;
use memory_core::*;

fn lib(name: &str) -> Library {
    let dir = std::env::temp_dir().join(format!("memory-core-defs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::new(dir, HandlerConfig::with_lang(Lang::En))
}

fn define(h: &MemoryHandler, label: &str, content: &str) -> NodeId {
    h.write(|g| {
        let id = g.add_node(MemoryNode::new(NodeKind::Concept, label, content, 0).with_weight(0.7)).unwrap();
        let root = g.root();
        g.connect(root, id, EdgeKind::Knows, 0.7, 0).unwrap();
        id
    })
}

#[test]
fn worlds_of_a_category_share_one_library_and_one_handler() {
    let lib = lib("share");
    let terra = lib.create_world("Arknights", "Terra", "Nomadic cities.", "").unwrap();
    let talos = lib.create_world("Arknights", "Talos II", "A frontier planet.", "").unwrap();
    let aerendal = lib.create_world("Fantasy", "Aerendal", "A kingdom.", "").unwrap();
    assert!(terra.definitions().is_empty(), "nothing exists until it is created");

    let arknights = terra.definition_library(DefinitionLevel::Category, true).unwrap().unwrap();
    assert_eq!(arknights.meta().scope, Scope::Library);
    assert_eq!(arknights.meta().name, "Arknights");
    let from_talos = talos.definition_library(DefinitionLevel::Category, false).unwrap().unwrap();
    assert_eq!(arknights.path(), from_talos.path());
    define(&arknights, "Originium", "A black crystal that stores energy");
    assert_eq!(from_talos.read(|g| g.node_count()), 2, "the same handler: an edit is seen by both worlds at once");
    assert!(arknights.path().ends_with("arknights/_category.mem"));

    let universal = aerendal.definition_library(DefinitionLevel::Universal, true).unwrap().unwrap();
    assert_eq!(terra.definitions().len(), 2, "category's, then universal");
    assert_eq!(aerendal.definitions().len(), 1, "another category does not see Arknights'");
    assert_eq!(terra.definitions()[1].path(), universal.path());

    // Reopened from disk (another run of the app), the library is there with its content.
    terra.save_all().unwrap();
    let again = Library::new(lib.root(), HandlerConfig::with_lang(Lang::En));
    let terra2 = again.open_world("arknights", "terra").unwrap();
    assert!(terra2.definitions()[0].read(|g| g.find_by_label("Originium", None).is_some()));
}

#[test]
fn a_turn_recalls_only_the_definitions_it_touches_and_never_twice() {
    let lib = lib("recall");
    let terra = lib.create_world("Arknights", "Terra", "Nomadic cities.", "").unwrap();
    let talos = lib.create_world("Arknights", "Talos II", "A frontier planet.", "").unwrap();
    let kal = terra.create_character("Kal'tsit", "A doctor", "Head of medicine", "").unwrap();
    let perlica = talos.create_character("Perlica", "A supervisor", "Endfield supervisor", "").unwrap();
    let arknights = terra.definition_library(DefinitionLevel::Category, true).unwrap().unwrap();
    define(&arknights, "Originium", "A black crystal that stores energy and spreads Oripathy");
    define(&arknights, "Lupo", "A people with wolf ears and tails");
    let universal = terra.definition_library(DefinitionLevel::Universal, true).unwrap().unwrap();
    define(&universal, "Iron", "A grey metal, strong and common");

    // Perlica knows nothing of her own about Originium: the shared definition fills the gap.
    let ctx = talos.scene_recall(perlica.id(), "Is that Originium dangerous?", &[perlica.id()]).unwrap().context;
    assert!(ctx.contains("<definitions>") && ctx.contains("## General knowledge (Arknights)"), "{ctx}");
    assert!(ctx.contains("Originium: A black crystal"), "{ctx}");
    assert!(!ctx.contains("Lupo") && !ctx.contains("Iron"), "only what the turn touched:\n{ctx}");

    // Kal'tsit has her own version: hers is said, the definition is not repeated.
    kal.write(|g| {
        let id = g.add_node(MemoryNode::new(NodeKind::SemanticFact, "Originium", "Her life's work and her people's curse", 0).with_weight(0.9)).unwrap();
        let root = g.root();
        g.connect(root, id, EdgeKind::Knows, 0.9, 0).unwrap();
    });
    let ctx = terra.scene_recall(kal.id(), "Is that Originium dangerous?", &[kal.id()]).unwrap().context;
    assert!(ctx.contains("Her life's work"), "{ctx}");
    assert!(!ctx.contains("A black crystal"), "her own knowledge wins:\n{ctx}");

    // A turn that touches nothing shared adds no block at all.
    let ctx = talos.scene_recall(perlica.id(), "Good morning.", &[perlica.id()]).unwrap().context;
    assert!(!ctx.contains("<definitions>"), "{ctx}");
    // The universal library is there for every world.
    let ctx = talos.scene_recall(perlica.id(), "Is the bridge made of iron?", &[perlica.id()]).unwrap().context;
    assert!(ctx.contains("## General knowledge (Universal)") && ctx.contains("Iron: A grey metal"), "{ctx}");
}

#[test]
fn a_library_holds_no_personality() {
    let lib = lib("rules");
    let w = lib.create_world("Arknights", "Terra", "Nomadic cities.", "").unwrap();
    let h = w.definition_library(DefinitionLevel::Category, true).unwrap().unwrap();
    let r = h.apply_ops(
        &OpBatch::parse_lenient(r#"{"ops":[{"op":"create_node","ref":"$a","kind":"trait","label":"Brave","content":"x","weight":0.5,"valence":0},
        {"op":"create_node","ref":"$b","kind":"concept","label":"Sarkaz","content":"A people with horns","weight":0.6,"valence":0}]}"#)
        .unwrap(),
    );
    assert_eq!((r.applied, r.rejected.len()), (1, 1), "{:?}", r.rejected);
    let sarkaz = r.created.iter().find(|(k, _)| k == "$b").unwrap().1;
    h.read(|g| assert!(g.edge(g.root(), sarkaz).is_some(), "a new definition hangs from the library's root"));
}

