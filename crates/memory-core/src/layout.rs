//! Folder layout of a Chats home and the move from the old (Spanish) names.
//!
//! ```text
//! <home>/
//! ├─ config/models.toml    model configuration (model-hub)
//! ├─ config/app.toml       preferences: language
//! ├─ models/               llm/, vision/, mmproj/, diffusion/, bin/
//! ├─ worlds/<category>/<world>/
//! │     world.mem, characters/*.mem, images/, references/, chats/
//! ├─ seeds/                world seeds (.toml)
//! └─ logs/
//! ```

use std::fs;
use std::path::{Path, PathBuf};

pub const MODELS_CONFIG: &str = "config/models.toml";
pub const APP_CONFIG: &str = "config/app.toml";
pub const MODELS_DIR: &str = "models";
pub const WORLDS_DIR: &str = "worlds";
pub const SEEDS_DIR: &str = "seeds";
pub const LOGS_DIR: &str = "logs";
/// Under `logs/`: the curators' rejected operations and errors.
pub const CURATOR_LOG: &str = "curator.log";

/// Old name → new name, relative to the home folder.
const LEGACY_HOME: &[(&str, &str)] = &[
    ("mundos", WORLDS_DIR),
    ("semillas", SEEDS_DIR),
    ("config/modelos.toml", MODELS_CONFIG),
    ("models/difusion", "models/diffusion"),
    ("models/fusion", "models/diffusion"),
];

/// Old name → new name inside each world folder.
const LEGACY_WORLD: &[(&str, &str)] = &[
    ("mundo.mem", crate::world::WORLD_FILE),
    ("personajes", crate::world::CHARACTERS_DIR),
    ("imagenes", crate::world::IMAGES_DIR),
    ("referencias", crate::world::REFERENCES_DIR),
];

/// Old prefix of the avatar paths stored in the `.mem` files.
pub(crate) const LEGACY_IMAGES_PREFIX: &str = "imagenes/";

/// Renames the old folders and files to the new names, only where the new one does not
/// exist yet (nothing is ever overwritten). Returns what was moved.
pub fn migrate_legacy(home: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut moved = Vec::new();
    for (old, new) in LEGACY_HOME {
        rename_if_free(&home.join(old), &home.join(new), &mut moved);
    }
    let worlds = home.join(WORLDS_DIR);
    for category in fs::read_dir(&worlds).into_iter().flatten().flatten() {
        for world in fs::read_dir(category.path()).into_iter().flatten().flatten() {
            if world.path().is_dir() {
                moved.extend(migrate_world_dir(&world.path()));
            }
        }
    }
    moved
}

/// Same as [`migrate_legacy`], for a single world folder.
pub fn migrate_world_dir(dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut moved = Vec::new();
    for (old, new) in LEGACY_WORLD {
        rename_if_free(&dir.join(old), &dir.join(new), &mut moved);
    }
    moved
}

fn rename_if_free(old: &Path, new: &Path, moved: &mut Vec<(PathBuf, PathBuf)>) {
    if old.exists() && !new.exists() {
        if let Some(parent) = new.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::rename(old, new).is_ok() {
            moved.push((old.to_owned(), new.to_owned()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_spanish_layout_is_moved_without_overwriting() {
        let home = std::env::temp_dir().join(format!("memory-core-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let world = home.join("mundos/arknights/terra");
        fs::create_dir_all(world.join("personajes")).unwrap();
        fs::create_dir_all(world.join("imagenes")).unwrap();
        fs::write(world.join("mundo.mem"), b"x").unwrap();
        fs::create_dir_all(home.join("config")).unwrap();
        fs::write(home.join("config/modelos.toml"), b"old").unwrap();
        fs::write(home.join("config/models.toml"), b"new").unwrap();

        let moved = migrate_legacy(&home);
        let terra = home.join("worlds/arknights/terra");
        assert!(terra.join("world.mem").is_file() && terra.join("characters").is_dir() && terra.join("images").is_dir(), "{moved:?}");
        assert_eq!(fs::read(home.join("config/models.toml")).unwrap(), b"new", "an existing new file is never overwritten");
        assert!(home.join("config/modelos.toml").is_file());
        assert!(migrate_legacy(&home).is_empty(), "running it twice does nothing");
    }
}
