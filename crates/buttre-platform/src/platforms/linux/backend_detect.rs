//! Which input-method path serves this Linux session — decided from several
//! independent signals, never one.
//!
//! Any distro can run GNOME or KDE, X11 or Wayland, IBus or fcitx5, so no
//! single fact identifies the path. [`Evidence`] collects what the session
//! is CONFIGURED for (KWin's `InputMethod`, the `XMODIFIERS`/`*_IM_MODULE`
//! environment, a shell that spawns IBus itself) next to what is RUNNING
//! (daemons on the session bus, the compositor's input-method protocol), and
//! [`judge`] weighs them. A path the session expects but whose daemon is not
//! up yet is [`Judgement::Pending`], not a verdict: at login the tray starts
//! in the same second as ibus-daemon (observed on Ubuntu GNOME — the tray
//! probed first, saw no IBus, and put a dead tray on the top bar), so the
//! caller waits for it instead of concluding.

use crate::shared::method_owner::SurfacePath;
use zbus::names::BusName;

/// An input-method framework buttre can be served through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    Ibus,
    Fcitx5,
}

/// What KWin's `[Wayland] InputMethod=` says (Plasma Wayland only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KwinIm {
    /// Not a Plasma Wayland session — kwinrc is irrelevant here.
    NotKwin,
    /// Plasma Wayland with no input method chosen.
    Unset,
    /// KWin spawns `buttre --ime`.
    Buttre,
    /// KWin spawns IBus's or fcitx5's Wayland frontend.
    Framework(Framework),
    /// Some other input method (an on-screen keyboard, …).
    Foreign(String),
}

/// Everything the decision looks at. Plain data, so [`judge`] is testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub wayland: bool,
    pub kwin: KwinIm,
    /// Majority of `XMODIFIERS`, `GTK_IM_MODULE`, `QT_IM_MODULE(S)`,
    /// `INPUT_METHOD`; `None` when unset or tied.
    pub env_im: Option<Framework>,
    /// GNOME Shell and Cinnamon spawn ibus-daemon themselves.
    pub shell_spawns_ibus: bool,
    pub ibus_running: bool,
    pub fcitx5_running: bool,
    pub fcitx5_addon: bool,
    /// The compositor offers `zwp_input_method_manager_v2` (sway, Hyprland,
    /// other wlroots) — `buttre --ime` can serve it directly.
    pub compositor_im_v2: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judgement {
    Settled(SurfacePath),
    /// The session is configured for this framework but its daemon is not
    /// running (yet). Ask again later; [`judge`] with `wait_for_expected =
    /// false` gives the verdict once waiting has run out.
    Pending(Framework),
}

impl Evidence {
    /// The framework the session is set up for, strongest signal first:
    /// KWin's own choice, then the IM environment, then a shell that
    /// starts IBus on its own.
    pub fn expected(&self) -> Option<Framework> {
        match self.kwin {
            KwinIm::Framework(f) => Some(f),
            _ => self
                .env_im
                .or(self.shell_spawns_ibus.then_some(Framework::Ibus)),
        }
    }

    fn running(&self, f: Framework) -> bool {
        match f {
            Framework::Ibus => self.ibus_running,
            Framework::Fcitx5 => self.fcitx5_running,
        }
    }
}

/// Weigh the evidence. `wait_for_expected`: report a configured-but-absent
/// framework as [`Judgement::Pending`] instead of deciding without it.
pub fn judge(e: &Evidence, wait_for_expected: bool) -> Judgement {
    // KWin spawns buttre itself: that IS the typing path, even with
    // ibus-daemon also alive in the session (Plasma often runs it).
    if e.kwin == KwinIm::Buttre {
        return Judgement::Settled(SurfacePath::LinuxWaylandNative);
    }
    let expected = e.expected();
    if let Some(f) = expected {
        if wait_for_expected && !e.running(f) {
            return Judgement::Pending(f);
        }
    }
    // A running daemon decides; when both run, the one the session is
    // configured for wins (fcitx5 when nothing says which).
    let served = match (e.fcitx5_running, e.ibus_running) {
        (true, true) => Some(expected.unwrap_or(Framework::Fcitx5)),
        (true, false) => Some(Framework::Fcitx5),
        (false, true) => Some(Framework::Ibus),
        (false, false) => None,
    };
    let surface = match served {
        Some(Framework::Fcitx5) if e.fcitx5_addon => SurfacePath::LinuxFcitx5,
        Some(Framework::Fcitx5) => SurfacePath::LinuxFcitx5NoAddon,
        Some(Framework::Ibus) => SurfacePath::LinuxIbus,
        // Plasma Wayland that does not spawn buttre: typing cannot work
        // until the user picks buttre in Virtual Keyboard.
        None if e.kwin != KwinIm::NotKwin => SurfacePath::LinuxKwinUnconfigured,
        None if e.wayland && e.compositor_im_v2 => SurfacePath::LinuxWaylandNative,
        None => SurfacePath::LinuxUndetected,
    };
    Judgement::Settled(surface)
}

