//! IBus Engine Implementation
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/platform_linux_tests.rs`.
//!
//! Engine behavior for Vietnamese input over IBus (zbus 3). The component
//! lifecycle (private-bus connection, Factory, name request) lives in
//! `ibus_bus.rs`.
//!
//! ## Composition model
//!
//! The `Keyboard` is built with `use_composition = true` — the same mode the
//! Windows TSF text service uses. The engine pipeline then owns ALL word
//! logic and emits:
//!
//! - `UpdateComposition { text, cursor }` — the full current word → mapped to
//!   IBus preedit (`update_preedit_text`).
//! - `ConfirmComposition(text)` — the boundary-repaired word at a separator →
//!   mapped to `commit_text`.
//! - `Commit(ch)` — the separator character itself → NOT committed by us; we
//!   return `false` so the daemon forwards the original key to the app
//!   (signals are queued before the method reply, so the committed word
//!   always lands first).
//!
//! `delete_surrounding_text` is deliberately absent: in the preedit model the
//! composition is not yet real text, so deleting committed text to its left
//! would eat the user's earlier words (debug report B1).

use buttre_core::Action;
use buttre_core::{Keyboard, KeyboardBuilder};
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

/// IBus Engine for Vietnamese input.
///
/// `Keyboard` owns its internal buffer; we hold it behind an `Arc<Mutex<>>` so
/// the `#[derive(Clone)]` on this struct works correctly with zbus.
#[derive(Clone)]
pub struct ButtreEngine {
    keyboard: Arc<Mutex<Keyboard>>,
    pub preedit: Arc<Mutex<String>>,
}

impl ButtreEngine {
    pub fn new() -> Self {
        Self::new_with_method("telex")
    }

    pub fn new_with_method(method_name: &str) -> Self {
        // Composition mode (like TSF): the pipeline emits
        // UpdateComposition/ConfirmComposition instead of hook-style
        // Commit/Replace screen diffs — see module docs.
        let keyboard = match method_name {
            "vni" => {
                KeyboardBuilder::vni_with_composition(true).expect("Failed to create VNI keyboard")
            }
            _ => KeyboardBuilder::telex_with_composition(true)
                .expect("Failed to create Telex keyboard"),
        };
        Self {
            keyboard: Arc::new(Mutex::new(keyboard)),
            preedit: Arc::new(Mutex::new(String::new())),
        }
    }

    /// Take the pending word for an out-of-band commit (focus loss, control
    /// combo, navigation key), applying the word-boundary final repair —
    /// these commit points bypass the pipeline's own PassThrough repair.
    /// Resets keyboard + preedit; returns `None` when nothing is composing.
    fn take_pending_commit(&self) -> Option<String> {
        let mut kb = self.keyboard.lock().unwrap();
        let mut preedit = self.preedit.lock().unwrap();
        if preedit.is_empty() {
            return None;
        }
        let text = kb.boundary_repair().unwrap_or_else(|| preedit.clone());
        kb.reset();
        preedit.clear();
        Some(text)
    }

    /// Discard the composition without committing (daemon Reset semantics).
    fn discard_composition(&self) -> bool {
        let mut kb = self.keyboard.lock().unwrap();
        let mut preedit = self.preedit.lock().unwrap();
        let had = !preedit.is_empty();
        kb.reset();
        preedit.clear();
        had
    }
}

impl Default for ButtreEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Key classification helpers
// ============================================================================

/// True when a control modifier (Ctrl / Alt / Super) is active.
/// We pass these through without engine processing to preserve shortcuts.
fn is_control_combo(state: u32) -> bool {
    state & (IBUS_CONTROL_MASK | IBUS_MOD1_MASK | IBUS_SUPER_MASK) != 0
}

/// True for modifier-only keyvals (Shift_L/R, Ctrl_L/R, Caps_Lock, …).
fn is_modifier_keyval(keyval: u32) -> bool {
    matches!(keyval, 0xFFE1..=0xFFEE | 0xFE01..=0xFE0F)
}

/// True for non-printable keys that end the composition and pass through
/// (navigation, Tab, Escape, Delete, …). Printable separators (space,
/// punctuation) are NOT classified here — the engine pipeline itself decides
/// those via PassThrough, keeping one source of truth for word boundaries.
fn is_break_keyval(keyval: u32) -> bool {
    matches!(
        keyval,
        0xFF09 // Tab
        | 0xFF1B // Escape
        | 0xFF50 // Home
        | 0xFF51
            ..=0xFF54 // Left/Up/Right/Down
        | 0xFF55 // Page_Up
        | 0xFF56 // Page_Down
        | 0xFF57 // End
        | 0xFF63 // Insert
        | 0xFFFF // Delete
    )
}

