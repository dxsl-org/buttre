//! Method list and method-choice persistence for OS-owned surfaces
//! (ADR-0003 `MethodOwner::Os`).
//!
//! [`method_items`] is the one method list every surface derives from — the
//! tray menu, the TSF profiles, the IBus engines (`ibus_engines`), the macOS
//! input modes. [`persist_command`] is the settings write behind an OS-surface
//! switch — an IBus engine becoming active and a macOS input-mode selection
//! record the same COMMAND (a method writes the method file and turns the IME
//! on), so the surfaces cannot drift.

use std::path::Path;

/// Key of the "Cấu hình" (settings) launcher on menus that carry one. Reserved:
/// a custom keyboard with this stem would be swallowed by the launcher.
pub const CONFIG_KEY: &str = "config";

/// Key used for separators on menus whose wire format needs one (IBus).
const SEPARATOR_KEY: &str = "-";

/// Built-in methods, in display order. `key` doubles as the engine method id
/// (see `method_sync::KNOWN_METHODS`), so a click maps straight to a method
/// without a lookup table. "English" is the passthrough command (engine goes
/// silent, OS input source untouched); it sits FIRST to mirror the tray
/// menu's order, so every surface reads identically.
pub const BUILTIN_METHOD_ITEMS: [(&str, &str); 4] = [
    ("english", "English"),
    ("telex", "Telex"),
    ("vni", "VNI"),
    ("nom", "Chữ Nôm"),
];

/// The full `(key, label)` list: built-ins first, then every custom keyboard
/// TOML in the custom dir, in directory scan order — the same order the
/// tray's registry scan produces.
///
/// The key is the TOML's FILENAME STEM, not `metadata.id`: the engine loads
/// `keyboards/{key}.toml` by stem (`KeyboardManager::set_method`), so a stem
/// key is the only one guaranteed to round-trip. Labels come from
/// `metadata.name` (a TOML that fails to parse is excluded — its item could
/// only ever fall back to Telex).
pub fn method_items() -> Vec<(String, String)> {
    method_items_in(&buttre_core::vietnamese::keyboard_dirs())
}

/// [`method_items`] against explicit keyboard dirs, highest priority first
/// (testable core — `keyboard_dirs()` is ambient). One item per stem; the
/// first dir holding it wins, like `find_keyboard_in`.
pub fn method_items_in<P: AsRef<Path>>(keyboard_dirs: &[P]) -> Vec<(String, String)> {
    let mut items: Vec<(String, String)> = BUILTIN_METHOD_ITEMS
        .iter()
        .map(|(key, label)| (key.to_string(), label.to_string()))
        .collect();
    for path in buttre_core::vietnamese::keyboard_files_in(keyboard_dirs) {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if BUILTIN_METHOD_ITEMS.iter().any(|(key, _)| *key == stem) {
            continue; // built-in override TOMLs are not separate methods
        }
        if stem == CONFIG_KEY || stem == SEPARATOR_KEY {
            tracing::warn!(
                "method_menu: keyboard filename {stem:?} collides with a menu key, skipping"
            );
            continue;
        }
        if stem != stem.to_lowercase() {
            // method_sync lowercases on read — an uppercase stem could be
            // rendered but never round-trip. Same rule as is_valid_custom_id.
            tracing::warn!("method_menu: keyboard filename {stem:?} must be lowercase, skipping");
            continue;
        }
        let Some(path_str) = path.to_str() else {
            continue;
        };
        match buttre_core::Config::load(path_str) {
            Ok(config) => items.push((stem.to_string(), config.metadata.name.clone())),
            Err(e) => {
                tracing::debug!("method_menu: skipping unloadable keyboard {path:?}: {e}");
            }
        }
    }
    items
}

/// Record a menu COMMAND in the settings store — `Settings::enabled`, plus the
/// picked `input_method` when given — so every engine instance AND every later
/// session converge (an OS-owned surface has no tray to mirror the switch into
/// settings.toml, and a later Wayland/Windows session boots from it). Callers
/// update their in-process `enabled` mirror themselves; the settings watcher
/// re-reads this write, a no-op by value. Blocking file I/O, click-frequency
/// only.
pub fn persist_command(on: bool, method: Option<&str>) {
    // Strict read: a settings.toml that exists but does not parse must refuse
    // this write — load-modify-save over the defaults `load()` degrades to
    // would rewrite the user's whole file over one typo. (`Ok(None)` = fresh
    // install, no file yet: proceed and MATERIALIZE it — the engine treats "no
    // file" as enabled, see `macro_sync::effective_enabled`, so an English
    // click whose value equals the unsaved default must still write or the
    // fresh-install rule would immediately overrule the user's click.)
    let (mut settings, file_exists) = match buttre_core::state::Settings::read_strict() {
        Ok(Some(s)) => (s, true),
        Ok(None) => (buttre_core::state::Settings::default(), false),
        Err(e) => {
            tracing::warn!("persist_command(on={on}, {method:?}): {e:?} — không ghi");
            return;
        }
    };
    let method_changed = method.is_some_and(|m| settings.input_method != m);
    if file_exists && settings.enabled == on && !method_changed {
        return;
    }
    settings.enabled = on;
    if let Some(m) = method {
        settings.input_method = m.to_string();
    }
    if let Err(e) = settings.save() {
        tracing::warn!("persist_command(on={on}, {method:?}): settings.toml save failed: {e:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal keyboard TOML `Config::load` accepts (`[rules]` has no serde
    /// default on the field, so the empty table must be present).
    const MINIMAL_KEYBOARD_TOML: &str = "\
[metadata]
id = \"cham\"
name = \"Cham Keyboard\"
language = \"cja\"

[transformations]

[tones]

[rules]
";

    fn tmp_custom_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("buttre-method-menu-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Customs append after the built-ins with the stem as key and
    /// `metadata.name` as label; unparseable TOMLs, built-in-stem overrides,
    /// reserved and non-lowercase stems are excluded.
    #[test]
    fn method_items_appends_valid_customs_after_builtins() {
        let dir = tmp_custom_dir("scan");
        std::fs::write(dir.join("cham.toml"), MINIMAL_KEYBOARD_TOML).unwrap();
        std::fs::write(dir.join("broken.toml"), "not = valid = toml").unwrap();
        std::fs::write(dir.join("telex.toml"), MINIMAL_KEYBOARD_TOML).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        std::fs::write(dir.join("config.toml"), MINIMAL_KEYBOARD_TOML).unwrap();
        std::fs::write(dir.join("Upper.toml"), MINIMAL_KEYBOARD_TOML).unwrap();

        let items = method_items_in(&[&dir]);
        let builtin_count = BUILTIN_METHOD_ITEMS.len();
        assert_eq!(
            items.len(),
            builtin_count + 1,
            "exactly one custom admitted"
        );
        for (i, (key, _)) in BUILTIN_METHOD_ITEMS.iter().enumerate() {
            assert_eq!(items[i].0, *key, "built-ins keep their order");
        }
        assert_eq!(
            items[builtin_count],
            ("cham".to_string(), "Cham Keyboard".to_string())
        );
    }

    /// Missing custom dir degrades to built-ins only — never an error.
    #[test]
    fn method_items_without_custom_dir_is_builtins_only() {
        let items = method_items_in(&[Path::new("/nonexistent/keyboards")]);
        assert_eq!(items.len(), BUILTIN_METHOD_ITEMS.len());
    }
}
