//! What a tray interaction asks for, independent of the tray toolkit: the
//! muda/tray-icon tray (Windows) and the StatusNotifierItem tray (Linux) both
//! translate their clicks into these, and the tray app runs them in one
//! place, so every surface means the same thing.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    /// Flip the IME on/off — tray left-click and "Bật bộ gõ".
    Toggle,
    /// Pick a method (and turn the IME on).
    Select(String),
    /// Open the "Cấu hình…" window.
    OpenConfig,
    /// "Thoát".
    Quit,
}
