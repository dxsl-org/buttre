//! `AppState` under the enabled/method split (ADR-0003): `input_method` is
//! always a real method, on/off is `enabled`, and neither is derived from the
//! other. Every test builds through `with_settings` to avoid touching the real
//! `settings.toml`. NOTE: `set_method`/`set_enabled`/`toggle` still SAVE to the
//! real path — mutation tests only assert in-memory state.

use buttre_core::state::{AppState, Settings};

#[test]
fn test_new_app_state() {
    // Fresh install: a method is pre-selected but the IME is OFF — installing
    // buttre must not start rewriting keystrokes on its own.
    let state = AppState::with_settings(Settings::default());
    assert!(!state.is_enabled());
    assert_eq!(state.current_method(), "telex");
}

#[test]
fn test_set_method_does_not_touch_enabled() {
    let mut state = AppState::with_settings(Settings::default());
    assert!(!state.is_enabled());

    // Picking a method while off changes ONLY the method (ADR-0003 invariant
    // 2). The turn-on lives with the caller (`select_method` in the tray) so
    // that lower layers never invent an enable the user didn't ask for.
    state.set_method("vni").unwrap();
    assert!(!state.is_enabled());
    assert_eq!(state.current_method(), "vni");
    assert_eq!(state.settings().input_method, "vni");
}

#[test]
fn test_toggle_preserves_the_method() {
    let settings = Settings {
        input_method: "vni".to_string(),
        enabled: true,
        ..Settings::default()
    };
    let mut state = AppState::with_settings(settings);

    // Off…
    state.toggle().unwrap();
    assert!(!state.is_enabled());
    assert_eq!(
        state.current_method(),
        "vni",
        "turning off must not overwrite the method — the old model's \
         last_vietnamese_method stash existed only because it did"
    );

    // …and back on, landing exactly where the user left.
    state.toggle().unwrap();
    assert!(state.is_enabled());
    assert_eq!(state.current_method(), "vni");
}

#[test]
fn test_set_enabled_is_idempotent() {
    let settings = Settings {
        enabled: true,
        ..Settings::default()
    };
    let mut state = AppState::with_settings(settings);

    // Same-value writes must be no-ops (no save, no observer storm) — several
    // surfaces may command the same state repeatedly (tray echo, OS mirror).
    state.set_enabled(true).unwrap();
    assert!(state.is_enabled());
    state.set_enabled(false).unwrap();
    state.set_enabled(false).unwrap();
    assert!(!state.is_enabled());
}

#[test]
fn test_app_state_with_custom_settings() {
    let settings = Settings {
        input_method: "vni".to_string(),
        enabled: true,
        ..Settings::default()
    };

    let state = AppState::with_settings(settings);
    assert!(state.is_enabled());
    assert_eq!(state.current_method(), "vni");
}

#[test]
fn test_enabled_comes_from_settings_not_from_the_method() {
    // The same method loads as on or off purely per the flag — proof the two
    // fields are independent.
    let off = AppState::with_settings(Settings {
        input_method: "telex".to_string(),
        enabled: false,
        ..Settings::default()
    });
    assert!(!off.is_enabled());
    assert_eq!(off.current_method(), "telex");

    let on = AppState::with_settings(Settings {
        input_method: "telex".to_string(),
        enabled: true,
        ..Settings::default()
    });
    assert!(on.is_enabled());
}

#[test]
fn test_default_settings() {
    let settings = Settings::default();
    assert_eq!(settings.input_method, "telex");
    assert!(!settings.enabled);
    assert!(!settings.auto_correct);
    assert!(!settings.shorthand);
    // Autostart defaults ON for a fresh install — an input method is expected
    // to come back after every login. Existing users keep their saved choice.
    assert!(settings.startup);
}

#[test]
fn test_settings_path() {
    let path = Settings::get_path();
    assert!(path.is_ok());
    let path = path.unwrap();
    assert!(path.to_string_lossy().contains("buttre"));
    assert!(path.to_string_lossy().ends_with("settings.toml"));
}
