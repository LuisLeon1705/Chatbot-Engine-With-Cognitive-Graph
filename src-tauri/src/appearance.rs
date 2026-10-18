//! Characters' appearance: reference images, canonical (editable) aspects, how the
//! others see them, description with vision, and portraits.

use base64::Engine;
use i18n::tr;
use memory_core::llm::{Backend, LlmClient, LlmEndpoint};
use memory_core::world::{Aspect, World};
use memory_core::{NodeKind, unix_now};
use model_hub::Slot;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::state::{AppState, Res, emit_status, lang, parse_id};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenBy {
    by: String,
    aspects: Vec<Aspect>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    file: String,
    data_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceDto {
    canonical: Vec<Aspect>,
    seen_by: Vec<SeenBy>,
    references: Vec<Reference>,
    /// Visual prompt used for portraits.
    prompt: String,
    avatar: Option<String>,
    /// Non-blocking warning (e.g. vision is not available).
    warning: Option<String>,
}

fn data_url(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

fn build(world: &World, id: u64, warning: Option<String>) -> Res<AppearanceDto> {
    let report = world.appearance_report(id).map_err(|e| e.to_string())?;
    let h = world.character(id).map_err(|e| e.to_string())?;
    let avatar = h.meta().avatar.map(|rel| world.dir.join(rel)).and_then(|p| data_url(&p));
    Ok(AppearanceDto {
        canonical: report.canonical,
        seen_by: report.seen_by.into_iter().map(|(by, aspects)| SeenBy { by, aspects }).collect(),
        references: world
            .references(id)
            .map_err(|e| e.to_string())?
            .iter()
            .filter_map(|p| Some(Reference { file: p.file_name()?.to_string_lossy().into_owned(), data_url: data_url(p)? }))
            .collect(),
        prompt: world.appearance_prompt(id).map_err(|e| e.to_string())?,
        avatar,
        warning,
    })
}

#[tauri::command]
pub fn appearance(state: State<'_, AppState>, world: String, character: String) -> Res<AppearanceDto> {
    let w = state.world(&world)?;
    build(&w, parse_id(&character)?, None)
}

/// Saves the canonical aspects edited by hand (they replace the previous ones).
#[tauri::command]
pub fn set_appearance(state: State<'_, AppState>, world: String, character: String, aspects: Vec<Aspect>) -> Res<AppearanceDto> {
    let w = state.world(&world)?;
    let id = parse_id(&character)?;
    let clean: Vec<Aspect> = aspects
        .into_iter()
        .filter(|a| !a.label.trim().is_empty() && !a.content.trim().is_empty())
        .map(|a| Aspect { label: a.label.trim().to_owned(), content: a.content.trim().to_owned(), weight: a.weight.clamp(0.05, 1.0) })
        .collect();
    w.set_appearance(id, &clean, true).map_err(|e| e.to_string())?;
    build(&w, id, None)
}

/// Saves a reference image (repeated images are not duplicated).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddReference {
    world: String,
    character: String,
    file_name: String,
    data_base64: String,
}

#[tauri::command]
pub fn add_reference(state: State<'_, AppState>, req: AddReference) -> Res<AppearanceDto> {
    let AddReference { world, character, file_name, data_base64 } = req;
    let w = state.world(&world)?;
    let id = parse_id(&character)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| tr!(lang(), "reference.bad_image", error = e))?;
    let ext = std::path::Path::new(&file_name).extension().and_then(|e| e.to_str()).unwrap_or("png").to_owned();
    w.add_reference(id, &bytes, &ext).map_err(|e| e.to_string())?;
    build(&w, id, None)
}

