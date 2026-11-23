//! UI components for buttre application.
//!
//! Two tray toolkits: tray-icon + muda on Windows (`tray`, `menu`,
//! `helpers`), a StatusNotifierItem on Linux (`sni_tray`). Both turn clicks
//! into [`TrayCommand`]s and share the tooltip text and icon bytes.

#[cfg(not(target_os = "linux"))]
pub mod helpers;
pub mod icons;
#[cfg(not(target_os = "linux"))]
pub mod menu;
#[cfg(target_os = "linux")]
pub mod sni_tray;
pub mod tooltip;
#[cfg(not(target_os = "linux"))]
pub mod tray;
pub mod tray_command;

// Re-export commonly used items
#[cfg(not(target_os = "linux"))]
pub use icons::{load_icon_from_bytes, load_menu_icon, IconSet};
pub use icons::{
    CHECK_ICON_BYTES, CUSTOM_ICON_BYTES, ENGLISH_ICON_BYTES, NOM_ICON_BYTES, TELEX_ICON_BYTES,
    VIETNAMESE_ICON_BYTES, VNI_ICON_BYTES,
};
#[cfg(not(target_os = "linux"))]
pub use menu::{build_menu, MenuItems};
#[cfg(not(target_os = "linux"))]
pub use tray::{create_tray_icon, TrayIcons};
pub use tray_command::TrayCommand;
