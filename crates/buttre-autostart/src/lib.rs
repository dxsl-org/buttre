//! OS autostart registration for buttre. Shared by the tray process
//! (`buttre-platform`'s Tùy chọn → "Tự động khởi động") and the config
//! window (`buttre-config`'s General tab) so both apply the exact same
//! per-OS registration logic — a bug fixed once fixes it everywhere.
//!
//! Registration is reconciled on every tray launch in BOTH directions (see
//! `buttre-platform/src/main.rs`): while the setting is on, a moved/updated
//! executable heals its own registration — the registry/desktop entry always
//! points at the exe that last ran; while it is off, a stale enabled entry
//! (e.g. left behind by an older build) is re-masked so it cannot keep
//! relaunching the tray at login.

/// Register or unregister launching buttre at login for the CURRENT user.
/// Never requires elevation on any platform.
#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "buttre";

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;
    if enabled {
        let exe = std::env::current_exe()?;
        // Quoted: the install path may contain spaces (Program Files).
        // `--autostart` for uniformity with the Linux entry: on Windows it
        // falls through to the tray (owner is always Buttre there), but a
        // login launch stays distinguishable from a user click.
        key.set_value(VALUE_NAME, &format!("\"{}\" --autostart", exe.display()))?;
    } else {
        match key.delete_value(VALUE_NAME) {
            Ok(()) => {}
            // Already absent — turning off twice is not an error.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// XDG autostart entry (`~/.config/autostart/buttre.desktop`) — the
/// freedesktop mechanism every major desktop (GNOME/KDE/XFCE) honors.
///
/// Disable does NOT delete the entry: the `.deb`/`.rpm` package ships a
/// system entry at `/etc/xdg/autostart/buttre.desktop`, and a per-user file of
/// the same basename overrides it. Deleting our per-user file would let the
/// system entry win and the tray would keep starting — so "off" writes a
/// `Hidden=true` MASK, which the spec defines to suppress the same-named entry
/// from every lower-priority directory.
#[cfg(target_os = "linux")]
pub fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("no XDG config dir"))?
        .join("autostart");
    let exe = std::env::current_exe()?;
    linux_impl::write_autostart(&dir, enabled, &exe)
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::path::{Path, PathBuf};

    /// Per-user autostart entry within `dir`. Its basename matches the packaged
    /// system entry so this file overrides it (XDG same-name precedence).
    pub(super) fn entry_path(dir: &Path) -> PathBuf {
        dir.join("buttre.desktop")
    }

    /// Write the per-user autostart entry: a launch entry when `enabled`, a
    /// `Hidden=true` mask when not (see [`super::set_enabled`] for why disable
    /// masks rather than deletes). Splits the path out of `set_enabled` so the
    /// enable/disable/round-trip behavior is unit-testable against a temp dir
    /// without touching the real `$XDG_CONFIG_HOME`.
    pub(super) fn write_autostart(dir: &Path, enabled: bool, exe: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        // `--autostart` (not a bare exe): a login launch must never open a
        // WINDOW. On OS-owned platforms (IBus/fcitx5 — ADR-0003) a bare
        // `buttre` opens the config window, so the flag lets main.rs tell a
        // login launch (exit quietly, the daemon spawns the engine) from a
        // user clicking the app (show the window).
        let content = if enabled {
            format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=buttre\n\
                 Comment=Bộ gõ tiếng Việt\n\
                 Exec=\"{}\" --autostart\n\
                 X-GNOME-Autostart-enabled=true\n",
                exe.display()
            )
        } else {
            // `Hidden=true` is the freedesktop "deleted at this level" marker;
            // `X-GNOME-Autostart-enabled=false` covers GNOME builds that key on
            // it. Either alone suppresses launch — both are written for breadth.
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=buttre\n\
             Hidden=true\n\
             X-GNOME-Autostart-enabled=false\n"
                .to_string()
        };
        std::fs::write(entry_path(dir), content)?;
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux_impl::{entry_path, write_autostart};
    use std::path::{Path, PathBuf};

    fn fresh_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("buttre-autostart-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn enable_writes_launch_entry() {
        let dir = fresh_dir("enable");
        write_autostart(&dir, true, Path::new("/usr/bin/buttre")).unwrap();
        let content = std::fs::read_to_string(entry_path(&dir)).unwrap();
        assert!(content.contains("Exec=\"/usr/bin/buttre\" --autostart"));
        assert!(content.contains("X-GNOME-Autostart-enabled=true"));
        assert!(!content.contains("Hidden=true"));
    }

    #[test]
    fn disable_masks_instead_of_deleting() {
        // Turning autostart off must leave a Hidden override, not remove the
        // file — otherwise the packaged /etc/xdg/autostart entry keeps the tray
        // launching and the toggle appears to do nothing.
        let dir = fresh_dir("disable");
        write_autostart(&dir, true, Path::new("/usr/bin/buttre")).unwrap();
        write_autostart(&dir, false, Path::new("/usr/bin/buttre")).unwrap();
        let path = entry_path(&dir);
        assert!(path.exists(), "disable must leave a masking file behind");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Hidden=true"));
        assert!(content.contains("X-GNOME-Autostart-enabled=false"));
        assert!(!content.contains("enabled=true"));
    }

    #[test]
    fn toggle_off_then_on_round_trips_to_a_launch_entry() {
        let dir = fresh_dir("roundtrip");
        write_autostart(&dir, false, Path::new("/usr/bin/buttre")).unwrap();
        write_autostart(&dir, true, Path::new("/usr/bin/buttre")).unwrap();
        let content = std::fs::read_to_string(entry_path(&dir)).unwrap();
        assert!(content.contains("X-GNOME-Autostart-enabled=true"));
        assert!(!content.contains("Hidden=true"));
    }
}

/// macOS: the IMKit host is launched by the SYSTEM when the input source is
/// selected — there is no tray process to autostart, so this is a
/// deliberate unsupported-with-reason error (the caller reverts the
/// checkbox and logs it).
#[cfg(target_os = "macos")]
pub fn set_enabled(_enabled: bool) -> anyhow::Result<()> {
    anyhow::bail!("autostart không áp dụng trên macOS (IMKit do hệ thống khởi chạy)")
}