/// Convert an IBus/X11 keyval to a character.
///
/// XKB resolves Shift/CapsLock BEFORE the keysym reaches us (`Shift+a`
/// arrives as keyval `0x41` = 'A'), so printable ASCII maps by identity —
/// re-applying modifier logic here would double-flip the case.
pub fn keyval_to_char(keyval: u32) -> Option<char> {
    match keyval {
        // Printable ASCII: letters, digits, space, punctuation.
        0x0020..=0x007E => char::from_u32(keyval),
        // Return
        0xFF0D => Some('\n'),
        // Backspace
        0xFF08 => Some('\x08'),
        _ => None,
    }
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
///
/// NOTE: Verify output against `dbus-monitor --session` before shipping.
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
    //
    // Signatures MUST match libibus's engine introspection XML exactly —
    // the daemon subscribes by signature and silently drops mismatches
    // (found the hard way: a 3-arg UpdatePreeditText never got relayed).

    #[dbus_interface(signal)]
    async fn commit_text(ctx: &SignalContext<'_>, text: zvariant::Value<'_>) -> zbus::Result<()>;

    /// `mode` is IBusPreeditFocusMode: what the CLIENT does with a visible
    /// preedit when focus changes. We always send 1 (COMMIT) so a mouse
    /// click elsewhere commits the word instead of eating it — the client
    /// handles focus-loss commits, the engine only resets (see `focus_out`).
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

        // Shortcuts (Ctrl+C, Alt+F4, …): commit the pending word so it isn't
        // lost, then let the app receive the combo.
        if is_control_combo(state) {
            self.commit_pending(&ctx).await;
            return false;
        }

        // Bare modifier presses don't touch the composition.
        if is_modifier_keyval(keyval) {
            return false;
        }

        // Navigation/editing keys end the word and pass through.
        if is_break_keyval(keyval) {
            self.commit_pending(&ctx).await;
            return false;
        }

        let Some(ch) = keyval_to_char(keyval) else {
            return false;
        };

        if ch == '\x08' {
            return self.handle_backspace(&ctx).await;
        }

        // Feed the engine. It classifies separators itself: a separator
        // yields [ConfirmComposition(word), Commit(separator)] — handle the
        // WHOLE action vector (taking only the first was debug-report B0).
        let actions = {
            let mut kb = self.keyboard.lock().unwrap();
            match kb.process(ch) {
                Ok(actions) => actions,
                Err(e) => {
                    tracing::warn!("Keyboard process error: {}", e);
                    return false;
                }
            }
        };

        let mut emitted = false;
        let mut pass_char = false;
        for action in actions {
            match action {
                Action::UpdateComposition { text, .. } => {
                    {
                        let mut p = self.preedit.lock().unwrap();
                        *p = text.clone();
                    }
                    self.emit_preedit(&ctx, &text).await;
                    emitted = true;
                }
                Action::ConfirmComposition(text) => {
                    {
                        let mut p = self.preedit.lock().unwrap();
                        p.clear();
                    }
                    // Clear the preedit region BEFORE the commit so the word
                    // isn't momentarily doubled in the client.
                    self.emit_preedit(&ctx, "").await;
                    Self::commit_text(&ctx, build_ibus_text(&text)).await.ok();
                    emitted = true;
                }
                Action::Commit(text) => {
                    // The engine echoing the input character back is a
                    // pass-through: return false below and the daemon
                    // forwards the ORIGINAL key event to the app (after our
                    // queued commit signals).
                    if text.chars().eq(std::iter::once(ch)) {
                        pass_char = true;
                    } else {
                        Self::commit_text(&ctx, build_ibus_text(&text)).await.ok();
                        emitted = true;
                    }
                }
                Action::DoNothing => {}
                Action::ShowCandidates { .. } | Action::HideCandidates => {
                    // Nôm candidate UI over IBus: future phase (needs
                    // update_lookup_table); harmless to ignore for Telex/VNI.
                }
                other => {
                    tracing::warn!(
                        "Unexpected hook-model action in composition mode: {:?}",
                        other
                    );
                }
            }
        }

        if pass_char {
            return false;
        }
        if emitted {
            return true;
        }
        // Pure DoNothing: swallow keys the engine deliberately ignored
        // mid-composition; pass through when nothing is composing.
        !self.preedit.lock().unwrap().is_empty()
    }

    fn focus_in(&mut self) {
        tracing::info!("FocusIn");
    }

    /// Focus loss: the CLIENT commits the visible preedit itself (we send
    /// every preedit update with mode=COMMIT), so the engine only resets its
    /// state — emitting our own commit here would double the word.
    fn focus_out(&mut self) {
        tracing::info!("FocusOut");
        self.discard_composition();
    }

    fn enable(&mut self) {
        tracing::info!("Enable");
    }

    fn disable(&mut self) {
        tracing::info!("Disable");
        self.discard_composition();
    }

    /// Daemon-initiated reset: discard the composition WITHOUT committing.
    async fn reset(&mut self, #[zbus(signal_context)] ctx: SignalContext<'_>) {
        tracing::debug!("Reset");
        if self.discard_composition() {
            self.emit_preedit(&ctx, "").await;
        }
    }

    fn set_cursor_location(&mut self, x: i32, y: i32, w: i32, h: i32) {
        tracing::debug!("SetCursorLocation: x={}, y={}, w={}, h={}", x, y, w, h);
    }

    fn set_capabilities(&mut self, caps: u32) {
        tracing::debug!("SetCapabilities: {}", caps);
    }

    /// `ContentType` is a write-only PROPERTY `(uu)` in the engine
    /// interface (purpose, hints; purpose 8 = password). Reserved for
    /// suppressing learning in sensitive fields; accepting the write also
    /// keeps the daemon's property-set out of the error log.
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
// Non-D-Bus helpers (need the same signal context as the interface methods)
// ============================================================================