/// Collect [`Evidence`] from the live session. Blocking (session-bus round
/// trips, one Wayland registry round trip) — startup / doctor / the tray's
/// slow re-check only, never a keystroke path.
pub fn gather() -> Evidence {
    let desktops = current_desktops();
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
    let kde = desktops.iter().any(|d| d == "kde");
    let kwin = if !wayland {
        KwinIm::NotKwin
    } else {
        match super::kwin_ime::kwinrc_input_method() {
            Some(v) if v.contains("buttre") && (kde || desktops.is_empty()) => KwinIm::Buttre,
            _ if !kde => KwinIm::NotKwin,
            None => KwinIm::Unset,
            Some(v) => classify_kwin_im(&v),
        }
    };
    let session = zbus::blocking::Connection::session().ok();
    let dbus = session
        .as_ref()
        .and_then(|conn| zbus::blocking::fdo::DBusProxy::new(conn).ok());
    let has_owner = |name: &str| -> bool {
        let (Some(proxy), Ok(bus_name)) = (&dbus, BusName::try_from(name)) else {
            return false;
        };
        proxy.name_has_owner(bus_name).unwrap_or(false)
    };
    let owner_pid = |name: &str| -> Option<u32> {
        let proxy = dbus.as_ref()?;
        proxy
            .get_connection_unix_process_id(BusName::try_from(name).ok()?)
            .ok()
    };
    // fcitx5's IBus frontend owns org.freedesktop.IBus and the portal
    // name and writes an IBus address file naming its own pid, so IBus
    // clients reach fcitx5. Only a process other than fcitx5 is a real
    // ibus-daemon; an unknown pid counts as real.
    let fcitx5_pid = owner_pid("org.fcitx.Fcitx5");
    let not_fcitx5 = |pid: Option<u32>| fcitx5_pid.is_none() || pid != fcitx5_pid;
    let ibus_name_real = |name: &str| has_owner(name) && not_fcitx5(owner_pid(name));
    // The file outlives its writer (an exited fcitx5 leaves one pointing
    // at the still-alive session bus), so a named pid must also be alive.
    let ibus_file_real = |(addr, pid): (String, Option<u32>)| {
        let writer_ok = pid.is_none_or(|p| {
            std::path::Path::new(&format!("/proc/{p}")).exists() && not_fcitx5(Some(p))
        });
        writer_ok && ibus_socket_alive(&addr)
    };
    Evidence {
        wayland,
        compositor_im_v2: wayland
            && matches!(kwin, KwinIm::NotKwin)
            && super::wayland::compositor_offers_input_method_v2(),
        kwin,
        env_im: env_im(|key| std::env::var(key).ok()),
        shell_spawns_ibus: desktops
            .iter()
            .any(|d| d == "gnome" || d == "x-cinnamon" || d == "cinnamon"),
        // ibus-daemon owns org.freedesktop.IBus (ibus-portal the portal
        // name); the address-file fallback catches --no-portal daemons, and
        // requires the advertised socket to accept a connect — a dead
        // daemon leaves the file behind.
        ibus_running: ibus_name_real("org.freedesktop.IBus")
            || ibus_name_real("org.freedesktop.portal.IBus")
            || super::ibus_bus::resolve_ibus_endpoint()
                .ok()
                .is_some_and(ibus_file_real),
        fcitx5_running: fcitx5_pid.is_some() || has_owner("org.fcitx.Fcitx5"),
        fcitx5_addon: fcitx5_addon_installed(),
    }
}

/// `XDG_CURRENT_DESKTOP`, lowercased, split on `:` (`ubuntu:GNOME`).
fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn classify_kwin_im(desktop_file: &str) -> KwinIm {
    let lower = desktop_file.to_lowercase();
    if lower.contains("fcitx") {
        KwinIm::Framework(Framework::Fcitx5)
    } else if lower.contains("ibus") {
        KwinIm::Framework(Framework::Ibus)
    } else {
        KwinIm::Foreign(desktop_file.to_string())
    }
}

/// Majority vote of the IM environment variables (`lookup` = env getter).
fn env_im(lookup: impl Fn(&str) -> Option<String>) -> Option<Framework> {
    let mut ibus = 0;
    let mut fcitx = 0;
    for key in [
        "XMODIFIERS",
        "GTK_IM_MODULE",
        "QT_IM_MODULE",
        "QT_IM_MODULES",
        "INPUT_METHOD",
    ] {
        let value = lookup(key).unwrap_or_default().to_lowercase();
        if value.contains("fcitx") {
            fcitx += 1;
        } else if value.contains("ibus") {
            ibus += 1;
        }
    }
    match ibus.cmp(&fcitx) {
        std::cmp::Ordering::Greater => Some(Framework::Ibus),
        std::cmp::Ordering::Less => Some(Framework::Fcitx5),
        std::cmp::Ordering::Equal => None,
    }
}

/// True when the fcitx5-buttre addon is installed: its manifest is in one of
/// the XDG data dirs fcitx5 itself searches (`XDG_DATA_HOME`, then
/// `XDG_DATA_DIRS` — NixOS and other non-FHS layouts live only there), or in
/// `/usr/share`, the built-in data dir of distro fcitx5 builds.
pub fn fcitx5_addon_installed() -> bool {
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs::data_dir()
        .into_iter()
        .chain(std::env::split_paths(&data_dirs))
        .chain(std::iter::once("/usr/share".into()))
        .any(|d| d.join("fcitx5/addon/buttre.conf").is_file())
}

/// True when an `unix:path=…` IBus address points at a socket that accepts
/// a connection RIGHT NOW. Other address forms are rare for IBus; treat
/// them as alive rather than claim a daemon we can't cheaply verify is down.
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
#[path = "backend_detect_tests.rs"]
mod tests;
