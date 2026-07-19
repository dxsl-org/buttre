//! Shorthand/gõ tắt store load + live reload for the Linux engine processes
//! (`buttre --ibus`, `buttre --ime`). Mirrors [`super::method_sync`]'s shape:
//! the tray/config window write `macros.toml` and `settings.toml`'s
//! `shorthand` flag from a SEPARATE process, so the daemon-spawned engine
//! needs its own background watcher to pick up either change.
//!
//! **Content-swap model** (phase-02 architecture): the store handle
//! (`Arc<Mutex<MacroStore>>`) is attached to every `Keyboard` ONCE, at
//! `EngineBridge` construction, and never detached. Enable/disable and
//! hand-edits are applied by swapping the store's CONTENTS in place —
//! [`reload`] — never by attach/detach: a disabled/empty store makes every
//! macro lookup miss, which is byte-identical to shorthand being off. This
//! is simpler than the tray's set/clear pattern because the composition seam
//! (Phase 1) caches nothing that needs clearing.
//!
//! `MacroStore::get_path` and `Settings::get_path` both resolve under
//! `dirs::data_dir()/buttre` today, so the two watch targets currently
//! coincide — but they are looked up and deduplicated independently here so
//! this keeps working if that ever diverges.

use buttre_core::state::macros::MacroStore;
use buttre_core::state::Settings;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Build the initial store for engine-process startup, gated on
/// `Settings::shorthand`. `Settings::load` never fails (it degrades to
/// `Settings::default()`, i.e. `shorthand = false`), so this can't crash the
/// daemon-spawned process even when its environment can't resolve the
/// config directory — it just starts with shorthand off.
pub fn load_initial() -> Arc<Mutex<MacroStore>> {
    let shorthand = Settings::load().shorthand;
    tracing::info!("macro_sync: initial shorthand = {shorthand}");
    Arc::new(Mutex::new(MacroStore::load_gated(shorthand)))
}

/// Re-read `Settings::shorthand` and swap `store`'s contents to match —
/// the single reload spelling both watch callbacks below share.
fn reload(store: &Arc<Mutex<MacroStore>>) {
    let shorthand = Settings::load().shorthand;
    *store.lock().unwrap_or_else(|e| e.into_inner()) = MacroStore::load_gated(shorthand);
    tracing::info!("macro_sync: reloaded (shorthand={shorthand})");
}

/// Collect the (deduplicated) directories that hold `macros.toml` and
/// `settings.toml`. Both getters create their directory as a side effect of
/// resolving it, so nothing else needs to `create_dir_all` here.
fn watch_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match MacroStore::get_path() {
        Ok(path) => push_parent(&mut dirs, path),
        Err(e) => tracing::warn!("macro_sync: macros.toml path unresolved: {e}"),
    }
    match Settings::get_path() {
        Ok(path) => push_parent(&mut dirs, path),
        Err(e) => tracing::warn!("macro_sync: settings.toml path unresolved: {e}"),
    }
    dirs
}

fn push_parent(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if let Some(dir) = path.parent() {
        let dir = dir.to_path_buf();
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
}

/// Watch `macros.toml`'s and `settings.toml`'s directories, reloading
/// `store` in place whenever either changes. Runs in a plain thread
/// (notify's callbacks are sync); lives for the process lifetime — the
/// daemon/compositor owns the engine process, so there is no teardown path
/// to plumb (mirrors `method_sync::spawn_watcher`).
pub fn spawn_watcher(store: Arc<Mutex<MacroStore>>) {
    let dirs = watch_dirs();
    if dirs.is_empty() {
        tracing::warn!("macro_sync: no watchable directory found, watcher not started");
        return;
    }

    std::thread::Builder::new()
        .name("buttre-macro-watch".into())
        .spawn(move || {
            use notify::{RecursiveMode, Watcher};
            let store_cb = store.clone();
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    // The watched dirs also hold unrelated state (method
                    // file, learning.toml), so filter to the two files that
                    // feed the store — an unfiltered reload would re-read
                    // both files on every neighbor write.
                    let Ok(event) = res else {
                        return;
                    };
                    let relevant = event.paths.iter().any(|p| {
                        matches!(
                            p.file_name().and_then(|n| n.to_str()),
                            Some("macros.toml") | Some("settings.toml")
                        )
                    });
                    if relevant {
                        reload(&store_cb);
                    }
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        tracing::warn!("macro_sync: watcher init failed: {e}");
                        return;
                    }
                };
            for dir in &dirs {
                if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                    tracing::warn!("macro_sync: watch {dir:?} failed: {e}");
                }
            }
            tracing::info!("macro_sync: watching {dirs:?}");
            // Park forever — the watcher lives as long as the thread does.
            loop {
                std::thread::park();
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| tracing::warn!("macro_sync: watcher thread spawn failed: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_parent_dedupes_identical_directories() {
        let mut dirs = Vec::new();
        push_parent(&mut dirs, PathBuf::from("/a/buttre/macros.toml"));
        push_parent(&mut dirs, PathBuf::from("/a/buttre/settings.toml"));
        assert_eq!(dirs, vec![PathBuf::from("/a/buttre")]);
    }

    #[test]
    fn push_parent_keeps_distinct_directories() {
        let mut dirs = Vec::new();
        push_parent(&mut dirs, PathBuf::from("/a/macros.toml"));
        push_parent(&mut dirs, PathBuf::from("/b/settings.toml"));
        assert_eq!(dirs, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn load_initial_never_panics() {
        // Smoke test mirroring `MacroStore::load`'s own test: the real
        // environment's dirs may or may not resolve, but this must never
        // panic — same guarantee `Settings::load`/`MacroStore::load` give.
        let store = load_initial();
        let _ = store.lock().unwrap().is_empty();
    }
}
