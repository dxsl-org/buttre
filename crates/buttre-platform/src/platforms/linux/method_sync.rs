//! Tray↔engine input-method sync (debug report B5).
//!
//! The tray app and the ibus-daemon-spawned engine are separate processes;
//! `~/.config/buttre/method` is the single source of truth between them:
//!
//! - tray side: `LinuxBackend::on_method_changed` → [`write_method`]
//!   (atomic temp+rename, so the engine never reads a torn file);
//! - engine side: a [`spawn_watcher`] thread (notify, same pattern as
//!   `shared/config_watcher.rs`) re-reads on change and bumps
//!   [`MethodState`]'s generation; each engine object compares generations
//!   per keystroke (one atomic load) and rebuilds its `Keyboard` lazily.
//!
//! "english" IS a synced method id: it means engine-side passthrough (the
//! Unikey model — the engine stays the active IBus engine but stops composing,
//! `EngineBridge::passthrough`). It deliberately does NOT switch the OS input
//! source: on GNOME the Shell owns source state (an external `SetGlobalEngine`
//! desyncs its indicator and is reverted on the next focus change), and when
//! the user has no English xkb source there is no meaningful target anyway.
//! The OS input-source switcher (Super+Space, buttre ⇄ another source) remains
//! an ORTHOGONAL mechanism, mirrored one-way engine→tray via the `enabled`
//! file below — never written by the tray.
//!
//! The LAST hop (engine → GNOME top-bar radio repaint) has its own contract —
//! `RegisterProperties` alone is ignored by GNOME Shell after the first
//! registration; see `ibus_props.rs` module docs ("Panel repaint contract")
//! before touching any property emission.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Method ids the engine knows how to build ("english" = passthrough, no
/// keyboard — see module docs). Anything else falls back to telex on read and
/// is skipped on write (a custom-TOML method silently degrading to telex
/// would be more surprising than the tray switch not applying to IBus).
pub const KNOWN_METHODS: [&str; 4] = ["telex", "vni", "nom", "english"];

/// `~/.config/buttre/method`
pub fn method_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("buttre/method"))
}

/// Atomically write the method id (temp file + rename in the same dir).
/// Unknown ids are skipped — see [`KNOWN_METHODS`].
pub fn write_method(method: &str) -> Result<()> {
    if !KNOWN_METHODS.contains(&method) {
        tracing::debug!("method_sync: skipping non-engine method {method:?}");
        return Ok(());
    }
    let path = method_file_path().ok_or_else(|| anyhow!("no XDG config directory"))?;
    write_method_to(&path, method)
}

fn write_method_to(path: &Path, method: &str) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("method path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(".method.tmp");
    std::fs::write(&tmp, method)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read the method id from a file, normalizing to a known id (fallback:
/// telex — matching the engine's historical default).
fn read_method_from(path: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path) {
        let method = content.trim().to_lowercase();
        if KNOWN_METHODS.contains(&method.as_str()) {
            return method;
        }
        if !method.is_empty() {
            tracing::warn!("method_sync: unknown method {method:?}, defaulting to telex");
        }
    }
    "telex".to_string()
}

/// Read the current method id from the shared file (telex fallback).
///
/// Public so the tray's Store-B watcher can converge on engine-side writes
/// (an IBus-panel switch lands here); mirrors [`MethodState::load`]'s read.
pub fn read_method() -> String {
    method_file_path()
        .map(|p| read_method_from(&p))
        .unwrap_or_else(|| "telex".to_string())
}

/// `~/.config/buttre/enabled` — the engine's active/disabled state ("1"/"0").
///
/// Separate from the `method` file so enable/disable never pollutes the method
/// ids (`KNOWN_METHODS`): the engine is the SOLE writer (on `Enable`/`Disable`
/// from the daemon when buttre becomes / stops being the global engine), and
/// the tray is the sole reader — so the tray can mirror an OS input-source
/// switch (buttre ⇄ English) that never touches Store B.
pub fn enabled_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("buttre/enabled"))
}

