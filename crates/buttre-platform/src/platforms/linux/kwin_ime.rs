//! KWin (KDE Plasma Wayland) input-method lifecycle control.
//!
//! On Plasma Wayland the compositor OWNS the IME process: kwinrc's
//! `[Wayland] InputMethod=` points at buttre's .desktop file and KWin
//! spawns/respawns `buttre --ime` itself. Quitting the tray therefore does
//! NOT stop typing — KWin keeps (re)starting the engine in the background.
//!
//! The off switch the "Virtual Keyboard" KCM used up to Plasma 6.6 is the
//! writable `enabled` property on `org.kde.kwin.VirtualKeyboard` at
//! `/VirtualKeyboard`: `enabled=false` makes KWin terminate the IME process
//! and stop respawning it; `enabled=true` respawns it immediately.
//!
//! Plasma 6.7 removed that property (replaced by the on-screen-keyboard
//! visibility `mode`, which never stops the process); the only remaining
//! stop is clearing kwinrc's `InputMethod`, which is the user's config. So
//! on those versions the tray's "Thoát" turns `Settings::enabled` off
//! instead — the running engine obeys it live (see `tray_app`).
//!
//! Everything here is best-effort: any failure only means the background
//! engine keeps running, never a tray fault.

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
            in_wayland = section
                .trim_end_matches(']')
                .eq_ignore_ascii_case("Wayland");
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

/// True when this session can drive KWin's IME switch: a Wayland session
/// whose kwinrc input method is buttre. Used by the tray's "Thoát"; the
/// tray-or-not decision weighs this together with the other signals in
/// `backend_detect`.
pub fn manages_buttre_ime() -> bool {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
    if !wayland {
        return false;
    }
    kwinrc_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|c| kwinrc_points_at_buttre(&c))
}

/// Tell the user how to turn buttre on in Plasma (shown by the tray when
/// KWin does not spawn buttre — `SurfacePath::LinuxKwinUnconfigured`).
/// kwinrc is the user's config; buttre never rewrites it. Best-effort: no
/// notification daemon only means no bubble.
pub fn notify_not_enabled() {
    let result = zbus::blocking::Connection::session().and_then(|conn| {
        let hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
            std::collections::HashMap::new();
        conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "buttre",
                0u32,
                "buttre",
                "buttre chưa được bật",
                "Mở System Settings → Keyboard → Virtual Keyboard và chọn \
                 \"buttre (Wayland IME)\" để gõ tiếng Việt.",
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )
        .map(|_| ())
    });
    if let Err(e) = result {
        tracing::warn!("kwin_ime: could not show the enable hint: {e}");
    }
}

/// Flip KWin's IME process on/off (no-op unless [`manages_buttre_ime`];
/// Plasma ≤ 6.6 only — see module docs).
///
/// `false`: KWin terminates `buttre --ime` and stops respawning it — part of
/// the tray's "Thoát". `true`: KWin starts the engine again — tray startup
/// uses this so relaunching the tray brings the process back after a quit.
/// Blocking (one session-bus round trip); call from the tray's event loop
/// only, never from a keystroke path.
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
    });
    match result {
        Ok(()) => tracing::info!("kwin_ime: set VirtualKeyboard.enabled = {enabled}"),
        // Plasma ≥ 6.7: the property is gone — expected, see module docs.
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.DBus.Error.UnknownProperty" =>
        {
            tracing::info!("kwin_ime: KWin has no IME on/off switch (Plasma ≥ 6.7)");
        }
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
