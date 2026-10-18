//! Commands for Collabs: places where several worlds meet. Their chats and memories go
//! through the usual chat and memory commands with the `collab:<slug>` key.

use memory_core::collab::CollabLink;
use serde::Deserialize;
use tauri::State;

use crate::chat::{WorldDto, collab_dto};
use crate::state::{AppState, COLLAB_PREFIX, Res};

/// Every Collab, shaped like a world.
#[tauri::command]
pub fn list_collabs(state: State<'_, AppState>) -> Res<Vec<WorldDto>> {
    state.lib.collabs().iter().map(|(slug, _)| collab_dto(&state, slug)).collect()
}

/// Each linked world exists and its moment is one of its timeline's.
fn check_links(state: &AppState, links: &[CollabLink]) -> Res<()> {
    for l in links {
        let w = state.world(&l.world)?;
        if let Some(m) = l.moment {
            w.moment(m).ok_or_else(|| memory_core::WorldError::NoSuchMoment(m).to_string())?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCollab {
    name: String,
    /// How the worlds meet: the root of its memory.
    premise: String,
    links: Vec<CollabLink>,
}

#[tauri::command]
pub fn create_collab(state: State<'_, AppState>, req: NewCollab) -> Res<WorldDto> {
    check_links(&state, &req.links)?;
    let c = state.lib.create_collab(&req.name, &req.premise, req.links).map_err(|e| e.to_string())?;
    let slug = state.insert_collab(c).slug.clone();
    collab_dto(&state, &slug)
}

/// Which worlds a Collab links and at which moment of each.
#[tauri::command]
pub fn set_collab_links(state: State<'_, AppState>, collab: String, links: Vec<CollabLink>) -> Res<WorldDto> {
    let slug = collab.strip_prefix(COLLAB_PREFIX).unwrap_or(&collab);
    check_links(&state, &links)?;
    state.collab(slug)?.set_links(links).map_err(|e| e.to_string())?;
    collab_dto(&state, slug)
}
