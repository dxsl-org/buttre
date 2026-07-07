//! IBus Engine Implementation
//!
//! **Tests**: `crates/buttre-platform/tests/platform_linux_tests.rs` (thin
//! layer) and `platform_linux_bridge_tests.rs` (composition semantics).
//!
//! Thin D-Bus adapter over [`EngineBridge`] — ALL composition semantics live
//! in `engine_bridge.rs`, shared with the Wayland-native backend so the two
//! cannot drift. The component lifecycle (private-bus connection, Factory,
//! name request) lives in `ibus_bus.rs`; method-file sync in `method_sync.rs`.
//!
//! Protocol notes (learned against a live ibus-daemon 1.5.29):
//! - Signal signatures MUST match libibus's engine introspection XML — the
//!   daemon subscribes by signature and silently drops mismatches. Engine
//!   `UpdatePreeditText` is 4-arg `(text, cursor_pos, visible, mode)`.
//! - There is no engine-side `HidePreeditText` signal (that's a Panel
//!   method); hide is an update with `visible=false`.
//! - `ContentType` is a write-only property `(uu)`, not a method.
//! - `delete_surrounding_text` is deliberately absent: in the preedit model
//!   the composition is not yet real text (debug report B1).

use super::engine_bridge::{is_break_keysym, is_modifier_keysym, keysym_to_char, EngineBridge};
use super::method_sync::MethodState;
use std::sync::{Arc, Mutex};
use zbus::zvariant;
use zbus::{dbus_interface, SignalContext};

// ============================================================================
// IBus modifier state bitmask (ibus.h)
// ============================================================================

const IBUS_CONTROL_MASK: u32 = 0x04;
const IBUS_MOD1_MASK: u32 = 0x08; // Alt
const IBUS_SUPER_MASK: u32 = 0x40;
/// Key-release events carry this bit; engines act on presses only —
/// processing releases would double every keystroke.
const IBUS_RELEASE_MASK: u32 = 1 << 30;

/// IBusPreeditFocusMode::COMMIT — the client commits a visible preedit when
/// focus changes, so a mouse click elsewhere never eats the current word.
const PREEDIT_FOCUS_COMMIT: u32 = 1;

// ============================================================================
// IBus Engine
// ============================================================================

/// IBus Engine for Vietnamese input — one instance per input context,
/// created by the Factory in `ibus_bus.rs`.
#[derive(Clone)]
pub struct ButtreEngine {
    bridge: Arc<Mutex<EngineBridge>>,
    /// Shared with the method-file watcher (B5). `None` in standalone
    /// construction (tests) — no live method switching there.
    method_state: Option<Arc<MethodState>>,
    /// Last [`MethodState::generation`] this engine applied; compared per
    /// keystroke (one atomic load) for lazy keyboard rebuild on switch.
    seen_generation: u64,
}

impl ButtreEngine {
    pub fn new() -> Self {
        Self::new_with_method("telex")
    }

    pub fn new_with_method(method_name: &str) -> Self {
        Self {
            bridge: Arc::new(Mutex::new(EngineBridge::new(method_name))),
            method_state: None,
            seen_generation: 0,
        }
    }

    /// Factory constructor: builds from the CURRENT shared method and keeps
    /// the state handle for per-keystroke switch detection.
    pub fn new_with_state(state: Arc<MethodState>) -> Self {
        let mut engine = Self::new_with_method(&state.method());
        engine.seen_generation = state.generation();
        engine.method_state = Some(state);
        engine
    }

    /// Current preedit text (test/diagnostic accessor).
    pub fn preedit_text(&self) -> String {
        self.bridge.lock().unwrap().preedit().to_string()
    }
}

impl Default for ButtreEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// True when a control modifier (Ctrl / Alt / Super) is active.
/// We pass these through without engine processing to preserve shortcuts.
fn is_control_combo(state: u32) -> bool {
    state & (IBUS_CONTROL_MASK | IBUS_MOD1_MASK | IBUS_SUPER_MASK) != 0
}

// ============================================================================
// IBusText D-Bus structure builder
// ============================================================================

/// Build an IBusText value for D-Bus signal arguments.
///
/// IBus wire format: `(sa{sv}sv)` wrapped in a `v` (variant).
/// - "IBusText" (type-name string)
/// - {} (empty attachments dict)
/// - text (the actual string)
/// - variant containing IBusAttrList `(sa{sv}av)` with no attributes
fn build_ibus_text(text: &str) -> zvariant::Value<'static> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    let empty: HashMap<String, Value<'static>> = HashMap::new();

    // IBusAttrList: ("IBusAttrList", a{sv}={}, av=[])
    let attr_list: Value<'static> = Value::from((
        "IBusAttrList".to_string(),
        empty.clone(),
        Vec::<Value<'static>>::new(),
    ));

    // IBusText: ("IBusText", a{sv}={}, text, v=attr_list)
    Value::from(("IBusText".to_string(), empty, text.to_string(), attr_list))
}

// ============================================================================
// D-Bus interface implementation
// ============================================================================

#[dbus_interface(name = "org.freedesktop.IBus.Engine")]
impl ButtreEngine {
    // --- Signal declarations (bodies generated by zbus macro) ---

    #[dbus_interface(signal)]
    async fn commit_text(ctx: &SignalContext<'_>, text: zvariant::Value<'_>) -> zbus::Result<()>;

