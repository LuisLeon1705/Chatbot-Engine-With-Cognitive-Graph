//! `chats`: command-line tool for testing worlds, memory, models and images.
//! Run `chats` without arguments for the list of commands (in the chosen language).
//!
//! It runs from the project folder: it uses `config/`, `models/`, `worlds/` and `logs/`.
//! Folders with the old Spanish names (`mundos/`, `semillas/`, `config/modelos.toml`…)
//! are renamed on start-up. Every command also accepts its old Spanish name.

mod seed;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use i18n::{Lang, Settings, tr};
use memory_core::layout::{APP_CONFIG, LOGS_DIR, MODELS_CONFIG, WORLDS_DIR};
use memory_core::llm::{Backend, ConsolidationWorker, LlmClient, LlmEndpoint, SceneTurn, TurnRecord, scene_turn};
use memory_core::prompts::scene_actor_prompt;
use memory_core::context::story_label;
use memory_core::world::{Library, World, slugify};
use memory_core::{GraphId, HandlerConfig, MemoryHandler, Moment, NodeKind, story_year, unix_now};
use model_hub::{HubConfig, Slot, Supervisor};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Recent messages the actor sees besides its memory.
const WINDOW: usize = 8;

type Res<T> = Result<T, String>;

static LANG: OnceLock<Lang> = OnceLock::new();

/// Language of this run: `CHATS_LANG`, then `config/app.toml`, then English.
fn lang() -> Lang {
    *LANG.get_or_init(|| i18n::resolve(Path::new(APP_CONFIG)))
}

#[tokio::main]
async fn main() {
    for (from, to) in memory_core::layout::migrate_legacy(Path::new(".")) {
        eprintln!("{}", tr!(lang(), "cli.migrated", from = from.display(), to = to.display()));
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("models" | "modelos") => cmd_models(),
        Some("worlds" | "mundos") => cmd_worlds(),
        Some("seed" | "sembrar") => cmd_seed(rest),
        Some("context" | "contexto") => cmd_context(rest),
        Some("memory" | "memoria") => cmd_memory(rest),
        Some("chat") => cmd_chat(rest).await,
        Some("image" | "imagen") => cmd_image(rest).await,
        Some("appearance" | "apariencia") => cmd_appearance(rest),
        Some("stress" | "estres" | "estrés") => cmd_stress(rest),
        Some("reference" | "referencia") => cmd_reference(rest).await,
        Some("lang" | "language" | "idioma") => cmd_lang(rest),
        _ => {
            println!("{}", i18n::t(lang(), "cli.help").trim());
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("{}", tr!(lang(), "common.error", error = e));
        std::process::exit(1);
    }
}

// --------------------------------------------------------------------------- arguments

/// Value after any of `names` (`--with A,B`, `--con A,B`).
fn flag_value<'a>(args: &'a [String], names: &[&str]) -> Option<&'a str> {
    args.iter().position(|a| names.contains(&a.as_str())).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn has_flag(args: &[String], names: &[&str]) -> bool {
    args.iter().any(|a| names.contains(&a.as_str()))
}

const WITH: &[&str] = &["--with", "--con"];
const AS: &[&str] = &["--as", "--como"];
const AT: &[&str] = &["--at", "--en"];

fn config() -> Res<HubConfig> {
    HubConfig::load_or_create(Path::new(MODELS_CONFIG)).map_err(|e| e.to_string())
}

fn library() -> Library {
    Library::new(WORLDS_DIR, HandlerConfig::with_lang(lang()))
}

fn open_world(spec: &str) -> Res<World> {
    let (cat, name) = spec.split_once('/').ok_or_else(|| tr!(lang(), "cli.world_spec"))?;
    library().open_world(&slugify(cat), &slugify(name)).map_err(|e| format!("{spec}: {e}"))
}

fn find_character(world: &World, name: &str) -> Res<MemoryHandler> {
    world.character_by_name(name).ok_or_else(|| {
        let names: Vec<String> = world.characters().iter().map(|h| h.meta().name).collect();
        tr!(lang(), "cli.no_character", name = name, list = names.join(", "))
    })
}

fn gb(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

/// The user's name in the scene (`--as Tomás`); "User" by default.
fn user_name(args: &[String]) -> String {
    flag_value(args, AS).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| tr!(lang(), "common.user"))
}

