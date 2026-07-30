//! UI update helper functions
//!
//! This module contains helper functions for updating menu and tray icons
//! to avoid code duplication in the main event loop.

use crate::shared::ui::menu::{set_method_checked, MethodMenuItem};
use crate::shared::ui::tray::{get_tooltip, TrayIcons};
use buttre_core::vietnamese::config_loader::MethodMetadata;
use muda::Submenu;
use tray_icon::TrayIcon as TrayIconType;

/// Reflect the active method on every menu row — exactly one row ends up
/// checked. Each row's state is SET (not just the active one), so a stale
/// check can never survive a switch whatever state the menu host held.
///
/// NOTE: muda's `Submenu` has no check state, so the "Chữ Việt" parent stays
/// unmarked (text-prefix hacks were explicitly rejected).
#[allow(clippy::too_many_arguments)] // reason: mirrors MenuItems' field-per-row shape; a row-group struct is a later cleanup
pub fn update_menu_checkmarks(
    method: &str,
    enabled: bool,
    enable_item: &MethodMenuItem,
    _chu_viet_menu: &Submenu,
    telex_item: &MethodMenuItem,
    vni_item: &MethodMenuItem,
    nom_item: &MethodMenuItem,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
) {
    // OFF renders as: "Bật bộ gõ" unchecked, no method checked — a method
    // checkmark must never claim the IME is typing Vietnamese while it is not.
    // Method rows stay CLICKABLE so the user can pick the method to come back
    // on with (picking one re-enables — see `select_method` in main.rs).
    set_method_checked(enable_item, enabled);
    set_method_checked(telex_item, enabled && method == "telex");
    set_method_checked(vni_item, enabled && method == "vni");
    set_method_checked(nom_item, enabled && method == "nom");
    for (data, item) in custom_items {
        set_method_checked(item, enabled && data.id == method);
    }
}

/// Update tray icon and tooltip for the current state.
///
/// OFF shows the CHOSEN method's icon in greyscale (see `TrayIcons::for_method`
/// for why not a separate "off icon"). Skips the `set_icon` call entirely when
/// the state hasn't changed — repainting the tray flickers visibly on Windows,
/// and observers may re-emit the same state (idempotent commands, ADR-0003).
pub fn update_tray_icon(
    method: &str,
    enabled: bool,
    tray_icon: &mut TrayIconType,
    icons: &TrayIcons,
    last_state: &mut Option<(String, bool)>,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
) {
    let state = (method.to_string(), enabled);
    if last_state.as_ref() == Some(&state) {
        return;
    }
    *last_state = Some(state);

    let icon = icons
        .for_method(method, custom_items)
        .variant(enabled)
        .clone();
    let _ = tray_icon.set_icon(Some(icon));
    let _ = tray_icon.set_tooltip(Some(get_tooltip(method, enabled, custom_items)));
}