impl ButtreEngine {
    /// Emit a preedit update; empty text clears the region (there is no
    /// separate engine-side hide signal — hide IS `visible=false`).
    async fn emit_preedit(&self, ctx: &SignalContext<'_>, text: &str) {
        let cursor = text.chars().count() as u32;
        Self::update_preedit_text(
            ctx,
            build_ibus_text(text),
            cursor,
            !text.is_empty(),
            PREEDIT_FOCUS_COMMIT,
        )
        .await
        .ok();
    }

    /// Commit the pending word out-of-band (shortcuts, navigation keys —
    /// cases with NO focus change, where the client's mode=COMMIT handling
    /// doesn't apply). No-op when nothing is composing.
    async fn commit_pending(&self, ctx: &SignalContext<'_>) {
        if let Some(text) = self.take_pending_commit() {
            self.emit_preedit(ctx, "").await;
            Self::commit_text(ctx, build_ibus_text(&text)).await.ok();
        }
    }

    /// Backspace shrinks the composition; the engine recomputes the word
    /// from raw keys (`Keyboard::backspace`), and the new preedit is the
    /// keyboard's canonical buffer. With no composition the app handles it.
    async fn handle_backspace(&mut self, ctx: &SignalContext<'_>) -> bool {
        if self.preedit.lock().unwrap().is_empty() {
            return false;
        }
        let text = {
            let mut kb = self.keyboard.lock().unwrap();
            if let Err(e) = kb.backspace() {
                tracing::warn!("Keyboard backspace error: {}", e);
            }
            kb.buffer().to_string()
        };
        {
            let mut p = self.preedit.lock().unwrap();
            *p = text.clone();
        }
        self.emit_preedit(ctx, &text).await;
        true
    }
}

// ============================================================================
// Method config loading
// ============================================================================

/// Load input method name from `~/.config/buttre/method`.
///
/// Returns "vni" if the file contains "vni" (trimmed, case-insensitive),
/// "telex" for any other content or on read failure.
pub(super) fn load_method_config() -> String {
    let path = dirs::config_dir().map(|p| p.join("buttre/method"));
    if let Some(path) = path {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let method = content.trim().to_lowercase();
            if method == "vni" {
                tracing::info!("Loaded method config: vni");
                return "vni".to_string();
            }
        } else {
            tracing::debug!("No method config at {:?}, defaulting to telex", path);
        }
    }
    "telex".to_string()
}

// NOTE: The component entry point (private-bus connection, Factory, name
// request) lives in `ibus_bus.rs` — this module owns only engine behavior.
// The old session-bus `run_engine` variants that lived here could never be
// reached by ibus-daemon (wrong bus, no Factory) and were removed.
