//! IBus Engine Implementation
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/platform_linux_tests.rs`.
//!
//! D-Bus service for Vietnamese input via IBus (zbus 3).

use anyhow::Result;
use buttre_core::Action;
use buttre_core::{Keyboard, KeyboardBuilder};
use std::sync::{Arc, Mutex};
use zbus::zvariant;
use zbus::{dbus_interface, ConnectionBuilder, SignalContext};

// ============================================================================
// IBus modifier state bitmask (ibus.h)
// ============================================================================

const IBUS_SHIFT_MASK: u32 = 0x01;
const IBUS_LOCK_MASK: u32 = 0x02; // Caps Lock
const IBUS_CONTROL_MASK: u32 = 0x04;
const IBUS_MOD1_MASK: u32 = 0x08; // Alt
const IBUS_SUPER_MASK: u32 = 0x40;

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
        let keyboard = match method_name {
            "vni" => KeyboardBuilder::vni().expect("Failed to create VNI keyboard"),
            _ => KeyboardBuilder::telex().expect("Failed to create Telex keyboard"),
        };
        Self {
            keyboard: Arc::new(Mutex::new(keyboard)),
            preedit: Arc::new(Mutex::new(String::new())),
        }
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

/// True for non-character keys that should reset and pass through.
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

/// True for punctuation / whitespace chars that break the composition.
fn is_break_char(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\n'
            | '\t'
            | '.'
            | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | '\''
            | '"'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '/'
            | '\\'
            | '|'
            | '`'
            | '~'
            | '@'
            | '#'
            | '$'
            | '%'
            | '^'
            | '&'
            | '*'
            | '+'
            | '='
            | '-'
            | '_'
            | '<'
            | '>'
    )
}

/// Convert GDK keyval + state to a character, applying CapsLock XOR Shift for letters.
pub fn keyval_to_char(keyval: u32, state: u32) -> Option<char> {
    let shift = state & IBUS_SHIFT_MASK != 0;
    let caps = state & IBUS_LOCK_MASK != 0;
    let upper = shift ^ caps;

    match keyval {
        // a-z
        0x0061..=0x007a => {
            let ch = (keyval as u8) as char;
            Some(if upper { ch.to_ascii_uppercase() } else { ch })
        }
        // A-Z (keyval already uppercase — respect it)
        0x0041..=0x005A => Some((keyval as u8) as char),
        // 0-9
        0x0030..=0x0039 => Some((keyval as u8) as char),
        // Space
        0x0020 => Some(' '),
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

    #[dbus_interface(signal)]
    async fn commit_text(ctx: &SignalContext<'_>, text: zvariant::Value<'_>) -> zbus::Result<()>;

    #[dbus_interface(signal)]
    async fn update_preedit_text(
        ctx: &SignalContext<'_>,
        text: zvariant::Value<'_>,
        cursor_pos: u32,
        visible: bool,
    ) -> zbus::Result<()>;

    #[dbus_interface(signal)]
    async fn delete_surrounding_text(
        ctx: &SignalContext<'_>,
        offset: i32,
        n_chars: u32,
    ) -> zbus::Result<()>;

    #[dbus_interface(signal)]
    async fn hide_preedit_text(ctx: &SignalContext<'_>) -> zbus::Result<()>;

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

        // Pass through modifier combos (Ctrl+C, Alt+F4, etc.)
        if is_control_combo(state) {
            return false;
        }

        // Pass through bare modifier keyvals
        if is_modifier_keyval(keyval) {
            return false;
        }

        // Pass through non-character break keys (arrows, Tab, Escape, …)
        if is_break_keyval(keyval) {
            let preedit_text = {
                let p = self.preedit.lock().unwrap();
                if p.is_empty() {
                    None
                } else {
                    Some(p.clone())
                }
            };
            if let Some(text) = preedit_text {
                self.reset();
                Self::commit_text(&ctx, build_ibus_text(&text)).await.ok();
            }
            return false;
        }

        let ch = match keyval_to_char(keyval, state) {
            Some(c) => c,
            None => return false,
        };

        // Backspace
        if ch == '\x08' {
            let action = {
                let mut kb = self.keyboard.lock().unwrap();
                match kb.backspace() {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::warn!("Keyboard backspace error: {}", e);
                        Action::DoNothing
                    }
                }
            };
            match action {
                Action::Replace { text, .. } => {
                    let cursor = text.chars().count() as u32;
                    {
                        let mut p = self.preedit.lock().unwrap();
                        *p = text.clone();
                    }
                    let visible = !text.is_empty();
                    if visible {
                        Self::update_preedit_text(&ctx, build_ibus_text(&text), cursor, true)
                            .await
                            .ok();
                    } else {
                        Self::hide_preedit_text(&ctx).await.ok();
                    }
                    return true;
                }
                _ => return false,
            }
        }

        // Break chars (space, punctuation) — commit preedit and pass through
        if is_break_char(ch) {
            let preedit_text = {
                let p = self.preedit.lock().unwrap();
                if p.is_empty() {
                    None
                } else {
                    Some(p.clone())
                }
            };
            if let Some(text) = preedit_text {
                self.reset();
                Self::commit_text(&ctx, build_ibus_text(&text)).await.ok();
            }
            return false;
        }

        // Normal character — process through engine
        let action = {
            let mut kb = self.keyboard.lock().unwrap();
            match kb.process(ch) {
                Ok(actions) => actions.into_iter().next().unwrap_or(Action::DoNothing),
                Err(e) => {
                    tracing::warn!("Keyboard process error: {}", e);
                    Action::DoNothing
                }
            }
        };

        match action {
            Action::Replace {
                text,
                backspace_count,
                ..
            } => {
                let cursor = text.chars().count() as u32;
                {
                    let mut p = self.preedit.lock().unwrap();
                    *p = text.clone();
                }
                if backspace_count > 0 {
                    let n = backspace_count as u32;
                    Self::delete_surrounding_text(&ctx, -(n as i32), n)
                        .await
                        .ok();
                }
                Self::update_preedit_text(&ctx, build_ibus_text(&text), cursor, true)
                    .await
                    .ok();
                true
            }
            Action::Commit(text) => {
                self.reset();
                Self::commit_text(&ctx, build_ibus_text(&text)).await.ok();
                true
            }
            _ => false,
        }
    }

    fn focus_in(&mut self) {
        tracing::info!("FocusIn");
    }

    fn focus_out(&mut self) {
        tracing::info!("FocusOut");
        self.reset();
    }

    fn enable(&mut self) {
        tracing::info!("Enable");
    }

    fn disable(&mut self) {
        tracing::info!("Disable");
        self.reset();
    }

    fn reset(&mut self) {
        let mut kb = self.keyboard.lock().unwrap();
        let mut preedit = self.preedit.lock().unwrap();
        kb.reset();
        preedit.clear();
    }

    fn set_cursor_location(&mut self, x: i32, y: i32, w: i32, h: i32) {
        tracing::debug!("SetCursorLocation: x={}, y={}, w={}, h={}", x, y, w, h);
    }

    fn set_capabilities(&mut self, caps: u32) {
        tracing::debug!("SetCapabilities: {}", caps);
    }
}