/// Describes all of the character's references at once with the vision reader and
/// returns the aspects as a PROPOSAL: they are not saved until the user accepts them.
#[tauri::command]
pub async fn describe_references(app: AppHandle, state: State<'_, AppState>, world: String, character: String) -> Res<Vec<Aspect>> {
    let _turn = state.turn_lock.lock().await;
    let l = lang();
    let w = state.world(&world)?;
    let id = parse_id(&character)?;
    let h = w.character(id).map_err(|e| e.to_string())?;
    let name = h.meta().name;
    let refs = w.references(id).map_err(|e| e.to_string())?;
    if refs.is_empty() {
        return Err(tr!(l, "reference.none", name = name));
    }
    let images: Vec<String> = refs
        .iter()
        .map(|p| std::fs::read(p).map(|b| base64::engine::general_purpose::STANDARD.encode(b)))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let hint = h.read(|g| g.node(g.root()).map(|n| n.content.clone()).unwrap_or_default());

    let sup = &state.sup;
    let spec = sup.spec(Slot::Reader).map_err(|e| e.to_string())?;
    if spec.mmproj.is_none() {
        return Err(tr!(l, "reference.no_vision", model = spec.alias));
    }
    emit_status(&app, tr!(l, "reference.loading_reader", model = spec.alias), true);
    let result = async {
        let url = sup.ensure(Slot::Reader).await.map_err(|e| e.to_string())?;
        if let Some(why) = sup.vision_error(Slot::Reader) {
            return Err(tr!(l, "reference.mmproj_failed", why = why));
        }
        emit_status(&app, tr!(l, "reference.describing", n = images.len(), name = name), true);
        let vision = LlmClient::new(LlmEndpoint::actor(Backend::LlamaServer, url, spec.alias.clone())).map_err(|e| e.to_string())?;
        memory_core::llm::describe_appearance(&vision, l, &images, &name, &hint).await.map_err(|e| e.to_string())
    }
    .await;
    // A dedicated reader takes the actor's GPU: it unloads as soon as it is done.
    if sup.has_reader() {
        sup.stop(Slot::Reader).await;
    }
    emit_status(&app, "", false);
    result
}

#[tauri::command]
pub fn delete_reference(state: State<'_, AppState>, world: String, character: String, file: String) -> Res<AppearanceDto> {
    let w = state.world(&world)?;
    let id = parse_id(&character)?;
    // Only this character's reference files (no paths).
    let target = w.references(id).map_err(|e| e.to_string())?.into_iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy() == file));
    match target {
        Some(p) => std::fs::remove_file(p).map_err(|e| e.to_string())?,
        None => return Err(tr!(lang(), "reference.missing")),
    }
    build(&w, id, None)
}

/// Portrait with the canonical aspects, the dominant emotion and an optional scene.
#[tauri::command]
pub async fn generate_portrait(app: AppHandle, state: State<'_, AppState>, world: String, character: String, scene: String) -> Res<AppearanceDto> {
    let _turn = state.turn_lock.lock().await;
    let l = lang();
    let w = state.world(&world)?;
    let id = parse_id(&character)?;
    let h = w.character(id).map_err(|e| e.to_string())?;
    let meta = h.meta();
    let appearance = w.appearance_prompt_for(id, &scene).map_err(|e| e.to_string())?;
    if appearance.trim().is_empty() {
        return Err(tr!(l, "portrait.no_appearance", name = meta.name));
    }
    let mood = h.read(|g| {
        g.nodes()
            .filter(|(_, n)| n.kind == NodeKind::Emotion)
            .max_by(|a, b| a.1.weight.total_cmp(&b.1.weight))
            .map(|(_, n)| tr!(l, "image.mood", emotion = n.label.to_lowercase()))
            .unwrap_or_default()
    });
    let style = state.sup.config().image.style.clone();
    let req = state.sup.portrait_request(&appearance, &mood, &scene, &style).ok_or_else(|| tr!(l, "models.no_image_model"))?;
    let file = format!("{}-{}.png", memory_core::world::slugify(&meta.name), unix_now());
    emit_status(&app, tr!(l, "portrait.painting", name = meta.name), true);
    let out = w.images_dir().join(&file);
    let r = state.sup.generate_image(&req, &out).await.map_err(|e| {
        emit_status(&app, "", false);
        e.to_string()
    })?;
    h.write(|g| g.meta.avatar = Some(format!("{}/{file}", memory_core::world::IMAGES_DIR)));
    h.save().map_err(|e| e.to_string())?;
    emit_status(&app, tr!(l, "portrait.ready", secs = format!("{:.0}", r.seconds)), false);
    build(&w, id, None)
}
