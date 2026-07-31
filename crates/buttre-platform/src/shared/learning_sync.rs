//! Personal-learning wiring for the Linux engine processes (`buttre --ibus`,
//! `buttre --ime`) — the engine-side counterpart of the tray's
//! `set_learning` + drain loop in `main.rs`. Mirrors [`super::macro_sync`]'s
//! shape: no tray relays anything on these platforms, so the engine process
//! loads the store, watches the files, and writes the disk itself.
//!
//! **Write model (many writers, ADR-0002):** every save goes through
//! `LearningStore::write_atomic_merged` — the tray, every TSF host app and
//! this process may all write `learning.toml`, and merged writes are what
//! keeps one process's snapshot from clobbering another's learning.
//! Writes happen on a dedicated thread ([`spawn_writer`]), never on the
//! key-event path: the keyboard only ENQUEUES snapshots on the channel.
//!
//! **Reload model:** `learning.toml` changes on disk (config-window edit,
//! another process's merged write) are content-swapped into the shared
//! store; the live compose snapshot refreshes at the next word commit
//! (`collect_and_refresh_learning` re-snapshots the store every time).
//! `Settings::learning_enabled` rides the same directory watch — both files
//! live in the same data dir — and engines consult the mirror per keystroke
//! (`sync_learning`), the same lazy pattern as method/strict/enabled.

use buttre_core::state::learning::{LearningFile, LearningStore};
use buttre_core::state::Settings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};

/// The cheap-to-clone handles every engine object needs: the shared store,
/// the save channel's sending end, and the `Settings::learning_enabled`
/// mirror. One bundle so the ibus factory / wayland state structs don't
/// grow three parameters each.
#[derive(Clone)]
pub struct LearningWiring {
    pub store: Arc<Mutex<LearningStore>>,
    pub save_tx: mpsc::Sender<LearningFile>,
    pub enabled: Arc<AtomicBool>,
}

/// Build the initial wiring for engine-process startup. Pure loads only —
/// no thread, no inotify handle — so a caller that later returns
/// `Unavailable` (the Wayland probe) leaks nothing; the receiver is handed
/// back for [`spawn_writer`] once availability is confirmed.
///
/// The store is loaded from disk only when learning is enabled — mirroring
/// the tray: disk is not read for a feature that is off.
pub fn load_initial() -> (LearningWiring, mpsc::Receiver<LearningFile>) {
    let enabled = Settings::load().learning_enabled;
    tracing::info!("learning_sync: initial learning_enabled = {enabled}");
    let store = if enabled {
        LearningStore::load()
    } else {
        LearningStore::default()
    };
    let (save_tx, save_rx) = mpsc::channel::<LearningFile>();
    (
        LearningWiring {
            store: Arc::new(Mutex::new(store)),
            save_tx,
            enabled: Arc::new(AtomicBool::new(enabled)),
        },
        save_rx,
    )
}

/// Spawn the dedicated save-writer thread (shared implementation — the TSF
/// hosts run the identical thread): block on the channel, debounce to the
/// latest snapshot, merged-write, repeat. Exits when every sender is
/// dropped — in practice never; the daemon/compositor owns the process.
pub fn spawn_writer(rx: mpsc::Receiver<LearningFile>) {
    crate::shared::learning_writer::spawn(rx);
}

/// Watch the data dir for `learning.toml` (content-swap the store) and
/// `settings.toml` (refresh the `learning_enabled` mirror). Both files
/// resolve under the same directory today, but the two paths are looked up
/// independently so this keeps working if that ever diverges. Lives for the
/// process lifetime (same contract as `macro_sync::spawn_watcher`).
pub fn spawn_watcher(wiring: &LearningWiring) {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for path in [LearningStore::get_path(), Settings::get_path()] {
        match path {
            Ok(p) => {
                if let Some(dir) = p.parent() {
                    let dir = dir.to_path_buf();
                    if !dirs.contains(&dir) {
                        dirs.push(dir);
                    }
                }
            }
            Err(e) => tracing::warn!("learning_sync: path unresolved: {e:?}"),
        }
    }
    if dirs.is_empty() {
        tracing::warn!("learning_sync: no watchable directory, watcher not started");
        return;
    }
    for dir in dirs {
        let store = wiring.store.clone();
        let enabled = wiring.enabled.clone();
        crate::fs_watch::spawn_dir_watch("learning_sync", dir, move |cue| {
            let (learning_changed, settings_changed) = match cue {
                // The dir may have been rewritten while unwatched — treat
                // both as changed.
                crate::fs_watch::WatchCue::Rearmed => (true, true),
                crate::fs_watch::WatchCue::Event(event) => {
                    let hit = |name: &str| {
                        event
                            .paths
                            .iter()
                            .any(|p| p.file_name().and_then(|n| n.to_str()) == Some(name))
                    };
                    (hit("learning.toml"), hit("settings.toml"))
                }
            };
            let mut reload_store = learning_changed;
            if settings_changed {
                let on = Settings::load().learning_enabled;
                let was = enabled.swap(on, Ordering::Relaxed);
                // Runtime OFF→ON: the store was never loaded (startup skips
                // disk while off) — without this reload the rest of the
                // session would learn against an EMPTY store.
                if on && !was {
                    reload_store = true;
                }
            }
            if reload_store && enabled.load(Ordering::Relaxed) {
                // Build before the lock (the same Mutex sits on the
                // keystroke path), then content-swap: every Keyboard holds
                // this same Arc; the live snapshot refreshes at the next
                // word commit. Includes our own merged writes — reloading
                // them is an idempotent no-op (the merge already folded
                // disk state in).
                let next = LearningStore::load();
                *store.lock().unwrap_or_else(PoisonError::into_inner) = next;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_initial_never_panics() {
        // Same guarantee as macro_sync::load_initial: whatever the real
        // machine's dirs hold, startup must not crash the engine process.
        let (wiring, _rx) = load_initial();
        let _ = wiring.store.lock().unwrap().is_dirty();
    }
}
