use crate::shared::observers::ui_observer::UICallback;
use log::info;
use std::sync::mpsc;

/// Events sent from UIObserver to the main thread
pub enum UIEvent {
    UpdateMenuCheckmarks(String, bool),
    UpdateTrayIcon(String, bool),
}

/// A thread-safe proxy that sends UI update events to the main thread
pub struct MainUICallback {
    sender: mpsc::Sender<UIEvent>,
}

impl MainUICallback {
    pub fn new(sender: mpsc::Sender<UIEvent>) -> Self {
        Self { sender }
    }
}

impl UICallback for MainUICallback {
    fn update_menu_checkmarks(&self, method: &str, enabled: bool) {
        info!("Proxying menu update for: {}", method);
        let _ = self
            .sender
            .send(UIEvent::UpdateMenuCheckmarks(method.to_string(), enabled));
    }

    fn update_tray_icon(&self, method: &str, enabled: bool) {
        info!(
            "Proxying tray update for: {} (enabled: {})",
            method, enabled
        );
        let _ = self
            .sender
            .send(UIEvent::UpdateTrayIcon(method.to_string(), enabled));
    }
}
