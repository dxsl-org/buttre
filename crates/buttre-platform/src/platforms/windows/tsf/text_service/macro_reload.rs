// SPDX-License-Identifier: GPL-3.0-only
//! Live-reload watcher for the shorthand/gõ tắt store inside the TSF text
//! service.
//!
//! `MacroStore::get_path()` and `Settings::get_path()` both resolve to
//! `dirs::data_dir()/buttre` on Windows (Windows has no separate config
//! dir), so ONE watcher on that single directory catches edits to either
//! file — no need for the two-watcher split the tray's `main.rs` uses.
//! `settings.toml` is watched too because it carries the `shorthand` gate
//! itself: flipping it externally (config window) must reload the store
//! into the empty/non-empty state without a TSF restart.

use buttre_core::state::macros::MacroStore;
use buttre_core::Settings;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::{Arc, Mutex, PoisonError};

/// Watch the buttre data directory for changes to `macros.toml` or
/// `settings.toml` and swap `store`'s contents in place on any such change.
///
/// The callback runs on notify's OWN background thread and swaps the store
/// DIRECTLY — unlike the tray (`buttre-platform/src/main.rs`), a TSF text
/// service has no polling event loop to drain a channel from, so there is
/// nowhere else to do the swap.
///
/// Returns `None` (with a `tracing::warn!`) when the directory can't be
/// resolved or watched. The TSF host process then simply keeps whatever
/// store was loaded at `VietnameseEngine::new` time — never a panic, since
/// a live-reload gap degrades to "restart the app to pick up an edit", not
/// a crash of the host process.
///
/// The returned watcher must be kept alive (stored as an engine field) for
/// as long as live reload should work; dropping it stops the watch — this
/// is deliberate: it ties the watch to the `VietnameseEngine`'s (and thus
/// the TSF text service instance's) lifetime, so no watcher thread survives
/// deactivation.
pub fn spawn_reload_watcher(store: Arc<Mutex<MacroStore>>) -> Option<RecommendedWatcher> {
    let dir = match MacroStore::get_path() {
        Ok(path) => path.parent()?.to_path_buf(),
        Err(e) => {
            tracing::warn!("TSF: macros dir unresolved, live reload disabled: {e:?}");
            return None;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("TSF: cannot create {dir:?}, live reload disabled: {e:?}");
        return None;
    }

    let mut watcher =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else {
                return;
            };
            let relevant = event.paths.iter().any(|p| {
                matches!(
                    p.file_name().and_then(|n| n.to_str()),
                    Some("macros.toml") | Some("settings.toml")
                )
            });
            if !relevant {
                return;
            }
            // Build the replacement BEFORE taking the lock: the same Mutex
            // sits on the keystroke path (`apply_macro`), so holding it
            // across file IO would stall the host app's input thread.
            let next = MacroStore::load_gated(Settings::load().shorthand);
            *store.lock().unwrap_or_else(PoisonError::into_inner) = next;
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("TSF: macros watcher failed, live reload disabled: {e:?}");
                return None;
            }
        };

    if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        tracing::warn!("TSF: macros watch failed, live reload disabled: {e:?}");
        return None;
    }
    Some(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_reload_watcher_degrades_to_none_on_reload_never_panicking() {
        // Smoke test: this must never panic even in a constrained test
        // environment (mirrors `MacroStore::load`'s own smoke test) — the
        // return value (Some or None depending on whether the test host can
        // resolve/watch a real data dir) is not asserted, only that calling
        // it is safe.
        let store = Arc::new(Mutex::new(MacroStore::default()));
        let _ = spawn_reload_watcher(store);
    }
}
