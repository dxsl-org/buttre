//! # buttre Core - State Management, Services & Event Bus
//!
//! **Architecture Layer**: Core Services & State
//!
//! ## 🎯 Purpose
//!
//! Provides centralized state management, services, and event-driven architecture
//! for the buttre application. This is the heart of the application logic.

// See buttre-engine/src/lib.rs's doc comment on this attribute — pedantic
// and nursery are deliberately excluded, matching the workspace lint policy.
#![warn(clippy::all)]
#![deny(unsafe_code)]
#![allow(clippy::module_name_repetitions, clippy::must_use_candidate)]
//!
//! ## 📊 Architecture Position
//!
//! ```text
//! buttre-platform (UI + OS Backends)
//!        ↓
//! buttre-core ← YOU ARE HERE (Services + State + Event Bus)
//!        ↓
//! buttre-engine (7-Stage Pipeline)
//! ```
//!
//! ## ✅ Responsibilities
//!
//! - **Event Bus**: Centralized event distribution (loose coupling)
//! - **Services**: KeyboardService, ConfigService, HotkeyService, etc.
//! - **State Management**: AppState, Settings, KeyboardState
//! - **Keyboard Core**: Keyboard wrapper, Config loader, Builder
//! - **Hotkey Management**: Global hotkey handling
//!
//! ## ❌ Does NOT Contain
//!
//! - ❌ Platform-specific code (→ buttre-platform)
//! - ❌ UI implementation (→ buttre-platform)
//! - ❌ Pipeline algorithms (→ buttre-engine)
//!
//! ## 🔧 Key Components
//!
//! ### Event Bus
//! - `events::EventBus` - Central event distribution
//! - `events::AppEvent` - All application events
//! - `events::create_event_bus()` - Create shared bus
//!
//! ### Services (Coming in Phase 3)
//! - `services::KeyboardService` - Keyboard instance management
//! - `services::ConfigService` - Config loading/saving
//! - `services::HotkeyService` - Hotkey management
//! - `services::SettingsService` - Settings persistence
//! - `services::MethodRegistry` - Available methods registry
//!
//! ### State
//! - `state::AppState` - Application state
//! - `state::Settings` - Persistent settings
//!
//! ## 📝 Usage (Event Bus Pattern)
//!
//! ```rust,ignore
//! use buttre_core::events::{create_event_bus, AppEvent};
//!
//! // Create event bus
//! let bus = create_event_bus();
//!
//! // Subscribe to events
//! bus.subscribe(|event| {
//!     match event {
//!         AppEvent::MethodChanged { method, enabled } => {
//!             println!("Method: {} (enabled: {})", method, enabled);
//!         }
//!         _ => {}
//!     }
//! });
//!
//! // Publish events
//! bus.publish(AppEvent::method_changed("telex", true));
//! ```
//!
//! See `.agent/plans/buttre-core-restructure.md` for full refactoring plan.

// Re-export core engine components (types only, NOT for processing!)
pub use buttre_engine::buffer;
pub use buttre_engine::types;
pub use buttre_engine::unicode;

// Core modules
pub mod core; // ButtreCore facade (NEW!)
pub mod events; // Event Bus
pub mod hotkey; // Hotkey management
pub mod keyboard;
pub mod services; // Services Layer
pub mod state; // Settings, AppState // Keyboard core (from buttre-keyboard)

// Compatibility stubs (will be refactored in Phase 4)
pub mod vietnamese; // MethodMetadata + resource lookup (keyboard_dirs, Nôm db) for UI

// Re-exports from buttre_engine (types only)
pub use buttre_engine::InputBuffer;
pub use types::Config as EngineConfig; // Rename to avoid conflict
pub use types::{Action, CharInfo, WordForm};
pub use unicode::{normalize_nfc, normalize_nfd, sanitize_filename, str_eq_normalized};

// State management exports
pub use state::{AppState, Settings, StateObserver};

// Event system exports
pub use events::{
    create_event_bus, AppEvent, EventBus, HotkeyAction, LogLevel, MethodInfo, MethodSource,
    SharedEventBus,
};

// Keyboard exports (from buttre-keyboard)
pub use keyboard::{
    Config as KeyboardConfig, // Renamed to avoid conflict with EngineConfig
    Keyboard,
    KeyboardBuilder,
    Metadata,
    Rules,
    // Note: Separators removed - buffer termination is now handled at engine level (key_utils.rs)
};

// Services exports
pub use services::{
    ConfigInfo, ConfigService, ConfigSource, HotkeyService, KeyboardService, MethodRegistry,
    Preset, SettingsService,
};

// ButtreCore facade - Main entry point
pub use core::ButtreCore;

// Note: buttre-keyboard merged into buttre-core in Phase 2 ✅ DONE
// Note: Services layer created in Phase 3 ✅ DONE
// Note: ButtreCore facade created in Phase 4+5 ✅ DONE

// ============================================================================
// BACKWARD COMPATIBILITY LAYER
// ============================================================================
// For existing platform code that uses buttre_keyboard
//
// This allows old code like:
//   use buttre_keyboard::{Keyboard, KeyboardBuilder, Config, Action};
// to work with:
//   use buttre_core::{Keyboard, KeyboardBuilder, Config, Action};
//
// without any changes to the platform code.

// Re-export keyboard types with backward-compatible names
// (Most types are already exported above, we just need the Config alias)
pub use KeyboardConfig as Config; // Alias for old code that uses "Config"
