//! A world's look in the chat: accent colour, light or dark, typeface and background
//! texture. It lives in `worlds/<category>/<world>/style.toml`, a small settings file like
//! `config/app.toml`: it can be edited by hand or set by a seed, and it does not touch the
//! `.mem` format. A world without the file uses the default look.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const STYLE_FILE: &str = "style.toml";
pub const DEFAULT_ACCENT: &str = "#e5a93c";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StyleMode {
    #[default]
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StyleFont {
    #[default]
    Serif,
    Sans,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StyleTexture {
    #[default]
    None,
    Grain,
    Paper,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct WorldStyle {
    /// `#rrggbb`.
    pub accent: String,
    pub mode: StyleMode,
    pub font: StyleFont,
    pub texture: StyleTexture,
}

impl Default for WorldStyle {
    fn default() -> Self {
        Self { accent: DEFAULT_ACCENT.into(), mode: StyleMode::default(), font: StyleFont::default(), texture: StyleTexture::default() }
    }
}

fn is_hex_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl WorldStyle {
    /// The same style with an invalid accent replaced by the default one.
    pub fn normalized(mut self) -> Self {
        self.accent = self.accent.trim().to_ascii_lowercase();
        if !is_hex_colour(&self.accent) {
            self.accent = DEFAULT_ACCENT.into();
        }
        self
    }

    /// The style of the world in `dir`. A missing or damaged file gives the default.
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(STYLE_FILE))
            .ok()
            .and_then(|s| toml::from_str::<WorldStyle>(&s).ok())
            .unwrap_or_default()
            .normalized()
    }

    /// Writes `style.toml` in `dir` (through a temporary file, so it is never half-written).
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let text = toml::to_string(&self.clone().normalized()).map_err(std::io::Error::other)?;
        let tmp = dir.join(format!("{STYLE_FILE}.tmp"));
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, dir.join(STYLE_FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_roundtrip_defaults_and_bad_values() {
        let dir = std::env::temp_dir().join(format!("memory-core-style-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(WorldStyle::load(&dir), WorldStyle::default(), "no file: the default look");

        let s = WorldStyle { accent: "#38BDF8".into(), mode: StyleMode::Light, font: StyleFont::Sans, texture: StyleTexture::Paper };
        s.save(&dir).unwrap();
        let back = WorldStyle::load(&dir);
        assert_eq!(back.accent, "#38bdf8");
        assert_eq!((back.mode, back.font, back.texture), (StyleMode::Light, StyleFont::Sans, StyleTexture::Paper));

        // Hand-edited file: unknown keys are ignored, missing ones take the default, and a
        // bad colour falls back to the default accent.
        std::fs::write(dir.join(STYLE_FILE), "accent = \"red\"\nmode = \"light\"\nextra = 1\n").unwrap();
        let hand = WorldStyle::load(&dir);
        assert_eq!((hand.accent.as_str(), hand.mode, hand.font), (DEFAULT_ACCENT, StyleMode::Light, StyleFont::Serif));
        std::fs::write(dir.join(STYLE_FILE), "not toml at all [").unwrap();
        assert_eq!(WorldStyle::load(&dir), WorldStyle::default());
    }
}
