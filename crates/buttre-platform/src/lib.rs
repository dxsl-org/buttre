//! Platform Abstraction Layer for buttre
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/platform_lib_tests.rs`.
//!
//! This crate provides a unified interface for platform-specific backends.
//! The correct backend is selected at **compile-time** based on the target OS.

// See buttre-engine/src/lib.rs's doc comment on this attribute — pedantic
// and nursery are deliberately excluded, matching the workspace lint policy.
#![warn(clippy::all)]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::module_name_repetitions, clippy::must_use_candidate)]
#![allow(unsafe_code)] // Platform crate requires unsafe for FFI/system calls
//!
//! # Architecture
//!
//! ```text
//! buttre-platform
//! ├── platforms/           (Platform-specific backends)
//! │   ├── windows/        (Windows Hook + TSF)
//! │   ├── macos/          (macOS IMKit)
//! │   └── linux/          (Linux IBus)
//! └── shared/             (Cross-platform code)
//!     ├── input/          (Engine management)
//!     ├── observers/      (State observers)
//!     ├── ui/             (UI components)
//!     └── pipe_server     (IPC server)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use buttre_platform::Backend;
//!
//! // Backend is automatically selected based on target OS
//! let mut backend = Backend::new()?;
//! backend.init()?;
//! backend.process_key('a');
//! ```
//!
//! # Compile-time Selection
//!
//! The platform is detected at **build-time** using `build.rs`:
//! - Windows: `cfg(platform_windows)`
//! - macOS: `cfg(platform_macos)`
//! - Linux: `cfg(platform_linux)`
//!
//! Only the target platform's code is compiled, resulting in:
//! - Smaller binary size (~30% reduction for cross-platform builds)
//! - Faster compilation
//! - No runtime overhead

use anyhow::Result;
use buttre_core::Action;
use buttre_core::Keyboard;
use std::sync::{Arc, RwLock};

/// Platform backend trait
///
/// All platform-specific backends must implement this trait.
pub trait PlatformBackend {
    /// Create a new backend instance
    fn new() -> Result<Self>
    where
        Self: Sized;

    /// Initialize the backend with the keyboard (Phase 4, Task 3: Using RwLock)
    fn init(&mut self, keyboard: Arc<RwLock<Option<Keyboard>>>) -> Result<()>;

    /// Process a keystroke
    fn process_key(&mut self, key: char) -> Action;

    /// Enable or disable the backend
    fn set_enabled(&mut self, enabled: bool);

    /// Cleanup resources
    fn cleanup(&mut self);
}

// ============================================================================
// Module organization
// ============================================================================

/// Platform-specific backend implementations
pub mod platforms;

/// Shared cross-platform code
pub mod shared;

// ============================================================================
// Platform re-exports
// ============================================================================

/// The platform-specific backend for the current target OS
///
/// This type alias points to:
/// - `WindowsBackend` on Windows
/// - `MacOSBackend` on macOS
/// - `LinuxBackend` on Linux
pub use platforms::Backend;

// Re-export shared utilities for convenience
pub use shared::ui::{build_menu, create_tray_icon, MenuItems};
pub use shared::{
    KeyboardManager, KeyboardObserver, MainUICallback, MethodInfo, MethodRegistry, MethodSource,
    UIEvent, UIObserver,
};

// ============================================================================
// Compile-time platform check
// ============================================================================

#[cfg(not(any(platform_windows, platform_macos, platform_linux)))]
compile_error!(
    "Unsupported platform. buttre-platform only supports Windows, macOS, and Linux. \
     Current target: {}",
);

// ============================================================================
// Platform detection utilities
// ============================================================================

/// Get the current platform name
pub const fn platform_name() -> &'static str {
    #[cfg(platform_windows)]
    return "Windows";

    #[cfg(platform_macos)]
    return "macOS";

    #[cfg(platform_linux)]
    return "Linux";
}

/// Check if running on Windows
pub const fn is_windows() -> bool {
    cfg!(platform_windows)
}

/// Check if running on macOS
pub const fn is_macos() -> bool {
    cfg!(platform_macos)
}

/// Check if running on Linux
pub const fn is_linux() -> bool {
    cfg!(platform_linux)
}
