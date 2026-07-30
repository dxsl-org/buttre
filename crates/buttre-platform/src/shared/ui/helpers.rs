//! UI update helper functions
//!
//! This module contains helper functions for updating menu and tray icons
//! to avoid code duplication in the main event loop.

use crate::shared::ui::load_icon_from_bytes;
use crate::shared::ui::menu::{set_method_checked, MethodMenuItem};
use buttre_core::vietnamese::config_loader::MethodMetadata;
use buttre_core::vietnamese::get_custom_dir;
use muda::Submenu;
use std::fs;
use tray_icon::{Icon as TrayIcon, TrayIcon as TrayIconType};

/// Reflect the active method on every menu row — exactly one row ends up
/// checked. Each row's state is SET (not just the active one), so a stale
/// check can never survive a switch whatever state the menu host held.
///
/// NOTE: muda's `Submenu` has no check state, so the "Chữ Việt" parent stays
/// unmarked (text-prefix hacks were explicitly rejected).
#[allow(clippy::too_many_arguments)] // reason: same shape as update_tray_icon below — the item-group struct is phase 02's IconSet cleanup
pub fn update_menu_checkmarks(
    method: &str,
    enabled: bool,
    english_item: &MethodMenuItem,
    _chu_viet_menu: &Submenu,
    telex_item: &MethodMenuItem,
    vni_item: &MethodMenuItem,
    nom_item: &MethodMenuItem,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
) {
    // OFF renders as: "English" checked, no method checked — a method checkmark
    // must never claim the IME is typing Vietnamese while it is not. The items
    // stay CLICKABLE so the user can pick the method to come back on with
    // (picking one re-enables — see `select_method` in main.rs).
    set_method_checked(english_item, !enabled);
    set_method_checked(telex_item, enabled && method == "telex");
    set_method_checked(vni_item, enabled && method == "vni");
    set_method_checked(nom_item, enabled && method == "nom");
    for (data, item) in custom_items {
        set_method_checked(item, enabled && data.id == method);
    }
}

/// Update tray icon and tooltip for the given method
///
/// # Algorithm
/// 1. If disabled, show English icon
/// 2. Otherwise, show icon for the active method
/// 3. For custom methods, try to load custom icon from file
///
/// One parameter per method icon — grouping into a struct is possible but
/// out of scope for a lint cleanup (would ripple through every call site in
/// UI init code that isn't covered by an automated test).
#[allow(clippy::too_many_arguments)]
pub fn update_tray_icon(
    method: &str,
    enabled: bool,
    tray_icon: &mut TrayIconType,
    telex_icon: &TrayIcon,
    vni_icon: &TrayIcon,
    english_icon: &TrayIcon,
    nom_icon: &TrayIcon,
    custom_icon: &TrayIcon,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
) {
    if !enabled {
        let _ = tray_icon.set_icon(Some(english_icon.clone()));
        let _ = tray_icon.set_tooltip(Some("buttre\nOFF".to_string()));
        return;
    }

    match method {
        "telex" => {
            let _ = tray_icon.set_icon(Some(telex_icon.clone()));
            let _ = tray_icon.set_tooltip(Some("buttre\nChữ Việt\nTELEX".to_string()));
        }
        "vni" => {
            let _ = tray_icon.set_icon(Some(vni_icon.clone()));
            let _ = tray_icon.set_tooltip(Some("buttre\nChữ Việt\nVNI".to_string()));
        }
        "nom" => {
            let _ = tray_icon.set_icon(Some(nom_icon.clone()));
            let _ = tray_icon.set_tooltip(Some("buttre\nChữ Nôm".to_string()));
        }
        _ => {
            // Handle custom methods
            let mut custom_icon_loaded = false;
            let mut name = method.to_string();

            if let Some((data, _)) = custom_items.iter().find(|(d, _)| d.id == method) {
                name = data.name.clone();
                if let Some(icon_path_str) = &data.icon {
                    let icon_path = get_custom_dir().join(icon_path_str);
                    if let Ok(bytes) = fs::read(&icon_path) {
                        if let Ok(icon) = load_icon_from_bytes(&bytes) {
                            let _ = tray_icon.set_icon(Some(icon));
                            custom_icon_loaded = true;
                        }
                    }
                }
            }

            if !custom_icon_loaded {
                let _ = tray_icon.set_icon(Some(custom_icon.clone()));
            }

            let _ = tray_icon.set_tooltip(Some(format!("buttre\nCustom\n{}", name)));
        }
    }
}
