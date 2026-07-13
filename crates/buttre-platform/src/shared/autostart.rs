//! OS autostart registration for the tray app (Tùy chọn → "Tự động khởi
//! động").
//!
//! Registration is re-applied on every launch while the setting is on (see
//! `main.rs`), so a moved/updated executable heals its own registration —
//! the registry/desktop entry always points at the exe that last ran.

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
        key.set_value(VALUE_NAME, &format!("\"{}\"", exe.display()))?;
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
#[cfg(target_os = "linux")]
pub fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("no XDG config dir"))?
        .join("autostart");
    let path = dir.join("buttre.desktop");
    if enabled {
        std::fs::create_dir_all(&dir)?;
        let exe = std::env::current_exe()?;
        std::fs::write(
            &path,
            format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=buttre\n\
                 Comment=Bộ gõ tiếng Việt\n\
                 Exec=\"{}\"\n\
                 X-GNOME-Autostart-enabled=true\n",
                exe.display()
            ),
        )?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// macOS: the IMKit host is launched by the SYSTEM when the input source is
/// selected — there is no tray process to autostart, so this is a
/// deliberate unsupported-with-reason error (the caller reverts the
/// checkbox and logs it).
#[cfg(target_os = "macos")]
pub fn set_enabled(_enabled: bool) -> anyhow::Result<()> {
    anyhow::bail!("autostart không áp dụng trên macOS (IMKit do hệ thống khởi chạy)")
}
