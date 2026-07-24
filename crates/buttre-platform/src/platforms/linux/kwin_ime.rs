//! KWin (KDE Plasma Wayland) input-method lifecycle control.
//!
//! On Plasma Wayland the compositor OWNS the IME process: kwinrc's
//! `[Wayland] InputMethod=` points at buttre's .desktop file and KWin
//! spawns/respawns `buttre --ime` itself. Quitting the tray therefore does
//! NOT stop typing — KWin keeps (re)starting the engine in the background.
//!
//! The supported off switch is the same one the "Virtual Keyboard" KCM
//! uses: the writable `enabled` property on `org.kde.kwin.VirtualKeyboard`
//! at `/VirtualKeyboard`. Verified live on Plasma 6: `enabled=false` makes
//! KWin terminate the IME process and stop respawning it; `enabled=true`
//! respawns it immediately.
//!
//! Everything here is best-effort: any failure only means the background
//! engine keeps running (the pre-existing behaviour), never a tray fault.

use std::path::PathBuf;

/// `~/.config/kwinrc`
fn kwinrc_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("kwinrc"))
}

/// Extract the `[Wayland] InputMethod=` value from kwinrc content. Manual
/// INI scan — kwinrc is KDE's own config, only ever read here, and pulling
/// an INI crate for one key is not worth it. The section check matters:
/// `InputMethod` could legitimately appear under other groups.
fn kwinrc_value(content: &str) -> Option<String> {
    let mut in_wayland = false;
    for line in content.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[') {
            in_wayland = section.trim_end_matches(']').eq_ignore_ascii_case("Wayland");
            continue;
        }
        if in_wayland {
            if let Some(value) = line.strip_prefix("InputMethod") {
                // Tolerate KDE's locale/flag markers: `InputMethod[$e]=...`.
                if let Some(eq) = value.find('=') {
                    return Some(value[eq + 1..].trim().to_string());
                }
            }
        }
    }
    None
}

/// The configured `[Wayland] InputMethod=` value, if any — what
/// `buttre --doctor` reports.
pub fn kwinrc_input_method() -> Option<String> {
    kwinrc_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .as_deref()
        .and_then(kwinrc_value)
}

/// True when kwinrc's `[Wayland] InputMethod=` names buttre.
fn kwinrc_points_at_buttre(content: &str) -> bool {
    kwinrc_value(content).is_some_and(|v| v.contains("buttre"))
}

/// True when this session can and should drive KWin's IME switch: a
/// Wayland session whose kwinrc input method is buttre.
fn manages_buttre_ime() -> bool {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
    if !wayland {
        return false;
    }
    kwinrc_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|c| kwinrc_points_at_buttre(&c))
}

/// Flip KWin's IME on/off (no-op unless [`manages_buttre_ime`]).
///
/// `false`: KWin terminates `buttre --ime` and stops respawning it — the
/// tray's "Thoát" uses this so quit really quits. `true`: KWin starts the
/// engine again — tray startup uses this so relaunching the tray restores
/// typing after a previous quit. Blocking (one session-bus round trip);
/// call from the tray's event loop only, never from a keystroke path.
pub fn set_kwin_ime_enabled(enabled: bool) {
    if !manages_buttre_ime() {
        return;
    }
    let result = zbus::blocking::Connection::session().and_then(|conn| {
        conn.call_method(
            Some("org.kde.KWin"),
            "/VirtualKeyboard",
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &(
                "org.kde.kwin.VirtualKeyboard",
                "enabled",
                zbus::zvariant::Value::from(enabled),
            ),
        )
        .map(|_| ())
        .map_err(Into::into)
    });
    match result {
        Ok(()) => tracing::info!("kwin_ime: set VirtualKeyboard.enabled = {enabled}"),
        // KWin absent (not Plasma, X11 session) or interface changed —
        // background engine just keeps its previous state.
        Err(e) => tracing::warn!("kwin_ime: could not set enabled={enabled}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_buttre_in_wayland_section() {
        let rc = "[SomeGroup]\nInputMethod=other\n\n[Wayland]\nInputMethod=/home/u/.local/share/applications/buttre-ime.desktop\n";
        assert!(kwinrc_points_at_buttre(rc));
    }

    #[test]
    fn tolerates_locale_flag_marker() {
        let rc = "[Wayland]\nInputMethod[$e]=/path/buttre-ime.desktop\n";
        assert!(kwinrc_points_at_buttre(rc));
    }

    #[test]
    fn ignores_other_sections_and_other_imes() {
        assert!(!kwinrc_points_at_buttre(
            "[Other]\nInputMethod=/path/buttre-ime.desktop\n"
        ));
        assert!(!kwinrc_points_at_buttre(
            "[Wayland]\nInputMethod=/usr/share/applications/org.fcitx.Fcitx5.desktop\n"
        ));
        assert!(!kwinrc_points_at_buttre(""));
    }
}
