//! One IBus engine per input method (ADR-0005) — the IBus counterpart of the
//! TSF profile table (`windows/tsf/profiles.rs`) and the macOS input modes.
//!
//! Each method — Telex, VNI, Chữ Nôm, every keyboard TOML — is its own entry
//! in the desktop's input-source list (GNOME Settings, `ibus-setup`), picked
//! with Super+Space like any other source. The engine NAME is the method: the
//! factory derives it in [`method_for_engine`], nothing reads the method file.
//!
//! The list reaches ibus-daemon through `<engines exec="buttre --ibus-engines"/>`
//! in the component XML: the daemon runs the command whenever it rebuilds its
//! registry cache and parses the `<engines>` document [`engines_xml`] prints,
//! so a keyboard TOML added later appears after the next cache rebuild
//! (`ibus write-cache` / re-login) without editing the installed XML.
//!
//! Engine names must never change once shipped: desktops record the user's
//! added sources by engine name (GNOME: `('ibus', '<name>')`).

use std::path::Path;

/// Telex keeps the name of the single engine buttre registered before this
/// table existed, so users who already added buttre keep their input source —
/// now as "buttre Telex", the method nearly all of them were typing with.
pub const TELEX_ENGINE: &str = "buttre";

/// Prefix of every other engine name: `buttre-vni`, `buttre-nom`, `buttre-<stem>`.
const ENGINE_PREFIX: &str = "buttre-";

/// One input source buttre offers IBus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEntry {
    /// IBus engine name (`buttre`, `buttre-vni`, …).
    pub name: String,
    /// Engine method id: `telex`, `vni`, `nom`, or a keyboard TOML's stem.
    pub method_id: String,
    /// Name shown in the desktop's input-source list.
    pub longname: String,
    /// Text GNOME draws in the top bar while the source is active — the only
    /// per-source indicator GNOME Shell renders there (a text label; the
    /// engine `<icon>` shows only in Settings / `ibus-setup` lists), so the
    /// built-ins spell the method out instead of an abbreviation.
    pub symbol: String,
    /// Icon theme name (hicolor, shipped by the package).
    pub icon: &'static str,
    /// Sort order in `ibus-setup` (higher first).
    pub rank: u32,
}

/// The IBus engine name of a method id.
pub fn engine_name(method_id: &str) -> String {
    if method_id == "telex" {
        TELEX_ENGINE.to_string()
    } else {
        format!("{ENGINE_PREFIX}{method_id}")
    }
}

/// Every engine to offer: the built-in methods, then each keyboard TOML in
/// `keyboard_dirs`, in `method_menu` order.
pub fn engines_in<P: AsRef<Path>>(keyboard_dirs: &[P]) -> Vec<EngineEntry> {
    crate::shared::method_menu::method_items_in(keyboard_dirs)
        .into_iter()
        // "English" is the absence of buttre — the user's plain layout source.
        .filter(|(id, _)| id != "english")
        .map(|(id, label)| {
            let (symbol, icon, rank) = match id.as_str() {
                "telex" => ("TELEX".to_string(), "buttre-telex", 99),
                "vni" => ("VNI".to_string(), "buttre-vni", 98),
                "nom" => ("NÔM".to_string(), "buttre-nom", 97),
                _ => (custom_symbol(&id), "buttre-custom", 50),
            };
            EngineEntry {
                name: engine_name(&id),
                longname: format!("buttre {label}"),
                method_id: id,
                symbol,
                icon,
                rank,
            }
        })
        .collect()
}

/// [`engines_in`] for this installation's keyboard dirs.
pub fn engines() -> Vec<EngineEntry> {
    engines_in(&buttre_core::vietnamese::keyboard_dirs())
}

/// The method an engine name stands for, or `None` for a name this table
/// never issues. `name` comes from ibus-daemon (`CreateEngine`) and ends up in
/// a keyboard path, so customs go through `is_engine_method_in`'s traversal
/// guard; the legacy `"english"` id and the alias `buttre-telex` are rejected
/// so every method has exactly one engine name.
pub fn method_for_engine(name: &str) -> Option<String> {
    method_for_engine_in(name, &buttre_core::vietnamese::keyboard_dirs())
}

fn method_for_engine_in<P: AsRef<Path>>(name: &str, keyboard_dirs: &[P]) -> Option<String> {
    if name == TELEX_ENGINE {
        return Some("telex".to_string());
    }
    let id = name.strip_prefix(ENGINE_PREFIX)?;
    if id == "telex" || id == "english" {
        return None;
    }
    crate::shared::method_sync::is_engine_method_in(id, keyboard_dirs).then(|| id.to_string())
}