/// The moment of `--at <name or year>` in the world's timeline (`None` without the flag).
fn moment_from_args(world: &World, args: &[String]) -> Res<Option<Moment>> {
    let Some(wanted) = flag_value(args, AT) else { return Ok(None) };
    let timeline = world.timeline();
    let folded = memory_core::text::fold(wanted.trim());
    let found = timeline
        .iter()
        .find(|m| memory_core::text::fold(&m.name) == folded || story_year(m.at).is_some_and(|y| y.to_string() == wanted.trim()));
    match found {
        Some(m) => Ok(Some(m.clone())),
        None => {
            let list = timeline.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ");
            Err(tr!(lang(), "cli.no_moment", name = wanted, list = list))
        }
    }
}

/// The speaker plus the characters of `--with A,B` (without repeating the speaker).
fn present_from_args(world: &World, speaker: &MemoryHandler, args: &[String]) -> Res<Vec<MemoryHandler>> {
    let mut present = vec![speaker.clone()];
    for n in flag_value(args, WITH).map(|s| s.split(',').collect::<Vec<_>>()).unwrap_or_default() {
        let h = find_character(world, n.trim())?;
        if h.id() != speaker.id() {
            present.push(h);
        }
    }
    Ok(present)
}

// --------------------------------------------------------------------------- language

fn cmd_lang(args: &[String]) -> Res<()> {
    match args.first() {
        None => println!("{}", lang().code()),
        Some(value) => {
            let new = Lang::parse(value).ok_or_else(|| tr!(lang(), "cli.lang_unknown", value = value))?;
            let mut settings = Settings::load(Path::new(APP_CONFIG));
            settings.language = new;
            settings.save(Path::new(APP_CONFIG)).map_err(|e| e.to_string())?;
            println!("{}", tr!(new, "cli.lang_set"));
        }
    }
    Ok(())
}

// --------------------------------------------------------------------------- models

fn cmd_models() -> Res<()> {
    let l = lang();
    let cfg = config()?;
    let reg = model_hub::Registry::scan_all(&cfg.models_dir, &cfg.extra_models_dirs);
    println!("{}", tr!(l, "cli.models.folder", dir = cfg.models_dir.display(), mb = cfg.budget_mb()));
    for d in &cfg.extra_models_dirs {
        println!("   + {}", d.display());
    }
    println!("\nLLM:");
    for m in &reg.llms {
        let vision = m
            .mmproj
            .as_ref()
            .map(|p| tr!(l, "cli.models.vision", file = p.file_name().unwrap_or_default().to_string_lossy()))
            .unwrap_or_else(|| tr!(l, "cli.models.no_vision"));
        println!(
            "  {:<44} {:>8}  {:<8} {:<10} {}",
            m.file_name,
            gb(m.size_bytes),
            m.size_label.clone().unwrap_or_default(),
            m.quant.clone().unwrap_or_default(),
            vision
        );
    }
    let orphan: Vec<_> = reg.mmprojs.iter().filter(|p| !reg.llms.iter().any(|l| l.mmproj.as_ref() == Some(&p.path))).collect();
    if !orphan.is_empty() {
        println!("\n{}", i18n::t(l, "cli.models.orphans"));
        for p in orphan {
            println!("  {}", p.file_name);
        }
    }
    println!("\n{}", i18n::t(l, "cli.models.images"));
    if reg.diffusion.is_empty() {
        println!("  {}", tr!(l, "cli.models.no_diffusion", dir = cfg.models_dir.join("diffusion").display()));
    }
    for d in &reg.diffusion {
        let kit = reg.diffusion_kit(&d.file_name);
        let state = match kit.as_ref().map(|k| k.missing()) {
            Some(m) if m.is_empty() => tr!(l, "cli.models.ready"),
            Some(m) => tr!(l, "cli.models.missing", list = missing_list(l, &m)),
            None => String::new(),
        };
        println!("  {:<44} {:>8}  {:?}  {state}", d.file_name, gb(d.size_bytes), d.family.unwrap_or(model_hub::DiffusionFamily::Unknown));
    }
    for (title, list) in [("VAE", &reg.vaes), (i18n::t(l, "cli.models.encoders"), &reg.encoders), ("LoRA", &reg.loras)] {
        if !list.is_empty() {
            println!("  {title}: {}", list.iter().map(|m| m.file_name.as_str()).collect::<Vec<_>>().join(", "));
        }
    }
    let (_, curator) = reg.suggest(cfg.budget_mb());
    println!("\n{}", i18n::t(l, "cli.models.roles"));
    let sup = Supervisor::new(cfg.clone(), LOGS_DIR);
    let slot = |s: &str| format!("{:<8}", i18n::t(l, s));
    match sup.spec(Slot::Actor) {
        Ok(s) => {
            let vision = if s.mmproj.is_some() { tr!(l, "cli.models.with_vision") } else { String::new() };
            println!("  {}→ {}", slot("slot.actor"), tr!(l, "cli.models.actor", model = s.alias, mb = s.gpu_mb, vision = vision));
        }
        Err(e) => println!("  {}→ {e}", slot("slot.actor")),
    }
    if sup.curator_shares_actor() {
        println!("  {}→ {}", slot("slot.curator"), tr!(l, "cli.models.curator_shared", config = MODELS_CONFIG));
        if let Some(c) = curator {
            println!("            {}", tr!(l, "cli.models.curator_hint", model = c.stem()));
        }
    } else {
        match sup.spec(Slot::Curator) {
            Ok(s) => println!("  {}→ {} ({})", slot("slot.curator"), s.alias, if s.gpu_mb == 0 { "CPU".into() } else { format!("≈{} MB", s.gpu_mb) }),
            Err(e) => println!("  {}→ {e}", slot("slot.curator")),
        }
    }
    if sup.has_reader() {
        match sup.spec(Slot::Reader) {
            Ok(s) => {
                let vision = if s.mmproj.is_some() { i18n::t(l, "cli.models.reader_vision") } else { i18n::t(l, "cli.models.reader_no_mmproj") };
                println!("  {}→ {}", slot("slot.reader"), tr!(l, "cli.models.reader", model = s.alias, vision = vision));
            }
            Err(e) => println!("  {}→ {e}", slot("slot.reader")),
        }
    } else {
        println!("  {}→ {}", slot("slot.reader"), i18n::t(l, "cli.models.reader_is_actor"));
    }
    let not_found = || tr!(l, "cli.models.not_found");
    println!("\n{}", i18n::t(l, "cli.models.binaries"));
    println!("  llama-server: {}", cfg.llama_server().map(|p| p.display().to_string()).unwrap_or_else(not_found));
    println!("  sd-cli:       {}", cfg.sd_cli().map(|p| p.display().to_string()).unwrap_or_else(not_found));
    for w in &reg.warnings {
        println!("  {}", tr!(l, "cli.models.warning", text = w));
    }
    Ok(())
}

