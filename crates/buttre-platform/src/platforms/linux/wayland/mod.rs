//! Wayland-native input method via `zwp_input_method_v2`.
//!
//! First-class IME on wlroots compositors (sway, Hyprland) and KDE — no
//! IBus daemon in the path. The compositor routes keys to us through an
//! input-method keyboard grab; composition semantics come from the shared
//! [`EngineBridge`](super::engine_bridge), identical to the IBus backend.
//!
//! ## Protocol shape (verified against sway 1.9 headless)
//!
//! - bind `zwp_input_method_manager_v2` + `wl_seat` +
//!   `zwp_virtual_keyboard_manager_v1`
//! - `get_input_method(seat)` → `Activate`/`Deactivate` (+ `ContentType`,
//!   `SurroundingText`) arrive double-buffered, applied on `Done`
//! - on activate we `grab_keyboard()`; on deactivate we release it — holding
//!   the grab outside text entry would swallow global keystrokes (and look
//!   exactly like the keylogger behavior this backend exists to avoid)
//! - keys we consume update preedit via `set_preedit_string` + `commit`
//!   (serial = count of `Done` events); keys we don't are re-injected
//!   through a `zwp_virtual_keyboard_v1` carrying the compositor's own
//!   keymap, so apps receive them untouched
//! - password fields (`ContentPurpose::Password`/`Pin`) bypass the engine
//!   entirely — nothing sensitive enters composition or learning
//!
//! ## Known v1 limitations (logged, revisit later)
//!
//! - No key repeat inside the composition (`RepeatInfo` ignored).
//! - Focus loss (Deactivate) discards an uncommitted preedit — text-input-v3
//!   has no IBus-style "commit preedit on focus change" mode.

mod dispatch;

use super::engine_bridge::EngineBridge;
use super::method_sync::{self, MethodState};
use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use wayland_client::protocol::wl_seat;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2, zwp_input_method_manager_v2, zwp_input_method_v2,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1, zwp_virtual_keyboard_v1,
};
use xkbcommon::xkb;

/// The compositor lacks `zwp_input_method_v2`, or another IME already holds
/// the seat — callers fall back to IBus.
#[derive(Debug)]
pub struct Unavailable(pub String);

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wayland input-method unavailable: {}", self.0)
    }
}

impl std::error::Error for Unavailable {}

/// text-input-v3 content purposes that must bypass the engine.
const PURPOSE_PASSWORD: u32 = 8;
const PURPOSE_PIN: u32 = 9;

pub(crate) struct ImeState {
    // --- globals ---
    seat: Option<wl_seat::WlSeat>,
    im_manager: Option<zwp_input_method_manager_v2::ZwpInputMethodManagerV2>,
    vk_manager: Option<zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1>,

    // --- live objects ---
    input_method: Option<zwp_input_method_v2::ZwpInputMethodV2>,
    grab: Option<zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2>,
    virtual_kb: Option<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1>,

    // --- xkb ---
    xkb_context: xkb::Context,
    xkb_state: Option<xkb::State>,
    /// Kept so the same keymap can seed the virtual keyboard.
    keymap_fd: Option<(OwnedFd, u32)>,

    // --- protocol state ---
    /// Count of `Done` events — the serial every `commit` must carry.
    serial: u32,
    active: bool,
    pending_active: bool,
    content_purpose: u32,
    pending_purpose: u32,
    unavailable: bool,

    /// Keycodes whose PRESS we consumed — their release is swallowed too.
    swallowed: HashSet<u32>,

    // --- engine ---
    bridge: EngineBridge,
    method_state: Arc<MethodState>,
    seen_generation: u64,
}

impl ImeState {
    fn new(method_state: Arc<MethodState>) -> Self {
        let method = method_state.method();
        let seen_generation = method_state.generation();
        Self {
            seat: None,
            im_manager: None,
            vk_manager: None,
            input_method: None,
            grab: None,
            virtual_kb: None,
            xkb_context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            xkb_state: None,
            keymap_fd: None,
            serial: 0,
            active: false,
            pending_active: false,
            content_purpose: 0,
            pending_purpose: 0,
            unavailable: false,
            swallowed: HashSet::new(),
            bridge: EngineBridge::new(&method),
            method_state,
            seen_generation,
        }
    }