/// Top-bar symbol of a keyboard TOML: initials of its `_`-separated stem
/// (`cham_akhar_thrah` → `CAT`), or the first two letters of a one-word stem
/// (`khmer` → `KH`) — distinct for every keyboard buttre ships.
fn custom_symbol(stem: &str) -> String {
    let parts: Vec<&str> = stem.split('_').filter(|p| !p.is_empty()).collect();
    let symbol: String = if parts.len() > 1 {
        parts.iter().filter_map(|p| p.chars().next()).collect()
    } else {
        stem.chars().take(2).collect()
    };
    symbol.to_uppercase()
}

/// The `<engines>` document `buttre --ibus-engines` prints for ibus-daemon.
pub fn engines_xml() -> String {
    render_engines_xml(&engines())
}

fn render_engines_xml(entries: &[EngineEntry]) -> String {
    // No `<?xml?>` declaration: the daemon parses this as a fragment with
    // GMarkup (`ibus_xml_parse_buffer`), the same shape ibus-table's `--xml`
    // prints.
    let mut xml = String::from("<engines>\n");
    for e in entries {
        xml.push_str("  <engine>\n");
        for (tag, value) in [
            ("name", e.name.as_str()),
            ("language", "vi"),
            ("license", "GPL-3.0"),
            ("author", "buttre project"),
            ("icon", e.icon),
            // "default", not "us": with a fixed layout GNOME switches the
            // keyboard to US QWERTY whenever a buttre source is picked, so
            // Dvorak/Colemak/AZERTY users typed garbage. The engine works on
            // keyvals and needs no particular layout.
            ("layout", "default"),
            ("longname", e.longname.as_str()),
            ("description", e.longname.as_str()),
            ("symbol", e.symbol.as_str()),
        ] {
            xml.push_str(&format!("    <{tag}>{}</{tag}>\n", escape_xml(value)));
        }
        xml.push_str(&format!("    <rank>{}</rank>\n", e.rank));
        xml.push_str("  </engine>\n");
    }
    xml.push_str("</engines>\n");
    xml
}

/// Keyboard labels come from user-editable TOML (`metadata.name`).
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYBOARD_TOML: &str = "\
[metadata]
id = \"x\"
name = \"Cham <Akhar> & Thrah\"
language = \"cja\"

[transformations]

[tones]

[rules]
";

    /// A throwaway keyboards dir holding `{stems}.toml`, as a one-dir list.
    fn keyboards_dir(tag: &str, stems: &[&str]) -> Vec<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("buttre-ibus-engines-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for stem in stems {
            std::fs::write(dir.join(format!("{stem}.toml")), KEYBOARD_TOML).unwrap();
        }
        vec![dir]
    }

    #[test]
    fn every_offered_engine_resolves_back_to_its_method() {
        let dir = keyboards_dir("roundtrip", &["cham_akhar_thrah", "khmer"]);
        let entries = engines_in(&dir);
        let ids: Vec<&str> = entries.iter().map(|e| e.method_id.as_str()).collect();
        assert_eq!(ids, ["telex", "vni", "nom", "cham_akhar_thrah", "khmer"]);
        for e in &entries {
            assert_eq!(
                method_for_engine_in(&e.name, &dir).as_deref(),
                Some(e.method_id.as_str())
            );
        }
    }

    #[test]
    fn telex_keeps_the_legacy_engine_name() {
        let dir = keyboards_dir("legacy", &[]);
        assert_eq!(engines_in(&dir)[0].name, "buttre");
        assert_eq!(
            method_for_engine_in("buttre", &dir).as_deref(),
            Some("telex")
        );
        assert_eq!(method_for_engine_in("buttre-telex", &dir), None);
    }

    #[test]
    fn unknown_english_and_traversal_names_are_rejected() {
        let dir = keyboards_dir("reject", &["khmer"]);
        for name in [
            "buttre-english",
            "buttre-missing",
            "buttre-../khmer",
            "buttre-",
            "other",
            "xkb:us::eng",
        ] {
            assert_eq!(method_for_engine_in(name, &dir), None, "{name}");
        }
    }

    #[test]
    fn custom_symbols_are_distinct_for_shipped_keyboards() {
        let stems = [
            "cham",
            "cham_akhar_thrah",
            "hmong",
            "khmer",
            "muong_tay_nung",
            "taynguyen",
            "thai",
        ];
        let symbols: std::collections::HashSet<String> =
            stems.iter().map(|s| custom_symbol(s)).collect();
        assert_eq!(symbols.len(), stems.len(), "{symbols:?}");
        assert_eq!(custom_symbol("cham_akhar_thrah"), "CAT");
    }

    #[test]
    fn xml_escapes_keyboard_labels() {
        let dir = keyboards_dir("escape", &["cham"]);
        let xml = render_engines_xml(&engines_in(&dir));
        assert!(xml.contains("<longname>buttre Cham &lt;Akhar&gt; &amp; Thrah</longname>"));
        assert!(!xml.contains("<Akhar>"));
        assert_eq!(xml.matches("<engine>").count(), 4);
    }
}
