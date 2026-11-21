// SPDX-License-Identifier: GPL-3.0-only
//! Open buttre's settings window from inside a host application.
//!
//! The text service is a DLL loaded into Word, Notepad, Terminal…; the
//! settings window is `buttre.exe --config`, installed next to that DLL.
//! It has its own single-instance lock, so a second request while it is open
//! is harmless.

use std::path::PathBuf;
use std::process::Command;

/// `buttre.exe` next to this DLL.
fn buttre_exe() -> Option<PathBuf> {
    let dll = super::registration::get_dll_path().ok()?;
    Some(dll.parent()?.join("buttre.exe"))
}

/// Start `buttre.exe --config`. Failures are logged and swallowed: a host
/// that forbids child processes (some sandboxed apps) must not lose its IME
/// over a settings click.
pub fn open_settings() {
    let Some(exe) = buttre_exe() else {
        tracing::warn!("settings: cannot locate buttre.exe next to the text service DLL");
        return;
    };
    match Command::new(&exe).arg("--config").spawn() {
        Ok(_) => tracing::debug!("settings: started {exe:?} --config"),
        Err(e) => tracing::warn!("settings: could not start {exe:?}: {e}"),
    }
}
