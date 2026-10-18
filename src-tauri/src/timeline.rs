//! A world's timeline (its moments of the story) and the moments each character belongs to.

use memory_core::{CharacterMoment, DAYS_PER_YEAR, Moment, story_of_year};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::{AppState, Place, Res, parse_id};

/// A moment of the timeline. Story time is in in-world days; the UI works with year and day.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MomentDto {
    /// 0 = a new moment.
    #[serde(default)]
    pub id: u32,
    pub name: String,
    pub year: i64,
    #[serde(default)]
    pub day: i64,
    #[serde(default)]
    pub summary: String,
    /// Story time (sent by the backend, ignored when received).
    #[serde(default)]
    pub at: i64,
}

impl From<&Moment> for MomentDto {
    fn from(m: &Moment) -> Self {
        Self { id: m.id, name: m.name.clone(), year: m.at.div_euclid(DAYS_PER_YEAR), day: m.at.rem_euclid(DAYS_PER_YEAR), summary: m.summary.clone(), at: m.at }
    }
}

impl From<MomentDto> for Moment {
    fn from(m: MomentDto) -> Self {
        Self { id: m.id, name: m.name, at: story_of_year(m.year, m.day), summary: m.summary }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMomentDto {
    pub moment: u32,
    #[serde(default)]
    pub scenario: String,
}

impl From<&CharacterMoment> for CharacterMomentDto {
    fn from(m: &CharacterMoment) -> Self {
        Self { moment: m.moment, scenario: m.scenario.clone() }
    }
}

/// Replaces a world's timeline and returns it as saved (new moments get their id).
#[tauri::command]
pub fn set_timeline(state: State<'_, AppState>, world: String, moments: Vec<MomentDto>) -> Res<Vec<MomentDto>> {
    let w = state.world(&world)?;
    let saved = w.set_timeline(moments.into_iter().map(Into::into).collect()).map_err(|e| e.to_string())?;
    Ok(saved.iter().map(Into::into).collect())
}

/// Saves how the world's chats look (`style.toml`) and returns it as stored.
#[tauri::command]
/// Also a Collab's, with its `collab:<slug>` key.
pub fn set_world_style(state: State<'_, AppState>, world: String, style: memory_core::WorldStyle) -> Res<memory_core::WorldStyle> {
    match state.place(&world)? {
        Place::World(w) => {
            w.set_style(&style).map_err(|e| e.to_string())?;
            Ok(w.style())
        }
        Place::Collab(c, _) => {
            c.set_style(&style).map_err(|e| e.to_string())?;
            Ok(c.style())
        }
    }
}

/// The moments a character belongs to, each with its own scenario (empty = any moment).
#[tauri::command]
pub fn set_character_moments(state: State<'_, AppState>, world: String, character: String, moments: Vec<CharacterMomentDto>) -> Res<()> {
    let w = state.world(&world)?;
    let moments = moments.into_iter().map(|m| CharacterMoment { moment: m.moment, scenario: m.scenario }).collect();
    w.set_character_moments(parse_id(&character)?, moments).map_err(|e| e.to_string())
}