    /// True in password/PIN fields — the engine must not see these keys.
    fn sensitive_field(&self) -> bool {
        matches!(self.content_purpose, PURPOSE_PASSWORD | PURPOSE_PIN)
    }

    /// Push the bridge's current state to the compositor and commit.
    /// input-method-v2 pending state resets after every commit, so the
    /// preedit must be re-declared each cycle it should stay visible.
    fn commit_ops(&mut self, ops: Vec<super::engine_bridge::ImeOp>) {
        let Some(im) = &self.input_method else { return };
        use super::engine_bridge::ImeOp;
        for op in &ops {
            if let ImeOp::Commit(text) = op {
                im.commit_string(text.clone());
            }
        }
        let preedit = self.bridge.preedit();
        if !preedit.is_empty() {
            let cursor = preedit.len() as i32; // byte offset, cursor at end
            im.set_preedit_string(preedit.to_string(), cursor, cursor);
        }
        im.commit(self.serial);
    }

    /// Re-inject a key the engine didn't consume, so the app receives it.
    fn forward_key(&self, time: u32, key: u32, pressed: bool) {
        if let Some(vk) = &self.virtual_kb {
            let state = if pressed { 1 } else { 0 };
            vk.key(time, key, state);
        }
    }

    /// Apply a pending tray-side method switch (B5) — same lazy
    /// generation check as the IBus backend.
    fn sync_method(&mut self) {
        let generation = self.method_state.generation();
        if generation == self.seen_generation {
            return;
        }
        self.seen_generation = generation;
        let method = self.method_state.method();
        match self.bridge.rebuild(&method) {
            Some(outcome) => {
                self.commit_ops(outcome.ops);
                tracing::info!("Wayland engine switched to method {method}");
            }
            None => tracing::warn!("Method switch to {method} failed; keeping current"),
        }
    }
}

/// Run the Wayland-native engine. Blocks for the process lifetime.
/// Returns [`Unavailable`] (as anyhow error) when the compositor lacks the
/// protocol or another IME owns the seat — callers then fall back to IBus.
pub fn run_engine() -> Result<()> {
    let conn = Connection::connect_to_env()
        .map_err(|e| anyhow!(Unavailable(format!("no wayland display: {e}"))))?;
    let display = conn.display();

    // NB: do NOT spawn the method watcher yet — this function may still
    // return Unavailable (compositor lacks the protocol / seat already
    // owned), and the caller then falls back to IBus which spawns its own
    // watcher. Spawning here first would leak an orphaned watcher thread +
    // inotify watch for the process lifetime. Defer until availability is
    // confirmed, just before the dispatch loop.
    let method_state = MethodState::load();

    let mut state = ImeState::new(method_state.clone());
    let mut queue = conn.new_event_queue::<ImeState>();
    let qh: QueueHandle<ImeState> = queue.handle();
    display.get_registry(&qh, ());
    queue.roundtrip(&mut state)?;

    let (Some(im_manager), Some(seat)) = (&state.im_manager, &state.seat) else {
        return Err(anyhow!(Unavailable(
            "compositor lacks zwp_input_method_manager_v2 or wl_seat".into()
        )));
    };
    let Some(vk_manager) = &state.vk_manager else {
        return Err(anyhow!(Unavailable(
            "compositor lacks zwp_virtual_keyboard_manager_v1".into()
        )));
    };

    state.input_method = Some(im_manager.get_input_method(seat, &qh, ()));
    state.virtual_kb = Some(vk_manager.create_virtual_keyboard(seat, &qh, ()));
    queue.roundtrip(&mut state)?;

    if state.unavailable {
        return Err(anyhow!(Unavailable(
            "another input method already owns the seat".into()
        )));
    }

    // Availability confirmed — now it's safe to start the tray↔engine
    // method watcher (no IBus fallback will run in this process).
    method_sync::spawn_watcher(method_state);

    tracing::info!("Wayland input method registered; waiting for text-input activation");
    loop {
        queue.blocking_dispatch(&mut state)?;
        if state.unavailable {
            return Err(anyhow!(Unavailable("input method was replaced".into())));
        }
    }
}
