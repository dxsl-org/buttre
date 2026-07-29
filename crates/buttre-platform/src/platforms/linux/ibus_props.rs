//! IBus property-menu wire format — `IBusPropList` / `IBusProperty`.
//!
//! The IBus panel renders an engine's properties as a menu; under GNOME the
//! Shell IS that panel, so this is how buttre's method switcher reaches the
//! top-bar input-source menu (the same mechanism ibus-bamboo / ibus-unikey
//! use). The radio list is DYNAMIC: the four built-ins plus every custom
//! keyboard TOML found in the custom dir at build time (see
//! [`method_items`]) — mind the first-register freeze below when reasoning
//! about customs added mid-session.
//!
//! ## Serialization contract
//!
//! A struct field whose Rust type is [`Value`] always serializes as a D-Bus
//! variant (`v`), because `Value`'s own signature is `"v"` — so `label`,
//! `tooltip`, `sub_props`, and `symbol` come out as the `v` libibus expects,
//! exactly like the `attr_list` field in `ibus::build_ibus_text`. Fixed-count
//! fields (`s`, `u`, `b`, `a{sv}`) keep their natural signatures. The daemon
//! subscribes by signature and silently drops a mismatch, so the field ORDER
//! and types below must match libibus's `ibus_property_serialize` /
//! `ibus_prop_list_serialize` byte for byte.
//!
//! ## Panel repaint contract (GNOME Shell) — DO NOT BREAK THIS FLOW
//!
//! GNOME Shell consumes an engine's `RegisterProperties` exactly ONCE per
//! global-engine activation: `ibusManager.js` (`_engineChanged`) installs a
//! one-shot `register-properties` handler and DISCONNECTS it after the first
//! non-empty list. Every wholesale re-register after that is delivered by the
//! daemon but silently ignored by the Shell — the top-bar radio never
//! repaints. Radio-state changes only reach the menu through per-property
//! `UpdateProperty` signals (`keyboard.js::_ibusPropertyUpdated` matches
//! key + prop type, then rebuilds the section). Verified on GNOME Shell 50.1
//! (Ubuntu 26.04) with dbus-monitor: the old register-only approach arrived
//! on the bus with correct checked-states and still never repainted.
//!
//! Therefore EVERY method switch must go through
//! `ButtreEngine::publish_method_props` (full register for the daemon's
//! cache + one [`method_prop_updates`] `UpdateProperty` per radio), from all
//! three trigger paths: the external-change refresh task (`ibus_bus`), the
//! per-keystroke `sync_method`, and `PropertyActivate`. Do not "simplify"
//! any of them back to a bare `RegisterProperties` — it will look correct in
//! a bus trace and still leave the GNOME menu stale.

use super::ibus::build_ibus_text;
use std::collections::HashMap;
use zbus::zvariant::{StructureBuilder, Value};

/// `IBusPropType` values (ibusproperty.h) used by the menu.
const PROP_TYPE_NORMAL: u32 = 0;
const PROP_TYPE_RADIO: u32 = 2;
const PROP_TYPE_SEPARATOR: u32 = 4;
/// `IBusPropState` — a radio is drawn checked (`1`) or unchecked (`0`). The
/// checked value is also what `PropertyActivate` carries for the radio the user
/// just selected; the engine keys off it to ignore the group's de-select
/// notifications (see `ibus::ButtreEngine::property_activate`).
const PROP_STATE_UNCHECKED: u32 = 0;
pub(crate) const PROP_STATE_CHECKED: u32 = 1;

/// Property key of the "Cấu hình" (settings) item, echoed back in
/// `PropertyActivate`. Distinct from any method id so the engine opens the
/// config window instead of switching method.
pub(crate) const CONFIG_KEY: &str = "config";

/// Built-in methods surfaced as radios, in display order. `key` doubles as
/// the engine method id (see `method_sync::KNOWN_METHODS`) and the property
/// name libibus echoes back in `PropertyActivate`, so a click maps straight
/// to a method without a lookup table. "English" is the passthrough method
/// (engine goes silent, OS input source untouched — the tray's "English"
/// item and this radio are the same Store-B state); it sits FIRST to mirror
/// the tray menu's order, so the two surfaces read identically.
const BUILTIN_METHOD_ITEMS: [(&str, &str); 4] = [
    ("english", "English"),
    ("telex", "Telex"),
    ("vni", "VNI"),
    ("nom", "Chữ Nôm"),
];

