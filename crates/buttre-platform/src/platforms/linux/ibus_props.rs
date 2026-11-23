//! IBus wire formats the engine publishes to the panel — the property menu
//! (`IBusPropList` / `IBusProperty`) and the Nôm candidate popup
//! (`IBusLookupTable`).
//!
//! The property menu holds ONE item, "Cấu hình buttre…": the method is the
//! input source itself (one engine per method, `ibus_engines`, ADR-0005), so
//! the menu no longer carries method radios — "off" is picking another source.
//! Under GNOME the Shell is the panel and lists these items in the top-bar
//! input-source menu below the sources.
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
//! `ibus_prop_list_serialize` / `ibus_lookup_table_serialize` byte for byte.

use super::ibus::build_ibus_text;
use std::collections::HashMap;
use zbus::zvariant::{StructureBuilder, Value};

/// `IBusPropType::PROP_TYPE_NORMAL` (ibusproperty.h).
const PROP_TYPE_NORMAL: u32 = 0;
/// `IBusPropState` — meaningless for a NORMAL item; sent unchecked.
const PROP_STATE_UNCHECKED: u32 = 0;

/// Property key of the "Cấu hình" (settings) item, echoed back in
/// `PropertyActivate`.
pub(crate) use crate::shared::method_menu::CONFIG_KEY;

/// One `IBusProperty`. Wire: `(sa{sv} s u v s v b b u v v)` = name, attachments,
/// key, type, label, icon, tooltip, sensitive, visible, state, sub_props,
/// symbol. A field typed as `Value` serializes as the `v` variant libibus
/// expects (see the module contract).
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

/// Wrap properties in an `IBusPropList`. Wire: `(sa{sv} av)` = name,
/// attachments, array-of-variant (each an `IBusProperty`).
fn prop_list(props: Vec<Value<'static>>) -> Value<'static> {
    let attachments: HashMap<String, Value<'static>> = HashMap::new();
    Value::from(("IBusPropList".to_string(), attachments, props))
}

/// The engine's property menu: the "Cấu hình buttre…" launcher. Pass the
/// result to the engine's `RegisterProperties` signal.
pub(crate) fn config_prop_list() -> Value<'static> {
    prop_list(vec![build_property(
        CONFIG_KEY,
        PROP_TYPE_NORMAL,
        "Cấu hình buttre…",
        PROP_STATE_UNCHECKED,
    )])
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

    /// libibus subscribes to the property signal BY SIGNATURE and silently
    /// drops a mismatch (GNOME then shows nothing) — so a drift in the outer
    /// `IBusPropList` shape must fail loudly here. `(sa{sv}av)` = type-name,
    /// attachments, array-of-variant properties.
    #[test]
    fn config_prop_list_has_ibus_proplist_signature() {
        assert_eq!(
            config_prop_list().value_signature().to_string(),
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
        for (list, cursor) in [(Vec::new(), 0u32), (vec!["x".to_string()], 0), (many, 12)] {
            let table = build_lookup_table(&list, cursor);
            let bytes = to_bytes(ctxt, &table).expect("IBusLookupTable must encode");
            assert!(!bytes.is_empty(), "encoded lookup table must not be empty");
        }
    }

    /// The whole nested tree — the 12-field `IBusProperty` (`suvsvbbuvv` after
    /// the serializable header) inside the `av` — must encode to D-Bus bytes.
    /// A wrong field type or count in `build_property` surfaces here rather
    /// than as an invisible empty menu.
    #[test]
    fn config_prop_list_encodes() {
        let ctxt = EncodingContext::<byteorder::LE>::new_dbus(0);
        let bytes = to_bytes(ctxt, &config_prop_list()).expect("IBusPropList must encode");
        assert!(!bytes.is_empty());
    }
}
