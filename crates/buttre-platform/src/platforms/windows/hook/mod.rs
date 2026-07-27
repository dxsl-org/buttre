// Filename deliberately NOT `hook.rs` — `mod hook;` inside the `hook` module
// directory triggers clippy::module_inception (a module with the same name
// as its containing module).
mod callback;
mod profiling;
mod queue;

pub use crate::platforms::windows::common::{send_backspaces, send_string, send_unicode_char};
pub use callback::{dispatch_toggle_last_word, install_hook, run_message_loop, uninstall_hook};
pub use profiling::{ProfileStats, HOOK_PROFILER};
pub use queue::{KeyEvent, QueueProcessor};

use anyhow::Result;
use buttre_core::Keyboard;
use std::sync::{Arc, RwLock};

/// Windows Keyboard Hook Backend
pub struct HookBackend {
    keyboard: Option<Arc<RwLock<Option<Keyboard>>>>,
}

impl HookBackend {
    pub fn new() -> Result<Self> {
        Ok(Self { keyboard: None })
    }

    pub fn init(&mut self, keyboard: Arc<RwLock<Option<Keyboard>>>) -> Result<()> {
        tracing::info!("Initializing Windows keyboard hook backend");
        self.keyboard = Some(keyboard.clone());
        install_hook(keyboard, None)?;
        tracing::info!("Hook installed successfully");
        Ok(())
    }

    pub fn set_enabled(&mut self, _enabled: bool) {
        // ====================================================================
        // ARCHITECTURE NOTE: This method does NOTHING (kept for API compat)
        // ====================================================================
        // Old design (WRONG):
        //   - Called hook::set_vietnamese_enabled() to set a flag
        //   - Flag could get out of sync with actual keyboard state
        //   - Caused bugs when user selected VNI/Nôm from menu
        //
        // New design (CORRECT):
        //   - Hook checks KEYBOARD.is_some() directly (single source of truth)
        //   - KeyboardManager updates KEYBOARD when method changes
        //   - No separate flag needed → No sync issues!
        //   - This function does nothing but is kept for API compatibility
        // ====================================================================
        tracing::debug!(
            "HookBackend::set_enabled({}) - ignored (uses KEYBOARD global)",
            _enabled
        );
    }

    pub fn cleanup(&mut self) {
        tracing::info!("Cleaning up Windows keyboard hook backend");
        let _ = uninstall_hook();
    }
}

/// Start the keyboard hook backend (legacy helper)
pub fn start_hook_backend(
    keyboard: Arc<RwLock<Option<Keyboard>>>,
    callback: Option<Box<dyn Fn(bool) + Send + Sync>>,
) -> Result<()> {
    install_hook(keyboard, callback)?;
    Ok(())
}

/// Stop the keyboard hook backend (legacy helper)
pub fn stop_hook_backend() -> Result<()> {
    uninstall_hook()?;
    Ok(())
}

/// Set hook mode (DEPRECATED - does nothing!)
///
/// This function is kept for backward compatibility but does NOTHING.
/// The hook now checks KEYBOARD.is_some() directly (single source of truth).
/// See hook.rs header comment for full explanation.
pub fn set_vietnamese_mode(_enabled: bool) {
    // Do nothing - hook checks KEYBOARD global directly
    tracing::debug!("set_vietnamese_mode({}) - ignored (deprecated)", _enabled);
}

/// Print hook profiling statistics
pub fn print_hook_stats() {
    let stats = HOOK_PROFILER.get_stats();
    println!("{}", stats);
}

/// Reset hook profiling statistics
pub fn reset_hook_stats() {
    HOOK_PROFILER.reset();
    tracing::info!("Hook profiling statistics reset");
}
