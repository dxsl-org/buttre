//! IBus property-menu wire format — `IBusPropList` / `IBusProperty`.
//!
//! The IBus panel renders an engine's properties as a menu; under GNOME the
//! Shell IS that panel, so this is how buttre's method switcher reaches the
//! top-bar input-source menu (the same mechanism ibus-bamboo / ibus-unikey
//! use). This is the SPIKE surface: only the Telex/VNI radios, enough to
//! confirm GNOME Shell renders the list and round-trips `PropertyActivate`
//! before the full menu (Nôm, custom methods, toggles, "Cấu hình…") is built.
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

/// Input methods surfaced as radios, in display order. `key` doubles as the
/// engine method id (see `method_sync::KNOWN_METHODS`) and the property name
/// libibus echoes back in `PropertyActivate`, so a click maps straight to a
/// method without a lookup table. Nôm switches the keyboard, but note its
/// dictionary/candidate UI is not wired on Linux yet (see
/// `shared::engine_bridge::build_keyboard`).
const METHOD_ITEMS: [(&str, &str); 3] =
    [("telex", "Telex"), ("vni", "VNI"), ("nom", "Chữ Nôm")];

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
    let mut props: Vec<Value<'static>> = METHOD_ITEMS
        .iter()
        .map(|(key, label)| {
            let state = if *key == current {
                PROP_STATE_CHECKED
            } else {
                PROP_STATE_UNCHECKED
            };
            build_property(key, PROP_TYPE_RADIO, label, state)
        })
        .collect();
    // Divider, then the settings launcher. The launcher is a NORMAL item, not
    // part of the radio group, so its click is routed by key (not check-state).
    props.push(build_property("-", PROP_TYPE_SEPARATOR, "", PROP_STATE_UNCHECKED));
    props.push(build_property(
        CONFIG_KEY,
        PROP_TYPE_NORMAL,
        "Cấu hình",
        PROP_STATE_UNCHECKED,
    ));
    prop_list(props)
}

/// Resolve a `PropertyActivate(name, state)` from the panel to the method the
/// engine should switch to, or `None` to ignore the event.
///
/// A radio-group click arrives as MULTIPLE activations: the selected radio with
/// `state == PROP_STATE_CHECKED`, plus every other radio with an UNCHECKED
/// state. Only the checked one — and only if its key is a menu method — is a
/// real switch; acting on the unchecked de-selects would overwrite the user's
/// actual choice with whichever notification arrives last.
pub(crate) fn method_for_activation(name: &str, state: u32) -> Option<&str> {
    if state == PROP_STATE_CHECKED && METHOD_ITEMS.iter().any(|(key, _)| *key == name) {
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
        assert_eq!(method_for_activation("telex", PROP_STATE_CHECKED), Some("telex"));
        assert_eq!(method_for_activation("vni", PROP_STATE_CHECKED), Some("vni"));
        assert_eq!(method_for_activation("nom", PROP_STATE_CHECKED), Some("nom"));
        // ...but the de-select half of the same click MUST be ignored, else it
        // overwrites the selection (the original "always lands on VNI" bug).
        assert_eq!(method_for_activation("vni", PROP_STATE_UNCHECKED), None);
        assert_eq!(method_for_activation("telex", PROP_STATE_UNCHECKED), None);
        // Non-method keys are ignored: the settings launcher is handled by the
        // engine as a config-open, never as a method switch.
        assert_eq!(method_for_activation(CONFIG_KEY, PROP_STATE_CHECKED), None);
        assert_eq!(method_for_activation("english", PROP_STATE_CHECKED), None);
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
        for (list, cursor) in [
            (Vec::new(), 0u32),
            (vec!["x".to_string()], 0),
            (many, 12),
        ] {
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
            let bytes =
                to_bytes(ctxt, &props).expect("IBusPropList must encode as valid D-Bus");
            assert!(!bytes.is_empty(), "encoded property list must not be empty");
        }
    }
}
