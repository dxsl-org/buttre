//! State management module for buttre
//!
//! This module provides centralized state management using the Observer pattern.
//! It serves as the single source of truth for application state and enables
//! reactive updates across different components (UI, Engine, Backend).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐
//! │  AppState   │ ◄─── Single Source of Truth
//! └──────┬──────┘
//!        │
//!        ├─► Observer 1 (UI Updates)
//!        ├─► Observer 2 (Engine Updates)
//!        └─► Observer 3 (Backend Updates)
//! ```
//!
//! # Example
//!
//! ```rust
//! use buttre_core::state::{AppState, StateObserver, Settings};
//! use std::sync::Arc;
//!
//! // Create state
//! let mut state = AppState::new();
//!
//! // Register observers
//! // state.add_observer(Arc::new(MyObserver));
//!
//! // Change state - observers will be notified automatically
//! state.set_method("telex").unwrap();
//! ```

mod app_state;
pub mod learning;
pub mod macros;
mod observer;
pub mod observers;
mod settings;

pub use app_state::AppState;
pub use observer::StateObserver;
pub use settings::Settings;