/// Missing components of an image kit, translated.
fn missing_list(l: Lang, keys: &[&str]) -> String {
    keys.iter().map(|k| i18n::t(l, &format!("kit.missing.{k}"))).collect::<Vec<_>>().join(", ")
}

// --------------------------------------------------------------------------- worlds

fn cmd_worlds() -> Res<()> {
    let l = lang();
    let lib = library();
    let listing = lib.worlds().map_err(|e| e.to_string())?;
    if listing.items.is_empty() {
        println!("{}", tr!(l, "cli.worlds.none", dir = WORLDS_DIR));
    }
    let mut last = String::new();
    for e in &listing.items {
        if e.category != last {
            println!("\n[{}]", e.category);
            last = e.category.clone();
        }
        println!("  {}", tr!(l, "cli.worlds.world", key = format!("{}/{}", e.category, e.slug), name = e.meta.name, nodes = e.nodes));
        for m in &e.meta.timeline {
            println!("     {}", tr!(l, "cli.worlds.moment", name = m.name, when = story_label(l, m.at, &[])));
        }
        if let Ok(w) = lib.open_world(&e.category, &e.slug) {
            for h in w.characters() {
                let m = h.meta();
                println!("     {}", tr!(l, "cli.worlds.character", name = m.name, tagline = m.tagline, nodes = h.read(|g| g.node_count()), id = m.id));
            }
        }
    }
    for (p, err) in &listing.broken {
        println!("  {}", tr!(l, "cli.worlds.broken", path = p.display(), error = err));
    }
    Ok(())
}

