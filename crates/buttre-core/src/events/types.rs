//! Event types for the application event bus
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/event_types_tests.rs`.
//!
//! All events that can occur in the buttre application are defined here.
//! This provides a centralized, type-safe way to communicate between components.

use crate::state::Settings;
use crate::types::Action;

/// All application events
///
/// Events are published to the EventBus and received by subscribers.
/// This enum provides loose coupling between components.
#[derive(Debug, Clone)]
pub enum AppEvent {
    // ========================================================================
    // State Events - Application state changes
    // ========================================================================
    /// Input state changed — the method, or the on/off flag (one event for
    /// both, so no subscriber ever sees half of a change).
    MethodChanged {
        /// New method ID: `"telex"`, `"vni"`, `"nom"`, or a custom id. Never
        /// `"english"` — off is carried by `enabled` (ADR-0003).
        method: String,
        /// Whether the input method is on
        enabled: bool,
    },

    /// Settings updated
    ///
    /// Published when application settings are modified and saved
    SettingsChanged(Settings),

    /// Application enabled/disabled state changed
    ///
    /// Published when Vietnamese input is toggled on/off
    EnabledChanged(bool),

    // ========================================================================
    // Keyboard Events - Input processing
    // ========================================================================
    /// Key input received
    ///
    /// Published when a keystroke is received for processing
    KeyboardInput(char),

    /// Action produced by keyboard processing
    ///
    /// Published after processing a keystroke, contains the action to execute
    KeyboardOutput(Action),

    /// Keyboard buffer reset
    ///
    /// Published when the input buffer is cleared (e.g., word boundary)
    KeyboardReset,

    // ========================================================================
    // Hotkey Events - Global hotkey actions
    // ========================================================================
    /// Global hotkey pressed
    ///
    /// Published when a registered hotkey combination is detected
    HotkeyPressed(HotkeyAction),

    // ========================================================================
    // Config Events - Configuration management
    // ========================================================================
    /// Configuration loaded
    ///
    /// Published when a keyboard config is successfully loaded
    ConfigLoaded {
        /// Config ID (e.g., "telex", "vni", "custom_method")
        id: String,
    },

    /// Custom method added
    ///
    /// Published when a new custom input method is detected
    MethodAdded(MethodInfo),

    /// Custom method removed
    ///
    /// Published when a custom input method is removed
    MethodRemoved(String),

    // ========================================================================
    // System Events - Errors and logging
    // ========================================================================
    /// Error occurred
    ///
    /// Published when an error happens during operation
    Error {
        /// Source component that generated the error
        source: String,
        /// Error message
        message: String,
    },

    /// Log message
    ///
    /// Published for debugging and monitoring
    Log {
        /// Log level
        level: LogLevel,
        /// Log message
        message: String,
    },
}

/// Hotkey actions that can be triggered
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Toggle between current method and English
    Toggle,
    /// Switch to Telex
    Telex,
    /// Switch to VNI
    Vni,
    /// Switch to Nôm
    Nom,
    /// Switch to custom method by index
    Custom(usize),
}

/// Information about an input method
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// Unique identifier (e.g., "telex", "vni", "my_custom")
    pub id: String,
    /// Display name
    pub name: String,
    /// Language (e.g., "vietnamese", "nom")
    pub language: String,
    /// Source of the method
    pub source: MethodSource,
}

/// Source of an input method
#[derive(Debug, Clone)]
pub enum MethodSource {
    /// Built-in method (telex, vni, nom)
    Builtin,
    /// Custom method from TOML file
    Custom(String), // Path to TOML file
}

/// Log level for Log events
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Debug information
    Debug,
    /// Informational messages
    Info,
    /// Warning messages
    Warn,
    /// Error messages
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

impl AppEvent {
    /// Helper: Create a method changed event
    pub fn method_changed(method: impl Into<String>, enabled: bool) -> Self {
        AppEvent::MethodChanged {
            method: method.into(),
            enabled,
        }
    }

    /// Helper: Create an error event
    pub fn error(source: impl Into<String>, message: impl Into<String>) -> Self {
        AppEvent::Error {
            source: source.into(),
            message: message.into(),
        }
    }

    /// Helper: Create a log event
    pub fn log(level: LogLevel, message: impl Into<String>) -> Self {
        AppEvent::Log {
            level,
            message: message.into(),
        }
    }

    /// Helper: Create an info log event
    pub fn info(message: impl Into<String>) -> Self {
        Self::log(LogLevel::Info, message)
    }

    /// Helper: Create a debug log event
    pub fn debug(message: impl Into<String>) -> Self {
        Self::log(LogLevel::Debug, message)
    }

    /// Helper: Create a warning log event
    pub fn warn(message: impl Into<String>) -> Self {
        Self::log(LogLevel::Warn, message)
    }
}
