//! The dictionary for the UI and the language switch.

use std::collections::HashMap;

use i18n::Lang;
use serde::Serialize;
use tauri::State;

use crate::state::{AppState, Res, lang};

#[derive(Serialize)]
pub struct LanguageOption {
    code: &'static str,
    name: &'static str,
}

#[derive(Serialize)]
pub struct Bundle {
    lang: &'static str,
    languages: Vec<LanguageOption>,
    /// Every dictionary entry in the current language (the UI only uses `ui.*`, `kind.*`,
    /// `edge.*`, `time.short.*` and a few shared ones, but the table is small).
    entries: HashMap<&'static str, &'static str>,
}

fn bundle(l: Lang) -> Bundle {
    Bundle {
        lang: l.code(),
        languages: Lang::ALL.iter().map(|x| LanguageOption { code: x.code(), name: x.native_name() }).collect(),
        entries: i18n::entries(l),
    }
}

#[tauri::command]
pub fn i18n_bundle() -> Bundle {
    bundle(lang())
}

/// Switches the language of the app and the prompts, saves it and returns the new dictionary.
#[tauri::command]
pub fn set_language(state: State<'_, AppState>, code: String) -> Res<Bundle> {
    let l = Lang::parse(&code).ok_or_else(|| i18n::tr!(lang(), "cli.lang_unknown", value = code))?;
    state.set_lang(l)?;
    Ok(bundle(l))
}
