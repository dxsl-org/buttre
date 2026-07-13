//! UI components for buttre application

pub mod helpers;
pub mod icons;
pub mod menu;
pub mod tray;

// Re-export commonly used items
pub use icons::{
    load_icon_from_bytes, load_menu_icon, CHECK_ICON_BYTES, CUSTOM_ICON_BYTES, ENGLISH_ICON_BYTES,
    NOM_ICON_BYTES, TELEX_ICON_BYTES, VIETNAMESE_ICON_BYTES, VNI_ICON_BYTES,
};
pub use menu::{build_menu, MenuItems};
pub use tray::create_tray_icon;
