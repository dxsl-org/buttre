//! Self-re-arming directory watcher shared by the tray and engine processes.
//!
//! A plain `notify` watch binds to the directory INODE captured at start-up.
//! If that directory is later deleted and re-created (a dev reset, a
//! reinstall, `rm -rf ~/.config/buttre`), the kernel drops the watch and
//! `notify` never re-establishes it — every consumer silently goes deaf for
//! the rest of the process lifetime (observed live: the tray held a watch on
//! an orphaned `~/.local/share/buttre` inode). This module fixes that by
//! watching BOTH the target directory and its parent: when the parent
//! reports the target being created (or renamed into place), the watch is
//! re-armed on the new inode and the handler receives [`WatchCue::Rearmed`]
//! so it can re-read state that may have changed while unwatched.

use notify::{EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Why the handler is being called.
pub enum WatchCue<'a> {
    /// A filesystem event under the watched directory. Filter by
    /// `event.paths` as with a plain `notify` callback.
    Event(&'a notify::Event),
    /// The watched directory was replaced and the watch was re-established
    /// on the new inode. Anything may have changed while unwatched — treat
    /// as "assume every watched file changed" and re-read.
    Rearmed,
}

/// Watch `dir` (non-recursive) for the process lifetime, re-arming the watch
/// whenever the directory is deleted and re-created. `label` names the
/// watcher thread and prefixes its log lines. Returns `false` (with a log)
/// when the directory can't be created or the watcher thread can't spawn —
/// callers degrade exactly as they did when a plain watch failed.
///
/// The handler runs on the watcher thread: keep it short (send on a channel,
/// swap an `Arc` target) — blocking it delays later events.
pub fn spawn_dir_watch(
    label: &'static str,
    dir: PathBuf,
    handler: impl Fn(WatchCue<'_>) + Send + 'static,
) -> bool {
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("{label}: cannot create {dir:?}, watch disabled: {e}");
        return false;
    }
    // Canonicalize BEFORE arming: notify's macOS (FSEvents) backend reports
    // event paths with symlink components resolved (e.g. `/var/...` — itself
    // a symlink to `/private/var/...` — comes back as `/private/var/...`).
    // Comparing un-resolved `dir` against those paths below would silently
    // never match, so the handler would never fire despite events arriving
    // (caught by `missing_dir_is_created`/`survives_dir_delete_and_recreate`
    // failing on macOS CI with a resolved-path callback dir). Canonicalize
    // once so every later `==`/`parent()` comparison uses the same form the
    // backend hands back.
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("{label}: watcher init failed: {e}");
            return false;
        }
    };
    // Arm the watches HERE, before returning — a caller may write the very
    // file it asked us to watch immediately after this returns, and a watch
    // armed later (on the thread) would silently miss that write.
    if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        tracing::warn!("{label}: watch {dir:?} failed: {e}");
        return false;
    }
    // Parent watch is what makes re-arming possible; without it a replaced
    // dir is still a dead watch, so failing to get it is worth a warning —
    // but the primary watch works, so keep going.
    if let Some(parent) = dir.parent() {
        if let Err(e) = watcher.watch(parent, RecursiveMode::NonRecursive) {
            tracing::warn!(
                "{label}: parent watch {parent:?} failed (no re-arm on dir replacement): {e}"
            );
        }
    }
    tracing::info!("{label}: watching {dir:?}");
    std::thread::Builder::new()
        // Linux truncates thread names to 15 bytes — keep the prefix short
        // so at least part of the label survives.
        .name(format!("bt-w-{label}"))
        .spawn(move || event_loop(label, &dir, watcher, &rx, &handler))
        .map(|_| true)
        .unwrap_or_else(|e| {
            tracing::warn!("{label}: watcher thread spawn failed: {e}");
            false
        })
}

