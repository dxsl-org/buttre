use buttre_core::events::create_event_bus;
use buttre_core::services::SettingsService;
use buttre_core::state::Settings;

#[test]
fn test_create_settings_service() {
    let bus = create_event_bus();
    let settings = Settings::default();
    let service = SettingsService::with_settings(settings, bus);

    // Fresh-install defaults after the enabled/method split (ADR-0003):
    // a real method is pre-selected, and "off" lives in `enabled`, not here.
    assert_eq!(service.input_method(), "telex");
    assert!(!service.settings().enabled);
}

#[test]
fn test_update_settings() {
    let bus = create_event_bus();
    let settings = Settings::default();
    let mut service = SettingsService::with_settings(settings, bus);

    service
        .update(|settings| {
            settings.auto_correct = true;
        })
        .unwrap();

    assert!(service.auto_correct());
}

#[test]
fn test_set_input_method() {
    let bus = create_event_bus();
    let settings = Settings::default();
    let mut service = SettingsService::with_settings(settings, bus);

    service.set_input_method("telex").unwrap();
    assert_eq!(service.input_method(), "telex");
}

#[test]
fn test_with_custom_settings() {
    let bus = create_event_bus();
    let settings = Settings {
        auto_correct: true,
        ..Settings::default()
    };

    let service = SettingsService::with_settings(settings, bus);
    assert!(service.auto_correct());
}
