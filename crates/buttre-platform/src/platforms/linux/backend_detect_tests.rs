//! Decision table for [`judge`] — one test per real session shape. A change
//! of verdict for any of them must be deliberate.

use super::*;

/// A session with nothing configured and nothing running.
fn bare() -> Evidence {
    Evidence {
        wayland: false,
        kwin: KwinIm::NotKwin,
        env_im: None,
        shell_spawns_ibus: false,
        ibus_running: false,
        fcitx5_running: false,
        fcitx5_addon: false,
        compositor_im_v2: false,
    }
}

fn ubuntu_gnome() -> Evidence {
    Evidence {
        wayland: true,
        env_im: Some(Framework::Ibus),
        shell_spawns_ibus: true,
        ibus_running: true,
        ..bare()
    }
}

fn settled(s: SurfacePath) -> Judgement {
    Judgement::Settled(s)
}

#[test]
fn gnome_login_race_waits_for_ibus_instead_of_concluding() {
    // Observed: the tray probed before ibus-daemon started.
    let login = Evidence {
        ibus_running: false,
        ..ubuntu_gnome()
    };
    assert_eq!(judge(&login, true), Judgement::Pending(Framework::Ibus));
    assert_eq!(
        judge(&ubuntu_gnome(), true),
        settled(SurfacePath::LinuxIbus)
    );
    // Waiting ran out with no daemon: no IBus to hand typing to.
    assert_eq!(judge(&login, false), settled(SurfacePath::LinuxUndetected));
}

#[test]
fn gnome_with_ibus_but_no_im_env_still_expects_ibus() {
    // GNOME Shell spawns ibus-daemon itself; the env vars are not required.
    let e = Evidence {
        env_im: None,
        ibus_running: false,
        ..ubuntu_gnome()
    };
    assert_eq!(judge(&e, true), Judgement::Pending(Framework::Ibus));
}

#[test]
fn cinnamon_x11_with_ibus_is_os_owned() {
    let mint = Evidence {
        wayland: false,
        ..ubuntu_gnome()
    };
    assert_eq!(judge(&mint, true), settled(SurfacePath::LinuxIbus));
}

#[test]
fn kwin_spawning_buttre_wins_over_a_running_ibus() {
    let plasma = Evidence {
        wayland: true,
        kwin: KwinIm::Buttre,
        env_im: Some(Framework::Ibus),
        ibus_running: true,
        ..bare()
    };
    assert_eq!(
        judge(&plasma, true),
        settled(SurfacePath::LinuxWaylandNative)
    );
}

#[test]
fn plasma_without_buttre_selected_is_unconfigured() {
    let fresh = Evidence {
        wayland: true,
        kwin: KwinIm::Unset,
        ..bare()
    };
    assert_eq!(
        judge(&fresh, true),
        settled(SurfacePath::LinuxKwinUnconfigured)
    );
    let osk = Evidence {
        kwin: KwinIm::Foreign("maliit-keyboard.desktop".into()),
        ..fresh
    };
    assert_eq!(
        judge(&osk, true),
        settled(SurfacePath::LinuxKwinUnconfigured)
    );
}

#[test]
fn plasma_set_to_fcitx5_follows_fcitx5() {
    let e = Evidence {
        wayland: true,
        kwin: KwinIm::Framework(Framework::Fcitx5),
        fcitx5_addon: true,
        ..bare()
    };
    assert_eq!(judge(&e, true), Judgement::Pending(Framework::Fcitx5));
    let up = Evidence {
        fcitx5_running: true,
        ..e
    };
    assert_eq!(judge(&up, true), settled(SurfacePath::LinuxFcitx5));
}

#[test]
fn configured_framework_wins_when_both_daemons_run() {
    // GNOME user on fcitx5: the shell still starts ibus-daemon.
    let e = Evidence {
        env_im: Some(Framework::Fcitx5),
        fcitx5_running: true,
        ..ubuntu_gnome()
    };
    assert_eq!(judge(&e, true), settled(SurfacePath::LinuxFcitx5NoAddon));
    // fcitx5 not up yet: wait for it rather than settle on the shell's IBus.
    let early = Evidence {
        fcitx5_running: false,
        ..e
    };
    assert_eq!(judge(&early, true), Judgement::Pending(Framework::Fcitx5));
    assert_eq!(judge(&early, false), settled(SurfacePath::LinuxIbus));
}

#[test]
fn wlroots_compositor_is_served_natively() {
    let sway = Evidence {
        wayland: true,
        compositor_im_v2: true,
        ..bare()
    };
    assert_eq!(judge(&sway, true), settled(SurfacePath::LinuxWaylandNative));
    // Wayland without the protocol and without a daemon: nothing serves it.
    let other = Evidence {
        compositor_im_v2: false,
        ..sway
    };
    assert_eq!(judge(&other, true), settled(SurfacePath::LinuxUndetected));
}

#[test]
fn env_vote_needs_a_majority() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    };
    assert_eq!(
        env_im(env(&[
            ("XMODIFIERS", "@im=ibus"),
            ("QT_IM_MODULES", "wayland;ibus")
        ])),
        Some(Framework::Ibus)
    );
    assert_eq!(
        env_im(env(&[
            ("XMODIFIERS", "@im=fcitx"),
            ("GTK_IM_MODULE", "fcitx")
        ])),
        Some(Framework::Fcitx5)
    );
    assert_eq!(
        env_im(env(&[
            ("XMODIFIERS", "@im=ibus"),
            ("GTK_IM_MODULE", "fcitx")
        ])),
        None
    );
    assert_eq!(env_im(env(&[])), None);
}

#[test]
fn kwin_input_method_files_are_classified() {
    assert_eq!(
        classify_kwin_im("/usr/share/applications/org.fcitx.Fcitx5.desktop"),
        KwinIm::Framework(Framework::Fcitx5)
    );
    assert_eq!(
        classify_kwin_im("/usr/share/applications/org.freedesktop.IBus.Panel.Wayland.Gtk3.desktop"),
        KwinIm::Framework(Framework::Ibus)
    );
    assert!(matches!(
        classify_kwin_im("com.github.maliit.keyboard.desktop"),
        KwinIm::Foreign(_)
    ));
}