// ============================================================================
// Method config loading
// ============================================================================

/// Load input method name from `~/.config/buttre/method`.
///
/// Returns "vni" if the file contains "vni" (trimmed, case-insensitive),
/// "telex" for any other content or on read failure.
fn load_method_config() -> String {
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

// ============================================================================
// Engine entry points
// ============================================================================

/// Run the IBus engine service (runs until process exits).
pub async fn run_engine() -> Result<()> {
    let method = load_method_config();
    tracing::info!("Starting buttre IBus Engine (method={})", method);

    let engine = ButtreEngine::new_with_method(&method);

    // The connection MUST be held for the lifetime of the service — dropping
    // a `zbus::Connection` closes the underlying socket, which would take the
    // engine off the bus the instant it was built.
    let _connection = ConnectionBuilder::session()?
        .name("org.freedesktop.IBus.buttre")?
        .serve_at("/org/freedesktop/IBus/Engine/buttre", engine)?
        .build()
        .await?;

    tracing::info!("Engine running on D-Bus");
    std::future::pending::<()>().await;
    Ok(())
}

/// Run the IBus engine service with a graceful shutdown channel.
///
/// Used by `LinuxBackend::init` so the engine thread can be stopped cleanly.
pub async fn run_engine_with_shutdown(shutdown: tokio::sync::oneshot::Receiver<()>) -> Result<()> {
    let method = load_method_config();
    tracing::info!("Starting buttre IBus Engine (method={}, managed)", method);

    let engine = ButtreEngine::new_with_method(&method);

    // Held for the lifetime of the service — see `run_engine`'s comment.
    let _connection = ConnectionBuilder::session()?
        .name("org.freedesktop.IBus.buttre")?
        .serve_at("/org/freedesktop/IBus/Engine/buttre", engine)?
        .build()
        .await?;

    tracing::info!("Engine running on D-Bus");

    tokio::select! {
        _ = std::future::pending::<()>() => {},
        _ = shutdown => {
            tracing::info!("Engine received shutdown signal");
        }
    }
    Ok(())
}