/// Atomically persist the engine's enabled state (temp file + rename).
pub fn write_enabled(enabled: bool) -> Result<()> {
    let path = enabled_file_path().ok_or_else(|| anyhow!("no XDG config directory"))?;
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("enabled path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(".enabled.tmp");
    std::fs::write(&tmp, if enabled { "1" } else { "0" })?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Read the engine's enabled state. Absent/unreadable ⇒ `true` (enabled): the
/// tray only ACTS on this from a watcher event (never at startup), so a missing
/// file must not spuriously disable — it means "no disable has happened".
pub fn read_enabled() -> bool {
    enabled_file_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim() != "0")
        .unwrap_or(true)
}

/// Current method + change generation, shared between the watcher thread,
/// the factory (new engines), and live engine objects (lazy rebuild).
pub struct MethodState {
    method: Mutex<String>,
    generation: AtomicU64,
    /// Woken on every genuine method change so an async consumer (the engine's
    /// property-refresh task) can react immediately instead of waiting for the
    /// next keystroke. `None` for backends that don't need the push (Wayland
    /// has no IBus panel to re-publish). Interior-mutable because `load` hands
    /// back an `Arc` before the consumer's `Notify` exists.
    change_notify: Mutex<Option<Arc<Notify>>>,
}

impl MethodState {
    /// Initialize from the method file (telex when absent/invalid).
    pub fn load() -> Arc<Self> {
        let method = method_file_path()
            .map(|p| read_method_from(&p))
            .unwrap_or_else(|| "telex".to_string());
        tracing::info!("method_sync: initial method = {method}");
        Arc::new(Self {
            method: Mutex::new(method),
            generation: AtomicU64::new(0),
            change_notify: Mutex::new(None),
        })
    }

    pub fn method(&self) -> String {
        self.method.lock().unwrap().clone()
    }

    /// One atomic load — cheap enough for the per-keystroke check.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Register the [`Notify`] woken on each change (see the field doc). Set
    /// once by the IBus engine before its watcher starts; a later `set` that
    /// actually changes the method wakes it.
    pub fn set_change_notify(&self, notify: Arc<Notify>) {
        *self.change_notify.lock().unwrap() = Some(notify);
    }

    fn set(&self, method: String) {
        let mut current = self.method.lock().unwrap();
        if *current != method {
            tracing::info!("method_sync: method changed {} -> {}", *current, method);
            *current = method;
            self.generation.fetch_add(1, Ordering::Release);
            // Drop the method lock before waking so the consumer can read
            // `method()` without contending on it.
            drop(current);
            if let Some(notify) = &*self.change_notify.lock().unwrap() {
                notify.notify_one();
            }
        }
    }
}

/// Watch the config dir and refresh `state` when the method file changes.
/// Runs in a plain thread (notify's callbacks are sync); lives for the
/// process lifetime — the daemon owns the engine process, so there is no
/// teardown path to plumb.
pub fn spawn_watcher(state: Arc<MethodState>) {
    let Some(path) = method_file_path() else {
        tracing::warn!("method_sync: no config dir, watcher not started");
        return;
    };
    let Some(dir) = path.parent().map(Path::to_path_buf) else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("method_sync: cannot create {dir:?}: {e}, watcher not started");
        return;
    }

    std::thread::Builder::new()
        .name("buttre-method-watch".into())
        .spawn(move || {
            use notify::{RecursiveMode, Watcher};
            let state_cb = state.clone();
            let file = path.clone();
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    // Any event in the dir is a cue to re-read; the atomic
                    // rename in write_method guarantees a consistent read.
                    if res.is_ok() {
                        state_cb.set(read_method_from(&file));
                    }
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        tracing::warn!("method_sync: watcher init failed: {e}");
                        return;
                    }
                };
            if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
                tracing::warn!("method_sync: watch {dir:?} failed: {e}");
                return;
            }
            tracing::info!("method_sync: watching {dir:?}");
            // Park forever — the watcher lives as long as the thread does.
            loop {
                std::thread::park();
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| tracing::warn!("method_sync: watcher thread spawn failed: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_method_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("buttre-method-sync-{tag}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("method")
    }

    #[test]
    fn write_then_read_round_trips() {
        let path = tmp_method_path("roundtrip");
        write_method_to(&path, "vni").unwrap();
        assert_eq!(read_method_from(&path), "vni");
        write_method_to(&path, "nom").unwrap();
        assert_eq!(read_method_from(&path), "nom");
    }

    #[test]
    fn malformed_or_missing_falls_back_to_telex() {
        let path = tmp_method_path("malformed");
        std::fs::write(&path, "not-a-method\n").unwrap();
        assert_eq!(read_method_from(&path), "telex");
        assert_eq!(read_method_from(Path::new("/nonexistent/method")), "telex");
    }

    #[test]
    fn read_normalizes_case_and_whitespace() {
        let path = tmp_method_path("normalize");
        std::fs::write(&path, "  VNI \n").unwrap();
        assert_eq!(read_method_from(&path), "vni");
    }

    #[test]
    fn state_set_bumps_generation_only_on_change() {
        let state = MethodState {
            method: Mutex::new("telex".into()),
            generation: AtomicU64::new(0),
            change_notify: Mutex::new(None),
        };
        state.set("telex".into());
        assert_eq!(state.generation(), 0);
        state.set("vni".into());
        assert_eq!(state.generation(), 1);
        assert_eq!(state.method(), "vni");
        state.set("vni".into());
        assert_eq!(state.generation(), 1);
    }
}
