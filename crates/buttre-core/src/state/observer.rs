//! Observer pattern for state change notifications
//!
//! This module provides the `StateObserver` trait that allows components
//! to react to state changes without tight coupling.

use super::Settings;

/// Observer trait for reacting to application state changes
///
/// Implementors of this trait will be notified when the application state changes,
/// allowing them to update UI, backend systems, or perform other side effects.
pub trait StateObserver: Send + Sync {
    /// Called when the input STATE changes — either the method or the on/off
    /// flag. Both arrive through this one call on purpose: it already carries
    /// the full pair, so an observer can never act on half of a change.
    ///
    /// # Arguments
    /// * `method` - The new input method ID: `"telex"`, `"vni"`, `"nom"`, or a
    ///   custom id. NEVER `"english"` — off is carried by `enabled` (ADR-0003).
    /// * `enabled` - Whether the input method is on
    fn on_method_changed(&self, method: &str, enabled: bool);

    /// Called when settings are updated
    ///
    /// # Arguments
    /// * `settings` - The updated settings object
    fn on_settings_changed(&self, settings: &Settings);
}
