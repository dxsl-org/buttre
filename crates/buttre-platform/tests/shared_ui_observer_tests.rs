use buttre_core::StateObserver;
use buttre_platform::shared::observers::ui_observer::{UICallback, UIObserver};
use std::sync::{Arc, Mutex};

struct MockUICallback {
    last_method: Mutex<String>,
    last_enabled: Mutex<bool>,
}

impl MockUICallback {
    fn new() -> Self {
        Self {
            last_method: Mutex::new(String::new()),
            last_enabled: Mutex::new(false),
        }
    }
}

impl UICallback for MockUICallback {
    fn update_menu_checkmarks(&self, method: &str, enabled: bool) {
        *self.last_method.lock().unwrap() = method.to_string();
        *self.last_enabled.lock().unwrap() = enabled;
    }

    fn update_tray_icon(&self, method: &str, enabled: bool) {
        *self.last_method.lock().unwrap() = method.to_string();
        *self.last_enabled.lock().unwrap() = enabled;
    }
}

#[test]
fn test_ui_observer() {
    let callback = Arc::new(MockUICallback::new());
    let observer = UIObserver::new(callback.clone());

    // Simulate state change
    observer.on_method_changed("telex", true);

    // Verify callback was called
    assert_eq!(*callback.last_method.lock().unwrap(), "telex");
    assert!(*callback.last_enabled.lock().unwrap());
}

#[test]
fn turning_off_keeps_the_method_in_the_notification() {
    // The enabled/method split (ADR-0003): OFF arrives as (method, false), so
    // the UI can grey the method's own icon instead of pretending the method
    // changed. The method must never read "english".
    let callback = Arc::new(MockUICallback::new());
    let observer = UIObserver::new(callback.clone());

    observer.on_method_changed("vni", false);

    assert_eq!(*callback.last_method.lock().unwrap(), "vni");
    assert!(!*callback.last_enabled.lock().unwrap());
}
