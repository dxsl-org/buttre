//! Tray icon management for buttre application

use crate::shared::ui::menu::MethodMenuItem;
use crate::shared::ui::{
    load_icon_from_bytes, CUSTOM_ICON_BYTES, ENGLISH_ICON_BYTES, NOM_ICON_BYTES, TELEX_ICON_BYTES,
    VIETNAMESE_ICON_BYTES, VNI_ICON_BYTES,
};
use anyhow::Result;
use buttre_core::state::Settings;
use buttre_core::vietnamese::config_loader::MethodMetadata;
use buttre_core::vietnamese::get_custom_dir;
use std::fs;
use tray_icon::{Icon as TrayIcon, TrayIconBuilder};

/// Create tray icon with the given menu and initial settings
/// Returns the tray icon and pre-loaded icon resources
pub fn create_tray_icon(
    menu: &muda::Menu,
    settings: &Settings,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
) -> Result<(
    tray_icon::TrayIcon,
    TrayIcon, // telex_icon
    TrayIcon, // vni_icon
    TrayIcon, // english_icon
    TrayIcon, // nom_icon
    TrayIcon, // custom_icon
)> {
    // Load all icons
    let telex_icon = load_icon_from_bytes(TELEX_ICON_BYTES)
        .unwrap_or_else(|_| TrayIcon::from_rgba(vec![0, 0, 0, 0], 1, 1).unwrap());

    let vni_icon = load_icon_from_bytes(VNI_ICON_BYTES)
        .unwrap_or_else(|_| TrayIcon::from_rgba(vec![0, 0, 0, 0], 1, 1).unwrap());

    let english_icon = load_icon_from_bytes(ENGLISH_ICON_BYTES)
        .unwrap_or_else(|_| TrayIcon::from_rgba(vec![0, 0, 0, 0], 1, 1).unwrap());

    let nom_icon = load_icon_from_bytes(NOM_ICON_BYTES)
        .unwrap_or_else(|_| TrayIcon::from_rgba(vec![0, 0, 0, 0], 1, 1).unwrap());

    let custom_icon = load_icon_from_bytes(CUSTOM_ICON_BYTES)
        .unwrap_or_else(|_| TrayIcon::from_rgba(vec![0, 0, 0, 0], 1, 1).unwrap());

    // Fallback Vietnamese icon (for unknown methods)
    let vietnamese_icon = load_icon_from_bytes(VIETNAMESE_ICON_BYTES)
        .unwrap_or_else(|_| TrayIcon::from_rgba(vec![0, 0, 0, 0], 1, 1).unwrap());

    // Determine initial tooltip and icon
    let initial_tooltip = get_tooltip(&settings.input_method, custom_items);
    let initial_icon = get_icon_for_method(
        &settings.input_method,
        custom_items,
        &telex_icon,
        &vni_icon,
        &english_icon,
        &nom_icon,
        &custom_icon,
        &vietnamese_icon,
    );

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_tooltip(&initial_tooltip)
        .with_icon(initial_icon)
        .build()?;

    Ok((
        tray_icon,
        telex_icon,
        vni_icon,
        english_icon,
        nom_icon,
        custom_icon,
    ))
}

/// Get tooltip text for a given input method
pub fn get_tooltip(method: &str, custom_items: &[(MethodMetadata, MethodMenuItem)]) -> String {
    match method {
        "english" => "buttre\nEnglish".to_string(),
        "telex" => "buttre\nChữ Việt\nTELEX".to_string(),
        "vni" => "buttre\nChữ Việt\nVNI".to_string(),
        "nom" => "buttre\nChữ Nôm".to_string(),
        method_id => {
            if let Some((data, _)) = custom_items.iter().find(|(d, _)| d.id == method_id) {
                format!("buttre\nCustom\n{}", data.name)
            } else {
                // Fallback
                format!("buttre\nChữ Việt\n{}", method_id.to_uppercase())
            }
        }
    }
}

/// Get the appropriate icon for a given input method
///
/// One parameter per method icon — see `helpers::update_tray_icon`'s doc for
/// why this isn't grouped into a struct in this cleanup pass.
#[allow(clippy::too_many_arguments)]
pub fn get_icon_for_method(
    method: &str,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
    telex_icon: &TrayIcon,
    vni_icon: &TrayIcon,
    english_icon: &TrayIcon,
    nom_icon: &TrayIcon,
    custom_icon: &TrayIcon,
    vietnamese_icon: &TrayIcon, // fallback
) -> TrayIcon {
    match method {
        "english" => english_icon.clone(),
        "telex" => telex_icon.clone(),
        "vni" => vni_icon.clone(),
        "nom" => nom_icon.clone(),
        method_id => {
            // Check if it's a custom method with an icon
            if let Some((data, _)) = custom_items.iter().find(|(d, _)| d.id == method_id) {
                if let Some(icon_path_str) = &data.icon {
                    let icon_path = get_custom_dir().join(icon_path_str);
                    if let Ok(bytes) = fs::read(&icon_path) {
                        if let Ok(loaded_icon) = load_icon_from_bytes(&bytes) {
                            return loaded_icon;
                        }
                    }
                }
            }

            // Use custom icon if it's a custom method, otherwise Vietnamese fallback
            if custom_items.iter().any(|(d, _)| d.id == method_id) {
                custom_icon.clone()
            } else {
                vietnamese_icon.clone()
            }
        }
    }
}
