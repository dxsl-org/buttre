//! Menu building utilities for buttre application

use crate::shared::input::MethodRegistry;
#[cfg(not(target_os = "linux"))]
use crate::shared::ui::{load_menu_icon, CHECK_ICON_BYTES};
use buttre_core::state::Settings;
use buttre_core::vietnamese::config_loader::MethodMetadata;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

/// The widget type of one method row in the tray menu.
///
/// Linux is a `CheckMenuItem`, not an `IconMenuItem`: the tray menu is
/// exported over DBusMenu (libappindicator), whose hosts render an item's
/// `icon-data` at the TRAILING edge (GNOME's appindicator extension) and do
/// not reliably clear it when the pixbuf is nulled — so a check drawn as an
/// icon appears after the label and the previous method's check sticks.
/// `toggle-type=checkmark` is the protocol's native selection state: leading
/// ornament, cleared via `toggle-state`. Windows/macOS keep the original
/// leading check icon.
#[cfg(target_os = "linux")]
pub type MethodMenuItem = muda::CheckMenuItem;
#[cfg(not(target_os = "linux"))]
pub type MethodMenuItem = muda::IconMenuItem;

/// One method row, checked or not — the per-OS check representation (see
/// [`MethodMenuItem`]) is decided here and in [`set_method_checked`] ONLY.
fn new_method_item(label: &str, checked: bool, accelerator: Option<Accelerator>) -> MethodMenuItem {
    #[cfg(target_os = "linux")]
    {
        muda::CheckMenuItem::new(label, true, checked, accelerator)
    }
    #[cfg(not(target_os = "linux"))]
    {
        muda::IconMenuItem::new(
            label,
            true,
            if checked {
                load_menu_icon(CHECK_ICON_BYTES)
            } else {
                None
            },
            accelerator,
        )
    }
}

/// Reflect selection state on a method row (see [`MethodMenuItem`] for why
/// this is per-OS).
pub fn set_method_checked(item: &MethodMenuItem, checked: bool) {
    #[cfg(target_os = "linux")]
    item.set_checked(checked);
    #[cfg(not(target_os = "linux"))]
    item.set_icon(if checked {
        load_menu_icon(CHECK_ICON_BYTES)
    } else {
        None
    });
}

/// Menu items that need to be accessed for event handling
pub struct MenuItems {
    pub english_item: MethodMenuItem,
    pub chu_viet_menu: Submenu,
    pub telex_item: MethodMenuItem,
    pub vni_item: MethodMenuItem,
    pub nom_item: MethodMenuItem, // Unified Nôm method
    pub custom_items: Vec<(MethodMetadata, MethodMenuItem)>,
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

    // 0. English (disable input method)
    let english_item = new_method_item(
        "English",
        settings.input_method == "english",
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

    let telex_item = new_method_item(
        &telex_meta.name,
        settings.input_method == "telex",
        Some(Accelerator::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::Digit1,
        )),
    );
    let vni_item = new_method_item(
        &vni_meta.name,
        settings.input_method == "vni",
        Some(Accelerator::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::Digit2,
        )),
    );
    let _ = chu_viet_menu.append_items(&[&telex_item, &vni_item]);

    // 2. Chữ Nôm - single unified method (no submenu)
    let nom_item = new_method_item(
        &nom_meta.name,
        settings.input_method == "nom",
        Some(Accelerator::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::Digit3,
        )),
    );

    // 3. Custom items - dynamically generated from config list
    // We don't use a submenu anymore, they are appended directly to the main menu
    let mut custom_items: Vec<(MethodMetadata, MethodMenuItem)> = Vec::new();

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

        // Assign accelerator if within limit
        let accelerator = if custom_count < digit_codes.len() {
            Some(Accelerator::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                digit_codes[custom_count],
            ))
        } else {
            None
        };

        let item = new_method_item(
            &method.name,
            settings.input_method == method.id,
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