/// The full `(key, label)` radio list: built-ins first, then every custom
/// keyboard TOML in the custom dir, in directory scan order — the same order
/// the tray's registry scan produces, so the two surfaces read identically.
///
/// The key is the TOML's FILENAME STEM, not `metadata.id`: the engine loads
/// `keyboards/{key}.toml` by stem (`KeyboardManager::set_method`), so a stem
/// key is the only one guaranteed to round-trip. Labels come from
/// `metadata.name` (a TOML that fails to parse is excluded — its radio could
/// only ever fall back to English).
///
/// Rebuilt on every publish, but note the GNOME first-register freeze
/// (module docs): the panel renders the radio SET it saw on the engine's
/// first `RegisterProperties`, so a custom TOML added mid-session appears
/// only after an engine restart (`ibus restart`). Matches Windows, which has
/// no keyboard hot-reload either.
fn method_items() -> Vec<(String, String)> {
    method_items_in(&buttre_core::vietnamese::get_custom_dir())
}

/// [`method_items`] against an explicit custom dir (testable core —
/// `get_custom_dir()` is ambient: exe dir / cwd / XDG data dir).
fn method_items_in(custom_dir: &std::path::Path) -> Vec<(String, String)> {
    let mut items: Vec<(String, String)> = BUILTIN_METHOD_ITEMS
        .iter()
        .map(|(key, label)| (key.to_string(), label.to_string()))
        .collect();
    let Ok(entries) = std::fs::read_dir(custom_dir) else {
        return items; // no custom dir — built-ins only
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if BUILTIN_METHOD_ITEMS.iter().any(|(key, _)| *key == stem) {
            continue; // built-in override TOMLs are not separate methods
        }
        if stem == CONFIG_KEY || stem == "-" {
            // A radio named "config" would be swallowed by the settings
            // launcher's PropertyActivate branch; "-" is the separator key.
            tracing::warn!(
                "ibus_props: keyboard filename {stem:?} collides with a menu key, skipping"
            );
            continue;
        }
        if stem != stem.to_lowercase() {
            // method_sync lowercases on read — an uppercase stem could be
            // rendered but never round-trip. Same rule as is_valid_custom_id.
            tracing::warn!("ibus_props: keyboard filename {stem:?} must be lowercase, skipping");
            continue;
        }
        match buttre_core::Config::load(path.to_str().unwrap_or_default()) {
            Ok(config) => items.push((stem.to_string(), config.metadata.name.clone())),
            Err(e) => {
                tracing::debug!("ibus_props: skipping unloadable keyboard {path:?}: {e}");
            }
        }
    }
    items
}

/// One `IBusProperty`. Wire: `(sa{sv} s u v s v b b u v v)` = name, attachments,
/// key, type, label, icon, tooltip, sensitive, visible, state, sub_props,
/// symbol. A field typed as `Value` serializes as the `v` variant libibus
/// expects (see the module contract). `state` is ignored by the panel for
/// non-radio types, so NORMAL/SEPARATOR items pass `PROP_STATE_UNCHECKED`.
fn build_property(key: &str, prop_type: u32, label: &str, state: u32) -> Value<'static> {
    let attachments: HashMap<String, Value<'static>> = HashMap::new();
    let structure = StructureBuilder::new()
        .add_field("IBusProperty".to_string()) // serializable type name
        .add_field(attachments) //               attachments a{sv}
        .add_field(key.to_string()) //            key
        .add_field(prop_type) //                  type
        .add_field(build_ibus_text(label)) //     label (v)
        .add_field(String::new()) //              icon (empty)
        .add_field(build_ibus_text("")) //        tooltip (v)
        .add_field(true) //                       sensitive
        .add_field(true) //                       visible
        .add_field(state) //                      state
        .add_field(empty_prop_list()) //          sub_props (v) — leaf
        .add_field(build_ibus_text("")) //        symbol (v)
        .build();
    Value::from(structure)
}

/// An empty `IBusPropList` — even a leaf property carries one for `sub_props`.
fn empty_prop_list() -> Value<'static> {
    prop_list(Vec::new())
}

/// Wrap radios in an `IBusPropList`. Wire: `(sa{sv} av)` = name, attachments,
/// array-of-variant (each an `IBusProperty`).
fn prop_list(props: Vec<Value<'static>>) -> Value<'static> {
    let attachments: HashMap<String, Value<'static>> = HashMap::new();
    Value::from(("IBusPropList".to_string(), attachments, props))
}

/// The menu property list: one radio per method (with `current` checked), a
/// separator, then the "Cấu hình" launcher. Pass the result to the engine's
/// `RegisterProperties` signal.
///
/// `current` is a method id (`"telex"`/`"vni"`/`"nom"`); an unknown value simply
/// leaves every radio unchecked rather than failing.
pub(crate) fn method_prop_list(current: &str) -> Value<'static> {
    let mut props = method_prop_updates(current);
    // Divider, then the settings launcher. The launcher is a NORMAL item, not
    // part of the radio group, so its click is routed by key (not check-state).
    props.push(build_property(
        "-",
        PROP_TYPE_SEPARATOR,
        "",
        PROP_STATE_UNCHECKED,
    ));
    props.push(build_property(
        CONFIG_KEY,
        PROP_TYPE_NORMAL,
        "Cấu hình Buttre...",
        PROP_STATE_UNCHECKED,
    ));
    prop_list(props)
}

