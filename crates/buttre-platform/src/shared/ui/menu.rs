//! Menu building utilities for buttre application

use crate::shared::input::MethodRegistry;
use crate::shared::ui::{load_menu_icon, CHECK_ICON_BYTES};
use buttre_core::state::Settings;
use buttre_core::vietnamese::config_loader::MethodMetadata;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{IconMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

/// Menu items that need to be accessed for event handling
pub struct MenuItems {
    pub english_item: IconMenuItem,
    pub chu_viet_menu: Submenu,
    pub telex_item: IconMenuItem,
    pub vni_item: IconMenuItem,
    pub nom_item: IconMenuItem, // Unified Nôm method
    pub custom_items: Vec<(MethodMetadata, IconMenuItem)>,
    /// Root-level "Cấu hình…": spawns `buttre --config` (the Slint config
    /// window, a separate process — see `buttre_config`'s crate doc). Owns
    /// everything the tray used to expose directly: Học thông minh, Tự động
    /// khởi động, Gõ tắt, Từ đã học, Quản lý gõ tắt, and Hướng dẫn (now the
    /// Giới thiệu tab) — see ADR-0002.
    pub cau_hinh_item: MenuItem,
    pub thoat_item: MenuItem,
}

/// Build the complete menu structure
pub fn build_menu(settings: &Settings, registry: &MethodRegistry) -> (Menu, MenuItems) {
    // Convert registry to MethodMetadata for compatibility
    let all_methods: Vec<MethodMetadata> = registry
        .get_all()
        .iter()
        .map(|info| MethodMetadata {
            id: info.id.clone(),
            name: info.name.clone(),
            description: info.description.clone().unwrap_or_default(),
            version: "1.0.0".to_string(),
            author: "buttre".to_string(),
            icon: None,
            is_builtin: matches!(info.source, crate::shared::input::MethodSource::BuiltIn),
        })
        .collect();

    // 0. English (disable input method) - IconMenuItem
    let english_item = IconMenuItem::new(
        "English",
        true,
        if settings.input_method == "english" {
            load_menu_icon(CHECK_ICON_BYTES)
        } else {
            None
        },
        Some(Accelerator::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::Space,
        )),
    );

    // 1. Chữ Việt submenu (enabled)
    // 1. Chữ Việt submenu (enabled)
    let chu_viet_menu = Submenu::new("Chữ Việt", true);

    // Find built-in methods
    let telex_meta = all_methods
        .iter()
        .find(|m| m.id == "telex")
        .cloned()
        .unwrap_or(MethodMetadata {
            id: "telex".to_string(),
            name: "Telex".to_string(),
            description: "".to_string(),
            version: "1.0.0".to_string(),
            author: "buttre".to_string(),
            icon: None,
            is_builtin: true,
        });

    let vni_meta = all_methods
        .iter()
        .find(|m| m.id == "vni")
        .cloned()
        .unwrap_or(MethodMetadata {
            id: "vni".to_string(),
            name: "VNI".to_string(),
            description: "".to_string(),
            version: "1.0.0".to_string(),
            author: "buttre".to_string(),
            icon: None,
            is_builtin: true,
        });

    let nom_meta = all_methods
        .iter()
        .find(|m| m.id == "nom")
        .cloned()
        .unwrap_or(MethodMetadata {
            id: "nom".to_string(),
            name: "Chữ Nôm".to_string(),
            description: "".to_string(),
            version: "1.0.0".to_string(),
            author: "buttre".to_string(),
            icon: None,
            is_builtin: true,
        });

    let telex_item = IconMenuItem::new(
        &telex_meta.name,
        true,
        if settings.input_method == "telex" {
            load_menu_icon(CHECK_ICON_BYTES)
        } else {
            None
        },
        Some(Accelerator::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::Digit1,
        )),
    );
    let vni_item = IconMenuItem::new(
        &vni_meta.name,
        true,
        if settings.input_method == "vni" {
            load_menu_icon(CHECK_ICON_BYTES)
        } else {
            None
        },
        Some(Accelerator::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::Digit2,
        )),
    );
    let _ = chu_viet_menu.append_items(&[&telex_item, &vni_item]);

    // 2. Chữ Nôm - single unified method (no submenu)
    let is_nom = settings.input_method == "nom";
    let nom_item = IconMenuItem::new(
        &nom_meta.name,
        true,
        if is_nom {
            load_menu_icon(CHECK_ICON_BYTES)
        } else {
            None
        },
        Some(Accelerator::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::Digit3,
        )),
    );

    // 3. Custom items - dynamically generated from config list
    // We don't use a submenu anymore, they are appended directly to the main menu
    let mut custom_items: Vec<(MethodMetadata, IconMenuItem)> = Vec::new();

    // Helper array for hotkeys (Ctrl+Shift+4..0)
    let digit_codes = [
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
        Code::Digit0,
    ];
    let mut custom_count = 0;

    // Filter custom methods (not built-in)
    for method in all_methods {
        if method.is_builtin {
            continue;
        }

        // Skip if it somehow matches a reserved id (though is_builtin should catch it)
        if method.id == "english"
            || method.id == "telex"
            || method.id == "vni"
            || method.id == "nom"
        {
            continue;
        }

        let is_selected = settings.input_method == method.id;

        // Assign accelerator if within limit
        let accelerator = if custom_count < digit_codes.len() {
            Some(Accelerator::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                digit_codes[custom_count],
            ))
        } else {
            None
        };

        let item = IconMenuItem::new(
            &method.name,
            true,
            if is_selected {
                load_menu_icon(CHECK_ICON_BYTES)
            } else {
                None
            },
            accelerator,
        );
        custom_items.push((method, item));
        custom_count += 1;
    }

    // 4. Root-level items — the tray is deliberately slim (ADR-0002): every
    // toggle/table that used to live directly in the tray (Học thông minh,
    // Tự động khởi động, Gõ tắt, Từ đã học, Quản lý gõ tắt, Hướng dẫn) now
    // lives inside the "Cấu hình…" window instead.
    let cau_hinh_item = MenuItem::new("Cấu hình…", true, None);
    let thoat_item = MenuItem::new("Thoát", true, None);

    // Assemble menu
    let menu = Menu::new();

    // Add built-in items
    let _ = menu.append_items(&[&english_item, &chu_viet_menu, &nom_item]);

    // Add custom items directly to main menu
    for (_, item) in &custom_items {
        let _ = menu.append(item);
    }

    // Add remaining items
    let _ = menu.append_items(&[
        &PredefinedMenuItem::separator(),
        &cau_hinh_item,
        &thoat_item,
    ]);

    let menu_items = MenuItems {
        english_item,
        chu_viet_menu,
        telex_item,
        vni_item,
        nom_item,
        custom_items,
        cau_hinh_item,
        thoat_item,
    };

    (menu, menu_items)
}