fn cmd_seed(args: &[String]) -> Res<()> {
    let l = lang();
    let file = args.iter().find(|a| !a.starts_with("--")).ok_or_else(|| tr!(l, "cli.usage.seed"))?;
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let seed: seed::Seed = toml::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
    if has_flag(args, &["--appearance-only", "--solo-apariencia"]) {
        let world = open_world(&format!("{}/{}", seed.world.category, seed.world.name))?;
        let n = seed::apply_appearance(&world, &seed)?;
        world.save_all().map_err(|e| e.to_string())?;
        println!("{}", tr!(l, "cli.seed.appearance_done", world = world.memory.meta().name, n = n));
        return Ok(());
    }
    if has_flag(args, &["--restore-characters", "--restaurar-personajes"]) {
        let world = open_world(&format!("{}/{}", seed.world.category, seed.world.name))?;
        let _ = seed::restore_characters(&world, &seed)?;
        world.save_all().map_err(|e| e.to_string())?;
        for h in world.characters() {
            println!("  {}", tr!(l, "cli.seed.character", name = h.meta().name, nodes = h.read(|g| g.node_count()), edges = h.read(|g| g.edge_count())));
        }
        return Ok(());
    }
    let world = seed::plant(&library(), &seed)?;
    println!("{}", tr!(l, "cli.seed.created", name = world.memory.meta().name, dir = world.dir.display()));
    for h in world.characters() {
        println!("  {}", tr!(l, "cli.seed.character", name = h.meta().name, nodes = h.read(|g| g.node_count()), edges = h.read(|g| g.edge_count())));
    }
    Ok(())
}

/// Shows the exact system prompt the actor would get, without loading models.
fn cmd_context(args: &[String]) -> Res<()> {
    let l = lang();
    let (world_spec, who, msg) = match args {
        [w, c, m, ..] => (w, c, m),
        _ => return Err(tr!(l, "cli.usage.context")),
    };
    let world = open_world(world_spec)?;
    let speaker = find_character(&world, who)?;
    let present = present_from_args(&world, &speaker, args)?;
    let ids: Vec<GraphId> = present.iter().map(|h| h.id()).collect();
    let moment = world.resolve_moment(moment_from_args(&world, args)?.map(|m| m.id));
    let story = world.story_now(moment.as_ref().map(|m| m.id));
    let recall = world.scene_recall_at(speaker.id(), msg, &ids, None, story).map_err(|e| e.to_string())?;
    let others: Vec<String> = present.iter().filter(|h| h.id() != speaker.id()).map(|h| h.meta().name).collect();
    println!("{}", scene_actor_prompt(l, &speaker.meta(), &world.memory.meta(), &recall.context, &others, &user_name(args), moment.as_ref()));
    eprintln!("\n{}", tr!(l, "cli.context.size", chars = recall.context.len(), tokens = recall.context.len() / 4));
    // Looking must not count as rehearsal: nothing is saved.
    Ok(())
}

/// Lists a character's most recent nodes (what the curator has been storing) and the
/// edges leaving them.
fn cmd_memory(args: &[String]) -> Res<()> {
    let l = lang();
    let (world_spec, who) = match args {
        [w, c, ..] => (w, c),
        _ => return Err(tr!(l, "cli.usage.memory")),
    };
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(15);
    let world = open_world(world_spec)?;
    let h = find_character(&world, who)?;
    let now = unix_now();
    let timeline = world.timeline();
    h.read(|g| {
        let mut nodes: Vec<_> = g.nodes().collect();
        nodes.sort_by_key(|(id, node)| (std::cmp::Reverse(node.created_at), std::cmp::Reverse(*id)));
        println!("{}\n", tr!(l, "cli.memory.header", name = g.meta.name, nodes = g.node_count(), edges = g.edge_count(), n = n.min(nodes.len())));
        for (id, node) in nodes.into_iter().take(n) {
            let link = g.external(id).map(|_| " ↗").unwrap_or("");
            println!("#{id:<3} [{}{link}] {} — {}", node.kind.context_label(l), node.label, node.content);
            if node.since != memory_core::STORY_ALWAYS {
                println!("      {}", tr!(l, "cli.memory.since", when = story_label(l, node.since, &timeline)));
            }
            println!(
                "      {}",
                tr!(
                    l,
                    "cli.memory.line",
                    weight = format!("{:.2}", node.weight),
                    valence = format!("{:+.2}", node.valence),
                    when = memory_core::context::relative_time(l, node.created_at, now),
                    count = node.access_count
                )
            );
            let edges: Vec<String> = g
                .neighbors(id)
                .filter(|(_, _, out)| *out)
                .filter_map(|(v, e, _)| {
                    let t = g.node(v)?;
                    let past = if e.is_open() { String::new() } else { format!(" {}", tr!(l, "cli.memory.closed", when = story_label(l, e.valid_until, &timeline))) };
                    Some(format!("{}→{}{past}", e.kind.label(l), t.label))
                })
                .collect();
            if !edges.is_empty() {
                println!("      {}", edges.join(", "));
            }
        }
    });
    Ok(())
}

// --------------------------------------------------------------------------- chat

