//! Hotkey Service - Global hotkey management with event bus integration
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/service_hotkey_tests.rs`.
//!
//! This service wraps the ButtreHotkeyManager and integrates it with the event bus,
//! automatically publishing hotkey events when keys are pressed.

use crate::hotkey::{ButtreHotkeyManager, HotkeyAction as CoreHotkeyAction};
use crate::events::{SharedEventBus, AppEvent, HotkeyAction};
use anyhow::Result;

/// Hotkey Service - Manages global hotkeys with event bus integration
///
/// This service provides a clean interface for hotkey management and
/// automatically publishes events when hotkeys are pressed.
///
/// # Example
///
/// ```rust,ignore
/// use buttre_core::services::HotkeyService;
/// use buttre_core::events::create_event_bus;
///
/// let bus = create_event_bus();
/// let service = HotkeyService::new(bus.clone())?;
///
/// // In your event loop:
/// service.poll(); // This will publish HotkeyPressed events
/// ```
pub struct HotkeyService {
    /// Underlying hotkey manager
    manager: ButtreHotkeyManager,
    
    /// Event bus for publishing events
    event_bus: SharedEventBus,
}

impl HotkeyService {
    /// Create a new HotkeyService
    ///
    /// This will register default hotkeys:
    /// - Ctrl+Shift+Space: Toggle
    /// - Ctrl+Shift+F1/1: Telex
    /// - Ctrl+Shift+F2/2: VNI
    /// - Ctrl+Shift+F3/3: Nôm
    ///
    /// # Arguments
    ///
    /// * `event_bus` - Shared event bus for publishing events
    pub fn new(event_bus: SharedEventBus) -> Result<Self> {
        let manager = ButtreHotkeyManager::new()?;
        
        Ok(Self {
            manager,
            event_bus,
        })
    }
    
    /// Poll for hotkey events
    ///
    /// This should be called regularly (e.g., in your event loop).
    /// When a hotkey is pressed, it will publish a HotkeyPressed event
    /// to the event bus.
    ///
    /// This method is non-blocking and will return immediately.
    pub fn poll(&self) {
        if let Some(action) = self.manager.check_hotkey() {
            // Convert core hotkey action to event hotkey action.
            // `ToggleLastWord` (event-sourcing-completion Phase 4) has no
            // event-bus equivalent yet — this event-bus service is not
            // wired into the running app (buttre-platform/src/main.rs polls
            // `ButtreHotkeyManager` directly instead), so skip publishing
            // for it rather than invent a lossy mapping onto an unrelated
            // `events::HotkeyAction` variant.
            let event_action = match action {
                CoreHotkeyAction::Toggle => HotkeyAction::Toggle,
                CoreHotkeyAction::Telex => HotkeyAction::Telex,
                CoreHotkeyAction::Vni => HotkeyAction::Vni,
                CoreHotkeyAction::Nom => HotkeyAction::Nom,
                CoreHotkeyAction::Custom(i) => HotkeyAction::Custom(i),
                CoreHotkeyAction::ToggleLastWord => return,
            };

            // Publish event
            self.event_bus.publish(AppEvent::HotkeyPressed(event_action));
        }
    }
    
    /// Register custom method hotkeys
    ///
    /// This will register hotkeys for custom methods:
    /// - Custom 1 → Ctrl+Shift+4
    /// - Custom 2 → Ctrl+Shift+5
    /// - ...
    /// - Custom 7 → Ctrl+Shift+0
    ///
    /// # Arguments
    ///
    /// * `count` - Number of custom methods to register (max 7)
    pub fn register_custom_methods(&mut self, count: usize) -> Result<()> {
        self.manager.register_custom_methods(count)
            .map_err(|e| anyhow::anyhow!("Failed to register custom methods: {}", e))
    }
}

