//! Shared cross-platform code
//!
//! This module contains code that is used across all platforms,
//! including UI components, keyboard management, and IPC.

pub mod autostart;
pub mod config_watcher;
pub mod engine_bridge;
pub mod input;
pub mod observers;
pub mod pipe_server;
pub mod ui;

// Re-export commonly used types
pub use config_watcher::{ConfigChangeEvent, ConfigWatcher};
pub use input::{KeyboardManager, MethodInfo, MethodRegistry, MethodSource};
pub use observers::{KeyboardObserver, MainUICallback, UIEvent, UIObserver};
pub use ui::{build_menu, create_tray_icon, show_help_dialog, MenuItems};
