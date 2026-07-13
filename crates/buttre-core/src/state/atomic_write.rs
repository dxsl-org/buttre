//! Shared atomic-write helper for the TOML config files under
//! `dirs::data_dir()/buttre/` (`settings.toml`, `learning.toml`,
//! `macros.toml`) — write to a uniquely-named temp file, then rename over
//! the target.
//!
//! ## Why the temp name must be unique per CALL, not just per process
//!
//! These files now have multiple writers across separate PROCESSES (the
//! tray and the config window), so a fixed temp name isn't enough to
//! protect a concurrent read from a half-written file — but a process-ID
//! suffix alone isn't enough EITHER: `cargo test` runs many tests as
//! parallel THREADS within one process (one PID), and two threads racing
//! `fs::write`+`fs::rename` on the SAME temp path can have one thread's
//! rename consume the file out from under the other, which then fails its
//! own rename with "file not found" (observed: `core_tests::test_toggle`
//! flaking on exactly this before the counter below was added). A
//! process-wide atomic counter makes every call's temp name unique
//! regardless of thread or process, closing both gaps at once.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temp path for `path` that is unique across every process AND every
/// thread that might call this concurrently.
pub(crate) fn unique_temp_path(path: &Path, extension: &str) -> PathBuf {
    let n = CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("{extension}.tmp.{}.{n}", std::process::id()))
}