struct Scene {
    world: World,
    speaker: MemoryHandler,
    present: Vec<MemoryHandler>,
    workers: Vec<(GraphId, ConsolidationWorker, tokio::task::JoinHandle<()>)>,
    history: Vec<(String, String)>,
    last_context: String,
    /// Moment of the timeline (`--at`); `None` = the latest.
    moment: Option<u32>,
}

async fn clients(sup: &Supervisor) -> Res<(LlmClient, LlmClient)> {
    eprintln!("{}", i18n::t(lang(), "cli.chat.loading_actor"));
    let actor_url = sup.ensure(Slot::Actor).await.map_err(|e| e.to_string())?;
    let actor_alias = sup.spec(Slot::Actor).map(|s| s.alias).unwrap_or_default();
    let curator_url = sup.ensure(Slot::Curator).await.map_err(|e| e.to_string())?;
    let curator_alias = sup.spec(Slot::Curator).map(|s| s.alias).unwrap_or_default();
    let actor = LlmClient::new(LlmEndpoint::actor(Backend::LlamaServer, actor_url, actor_alias)).map_err(|e| e.to_string())?;
    let curator = LlmClient::new(LlmEndpoint::curator(Backend::LlamaServer, curator_url, curator_alias)).map_err(|e| e.to_string())?;
    Ok((actor, curator))
}

/// A single curator at a time for the whole scene: the actor always has a free slot.
type Gate = Arc<tokio::sync::Semaphore>;

fn spawn_worker(h: &MemoryHandler, curator: &LlmClient, gate: &Gate) -> (GraphId, ConsolidationWorker, tokio::task::JoinHandle<()>) {
    let name = h.log_label();
    let (w, j) = ConsolidationWorker::spawn_gated(h.clone(), curator.clone(), Some(gate.clone()), move |r| {
        let l = lang();
        memory_core::llm::log_curator_result(&Path::new(LOGS_DIR).join(memory_core::layout::CURATOR_LOG), l, &name, &r);
        match r {
            Ok(rep) if rep.applied > 0 => {
                let rejected = if rep.rejected.is_empty() { String::new() } else { tr!(l, "cli.chat.memory_rejected", n = rep.rejected.len()) };
                eprintln!("  {}{rejected}", tr!(l, "cli.chat.memory_changes", name = name, n = rep.applied));
            }
            Ok(_) => {}
            Err(e) => eprintln!("  {}", tr!(l, "cli.chat.curator_error", name = name, error = e)),
        }
    });
    (h.id(), w, j)
}