/// Update menu icons to reflect the selected input method
#[allow(dead_code)]
pub fn update_menu_for_method(
    menu_items: &MenuItems,
    method: &str,
    custom_methods: &[(MethodMetadata, IconMenuItem)],
) {
    // Clear all icons first
    menu_items.english_item.set_icon(None);
    menu_items.telex_item.set_icon(None);
    menu_items.vni_item.set_icon(None);
    menu_items.nom_item.set_icon(None);
    for (_, item) in &menu_items.custom_items {
        item.set_icon(None);
    }

    // Set check icon for selected method
    let check_icon = load_menu_icon(CHECK_ICON_BYTES);
    match method {
        "english" => {
            if let Some(icon) = check_icon {
                menu_items.english_item.set_icon(Some(icon));
            }
        }
        "telex" => {
            if let Some(icon) = check_icon {
                menu_items.telex_item.set_icon(Some(icon));
            }
        }
        "vni" => {
            if let Some(icon) = check_icon {
                menu_items.vni_item.set_icon(Some(icon));
            }
        }
        "nom" => {
            if let Some(icon) = check_icon {
                menu_items.nom_item.set_icon(Some(icon));
            }
        }
        custom_id => {
            // Check if it's a custom method
            if let Some((_, item)) = custom_methods.iter().find(|(d, _)| d.id == custom_id) {
                if let Some(icon) = check_icon {
                    item.set_icon(Some(icon));
                }
            }
        }
    }
}
