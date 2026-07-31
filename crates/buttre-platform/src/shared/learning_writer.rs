//! Dedicated save-writer thread for `learning.toml` — shared by every host
//! that writes learning outside the tray's event loop (the Linux engine
//! processes and each TSF host app).
//!
//! One contract: the keyboard layer only ENQUEUES full-state snapshots on
//! an mpsc channel (never disk I/O on a keystroke path); this thread blocks
//! on the channel, debounces a burst down to the LATEST snapshot (each is
//! the full state, so intermediate ones are pure wasted I/O), and persists
//! through `LearningStore::write_atomic_merged` — the many-writer-safe
//! write that folds the file's current on-disk state in first.

use buttre_core::state::learning::{LearningFile, LearningStore};
use std::sync::mpsc;

/// Drain `rx` down to the LATEST queued snapshot (lossless debounce).
fn drain_latest(rx: &mpsc::Receiver<LearningFile>, first: LearningFile) -> LearningFile {
    let mut latest = first;
    while let Ok(file) = rx.try_recv() {
        latest = file;
    }
    latest
}

/// Spawn the writer thread. Exits when every sender is dropped; failure to
/// spawn only disables persistence (logged), never the host process.
pub fn spawn(rx: mpsc::Receiver<LearningFile>) {
    std::thread::Builder::new()
        .name("learning-save".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let latest = drain_latest(&rx, first);
                if let Err(e) = LearningStore::write_atomic_merged(&latest) {
                    tracing::warn!("learning_writer: merged write failed: {e:?}");
                }
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| tracing::warn!("learning_writer: thread failed to spawn: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_latest_keeps_only_the_last_snapshot() {
        let (tx, rx) = mpsc::channel::<LearningFile>();
        let mut a = LearningFile::default();
        a.user_attested.insert("a".into(), 1);
        let mut b = LearningFile::default();
        b.user_attested.insert("b".into(), 1);
        tx.send(b.clone()).unwrap();
        let latest = drain_latest(&rx, a);
        assert!(latest.user_attested.contains_key("b"));
        assert!(!latest.user_attested.contains_key("a"));
    }
}
