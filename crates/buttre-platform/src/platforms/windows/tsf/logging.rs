// SPDX-License-Identifier: GPL-3.0-only
// Logging infrastructure for buttre TSF

use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::{Mutex, Once};

static INIT: Once = Once::new();

/// Directory holding the text service's logs and its diagnostics marker.
///
/// `%LOCALAPPDATA%\buttre` — per-user and writable from inside whatever
/// application has loaded the DLL, which `%ProgramFiles%` is not.
fn log_dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("buttre");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// True when the user has opted into verbose logging by creating
/// `%LOCALAPPDATA%\buttre\tsf-debug` (contents irrelevant, existence is the
/// switch).
///
/// Release builds need this because DEBUG-level events include the characters
/// being typed. Nobody's keystrokes go to disk without them asking for it, and
/// asking is a file they create and delete — reachable without a rebuild,
/// which matters for a DLL that runs inside other people's applications.
fn verbose_requested() -> bool {
    log_dir().is_some_and(|d| d.join("tsf-debug").exists())
}

/// Open this process's log file.
///
/// One file per PID: the DLL is loaded into EVERY application that uses TSF,
/// so a shared file would interleave lines from Word, the browser and the
/// shell with no way to tell them apart — and which application it was is
/// exactly what you need to know when only one of them misbehaves.
fn log_file() -> Option<File> {
    let path = log_dir()?.join(format!("tsf-{}.log", std::process::id()));
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Initialize logging for the text service.
///
/// Writes to a FILE, not stderr. A TSF text service runs inside the host
/// application — Word, a browser, the shell — none of which has a console, so
/// everything previously logged went straight to nowhere. That is why a text
/// service dying after one keystroke produced no evidence at all.
///
/// Level is WARN normally (no keystroke content, rare enough to leave on) and
/// DEBUG in a debug build or when [`verbose_requested`].
pub fn init_logging() {
    INIT.call_once(|| {
        use tracing_subscriber::fmt;

        let verbose = cfg!(debug_assertions) || verbose_requested();
        let level = if verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::WARN
        };

        let builder = fmt()
            .with_max_level(level)
            .with_target(false)
            .with_ansi(false)
            .compact();

        match log_file() {
            Some(file) => {
                builder.with_writer(Mutex::new(file)).try_init().ok();
            }
            // No writable log directory: fall back to stderr rather than
            // losing the subscriber entirely — a console host still sees it.
            None => {
                builder.try_init().ok();
            }
        }
    });
}

/// Trace one message. A no-op unless verbose logging is on, since TRACE sits
/// below both levels [`init_logging`] ever selects.
#[inline(always)]
pub fn log_debug(msg: &str) {
    tracing::trace!("{}", msg);
}