async fn cmd_chat(args: &[String]) -> Res<()> {
    let l = lang();
    let (world_spec, who) = match args {
        [w, c, ..] => (w, c),
        _ => return Err(tr!(l, "cli.usage.chat")),
    };
    let world = open_world(world_spec)?;
    let speaker = find_character(&world, who)?;
    let present = present_from_args(&world, &speaker, args)?;
    let user = user_name(args);
    let moment = moment_from_args(&world, args)?.map(|m| m.id);

    let sup = Arc::new(Supervisor::new(config()?, LOGS_DIR));
    let (mut actor, curator) = clients(&sup).await?;
    let gate: Gate = Arc::new(tokio::sync::Semaphore::new(1));
    let workers = present.iter().map(|h| spawn_worker(h, &curator, &gate)).collect();
    let mut scene = Scene { world, speaker, present, workers, history: Vec::new(), last_context: String::new(), moment };
    let names = |s: &Scene| s.present.iter().map(|h| h.meta().name).collect::<Vec<_>>().join(", ");

    println!("{}", tr!(l, "cli.chat.start", world = scene.world.memory.meta().name, people = names(&scene)));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        eprint!("\n{}: ", i18n::t(l, "cli.chat.you"));
        // Ctrl+C leaves like /exit: memory is saved and the GPU freed.
        let line = tokio::select! {
            l = lines.next_line() => match l {
                Ok(Some(l)) => l,
                _ => break,
            },
            _ = tokio::signal::ctrl_c() => {
                eprintln!();
                break;
            }
        };
        let line = line.trim().to_owned();
        if line.is_empty() {
            continue;
        }
        if let Some(cmd) = line.strip_prefix('/') {
            let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
            match name {
                "exit" | "quit" | "salir" => break,
                "memory" | "memoria" => println!("{}", if scene.last_context.is_empty() { i18n::t(l, "cli.chat.no_turn") } else { &scene.last_context }),
                "speak" | "habla" => match scene.present.iter().find(|h| memory_core::text::fold(&h.meta().name) == memory_core::text::fold(rest.trim())) {
                    Some(h) => {
                        scene.speaker = h.clone();
                        println!("{}", tr!(l, "cli.chat.now_speaks", name = h.meta().name));
                    }
                    None => println!("{}", tr!(l, "cli.chat.not_in_scene", name = rest.trim())),
                },
                "with" | "con" => {
                    let mut new = vec![scene.speaker.clone()];
                    for n in rest.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                        match find_character(&scene.world, n) {
                            Ok(h) if h.id() != scene.speaker.id() => new.push(h),
                            Ok(_) => {}
                            Err(e) => println!("· {e}"),
                        }
                    }
                    for h in &new {
                        if !scene.workers.iter().any(|(id, ..)| *id == h.id()) {
                            scene.workers.push(spawn_worker(h, &curator, &gate));
                        }
                    }
                    scene.present = new;
                    println!("{}", tr!(l, "cli.chat.in_scene", people = names(&scene)));
                }
                "image" | "imagen" => {
                    match portrait(&sup, &scene.world, &scene.speaker, rest).await {
                        Ok(msg) => println!("{msg}"),
                        Err(e) => println!("· {e}"),
                    }
                    // The actor may have been unloaded to make room: it reloads on the next turn.
                    actor = clients(&sup).await?.0;
                }
                _ => println!("{}", i18n::t(l, "cli.chat.unknown_command")),
            }
            continue;
        }

        let me = scene.speaker.meta();
        let present_ids: Vec<GraphId> = scene.present.iter().map(|h| h.id()).collect();
        if sup.ensure(Slot::Actor).await.is_err() {
            actor = clients(&sup).await?.0;
        }
        let started = std::time::Instant::now();
        print!("\n{}: ", me.name);
        let turn = SceneTurn {
            world: &scene.world,
            speaker: me.id,
            present: &present_ids,
            user_name: &user,
            history: &scene.history,
            text: &line,
            window: WINDOW,
            // Console scenes are not saved conversations: no chat memory.
            chat: None,
            moment: scene.moment,
            collab: None,
        };
        let stream = |t: &str| {
            use std::io::Write;
            print!("{t}");
            let _ = std::io::stdout().flush();
        };
        let reply = match scene_turn(&actor, turn, stream).await {
            Ok((reply, recall)) => {
                scene.last_context = recall.context;
                reply
            }
            Err(e) => {
                println!("· {}", tr!(l, "chat.actor_error", error = e));
                continue;
            }
        };
        println!();
        eprintln!("  ({:.1} s)", started.elapsed().as_secs_f32());
        scene.history.push((user.clone(), line.clone()));
        scene.history.push((me.name.clone(), reply.clone()));
        let turn = TurnRecord { lines: vec![(user.clone(), line), (me.name.clone(), reply)], story: scene.world.story_now(scene.moment) };
        for (id, w, _) in &scene.workers {
            if present_ids.contains(id) && !w.submit(turn.clone()) {
                eprintln!("  · {}", i18n::t(l, "chat.queue_full"));
            }
        }
    }

    eprintln!("{}", i18n::t(l, "cli.chat.saving"));
    for (_, w, j) in scene.workers {
        drop(w);
        let _ = j.await;
    }
    scene.world.save_all().map_err(|e| e.to_string())?;
    sup.stop_all().await;
    Ok(())
}

// --------------------------------------------------------------------------- images

async fn portrait(sup: &Supervisor, world: &World, who: &MemoryHandler, scene: &str) -> Res<String> {
    let l = lang();
    let meta = who.meta();
    let appearance = world.appearance_prompt_for(meta.id, scene).map_err(|e| e.to_string())?;
    if appearance.trim().is_empty() {
        return Err(tr!(l, "cli.image.no_appearance", name = meta.name));
    }
    // Mood: the character's strongest emotion right now.
    let mood = who.read(|g| {
        g.nodes()
            .filter(|(_, n)| n.kind == NodeKind::Emotion)
            .max_by(|a, b| a.1.weight.total_cmp(&b.1.weight))
            .map(|(_, n)| tr!(l, "image.mood", emotion = n.label.to_lowercase()))
            .unwrap_or_default()
    });
    let style = sup.config().image.style.clone();
    let req = sup.portrait_request(&appearance, &mood, scene, &style).ok_or_else(|| tr!(l, "models.no_image_model"))?;
    let file = format!("{}-{}.png", slugify(&meta.name), unix_now());
    let out: PathBuf = world.images_dir().join(&file);
    eprintln!("{}", tr!(l, "cli.image.generating", width = req.width, height = req.height));
    let r = sup.generate_image(&req, &out).await.map_err(|e| e.to_string())?;
    if meta.avatar.is_none() {
        who.write(|g| g.meta.avatar = Some(format!("{}/{file}", memory_core::world::IMAGES_DIR)));
        who.save().map_err(|e| e.to_string())?;
    }
    let freed = if r.stopped.is_empty() { String::new() } else { tr!(l, "cli.image.freed", slots = format!("{:?}", r.stopped)) };
    Ok(tr!(l, "cli.image.done", path = r.path.display(), secs = format!("{:.0}", r.seconds), model = r.model, freed = freed))
}