    /// 4-arg per libibus XML; `mode` is IBusPreeditFocusMode (see const).
    #[dbus_interface(signal)]
    async fn update_preedit_text(
        ctx: &SignalContext<'_>,
        text: zvariant::Value<'_>,
        cursor_pos: u32,
        visible: bool,
        mode: u32,
    ) -> zbus::Result<()>;

    // --- Method handlers ---

    /// Process keyboard event. Returns true if the event was consumed.
    async fn process_key_event(
        &mut self,
        #[zbus(signal_context)] ctx: SignalContext<'_>,
        keyval: u32,
        _keycode: u32,
        state: u32,
    ) -> bool {
        tracing::debug!(
            "ProcessKeyEvent: keyval=0x{:x}, state=0x{:x}",
            keyval,
            state
        );

        // Key releases would double every keystroke — presses only.
        if state & IBUS_RELEASE_MASK != 0 {
            return false;
        }

        // Apply a pending tray-side method switch before processing (B5).
        self.sync_method(&ctx).await;

        // Shortcuts (Ctrl+C, Alt+F4, …): commit the pending word so it isn't
        // lost, then let the app receive the combo.
        if is_control_combo(state) {
            let outcome = self.bridge.lock().unwrap().flush_pending();
            self.emit_ops(&ctx, outcome.ops).await;
            return false;
        }

        // Bare modifier presses don't touch the composition.
        if is_modifier_keysym(keyval) {
            return false;
        }

        // Navigation/editing keys end the word and pass through.
        if is_break_keysym(keyval) {
            let outcome = self.bridge.lock().unwrap().flush_pending();
            self.emit_ops(&ctx, outcome.ops).await;
            return false;
        }

        let Some(ch) = keysym_to_char(keyval) else {
            return false;
        };

        let outcome = {
            let mut bridge = self.bridge.lock().unwrap();
            if ch == '\x08' {
                bridge.backspace()
            } else {
                bridge.process_char(ch)
            }
        };
        self.emit_ops(&ctx, outcome.ops).await;
        outcome.handled
    }

    fn focus_in(&mut self) {
        tracing::info!("FocusIn");
    }

    /// Focus loss: the CLIENT commits the visible preedit itself (we send
    /// every preedit update with mode=COMMIT), so the engine only resets its
    /// state — emitting our own commit here would double the word.
    fn focus_out(&mut self) {
        tracing::info!("FocusOut");
        self.bridge.lock().unwrap().discard();
    }

    fn enable(&mut self) {
        tracing::info!("Enable");
    }

    fn disable(&mut self) {
        tracing::info!("Disable");
        self.bridge.lock().unwrap().discard();
    }

    /// Daemon-initiated reset: discard the composition WITHOUT committing.
    async fn reset(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        tracing::debug!("Reset");
        let outcome = self.bridge.lock().unwrap().discard();
        self.emit_ops(&ctx, outcome.ops).await;
    }

    fn set_cursor_location(&mut self, x: i32, y: i32, w: i32, h: i32) {
        tracing::debug!("SetCursorLocation: x={}, y={}, w={}, h={}", x, y, w, h);
    }

    fn set_capabilities(&mut self, caps: u32) {
        tracing::debug!("SetCapabilities: {}", caps);
    }

    /// `ContentType` is a write-only PROPERTY `(uu)` in the engine
    /// interface (purpose, hints; purpose 8 = password). Reserved for
    /// suppressing learning in sensitive fields.
    #[dbus_interface(property)]
    fn content_type(&self) -> (u32, u32) {
        (0, 0)
    }

    #[dbus_interface(property)]
    fn set_content_type(&mut self, content_type: (u32, u32)) {
        tracing::debug!(
            "ContentType: purpose={}, hints={}",
            content_type.0,
            content_type.1
        );
    }
}

// ============================================================================
// Signal-emission helpers
// ============================================================================

impl ButtreEngine {
    /// Emit bridge operations as IBus signals, in order. Signals are queued
    /// before the ProcessKeyEvent reply, so a Commit always lands before a
    /// forwarded (unhandled) key.
    async fn emit_ops(&self, ctx: &SignalContext<'_>, ops: Vec<super::engine_bridge::ImeOp>) {
        use super::engine_bridge::ImeOp;
        for op in ops {
            match op {
                ImeOp::Preedit(text) => {
                    let cursor = text.chars().count() as u32;
                    Self::update_preedit_text(
                        ctx,
                        build_ibus_text(&text),
                        cursor,
                        !text.is_empty(),
                        PREEDIT_FOCUS_COMMIT,
                    )
                    .await
                    .ok();
                }
                ImeOp::Commit(text) => {
                    Self::commit_text(ctx, build_ibus_text(&text)).await.ok();
                }
            }
        }
    }

    /// Apply a pending tray-side method switch (B5).
    async fn sync_method(&mut self, ctx: &SignalContext<'_>) {
        let Some(state) = &self.method_state else {
            return;
        };
        let generation = state.generation();
        if generation == self.seen_generation {
            return;
        }
        self.seen_generation = generation;
        let method = state.method();
        // rebuild returns owned Option — the lock guard is dropped at the
        // end of this statement, so no lock is held across the await.
        let outcome = self.bridge.lock().unwrap().rebuild(&method);
        match outcome {
            Some(outcome) => {
                self.emit_ops(ctx, outcome.ops).await;
                tracing::info!("Engine switched to method {method}");
            }
            // Build failed (already logged): keep the current keyboard
            // rather than crash. seen_generation is advanced so we don't
            // retry the same broken method every keystroke.
            None => tracing::warn!("Method switch to {method} failed; keeping current"),
        }
    }
}
