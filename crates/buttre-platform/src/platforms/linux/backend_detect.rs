//! Linux IME-backend detection with a fixed priority: fcitx5 → IBus →
//! Wayland `--ime` (plan: fcitx-backend-auto-priority, Phase 1).
//!
//! One machine should serve buttre through ONE path at a time — two live
//! engine registrations would both write the tri-surface sync files
//! (`~/.config/buttre/method`, `enabled`) and fight each other. This module
//! is the single source of truth for "which path should this machine use":
//! the tray logs it, `buttre --doctor` prints it, and the install scripts
//! mirror the same order.
//!
//! Note fcitx5 is detected but not yet SERVED: buttre's fcitx5 addon
//! (Phase 3 — in-process C++ shim, fcitx5 has no out-of-process engine
//! protocol) does not exist yet, so a detected fcitx5 currently means
//! "warn about the conflict", not "register with fcitx5".

use zbus::names::BusName;

/// The engine path buttre should use, ordered by priority (top wins).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeBackend {
    /// fcitx5 is running. buttre's addon is not built yet (Phase 3) —
    /// today this only drives conflict warnings and installer guidance.
    Fcitx5,
    /// ibus-daemon is running (or its private-bus address resolves).
    IBus,
    /// A Wayland session without either daemon — the compositor-managed
    /// `buttre --ime` path (KWin spawns it per kwinrc).
    WaylandIme,
}

/// Raw probe results, separated from [`pick`] so the priority rule is a
/// pure function the tests can pin down without a live bus.
#[derive(Debug, Clone, Copy, Default)]
pub struct Probes {
    pub fcitx5: bool,
    pub ibus: bool,
    pub wayland: bool,
}

/// The fixed priority rule: fcitx5 → IBus → Wayland. `None` = headless /
/// X11 with no IM daemon; there is nothing for buttre to register with.
pub fn pick(p: Probes) -> Option<ImeBackend> {
    if p.fcitx5 {
        Some(ImeBackend::Fcitx5)
    } else if p.ibus {
        Some(ImeBackend::IBus)
    } else if p.wayland {
        Some(ImeBackend::WaylandIme)
    } else {
        None
    }
}

/// Probe the live session. Blocking (session-bus round trips) — call from
/// startup or `--doctor`, never from a keystroke path.
pub fn probe() -> Probes {
    let session = zbus::blocking::Connection::session().ok();
    let has_owner = |name: &str| -> bool {
        let Some(conn) = &session else { return false };
        let Ok(bus_name) = BusName::try_from(name) else {
            return false;
        };
        zbus::blocking::fdo::DBusProxy::new(conn)
            .ok()
            .and_then(|p| p.name_has_owner(bus_name).ok())
            .unwrap_or(false)
    };
    Probes {
        fcitx5: has_owner("org.fcitx.Fcitx5"),
        // ibus-daemon owns org.freedesktop.IBus on the SESSION bus (the
        // portal name is the modern spelling); the address-file fallback
        // catches daemons started with --no-portal or a broken session bus.
        // The file alone is NOT trusted — a dead daemon leaves it behind
        // (observed: stale X11-session file on a Wayland login) — so the
        // fallback also requires the advertised socket to accept a connect.
        ibus: has_owner("org.freedesktop.IBus")
            || has_owner("org.freedesktop.portal.IBus")
            || super::ibus_bus::resolve_ibus_address()
                .ok()
                .is_some_and(|addr| ibus_socket_alive(&addr)),
        wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
    }
}

/// [`probe`] + [`pick`] in one call — what the tray and doctor use.
pub fn detect() -> Option<ImeBackend> {
    pick(probe())
}

/// True when an `unix:path=…` IBus address points at a socket that accepts
/// a connection RIGHT NOW. Non-unix address forms (abstract sockets, TCP)
/// are rare for IBus; treat them as alive rather than claim the daemon is
/// down on an address we can't cheaply verify.
fn ibus_socket_alive(addr: &str) -> bool {
    match addr
        .strip_prefix("unix:path=")
        .map(|rest| rest.split(',').next().unwrap_or(rest))
    {
        Some(path) => std::os::unix::net::UnixStream::connect(path).is_ok(),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_is_fcitx_then_ibus_then_wayland() {
        let all = Probes {
            fcitx5: true,
            ibus: true,
            wayland: true,
        };
        assert_eq!(pick(all), Some(ImeBackend::Fcitx5));
        assert_eq!(
            pick(Probes {
                fcitx5: false,
                ..all
            }),
            Some(ImeBackend::IBus)
        );
        assert_eq!(
            pick(Probes {
                fcitx5: false,
                ibus: false,
                ..all
            }),
            Some(ImeBackend::WaylandIme)
        );
        assert_eq!(pick(Probes::default()), None);
    }
}