async fn cmd_image(args: &[String]) -> Res<()> {
    let (world_spec, who) = match args {
        [w, c, ..] => (w, c),
        _ => return Err(tr!(lang(), "cli.usage.image")),
    };
    let world = open_world(world_spec)?;
    let h = find_character(&world, who)?;
    let sup = Supervisor::new(config()?, LOGS_DIR);
    println!("{}", portrait(&sup, &world, &h, &args[2..].join(" ")).await?);
    Ok(())
}

/// Shows the appearance, or replaces the fallback visual description with a text.
fn cmd_appearance(args: &[String]) -> Res<()> {
    let l = lang();
    let (world_spec, who) = match args {
        [w, c, ..] => (w, c),
        _ => return Err(tr!(l, "cli.usage.appearance")),
    };
    let world = open_world(world_spec)?;
    let h = find_character(&world, who)?;
    match args.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(text) => {
            h.write(|g| g.meta.appearance = text.to_owned());
            h.save().map_err(|e| e.to_string())?;
            println!("{}", tr!(l, "cli.appearance.updated", name = h.meta().name));
        }
        None => {
            let report = world.appearance_report(h.id()).map_err(|e| e.to_string())?;
            println!("{}", tr!(l, "cli.appearance.canonical", name = h.meta().name));
            for a in &report.canonical {
                println!("  · {} ({:.0}%): {}", a.label, a.weight * 100.0, a.content);
            }
            if report.canonical.is_empty() {
                println!("  {}", h.meta().appearance);
            }
            for (who, aspects) in &report.seen_by {
                println!("\n{}", tr!(l, "cli.appearance.seen_by", who = who));
                for a in aspects {
                    println!("  · {}: {}", a.label, a.content);
                }
            }
        }
    }
    Ok(())
}

