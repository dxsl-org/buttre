//! File logging for the IMKit host.
//!
//! The input method is a background-only app launched by the system — it has
//! no console, so without a subscriber every `tracing` event (keyboard build
//! failure, missing Nôm dictionary, method persist failure) is dropped.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Mutex, Once};

static INIT: Once = Once::new();

/// `~/Library/Logs/buttre` — where Console.app looks for per-user app logs.
fn log_dir() -> Option<PathBuf> {
    let dir = dirs::home_dir()?.join("Library/Logs/buttre");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Install the subscriber once per process.
///
/// Level is WARN normally (no keystroke content reaches disk) and DEBUG in a
/// debug build or when the user creates `~/Library/Logs/buttre/debug` —
/// DEBUG events include typed text, so it is opt-in by an explicit file.
pub fn init() {
    INIT.call_once(|| {
        let dir = log_dir();
        let verbose =
            cfg!(debug_assertions) || dir.as_ref().is_some_and(|d| d.join("debug").exists());
        let level = if verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::WARN
        };
        let builder = tracing_subscriber::fmt()
            .with_max_level(level)
            .with_target(false)
            .with_ansi(false)
            .compact();
        let file = dir.and_then(|d| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(d.join("imk.log"))
                .ok()
        });
        match file {
            Some(file) => {
                builder.with_writer(Mutex::new(file)).try_init().ok();
            }
            None => {
                builder.try_init().ok();
            }
        }
    });
}
