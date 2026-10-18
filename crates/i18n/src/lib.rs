//! Dictionary for Chats. Everything a person reads (app, CLI, status messages) and every
//! prompt sent to the models comes from here, in English or Spanish. Code, identifiers and
//! comments are English; only these files carry other languages.
//!
//! - `dictionary.toml`: short strings grouped in sections. A leaf is a table with one
//!   string per language: `new = { en = "New chat", es = "Nuevo chat" }` under `[chat]`
//!   is the key `chat.new`. Placeholders are written `{name}`.
//! - `prompts/<lang>/*.md`: the long prompts (memory curator, vision reader).
//!
//! Adding a language means adding a [`Lang`] variant, a column to every entry and a
//! `prompts/<code>/` folder; the tests below fail until every entry has it.

use std::collections::HashMap;
use std::fmt::Display;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Es];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
        }
    }

    /// The language's name in itself, for language pickers.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }

    /// Accepts codes and names: `en`, `en-US`, `english`, `es`, `es_MX`, `español`, `spanish`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_lowercase();
        let base = s.split(['-', '_', '.']).next().unwrap_or("");
        match base {
            "en" | "english" | "ingles" | "inglés" => Some(Lang::En),
            "es" | "spanish" | "espanol" | "español" | "castellano" => Some(Lang::Es),
            _ => None,
        }
    }

    /// Position in [`Lang::ALL`] (stable: usable to store the language in an atomic).
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

const DICTIONARY: &str = include_str!("../dictionary.toml");

type Table = HashMap<String, [String; Lang::ALL.len()]>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| parse_dictionary(DICTIONARY).unwrap_or_else(|e| panic!("dictionary.toml: {e}")))
}

/// Flattens the sections into `section.sub.key → [en, es]`.
fn parse_dictionary(text: &str) -> Result<Table, String> {
    let root: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut out = Table::new();
    flatten(&root, "", &mut out)?;
    Ok(out)
}

fn flatten(t: &toml::Table, prefix: &str, out: &mut Table) -> Result<(), String> {
    for (k, v) in t {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        let toml::Value::Table(inner) = v else { return Err(format!("{key}: expected a table")) };
        if inner.contains_key(Lang::En.code()) {
            let mut row: [String; Lang::ALL.len()] = Default::default();
            for lang in Lang::ALL {
                row[lang.index()] = inner
                    .get(lang.code())
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| format!("{key}: missing \"{}\"", lang.code()))?
                    .to_owned();
            }
            if let Some(extra) = inner.keys().find(|k| Lang::parse(k).is_none()) {
                return Err(format!("{key}: unknown language \"{extra}\""));
            }
            if out.insert(key.clone(), row).is_some() {
                return Err(format!("{key}: defined twice"));
            }
        } else {
            flatten(inner, &key, out)?;
        }
    }
    Ok(())
}

/// The text for `key`. A missing key returns the key itself, so a gap shows up on
/// screen instead of crashing (and the tests catch it before that).
pub fn t(lang: Lang, key: &str) -> &'static str {
    match table().get(key) {
        Some(row) => row[lang.index()].as_str(),
        None => {
            debug_assert!(false, "missing dictionary key: {key}");
            // Leaked once per missing key and call; only reachable in release builds.
            Box::leak(key.to_owned().into_boxed_str())
        }
    }
}

pub fn has(key: &str) -> bool {
    table().contains_key(key)
}

/// All keys, sorted.
pub fn keys() -> Vec<&'static str> {
    let mut k: Vec<&'static str> = table().keys().map(String::as_str).collect();
    k.sort_unstable();
    k
}

/// Every entry in one language (what the UI loads at startup).
pub fn entries(lang: Lang) -> HashMap<&'static str, &'static str> {
    table().iter().map(|(k, row)| (k.as_str(), row[lang.index()].as_str())).collect()
}

/// Replaces `{name}` placeholders. Unknown placeholders are left as they are.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let value = after
            .find('}')
            .and_then(|end| args.iter().find(|(name, _)| *name == &after[..end]).map(|(_, v)| (end, v.to_string())));
        match value {
            Some((end, v)) => {
                out.push_str(&v);
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Placeholder names in a template (`{name}`), in order of appearance.
pub fn placeholders(template: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if after[..end].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && end > 0 => {
                out.push(&after[..end]);
                rest = &after[end + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// Translated text as an owned `String`, with optional `{name}` arguments:
/// `tr!(lang, "chat.typing", name = speaker)`.
#[macro_export]
macro_rules! tr {
    ($lang:expr, $key:expr $(,)?) => {
        $crate::t($lang, $key).to_owned()
    };
    ($lang:expr, $key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::fill($crate::t($lang, $key), &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+])
    };
}

/// Long prompts kept as Markdown files per language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// System prompt of the memory curator. Placeholders: `{{char}}`, `{{root}}`.
    CuratorSystem,
    /// System prompt of the vision reader that turns reference images into aspects.
    AppearanceReader,
    /// System prompt of the curator of a chat's memory. Placeholders: `{{chat}}`, `{{root}}`.
    ChatCuratorSystem,
}

impl Prompt {
    pub const ALL: [Prompt; 3] = [Prompt::CuratorSystem, Prompt::AppearanceReader, Prompt::ChatCuratorSystem];
}

pub fn prompt(lang: Lang, p: Prompt) -> &'static str {
    match (lang, p) {
        (Lang::En, Prompt::CuratorSystem) => include_str!("../prompts/en/curator_system.md"),
        (Lang::Es, Prompt::CuratorSystem) => include_str!("../prompts/es/curator_system.md"),
        (Lang::En, Prompt::AppearanceReader) => include_str!("../prompts/en/appearance_reader.md"),
        (Lang::Es, Prompt::AppearanceReader) => include_str!("../prompts/es/appearance_reader.md"),
        (Lang::En, Prompt::ChatCuratorSystem) => include_str!("../prompts/en/chat_curator_system.md"),
        (Lang::Es, Prompt::ChatCuratorSystem) => include_str!("../prompts/es/chat_curator_system.md"),
    }
}

// ---------------------------------------------------------------------- settings

/// `config/app.toml`: preferences shared by the app and the CLI.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Lang,
}

impl Settings {
    /// Reads the file; a missing or broken file gives the defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, format!("# Chats preferences. language: \"en\" or \"es\".\n{body}"))
    }
}