/// Engine benchmark with synthetic characters (build it in release for real figures).
fn cmd_stress(args: &[String]) -> Res<()> {
    use memory_core::synth::{Rng, SynthSpec, synth_character};
    use std::time::Instant;
    let l = lang();
    let sizes: Vec<usize> = if args.is_empty() { vec![1_000, 10_000, 50_000] } else { args.iter().filter_map(|a| a.parse().ok()).collect() };
    let now = unix_now();
    let p = memory_core::ActivationParams::default();
    let header: Vec<&str> = i18n::t(l, "cli.stress.header").split('|').collect();
    let widths = [8, 8, 9, 9, 9, 9, 10, 10, 10];
    println!("{}", header.iter().zip(widths).map(|(h, w)| format!("{h:>w$}")).collect::<Vec<_>>().join(" "));
    for n in sizes {
        let t = Instant::now();
        let s = synth_character(&SynthSpec { nodes: n, seed: n as u64, now, ..Default::default() });
        let build = t.elapsed();
        let g = &s.graph;
        let t = Instant::now();
        let bytes = g.to_bytes().map_err(|e| e.to_string())?;
        let save = t.elapsed();
        let t = Instant::now();
        let back = memory_core::MemoryGraph::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let load = t.elapsed();
        assert_eq!(back.node_count(), g.node_count());

        let mut rng = Rng::new(7);
        let mut recall = Vec::new();
        let mut block = Vec::new();
        for i in 0..200 {
            let (_, name) = &s.entities[rng.zipf(s.entities.len())];
            let q = match i % 4 {
                0 => format!("What do you know about {name}?"),
                1 => format!("{name} and the letter at the harbour"),
                2 => "Do you remember what happened in the rain at the market?".to_owned(),
                _ => format!("Tell me something about {name}, before dawn"),
            };
            let t = Instant::now();
            let active = g.retrieve(&q, now, &p);
            recall.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            let _ = g.actor_block(&active, now, memory_core::ContextBudget::default(), l);
            block.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let pct = |v: &mut Vec<f64>, q: f64| {
            v.sort_by(|a, b| a.total_cmp(b));
            v[((v.len() - 1) as f64 * q) as usize]
        };
        println!(
            "{:>8} {:>8} {:>7.0}ms {:>7.0}ms {:>7.0}ms {:>7.1}MB {:>8.2}ms {:>8.2}ms {:>8.2}ms",
            g.node_count(),
            g.edge_count(),
            build.as_secs_f64() * 1000.0,
            save.as_secs_f64() * 1000.0,
            load.as_secs_f64() * 1000.0,
            bytes.len() as f64 / 1e6,
            pct(&mut recall, 0.5),
            pct(&mut recall, 0.95),
            pct(&mut block, 0.5),
        );
    }
    Ok(())
}

/// Copies reference images into the world and describes ALL of the character's at once
/// with the vision reader. `--dry-run` only shows the result (the canonical appearance
/// stays as it is); `--reader <fragment>` switches the reader model to compare.
async fn cmd_reference(args: &[String]) -> Res<()> {
    let l = lang();
    let usage = || tr!(l, "cli.usage.reference");
    let (world_spec, who) = match args {
        [w, c, ..] => (w, c),
        _ => return Err(usage()),
    };
    let (mut add, mut dry, mut reader, mut images) = (false, false, None, Vec::new());
    let mut rest = args[2..].iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--add" | "--añadir" | "--anadir" => add = true,
            "--dry-run" | "--prueba" => dry = true,
            "--reader" | "--lector" => reader = Some(rest.next().ok_or_else(usage)?.clone()),
            _ => images.push(a.clone()),
        }
    }
    let world = open_world(world_spec)?;
    let h = find_character(&world, who)?;
    for image in &images {
        let bytes = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
        let ext = Path::new(image).extension().and_then(|e| e.to_str()).unwrap_or("png");
        let saved = world.add_reference(h.id(), &bytes, ext).map_err(|e| e.to_string())?;
        println!("{}", tr!(l, "cli.reference.saved", path = saved.display()));
    }
    let refs = world.references(h.id()).map_err(|e| e.to_string())?;
    if refs.is_empty() {
        return Err(tr!(l, "reference.none", name = h.meta().name));
    }
    let images_b64 = refs.iter().map(|p| std::fs::read(p).map(|b| base64_encode(&b)).map_err(|e| format!("{}: {e}", p.display()))).collect::<Res<Vec<_>>>()?;

    let mut cfg = config()?;
    if let Some(m) = reader {
        cfg.reader.model = m;
    }
    let sup = Supervisor::new(cfg, LOGS_DIR);
    let spec = sup.spec(Slot::Reader).map_err(|e| e.to_string())?;
    if spec.mmproj.is_none() {
        return Err(tr!(l, "reference.no_vision", model = spec.alias));
    }
    eprintln!("{}", tr!(l, "cli.reference.loading", model = spec.alias));
    let url = sup.ensure(Slot::Reader).await.map_err(|e| e.to_string())?;
    if let Some(why) = sup.vision_error(Slot::Reader) {
        sup.stop_all().await;
        return Err(tr!(l, "reference.mmproj_failed", why = why));
    }
    let vision = LlmClient::new(LlmEndpoint::actor(Backend::LlamaServer, url, spec.alias.clone())).map_err(|e| e.to_string())?;
    let hint = h.read(|g| g.node(g.root()).map(|n| n.content.clone()).unwrap_or_default());
    eprintln!("{}", tr!(l, "cli.reference.describing", n = images_b64.len()));
    let started = std::time::Instant::now();
    let aspects = memory_core::llm::describe_appearance(&vision, l, &images_b64, &h.meta().name, &hint).await;
    sup.stop_all().await;
    let aspects = aspects.map_err(|e| e.to_string())?;
    if !dry {
        world.set_appearance(h.id(), &aspects, !add).map_err(|e| e.to_string())?;
    }
    let dry_note = if dry { tr!(l, "cli.reference.dry") } else { String::new() };
    println!(
        "{}",
        tr!(
            l,
            "cli.reference.result",
            n = aspects.len(),
            images = images_b64.len(),
            secs = format!("{:.0}", started.elapsed().as_secs_f32()),
            model = spec.alias,
            dry = dry_note
        )
    );
    for a in &aspects {
        println!("  · {} ({:.0}%): {}", a.label, a.weight * 100.0, a.content);
    }
    Ok(())
}

/// Standard base64 (no extra dependencies).
fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