/// Owns the watcher for the thread's lifetime (dropping it would kill the
/// watch). Events are pumped through a channel rather than handled in
/// notify's callback because re-arming needs `&mut` access to the watcher —
/// which the callback itself can never have.
fn event_loop(
    label: &'static str,
    dir: &Path,
    mut watcher: notify::RecommendedWatcher,
    rx: &mpsc::Receiver<notify::Result<notify::Event>>,
    handler: &dyn Fn(WatchCue<'_>),
) {
    // rx.iter() ends only if every sender is gone, i.e. the watcher (which
    // owns the sending callback) died — so this loop runs for the process
    // lifetime in normal operation.
    for res in rx.iter() {
        let event = match res {
            Ok(event) => event,
            Err(e) => {
                tracing::warn!("{label}: watch error: {e}");
                continue;
            }
        };
        tracing::debug!("{label}: event {:?} paths={:?}", event.kind, event.paths);
        if event.paths.iter().any(|p| p == dir) {
            handle_dir_event(label, dir, &mut watcher, &event.kind, handler);
        } else if event.paths.iter().any(|p| p.parent() == Some(dir)) {
            handler(WatchCue::Event(&event));
        }
        // Events about the parent's OTHER children are none of our business.
    }
    tracing::warn!("{label}: event channel closed, watch ended");
}

/// An event whose path is the watched dir itself: re-arm on (re)creation,
/// clean up bookkeeping on removal.
fn handle_dir_event(
    label: &'static str,
    dir: &Path,
    watcher: &mut notify::RecommendedWatcher,
    kind: &EventKind,
    handler: &dyn Fn(WatchCue<'_>),
) {
    match kind {
        EventKind::Create(_) | EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
            // The dir (re)appeared under the parent watch. Drop the stale
            // watch entry if the kernel hasn't already (unwatch after
            // IN_IGNORED reports "not found" — ignore it) and bind to the
            // new inode.
            let _ = watcher.unwatch(dir);
            match watcher.watch(dir, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    tracing::info!("{label}: {dir:?} replaced, watch re-armed");
                    handler(WatchCue::Rearmed);
                }
                // A create-then-delete race lands here; the next Create
                // event retries, so this is a warning, not a dead end.
                Err(e) => tracing::warn!("{label}: re-arm on {dir:?} failed: {e}"),
            }
        }
        EventKind::Remove(_) => {
            let _ = watcher.unwatch(dir);
            tracing::info!("{label}: {dir:?} removed, waiting for it to reappear");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Surface the module's tracing warnings in `--nocapture` runs — watch
    /// failures inside the watcher thread are otherwise invisible to tests.
    fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("debug")
            .with_test_writer()
            .try_init();
    }

    fn unique_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("buttre-fs-watch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Drain cues until one arrives for `name` (or timeout). Rearmed counts
    /// as a match — it means "assume changed", which is what callers do.
    fn wait_for_file_cue(rx: &mpsc::Receiver<Option<String>>, name: &str) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while let Ok(cue) =
            rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        {
            match cue {
                None => return true, // Rearmed
                Some(n) if n == name => return true,
                Some(_) => {}
            }
        }
        false
    }

    #[test]
    fn survives_dir_delete_and_recreate() {
        init_test_tracing();
        let dir = unique_dir("rearm");
        let (tx, rx) = mpsc::channel::<Option<String>>();
        let watch_dir = dir.clone();
        assert!(spawn_dir_watch("test-rearm", watch_dir, move |cue| {
            let _ = tx.send(match cue {
                WatchCue::Rearmed => None,
                WatchCue::Event(event) => Some(
                    event
                        .paths
                        .iter()
                        .filter_map(|p| p.file_name())
                        .next()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
            });
        }));

        std::fs::write(dir.join("first"), "1").unwrap();
        assert!(wait_for_file_cue(&rx, "first"), "no cue for initial write");

        // Replace the directory wholesale — the plain-watch failure mode.
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        // Give the parent-watch Create event time to re-arm before writing.
        std::thread::sleep(Duration::from_millis(500));

        std::fs::write(dir.join("second"), "2").unwrap();
        assert!(
            wait_for_file_cue(&rx, "second"),
            "watch did not survive dir replacement"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Minimal notify-crate sanity probe, byte-for-byte the pre-fs_watch
    /// production pattern: watcher on the test thread, kept alive as a
    /// local. Isolates "notify broken in this environment" from "fs_watch
    /// has a bug".
    #[test]
    fn notify_probe() {
        use notify::{RecursiveMode, Watcher};
        let dir = unique_dir("probe");
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = mpsc::channel();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let _ = tx.send(res);
        })
        .unwrap();
        w.watch(&dir, RecursiveMode::NonRecursive).unwrap();
        std::fs::write(dir.join("f"), "x").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5));
        eprintln!("notify_probe got: {got:?}");
        assert!(got.is_ok(), "no event from plain notify watcher");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_dir_is_created() {
        init_test_tracing();
        let dir = unique_dir("create");
        let (tx, rx) = mpsc::channel::<Option<String>>();
        let watch_dir = dir.clone();
        assert!(spawn_dir_watch("test-create", watch_dir, move |cue| {
            if let WatchCue::Event(event) = cue {
                if let Some(name) = event.paths.iter().filter_map(|p| p.file_name()).next() {
                    let _ = tx.send(Some(name.to_string_lossy().into_owned()));
                }
            }
        }));
        assert!(dir.is_dir(), "spawn_dir_watch must create the directory");
        std::fs::write(dir.join("hello"), "x").unwrap();
        assert!(wait_for_file_cue(&rx, "hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
