use buttre_core::state::{AppState, Settings};

#[test]
fn test_new_app_state() {
    // Use default settings directly to avoid environment contamination
    let state = AppState::with_settings(Settings::default());
    // Default settings start with English
    assert!(!state.is_enabled());
    assert_eq!(state.current_method(), "english");
}

#[test]
fn test_set_method() {
    let mut state = AppState::with_settings(Settings::default());

    // Switch to Telex
    state.set_method("telex").unwrap();
    assert!(state.is_enabled());
    assert_eq!(state.current_method(), "telex");
    assert_eq!(state.settings().input_method, "telex");

    // Switch to English
    state.set_method("english").unwrap();
    assert!(!state.is_enabled());
    assert_eq!(state.current_method(), "english");
}

#[test]
fn test_toggle() {
    let mut state = AppState::with_settings(Settings::default());

    // Start with English, toggle should switch to Telex (default)
    assert_eq!(state.current_method(), "english");
    state.toggle().unwrap();
    assert_eq!(state.current_method(), "telex");
    assert!(state.is_enabled());

    // Toggle back to English
    state.toggle().unwrap();
    assert_eq!(state.current_method(), "english");
    assert!(!state.is_enabled());

    // Set to VNI, then toggle to English, then toggle back should restore VNI
    state.set_method("vni").unwrap();
    state.toggle().unwrap(); // -> English
    assert_eq!(state.current_method(), "english");
    state.toggle().unwrap(); // -> VNI (remembered)
    assert_eq!(state.current_method(), "vni");
}

#[test]
fn test_app_state_with_custom_settings() {
    let settings = Settings {
        input_method: "vni".to_string(),
        ..Settings::default()
    };

    let state = AppState::with_settings(settings);
    assert!(state.is_enabled());
    assert_eq!(state.current_method(), "vni");
}

#[test]
fn test_default_settings() {
    let settings = Settings::default();
    assert_eq!(settings.input_method, "english");
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