/// Language to use: the `CHATS_LANG` environment variable, then the settings file, then English.
pub fn resolve(settings_path: &Path) -> Lang {
    std::env::var("CHATS_LANG").ok().and_then(|v| Lang::parse(&v)).unwrap_or_else(|| Settings::load(settings_path).language)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn every_entry_is_complete_and_placeholders_match() {
        let table = parse_dictionary(DICTIONARY).unwrap();
        assert!(table.len() > 100, "{} entries", table.len());
        let mut problems = Vec::new();
        for (key, row) in &table {
            let base: BTreeSet<&str> = placeholders(&row[Lang::En.index()]).into_iter().collect();
            for lang in Lang::ALL {
                let text = &row[lang.index()];
                if text.trim().is_empty() {
                    problems.push(format!("{key} [{lang}] is empty"));
                }
                let mine: BTreeSet<&str> = placeholders(text).into_iter().collect();
                if mine != base {
                    problems.push(format!("{key} [{lang}] placeholders {mine:?} ≠ en {base:?}"));
                }
            }
        }
        problems.sort();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn prompts_exist_in_every_language_with_the_same_placeholders() {
        for p in Prompt::ALL {
            let en = prompt(Lang::En, p);
            for lang in Lang::ALL {
                let text = prompt(lang, p);
                assert!(text.len() > 200, "{p:?} [{lang}] looks empty");
                for marker in ["{{char}}", "{{chat}}", "{{root}}"] {
                    assert_eq!(en.contains(marker), text.contains(marker), "{p:?} [{lang}] {marker}");
                }
            }
        }
    }

    #[test]
    fn fill_and_parse() {
        assert_eq!(fill("{who} has {n} new memories", &[("who", &"Amiya"), ("n", &3)]), "Amiya has 3 new memories");
        assert_eq!(fill("keep {unknown} and {{json}}", &[]), "keep {unknown} and {{json}}");
        assert_eq!(placeholders("{a} {b_c} {{x}} {}"), ["a", "b_c", "x"]);
        assert_eq!(Lang::parse("es-MX"), Some(Lang::Es));
        assert_eq!(Lang::parse("English"), Some(Lang::En));
        assert_eq!(Lang::parse("fr"), None);
        assert_eq!(tr!(Lang::Es, "common.cancel"), "Cancelar");
        assert_eq!(tr!(Lang::En, "chat.typing", name = "Amiya"), "Amiya is typing…");
    }

    #[test]
    fn settings_roundtrip() {
        let path = std::env::temp_dir().join(format!("i18n-settings-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(Settings::load(&path).language, Lang::En);
        Settings { language: Lang::Es }.save(&path).unwrap();
        assert_eq!(Settings::load(&path).language, Lang::Es);
    }

    /// Every literal key used in the sources (`tr!(lang, "…")`, `i18n::t(lang, "…")` in
    /// Rust and `t("…")` in the UI) must exist in the dictionary.
    #[test]
    fn keys_used_in_the_sources_exist() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        for dir in ["crates", "src-tauri/src", "ui/src"] {
            walk(&root.join(dir), &mut files);
        }
        let mut missing = BTreeSet::new();
        let mut used = 0;
        for f in files {
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            let is_ts = f.extension().is_some_and(|e| e == "ts" || e == "tsx");
            let markers: &[&str] = if is_ts { &["t(\"", "tf(\""] } else { &["tr!(", "i18n::t("] };
            for marker in markers {
                for (pos, _) in text.match_indices(marker) {
                    // `t("` must not be the tail of another identifier (`set("`, `split("`).
                    if is_ts && text[..pos].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        continue;
                    }
                    let after = &text[pos + marker.len()..];
                    let literal = if is_ts {
                        after.split('"').next()
                    } else {
                        // Skip the language argument: the key is the first string literal.
                        after.split_once('"').filter(|(head, _)| head.len() < 80 && !head.contains(';')).and_then(|(_, tail)| tail.split('"').next())
                    };
                    if let Some(key) = literal.filter(|k| k.contains('.') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')) {
                        used += 1;
                        if !has(key) {
                            missing.insert(format!("{key} ({})", f.strip_prefix(&root).unwrap_or(&f).display()));
                        }
                    }
                }
            }
        }
        assert!(used > 100, "only {used} keys found: is the scanner broken?");
        assert!(missing.is_empty(), "missing keys:\n{}", missing.into_iter().collect::<Vec<_>>().join("\n"));
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if !p.ends_with("target") && !p.ends_with("node_modules") {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs" || x == "ts" || x == "tsx") {
                out.push(p);
            }
        }
    }
}