/// One `UpdateProperty` payload per method radio, with `current` checked.
///
/// Why per-property updates exist alongside [`method_prop_list`]: GNOME Shell
/// consumes an engine's `RegisterProperties` ONCE per global-engine change —
/// `ibusManager.js` disconnects its `register-properties` handler after the
/// first non-empty list — so a later wholesale re-register never repaints the
/// top-bar radio. Per-property `UpdateProperty` signals are the channel the
/// Shell keeps open permanently (`_ibusPropertyUpdated` matches key + type and
/// repaints). Verified against GNOME Shell 50.1 sources on Ubuntu.
///
/// The whole radio group is emitted (checked AND unchecked) so the panel never
/// shows two checked radios, whatever state it held before.
pub(crate) fn method_prop_updates(current: &str) -> Vec<Value<'static>> {
    method_prop_updates_with(current, &method_items())
}

/// [`method_prop_updates`] over an explicit item list (testable core).
fn method_prop_updates_with(current: &str, items: &[(String, String)]) -> Vec<Value<'static>> {
    items
        .iter()
        .map(|(key, label)| {
            let state = if key == current {
                PROP_STATE_CHECKED
            } else {
                PROP_STATE_UNCHECKED
            };
            build_property(key, PROP_TYPE_RADIO, label, state)
        })
        .collect()
}

/// Resolve a `PropertyActivate(name, state)` from the panel to the method the
/// engine should switch to, or `None` to ignore the event.
///
/// A radio-group click arrives as MULTIPLE activations: the selected radio with
/// `state == PROP_STATE_CHECKED`, plus every other radio with an UNCHECKED
/// state. Only the checked one — and only if its key is a method the engine
/// can build (built-in or present custom TOML, the same
/// `method_sync::is_engine_method_in` rule the sync channel enforces) — is a
/// real switch; acting on the unchecked de-selects would overwrite the user's
/// actual choice with whichever notification arrives last.
pub(crate) fn method_for_activation(name: &str, state: u32) -> Option<&str> {
    method_for_activation_in(name, state, &buttre_core::vietnamese::get_custom_dir())
}

/// [`method_for_activation`] against an explicit custom dir (testable core).
/// The activation `name` is attacker-influenceable panel input interpolated
/// into a path downstream — `is_engine_method_in` carries the traversal guard.
fn method_for_activation_in<'a>(
    name: &'a str,
    state: u32,
    custom_dir: &std::path::Path,
) -> Option<&'a str> {
    if state == PROP_STATE_CHECKED && super::method_sync::is_engine_method_in(name, custom_dir) {
        Some(name)
    } else {
        None
    }
}

/// `IBusOrientation::IBUS_ORIENTATION_SYSTEM` — let the panel pick
/// horizontal/vertical. Signed (`i`) in the wire format, unlike the `u` fields.
const ORIENTATION_SYSTEM: i32 = 2;
/// Candidates per page. The panel shows this many at once and pages the rest;
/// number labels "1".."9" address slots within the current page.
pub(crate) const LOOKUP_PAGE_SIZE: u32 = 9;

