//! buttre Hotkey Module
//!
//! Provides global hotkey management for buttre using the global-hotkey crate.

mod chords;
mod error;
mod manager;

pub use chords::{ctrl_shift_action, CUSTOM_CHORD_KEYS, FIXED_CHORDS};
pub use error::{HotkeyError, Result};
pub use manager::{ButtreHotkeyManager, HotkeyAction};

// Re-export commonly used types
pub use global_hotkey::hotkey::{Code, Modifiers};
