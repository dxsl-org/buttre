//! Tray icon management for buttre application (Windows + Wayland-native — the
//! platforms where buttre owns method selection, ADR-0003).

use crate::shared::ui::menu::MethodMenuItem;
use crate::shared::ui::{
    IconSet, CUSTOM_ICON_BYTES, NOM_ICON_BYTES, TELEX_ICON_BYTES, VIETNAMESE_ICON_BYTES,
    VNI_ICON_BYTES,
};
use anyhow::Result;
use buttre_core::state::Settings;
use buttre_core::vietnamese::config_loader::MethodMetadata;
use buttre_core::vietnamese::get_custom_dir;
use std::fs;
use tray_icon::TrayIconBuilder;

/// Every icon the tray can show, each in its ON (colour) and OFF (greyscale)
/// variant. Built once at startup; owned by `main.rs` for the tray's lifetime.
pub struct TrayIcons {
    pub telex: IconSet,
    pub vni: IconSet,
    pub nom: IconSet,
    pub custom: IconSet,
    /// Fallback for method ids nothing else matches.
    pub vietnamese: IconSet,
}

impl TrayIcons {
    pub fn load() -> Self {
        Self {
            telex: IconSet::from_bytes(TELEX_ICON_BYTES),
            vni: IconSet::from_bytes(VNI_ICON_BYTES),
            nom: IconSet::from_bytes(NOM_ICON_BYTES),
            custom: IconSet::from_bytes(CUSTOM_ICON_BYTES),
            vietnamese: IconSet::from_bytes(VIETNAMESE_ICON_BYTES),
        }
    }

    /// The icon set for a method id. OFF is not a method: callers pick the
    /// variant with [`IconSet::variant`], so a disabled IME shows the CHOSEN
    /// method's icon in greyscale — the user still sees what they will get
    /// when they switch back on, and grey reads unambiguously as "off"
    /// (the old English icon read as "some other method is active").
    pub fn for_method(
        &self,
        method: &str,
        custom_items: &[(MethodMetadata, MethodMenuItem)],
    ) -> IconSet {
        match method {
            "telex" => self.telex.clone(),
            "vni" => self.vni.clone(),
            "nom" => self.nom.clone(),
            method_id => {
                // A custom method may ship its own icon file next to its TOML.
                if let Some((data, _)) = custom_items.iter().find(|(d, _)| d.id == method_id) {
                    if let Some(icon_path_str) = &data.icon {
                        let icon_path = get_custom_dir().join(icon_path_str);
                        if let Ok(bytes) = fs::read(&icon_path) {
                            return IconSet::from_bytes(&bytes);
                        }
                    }
                }
                if custom_items.iter().any(|(d, _)| d.id == method_id) {
                    self.custom.clone()
                } else {
                    self.vietnamese.clone()
                }
            }
        }
    }
}

/// Create the tray icon with the given menu and initial settings. Returns the
/// live tray handle plus the loaded icon sets.
///
/// `show_menu_on_left_click(false)`: left-click is the on/off toggle (the
/// single most frequent interaction an IME has), handled via `TrayIconEvent`
/// in `main.rs`'s event loop. The menu stays on right-click.
pub fn create_tray_icon(
    menu: &muda::Menu,
    settings: &Settings,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
) -> Result<(tray_icon::TrayIcon, TrayIcons)> {
    let icons = TrayIcons::load();

    let initial_tooltip = get_tooltip(&settings.input_method, settings.enabled, custom_items);
    let initial_icon = icons
        .for_method(&settings.input_method, custom_items)
        .variant(settings.enabled)
        .clone();

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(false)
        .with_tooltip(&initial_tooltip)
        .with_icon(initial_icon)
        .build()?;

    Ok((tray_icon, icons))
}

/// Tooltip for the current state. OFF still names the chosen method — the
/// tooltip answers "what will I get when I turn it back on".
pub fn get_tooltip(
    method: &str,
    enabled: bool,
    custom_items: &[(MethodMetadata, MethodMenuItem)],
) -> String {
    let method_label = match method {
        "telex" => "Chữ Việt\nTELEX".to_string(),
        "vni" => "Chữ Việt\nVNI".to_string(),
        "nom" => "Chữ Nôm".to_string(),
        method_id => {
            if let Some((data, _)) = custom_items.iter().find(|(d, _)| d.id == method_id) {
                format!("Custom\n{}", data.name)
            } else {
                format!("Chữ Việt\n{}", method_id.to_uppercase())
            }
        }
    };
    if enabled {
        format!("buttre\n{method_label}")
    } else {
        format!("buttre\nĐã tắt ({})", method_label.replace('\n', " "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_when_off_still_names_the_method() {
        let on = get_tooltip("telex", true, &[]);
        let off = get_tooltip("telex", false, &[]);
        assert!(on.contains("TELEX"));
        assert!(off.contains("Đã tắt"), "off must say so: {off}");
        assert!(
            off.contains("TELEX"),
            "off must still show what turning on gives: {off}"
        );
    }

    #[test]
    fn tooltip_never_mentions_english() {
        // "english" is not a method (ADR-0003) — no state renders it.
        for method in ["telex", "vni", "nom", "sometoml"] {
            for enabled in [true, false] {
                let tip = get_tooltip(method, enabled, &[]);
                assert!(!tip.to_lowercase().contains("english"), "{tip}");
            }
        }
    }
}
