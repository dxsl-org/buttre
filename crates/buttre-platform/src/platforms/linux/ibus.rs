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
use super::ibus_props;
use super::method_sync::{self, MethodState};
use buttre_core::state::macros::MacroStore;
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

// Keyvals used to drive the Nôm candidate popup (X11 keysyms).
const KEY_SPACE: u32 = 0x20;
const KEY_RETURN: u32 = 0xFF0D;
const KEY_ESCAPE: u32 = 0xFF1B;
const KEY_1: u32 = 0x31;
const KEY_9: u32 = 0x39;
const KEY_LEFT: u32 = 0xFF51;
const KEY_UP: u32 = 0xFF52;
const KEY_RIGHT: u32 = 0xFF53;
const KEY_DOWN: u32 = 0xFF54;
const KEY_PAGE_UP: u32 = 0xFF55;
const KEY_PAGE_DOWN: u32 = 0xFF56;

/// `IBusCapabilite::IBUS_CAP_SURROUNDING_TEXT` = `1 << 5` — the client can
/// report and delete text around the cursor. Preferred delete primitive for the
/// no-preedit model (`delete_surrounding_text`). (NOT `4`; that bit is
/// `IBUS_CAP_LOOKUP_TABLE`. Verified against live SetCapabilities values:
/// gnome-text-editor reports 41 = 0x29 with this bit set, a browser reports 9
/// without it.)
const IBUS_CAP_SURROUNDING_TEXT: u32 = 1 << 5;

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
    /// Shared `Settings::use_preedit` mirror (`macro_sync`). `None` in
    /// standalone construction (tests). `true` = preedit/underline model,
    /// `false` = commit-as-you-go. Consulted per keystroke.
    use_preedit: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Client capability bits from the last `SetCapabilities`. The no-preedit
    /// model is only engaged when `caps & IBUS_CAP_SURROUNDING_TEXT != 0`
    /// (the client can delete already-committed text); otherwise the engine
    /// stays on preedit even with the setting off, so input is never corrupted.
    caps: u32,
    /// The no-preedit state currently applied to the bridge — compared per
    /// keystroke against the desired state so a setting/capability change flips
    /// the model exactly once. `false` = preedit (the safe startup default).
    applied_no_preedit: bool,
    /// This engine object's D-Bus path (assigned by the factory). `None` in
    /// standalone construction (tests). Published into [`Self::focused`] on
    /// `focus_in` so the async property-refresh task knows where to emit.
    path: Option<zvariant::OwnedObjectPath>,
    /// Shared "currently focused engine path" — the async property-refresh
    /// task (see `ibus_bus::run_engine`) emits `RegisterProperties` here when
    /// the method changes externally (tray/config), so the panel radio follows
    /// immediately instead of only on the next keystroke. `None` in tests.
    focused: Option<Arc<Mutex<Option<zvariant::OwnedObjectPath>>>>,
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
            use_preedit: None,
            caps: 0,
            applied_no_preedit: false,
            path: None,
            focused: None,
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

    /// Factory constructor used once shorthand is wired in: same as
    /// [`Self::new_with_state`], plus the shared macro store and the
    /// strict-spelling mirror, injected into the bridge at construction so
    /// they survive every later method-switch `rebuild`
    /// (`EngineBridge::rebuild` re-applies both).
    #[allow(clippy::too_many_arguments)] // reason: factory wires every shared handle in one call
    pub fn new_with_state_and_macros(
        state: Arc<MethodState>,
        macros: Arc<Mutex<MacroStore>>,
        strict: Arc<std::sync::atomic::AtomicBool>,
        use_preedit: Arc<std::sync::atomic::AtomicBool>,
        path: zvariant::OwnedObjectPath,
        focused: Arc<Mutex<Option<zvariant::OwnedObjectPath>>>,
    ) -> Self {
        let mut bridge = EngineBridge::new_with_macros(&state.method(), macros);
        bridge.set_strict_flag(strict);
        Self {
            seen_generation: state.generation(),
            bridge: Arc::new(Mutex::new(bridge)),
            method_state: Some(state),
            use_preedit: Some(use_preedit),
            caps: 0,
            applied_no_preedit: false,
            path: Some(path),
            focused: Some(focused),
        }
    }

    /// Current preedit text (test/diagnostic accessor).
    pub fn preedit_text(&self) -> String {
        self.bridge.lock().unwrap().preedit().to_string()
    }

    /// Push the current method's radio states to the panel: a full
    /// `RegisterProperties` (refreshes the daemon's property cache and panels
    /// that honor re-registration), then one `UpdateProperty` per radio — the
    /// only signal GNOME Shell applies after an engine's first registration
    /// (see [`Self::update_property`]). Best-effort: a failed emit only stales
    /// the panel radio, never typing.
    pub(crate) async fn publish_method_props(ctx: &SignalContext<'_>, method: &str) {
        if let Err(e) = Self::register_properties(ctx, ibus_props::method_prop_list(method)).await {
            tracing::warn!("publish_method_props: RegisterProperties failed: {e}");
        }
        for prop in ibus_props::method_prop_updates(method) {
            if let Err(e) = Self::update_property(ctx, prop).await {
                tracing::warn!("publish_method_props: UpdateProperty failed: {e}");
            }
        }
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
pub(crate) fn build_ibus_text(text: &str) -> zvariant::Value<'static> {
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

    /// Publish the property menu the IBus panel (GNOME Shell) renders. `props`
    /// is an `IBusPropList` variant built in [`ibus_props`]; the daemon
    /// subscribes by signature, so its wire shape must match libibus exactly.
    #[dbus_interface(signal)]
    pub(crate) async fn register_properties(
        ctx: &SignalContext<'_>,
        props: zvariant::Value<'_>,
    ) -> zbus::Result<()>;

    /// Update ONE property in-place on the panel. `prop` is an `IBusProperty`
    /// variant ([`ibus_props::method_prop_updates`]). This is the ONLY channel
    /// GNOME Shell keeps open for radio-state changes after an engine's first
    /// registration — its `register-properties` handler is one-shot — so every
    /// method switch must be pushed through here or the top-bar radio never
    /// repaints (the root cause of "tray switch not reflected in IBus menu").
    #[dbus_interface(signal)]
    pub(crate) async fn update_property(
        ctx: &SignalContext<'_>,
        prop: zvariant::Value<'_>,
    ) -> zbus::Result<()>;

    /// Replace the candidate popup the panel renders. `table` is an
    /// `IBusLookupTable` variant ([`ibus_props::build_lookup_table`]); `visible`
    /// shows it in the same call. Signature-matched to libibus.
    #[dbus_interface(signal)]
    async fn update_lookup_table(
        ctx: &SignalContext<'_>,
        table: zvariant::Value<'_>,
        visible: bool,
    ) -> zbus::Result<()>;

    /// Show the last-sent lookup table (belt-and-suspenders alongside the
    /// `visible=true` in `UpdateLookupTable`).
    #[dbus_interface(signal)]
    async fn show_lookup_table(ctx: &SignalContext<'_>) -> zbus::Result<()>;

    /// Hide the candidate popup.
    #[dbus_interface(signal)]
    async fn hide_lookup_table(ctx: &SignalContext<'_>) -> zbus::Result<()>;

    /// Delete text around the cursor in the focused client (no-preedit mode).
    /// `offset` is signed CHARACTERS relative to the cursor; `(-n, n)` deletes
    /// the `n` characters immediately before it. Only sent to clients that
    /// advertised `IBUS_CAP_SURROUNDING_TEXT`.
    #[dbus_interface(signal)]
    async fn delete_surrounding_text(
        ctx: &SignalContext<'_>,
        offset: i32,
        nchars: u32,
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

        // Apply a pending preedit-model change (setting toggle or capability
        // update) before processing this key.
        self.sync_use_preedit(&ctx).await;

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

        // Nôm candidate popup open: intercept selection/cancel keys BEFORE the
        // break-keysym flush below — Escape and Return are break keysyms, and
        // Space/digits are printable (they'd otherwise be composed). Other keys
        // (more letters, backspace) fall through and refine or clear the list.
        let candidate_count = self.bridge.lock().unwrap().candidate_count();
        if candidate_count > 0 {
            let page = ibus_props::LOOKUP_PAGE_SIZE as usize;
            let outcome = match keyval {
                KEY_ESCAPE => Some(self.bridge.lock().unwrap().discard()),
                KEY_RETURN | KEY_SPACE => Some(self.bridge.lock().unwrap().select_current()),
                // Vertical or horizontal panel: next/prev either way.
                KEY_DOWN | KEY_RIGHT => Some(self.bridge.lock().unwrap().cursor_next()),
                KEY_UP | KEY_LEFT => Some(self.bridge.lock().unwrap().cursor_prev()),
                KEY_PAGE_DOWN => Some(self.bridge.lock().unwrap().cursor_page_down(page)),
                KEY_PAGE_UP => Some(self.bridge.lock().unwrap().cursor_page_up(page)),
                // Number keys pick a slot on the current page. Always consume
                // 1..=9 while the popup is open (out-of-range is a no-op) so a
                // stray digit never leaks into the Nôm composition.
                KEY_1..=KEY_9 => Some(
                    self.bridge
                        .lock()
                        .unwrap()
                        .select_at_page((keyval - KEY_1) as usize, page),
                ),
                // Anything else (more letters, backspace) refines the list.
                _ => None,
            };
            if let Some(outcome) = outcome {
                self.emit_ops(&ctx, outcome.ops).await;
                return true;
            }
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

    /// The daemon focuses a text field: (re)publish the property menu so the
    /// IBus panel shows the method radios with the current method checked.
    /// Emitted per focus rather than once at construction because the panel
    /// tracks properties against the focused engine, not the component.
    async fn focus_in(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        tracing::info!("FocusIn");
        // Mark this engine as the focused one so the async property-refresh
        // task (ibus_bus) knows which path to re-publish on an external method
        // change. Cleared on focus_out.
        if let (Some(focused), Some(path)) = (&self.focused, &self.path) {
            *focused.lock().unwrap() = Some(path.clone());
        }
        let current = self
            .method_state
            .as_ref()
            .map(|s| s.method())
            .unwrap_or_else(|| "telex".to_string());
        // Register (consumed by GNOME Shell's one-shot handler after an engine
        // switch) AND per-radio updates (repaint when already registered) — a
        // method switched while this engine was unfocused lands either way.
        Self::publish_method_props(&ctx, &current).await;
    }

    /// IBus panel → engine: the user clicked a property (a method radio). Route
    /// the click to the shared method file so the tray and every live engine
    /// converge on the same method (`method_sync`), then re-publish the list so
    /// the radio check follows the selection immediately (the panel does not
    /// move a radio on its own — the engine owns that state). Unknown keys are
    /// ignored; only the engine-buildable ids in `KNOWN_METHODS` are honored.
    async fn property_activate(
        &mut self,
        #[zbus(signal_context)] ctx: SignalContext<'_>,
        name: &str,
        state: u32,
    ) {
        tracing::info!("PropertyActivate: {name} (state={state})");
        // The settings launcher is a NORMAL item, not a radio — route it by key
        // (its state is meaningless) and open the config window, then stop.
        if name == ibus_props::CONFIG_KEY {
            Self::open_config_window();
            return;
        }
        // A radio-group click arrives as MULTIPLE PropertyActivate calls: the
        // newly-selected radio (CHECKED) plus every other radio (UNCHECKED).
        // `method_for_activation` keeps only the checked, known one — acting on
        // an unchecked de-select would overwrite the real choice (observed:
        // clicking Telex sent telex=checked then vni=unchecked → stuck on VNI).
        let Some(method) = ibus_props::method_for_activation(name, state) else {
            return;
        };
        if let Err(e) = method_sync::write_method(method) {
            tracing::warn!("PropertyActivate: write_method({method}) failed: {e}");
        }
        Self::publish_method_props(&ctx, method).await;
    }

    /// Focus loss: the CLIENT commits the visible preedit itself (we send
    /// every preedit update with mode=COMMIT), so the engine only resets its
    /// state — emitting our own preedit-clear would erase the word. A Nôm
    /// candidate popup, however, must be hidden explicitly so it can't linger
    /// over the newly focused field (some panels don't auto-hide it).
    async fn focus_out(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        tracing::info!("FocusOut");
        // Relinquish focused-engine ownership so the refresh task doesn't emit
        // to an unfocused path. Guard on identity: a focus_in on the NEXT
        // engine may already have overwritten the slot, and clearing it then
        // would drop that live target.
        if let (Some(focused), Some(path)) = (&self.focused, &self.path) {
            let mut slot = focused.lock().unwrap();
            if slot.as_ref() == Some(path) {
                *slot = None;
            }
        }
        let had_candidates = {
            let mut bridge = self.bridge.lock().unwrap();
            let had = bridge.candidate_count() > 0;
            bridge.discard();
            had
        };
        if had_candidates {
            Self::hide_lookup_table(&ctx).await.ok();
        }
    }

    fn enable(&mut self) {
        tracing::info!("Enable");
        // Mirror the active state to the tray: buttre just became the global
        // engine (OS input-source switch or startup). The tray reflects the
        // real Vietnamese method again. Best-effort — a failed write only means
        // the tray's English/Vietnamese indicator lags, never a typing fault.
        if let Err(e) = method_sync::write_enabled(true) {
            tracing::warn!("Enable: write_enabled(true) failed: {e}");
        }
    }

    /// Same contract as [`Self::focus_out`] for the candidate popup: hide it
    /// when the engine is disabled while a Nôm list is showing.
    async fn disable(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        tracing::info!("Disable");
        // buttre stopped being the global engine (OS switched to English/another
        // source). Tell the tray so it can show the disabled/English state.
        if let Err(e) = method_sync::write_enabled(false) {
            tracing::warn!("Disable: write_enabled(false) failed: {e}");
        }
        let had_candidates = {
            let mut bridge = self.bridge.lock().unwrap();
            let had = bridge.candidate_count() > 0;
            bridge.discard();
            had
        };
        if had_candidates {
            Self::hide_lookup_table(&ctx).await.ok();
        }
    }

    /// Daemon-initiated reset: discard the composition WITHOUT committing.
    async fn reset(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        tracing::debug!("Reset");
        let outcome = self.bridge.lock().unwrap().discard();
        self.emit_ops(&ctx, outcome.ops).await;
    }

    /// Panel navigation (mouse on the popup's scroll arrows, or the panel's
    /// own key handling): move the highlight and re-emit the table. No-ops when
    /// no Nôm popup is showing. Mirror the keyboard routing in
    /// `process_key_event` so both input paths stay consistent.
    async fn cursor_up(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        let outcome = self.bridge.lock().unwrap().cursor_prev();
        self.emit_ops(&ctx, outcome.ops).await;
    }

    async fn cursor_down(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        let outcome = self.bridge.lock().unwrap().cursor_next();
        self.emit_ops(&ctx, outcome.ops).await;
    }

    async fn page_up(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        let page = ibus_props::LOOKUP_PAGE_SIZE as usize;
        let outcome = self.bridge.lock().unwrap().cursor_page_up(page);
        self.emit_ops(&ctx, outcome.ops).await;
    }

    async fn page_down(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        let page = ibus_props::LOOKUP_PAGE_SIZE as usize;
        let outcome = self.bridge.lock().unwrap().cursor_page_down(page);
        self.emit_ops(&ctx, outcome.ops).await;
    }

    /// The user clicked candidate `index` (0-based WITHIN the current page) with
    /// mouse `button` (1 = left). Left-click commits it; other buttons are
    /// ignored. `state` (modifiers) is unused.
    async fn candidate_clicked(
        &mut self,
        #[zbus(signal_context)] ctx: SignalContext<'_>,
        index: u32,
        button: u32,
        _state: u32,
    ) {
        const LEFT_BUTTON: u32 = 1;
        if button != LEFT_BUTTON {
            return;
        }
        let page = ibus_props::LOOKUP_PAGE_SIZE as usize;
        let outcome = self
            .bridge
            .lock()
            .unwrap()
            .select_at_page(index as usize, page);
        self.emit_ops(&ctx, outcome.ops).await;
    }

    fn set_cursor_location(&mut self, x: i32, y: i32, w: i32, h: i32) {
        tracing::debug!("SetCursorLocation: x={}, y={}, w={}, h={}", x, y, w, h);
    }

    /// Record the client's capabilities. Sync (no `SignalContext`), so it only
    /// stores the bits; the next `process_key_event` calls `sync_use_preedit`
    /// which applies any resulting model change. Caps arrive before typing, so
    /// this self-heals on the first keystroke.
    fn set_capabilities(&mut self, caps: u32) {
        tracing::debug!("SetCapabilities: {}", caps);
        self.caps = caps;
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
    /// Open the settings window from the property menu's "Cấu hình" item by
    /// spawning this same binary with `--config` (the arg dispatch in `main`
    /// runs the Slint window). A separate PROCESS, exactly like the tray's
    /// "Cấu hình" handler — the config window owns a winit event loop that must
    /// never share this process. Non-blocking; failure is logged, never fatal.
    fn open_config_window() {
        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(e) = std::process::Command::new(exe).arg("--config").spawn() {
                    tracing::warn!("failed to spawn config window: {e}");
                }
            }
            Err(e) => tracing::warn!("current_exe for config window failed: {e}"),
        }
    }

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
                ImeOp::Candidates { items, cursor } => {
                    let displays: Vec<String> = items.iter().map(|v| v.display.clone()).collect();
                    let table = ibus_props::build_lookup_table(&displays, cursor as u32);
                    Self::update_lookup_table(ctx, table, true).await.ok();
                    Self::show_lookup_table(ctx).await.ok();
                }
                ImeOp::HideCandidates => {
                    Self::hide_lookup_table(ctx).await.ok();
                }
                ImeOp::DeleteSurrounding(n) => {
                    // Atomic delete of the n chars before the cursor. Only ever
                    // reached when the client advertised surrounding-text (the
                    // gate in sync_use_preedit), so no fallback is needed.
                    Self::delete_surrounding_text(ctx, -(n as i32), n as u32)
                        .await
                        .ok();
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
                // Move the panel radio to the new method now instead of waiting
                // for the next focus_in — a switch made from the tray or the
                // config window would otherwise leave the IBus menu checked on
                // the old method until the field is refocused.
                Self::publish_method_props(ctx, &method).await;
                tracing::info!("Engine switched to method {method}");
            }
            // Build failed (already logged): keep the current keyboard rather
            // than crash. Do NOT re-emit properties here — the method did not
            // actually change, so the radio should keep reflecting the current
            // one. seen_generation is advanced so we don't retry the same
            // broken method every keystroke.
            None => tracing::warn!("Method switch to {method} failed; keeping current"),
        }
    }

    /// Apply a pending preedit-model change. No-preedit (commit-as-you-go)
    /// engages only when the user turned the setting off AND the client can
    /// delete already-committed text (`IBUS_CAP_SURROUNDING_TEXT`). Clients
    /// without it (terminals, some GUIs) stay on preedit: neither
    /// `forward_key_event` nor a committed DEL reliably deletes there — verified
    /// on Ptyxis (GTK4/VTE), which ignores forwarded keys and drops committed
    /// control chars, so no-preedit would corrupt input. Nôm ignores the whole
    /// thing in the bridge (`set_use_composition` no-ops for Nôm).
    async fn sync_use_preedit(&mut self, ctx: &SignalContext<'_>) {
        let want_no_preedit = self
            .use_preedit
            .as_ref()
            .map(|flag| !flag.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false);
        let has_surrounding = self.caps & IBUS_CAP_SURROUNDING_TEXT != 0;
        let effective = want_no_preedit && has_surrounding;
        if effective == self.applied_no_preedit {
            return;
        }
        self.applied_no_preedit = effective;
        // set_use_composition commits any pending word before switching models;
        // the guard is dropped before the await (owned outcome returned).
        let outcome = self.bridge.lock().unwrap().set_use_composition(!effective);
        self.emit_ops(ctx, outcome.ops).await;
        tracing::info!("Preedit model: no_preedit={effective}");
    }
}