/// Build an `IBusLookupTable` (the candidate popup the panel renders). Wire:
/// `(sa{sv} u u b b i av av)` = name, attachments, page_size, cursor_pos,
/// cursor_visible, round, orientation, candidates (av of IBusText), labels (av
/// of IBusText). The daemon subscribes by signature, so field order/types must
/// match libibus's `ibus_lookup_table_serialize` exactly — note `orientation`
/// is signed `i`, every other integer is `u`.
///
/// The FULL candidate list is sent; `cursor` (a global index) plus `page_size`
/// let the panel page and highlight. Labels are a fixed "1".."9" applied per
/// page, matching number-key and `CandidateClicked` selection in the engine.
pub(crate) fn build_lookup_table(candidates: &[String], cursor: u32) -> Value<'static> {
    let attachments: HashMap<String, Value<'static>> = HashMap::new();
    let cand_texts: Vec<Value<'static>> = candidates.iter().map(|c| build_ibus_text(c)).collect();
    let labels: Vec<Value<'static>> = (1..=LOOKUP_PAGE_SIZE)
        .map(|i| build_ibus_text(&i.to_string()))
        .collect();
    let structure = StructureBuilder::new()
        .add_field("IBusLookupTable".to_string()) // serializable type name
        .add_field(attachments) //                   attachments a{sv}
        .add_field(LOOKUP_PAGE_SIZE) //              page_size (u)
        .add_field(cursor) //                        cursor_pos (u)
        .add_field(true) //                          cursor_visible (b)
        .add_field(true) //                          round (b)
        .add_field(ORIENTATION_SYSTEM) //            orientation (i, signed)
        .add_field(cand_texts) //                    candidates (av of IBusText)
        .add_field(labels) //                        labels (av of IBusText)
        .build();
    Value::from(structure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{to_bytes, EncodingContext};

    #[test]
    fn activation_honors_only_a_checked_known_radio() {
        // The newly-selected radio switches methods...
        assert_eq!(
            method_for_activation("telex", PROP_STATE_CHECKED),
            Some("telex")
        );
        assert_eq!(
            method_for_activation("vni", PROP_STATE_CHECKED),
            Some("vni")
        );
        assert_eq!(
            method_for_activation("nom", PROP_STATE_CHECKED),
            Some("nom")
        );
        // ...but the de-select half of the same click MUST be ignored, else it
        // overwrites the selection (the original "always lands on VNI" bug).
        assert_eq!(method_for_activation("vni", PROP_STATE_UNCHECKED), None);
        assert_eq!(method_for_activation("telex", PROP_STATE_UNCHECKED), None);
        // English is a real radio (passthrough method) since the tri-surface
        // sync — a checked click switches to it like any other method.
        assert_eq!(
            method_for_activation("english", PROP_STATE_CHECKED),
            Some("english")
        );
        assert_eq!(method_for_activation("english", PROP_STATE_UNCHECKED), None);
        // Non-method keys are ignored: the settings launcher is handled by the
        // engine as a config-open, never as a method switch.
        assert_eq!(method_for_activation(CONFIG_KEY, PROP_STATE_CHECKED), None);
        assert_eq!(method_for_activation("french", PROP_STATE_CHECKED), None);
    }

    /// libibus subscribes to the property signal BY SIGNATURE and silently
    /// drops a mismatch (GNOME then shows nothing) — so a drift in the outer
    /// `IBusPropList` shape must fail loudly here. `(sa{sv}av)` = type-name,
    /// attachments, array-of-variant properties.
    #[test]
    fn method_prop_list_has_ibus_proplist_signature() {
        assert_eq!(
            method_prop_list("telex").value_signature().to_string(),
            "(sa{sv}av)"
        );
    }

    /// An owned item list: the four built-ins plus one custom entry — the
    /// shape `method_items()` produces with one custom TOML present. Injected
    /// so tests stay hermetic whatever the machine's real custom dir holds.
    fn items_with_custom() -> Vec<(String, String)> {
        let mut items: Vec<(String, String)> = BUILTIN_METHOD_ITEMS
            .iter()
            .map(|(k, l)| (k.to_string(), l.to_string()))
            .collect();
        items.push(("cham".to_string(), "Cham".to_string()));
        items
    }

    /// `UpdateProperty` carries ONE `IBusProperty` — same 12-field shape as the
    /// list elements. The daemon subscribes by signature and silently drops a
    /// mismatch, so drift must fail loudly here. `(sa{sv}suvsvbbuvv)`.
    #[test]
    fn method_prop_updates_have_ibus_property_signature() {
        let items = items_with_custom();
        let updates = method_prop_updates_with("vni", &items);
        assert_eq!(updates.len(), items.len(), "one update per method radio");
        for prop in &updates {
            assert_eq!(prop.value_signature().to_string(), "(sa{sv}suvsvbbuvv)");
        }
    }

    /// Exactly the `current` radio is checked — including a CUSTOM current —
    /// the rest explicitly unchecked so the panel can never end up with two
    /// checked radios.
    #[test]
    fn method_prop_updates_check_only_current() {
        let items = items_with_custom();
        for (current, expect_idx) in [
            ("english", 0usize),
            ("telex", 1),
            ("vni", 2),
            ("nom", 3),
            ("cham", 4),
        ] {
            let updates = method_prop_updates_with(current, &items);
            for (i, prop) in updates.iter().enumerate() {
                let Value::Structure(s) = prop else {
                    panic!("IBusProperty must be a structure")
                };
                let state = state_field(s);
                let expected = if i == expect_idx {
                    PROP_STATE_CHECKED
                } else {
                    PROP_STATE_UNCHECKED
                };
                assert_eq!(state, expected, "radio {i} state for current={current}");
            }
        }
    }

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

    /// A throwaway custom dir for the scan tests.
    fn tmp_custom_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("buttre-ibus-props-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Customs append after the built-ins with the stem as key and
    /// `metadata.name` as label; unparseable TOMLs and built-in-stem
    /// overrides are excluded.
    #[test]
    fn method_items_appends_valid_customs_after_builtins() {
        let dir = tmp_custom_dir("scan");
        std::fs::write(dir.join("cham.toml"), MINIMAL_KEYBOARD_TOML).unwrap();
        std::fs::write(dir.join("broken.toml"), "not = valid = toml").unwrap();
        std::fs::write(dir.join("telex.toml"), MINIMAL_KEYBOARD_TOML).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        // Colliding / non-lowercase stems are excluded (see method_items_in).
        std::fs::write(dir.join("config.toml"), MINIMAL_KEYBOARD_TOML).unwrap();
        std::fs::write(dir.join("Upper.toml"), MINIMAL_KEYBOARD_TOML).unwrap();

        let items = method_items_in(&dir);
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
        let items = method_items_in(std::path::Path::new("/nonexistent/keyboards"));
        assert_eq!(items.len(), BUILTIN_METHOD_ITEMS.len());
    }

    /// A checked activation for a custom key switches only when its TOML
    /// exists; traversal keys never resolve (panel names are untrusted).
    #[test]
    fn activation_admits_custom_ids_with_guard() {
        let dir = tmp_custom_dir("activate");
        std::fs::write(dir.join("cham.toml"), MINIMAL_KEYBOARD_TOML).unwrap();
        assert_eq!(
            method_for_activation_in("cham", PROP_STATE_CHECKED, &dir),
            Some("cham")
        );
        assert_eq!(
            method_for_activation_in("cham", PROP_STATE_UNCHECKED, &dir),
            None
        );
        assert_eq!(
            method_for_activation_in("khmer", PROP_STATE_CHECKED, &dir),
            None
        );
        assert_eq!(
            method_for_activation_in("../cham", PROP_STATE_CHECKED, &dir),
            None
        );
    }

    /// Field 9 of the 12-field IBusProperty struct is `state` (see
    /// `build_property`'s field order).
    fn state_field(s: &zbus::zvariant::Structure<'_>) -> u32 {
        match s.fields()[9] {
            Value::U32(state) => state,
            ref other => panic!("state field must be u32, got {other:?}"),
        }
    }

    /// The lookup table has its own libibus signature that must not drift, or
    /// the panel drops it and no candidates render. `(sa{sv}uubbiavav)`.
    #[test]
    fn lookup_table_has_ibus_lookuptable_signature() {
        let table = build_lookup_table(&["𡗶".to_string(), "天".to_string()], 0);
        assert_eq!(table.value_signature().to_string(), "(sa{sv}uubbiavav)");
    }

    /// The whole table — including the two `av` IBusText arrays — must encode,
    /// for an empty list, a small list, and one larger than a page (capped).
    #[test]
    fn lookup_table_encodes_for_any_size() {
        let ctxt = EncodingContext::<byteorder::LE>::new_dbus(0);
        let many: Vec<String> = (0..15).map(|i| format!("c{i}")).collect();
        // include a cursor on a later page to exercise paged cursor_pos.
        for (list, cursor) in [(Vec::new(), 0u32), (vec!["x".to_string()], 0), (many, 12)] {
            let table = build_lookup_table(&list, cursor);
            let bytes = to_bytes(ctxt, &table).expect("IBusLookupTable must encode");
            assert!(!bytes.is_empty(), "encoded lookup table must not be empty");
        }
    }

    /// The whole nested tree — the 12-field `IBusProperty` (`suvsvbbuvv` after
    /// the serializable header) inside each `av` element — must encode to
    /// D-Bus bytes without error. A wrong field type or count in
    /// `build_property` surfaces here rather than as an invisible empty menu.
    #[test]
    fn method_prop_list_encodes_for_every_current() {
        let ctxt = EncodingContext::<byteorder::LE>::new_dbus(0);
        for current in ["telex", "vni", "nom", "english", ""] {
            let props = method_prop_list(current);
            let bytes = to_bytes(ctxt, &props).expect("IBusPropList must encode as valid D-Bus");
            assert!(!bytes.is_empty(), "encoded property list must not be empty");
        }
    }
}
