//! KDE Plasma (KWin) input method via `zwp_input_method_v1`.
//!
//! KWin does NOT implement `zwp_input_method_v2` (verified on Plasma 6,
//! kwin 6.4): its input-method socket advertises only the older
//! `zwp_input_method_v1` (Weston/Maliit lineage) — the protocol fcitx5 uses
//! on Plasma. This module is the v1 twin of the v2 backend in `mod.rs`,
//! sharing [`EngineBridge`] so composition semantics are identical.
//!
//! ## How KWin hands us the seat
//!
//! KWin never exposes input-method globals on the regular socket. It spawns
//! the configured IME itself (`kwinrc` → `[Wayland] InputMethod=<desktop
//! file>`) and passes a privileged socketpair via `WAYLAND_SOCKET`; only that
//! connection sees `zwp_input_method_v1`. `Connection::connect_to_env`
//! consumes (and unsets) `WAYLAND_SOCKET`, which is why this module receives
//! the already-open [`Connection`] from `run_engine` instead of connecting
//! itself — a second connect would silently land on the unprivileged socket.
//!
//! ## Protocol shape (vs v2)
//!
//! - No manager: the daemon-side `activate` event CREATES a per-text-input
//!   `zwp_input_method_context_v1`; `deactivate` retires it (we destroy it).
//! - Keys arrive through `context.grab_keyboard()` (a plain `wl_keyboard`);
//!   keys the engine does not consume are re-injected with `context.key`
//!   (there is no virtual-keyboard protocol on this socket).
//! - Output is immediate, not double-buffered: `commit_string` /
//!   `preedit_string` apply as sent. The serial they carry echoes the latest
//!   `commit_state` event.
//! - `preedit_string`'s `commit` argument is the text the client inserts if
//!   the field resets (focus loss) — passing the preedit itself gives the
//!   same "commit on focus change" behavior as the IBus backend.

use super::super::engine_bridge::{
    is_break_keysym, is_modifier_keysym, keysym_to_char, EngineBridge, ImeOp,
};
use super::super::macro_sync;
use super::super::method_sync::{self, MethodState};
use super::Unavailable;
use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use wayland_client::protocol::{wl_keyboard, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::wp::input_method::zv1::client::{
    zwp_input_method_context_v1, zwp_input_method_v1,
};
use xkbcommon::xkb;

/// text-input-unstable-v1 `content_purpose` password value — the only
/// sensitive purpose in v1 (there is no separate PIN; KWin folds v3's Pin
/// into Password when translating for the IM).
const PURPOSE_PASSWORD_V1: u32 = 8;
/// text-input `content_purpose` terminal value. Terminals (VTE/Ptyxis,
/// Konsole) DO report surrounding text yet ignore `delete_surrounding_text`
/// — the no-preedit model would corrupt corrections there (observed: Ptyxis
/// on Plasma 6, "backspace does nothing") — so this purpose pins the preedit
/// model regardless of [`ImeV1State::saw_surrounding`]. Same client family
/// the IBus backend documents in its `sync_use_preedit` gate.
const PURPOSE_TERMINAL_V1: u32 = 12;

pub(crate) struct ImeV1State {
    im: Option<zwp_input_method_v1::ZwpInputMethodV1>,

    /// Live per-activation context (`None` between text inputs). Replaced on
    /// `activate`, destroyed on `deactivate`.
    context: Option<zwp_input_method_context_v1::ZwpInputMethodContextV1>,
    /// The grab keyboard of the CURRENT context; dropped with it.
    keyboard: Option<wl_keyboard::WlKeyboard>,

    xkb_context: xkb::Context,
    xkb_state: Option<xkb::State>,

    /// Latest `commit_state` serial — echoed by every commit/preedit/key
    /// request so the client can drop reactions to an outdated state.
    serial: u32,
    content_purpose: u32,

    /// Keycodes whose PRESS we consumed — their release is swallowed too.
    swallowed: HashSet<u32>,

    /// `Settings::use_preedit` mirror (macro_sync watcher keeps it live).
    /// `false` + surrounding-text support ⇒ the no-preedit model, same
    /// contract as the IBus backend's `sync_use_preedit`.
    use_preedit: Arc<std::sync::atomic::AtomicBool>,
    /// The CURRENT context sent a `surrounding_text` event WITH CONTENT —
    /// the v1 stand-in for IBus's surrounding-text capability bit: only
    /// clients that demonstrably report real surrounding text can be trusted
    /// to apply `delete_surrounding_text`. The non-empty requirement is
    /// load-bearing: Konsole emits the event with an always-empty string
    /// while silently dropping deletes (observed on Plasma 6 — every tone
    /// mark duplicated the word), so an empty event proves nothing. Reset
    /// per context. VTE terminals DO send real content and are pinned to
    /// preedit via [`PURPOSE_TERMINAL_V1`] instead — the two gates cover the
    /// two terminal families independently.
    saw_surrounding: bool,
    /// Composition model last pushed into the bridge (`true` = preedit).
    applied_composition: bool,
    /// Shadow of the app-side current word while in no-preedit mode.
    /// `delete_surrounding_text` counts BYTES but the bridge's
    /// `DeleteSurrounding` op counts CHARACTERS (the IBus unit) — popping
    /// chars off this shadow yields the byte length to delete. Cleared on
    /// word boundaries, resets, and (de)activation.
    committed_word: String,

    bridge: EngineBridge,
    method_state: Arc<MethodState>,
    seen_generation: u64,
}

impl ImeV1State {
    fn new(
        method_state: Arc<MethodState>,
        macros: Arc<Mutex<buttre_core::state::macros::MacroStore>>,
        strict: Arc<std::sync::atomic::AtomicBool>,
        use_preedit: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let method = method_state.method();
        let seen_generation = method_state.generation();
        let mut bridge = EngineBridge::new_with_macros(&method, macros);
        bridge.set_strict_flag(strict);
        Self {
            im: None,
            context: None,
            keyboard: None,
            xkb_context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            xkb_state: None,
            serial: 0,
            content_purpose: 0,
            swallowed: HashSet::new(),
            use_preedit,
            saw_surrounding: false,
            applied_composition: true,
            committed_word: String::new(),
            bridge,
            method_state,
            seen_generation,
        }
    }

    fn sensitive_field(&self) -> bool {
        self.content_purpose == PURPOSE_PASSWORD_V1
    }

    /// Push commits + the current preedit to the client. Unlike v2 there is
    /// no `commit` batching: requests apply as sent, and `commit_string`
    /// clears any previous preedit — so the preedit is re-declared (or
    /// cleared, via an empty string) after every op batch.
    ///
    /// `DeleteSurrounding` is converted from the bridge's CHARACTER count to
    /// the protocol's BYTE count via [`Self::committed_word`]. Per the v1
    /// spec a `delete_surrounding_text` is only applied by the client
    /// "directly following a commit_string request", so a trailing delete
    /// with no commit after it is flushed with an empty `commit_string`.
    fn commit_ops(&mut self, ops: Vec<ImeOp>) {
        let Some(ctx) = &self.context else { return };
        let mut delete_pending = false;
        for op in &ops {
            match op {
                ImeOp::Commit(text) => {
                    self.committed_word.push_str(text);
                    ctx.commit_string(self.serial, text.clone());
                    delete_pending = false;
                }
                ImeOp::DeleteSurrounding(chars) => {
                    let mut bytes = 0usize;
                    for _ in 0..*chars {
                        // Underflow (word started before we were watching):
                        // delete only what we tracked — a short correction
                        // beats corrupting unknown text.
                        match self.committed_word.pop() {
                            Some(c) => bytes += c.len_utf8(),
                            None => break,
                        }
                    }
                    if bytes > 0 {
                        ctx.delete_surrounding_text(-(bytes as i32), bytes as u32);
                        delete_pending = true;
                    }
                }
                ImeOp::Preedit(_)
                | ImeOp::Candidates { .. }
                | ImeOp::HideCandidates => {}
            }
        }
        if delete_pending {
            ctx.commit_string(self.serial, String::new());
        }
        let preedit = self.bridge.preedit().to_string();
        // Cursor at the end (byte offset); `commit` = the preedit itself so a
        // field reset (focus loss) inserts the pending word — the same
        // behavior the IBus backend gets from preedit COMMIT mode.
        ctx.preedit_cursor(preedit.len() as i32);
        ctx.preedit_string(self.serial, preedit.clone(), preedit);
    }

    /// Re-inject a key the engine didn't consume. `serial`/`time` are the
    /// originating `wl_keyboard::key` arguments, as the protocol requires.
    fn forward_key(&self, serial: u32, time: u32, key: u32, pressed: bool) {
        if let Some(ctx) = &self.context {
            ctx.key(serial, time, key, if pressed { 1 } else { 0 });
        }
    }

    /// Apply a pending preedit-model change, mirroring the IBus backend's
    /// `sync_use_preedit`: no-preedit (commit-as-you-go — no underline)
    /// engages only when the user turned the setting off AND the current
    /// client reports surrounding text (see [`Self::saw_surrounding`]).
    /// Clients without it (terminals) stay on preedit, where
    /// `delete_surrounding_text` is never needed.
    fn sync_use_preedit(&mut self) {
        let want_composition = self
            .use_preedit
            .load(std::sync::atomic::Ordering::Relaxed)
            || !self.saw_surrounding
            || self.content_purpose == PURPOSE_TERMINAL_V1;
        if want_composition != self.applied_composition {
            // Never flip mid-word: the flush inside set_use_composition would
            // commit a half-composed word and break its tone placement. The
            // confirming surrounding_text usually lands right after a word
            // commit, so the preedit is empty in the normal flow anyway.
            if !self.bridge.preedit().is_empty() {
                return;
            }
            let outcome = self.bridge.set_use_composition(want_composition);
            self.commit_ops(outcome.ops);
            self.applied_composition = want_composition;
            self.committed_word.clear();
            tracing::debug!("preedit model -> composition={want_composition}");
        }
    }

    /// Apply a pending tray-side method switch — same lazy generation check
    /// as the v2 and IBus backends.
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
                self.committed_word.clear();
                tracing::info!("Wayland v1 engine switched to method {method}");
            }
            None => tracing::warn!("Method switch to {method} failed; keeping current"),
        }
    }

    /// Route one grabbed key: engine-consumed keys update the composition,
    /// everything else is re-injected via `context.key`. Mirrors the v2
    /// `handle_key` contract (swallowed releases, combo flush, password
    /// bypass) with v1 forwarding.
    fn handle_key(&mut self, serial: u32, time: u32, key: u32, pressed: bool) {
        if !pressed {
            if self.swallowed.remove(&key) {
                return;
            }
            self.forward_key(serial, time, key, false);
            return;
        }

        self.sync_method();
        self.sync_use_preedit();

        let Some(keysym) = self
            .xkb_state
            .as_ref()
            .map(|s| s.key_get_one_sym(xkb::Keycode::new(key + 8)).raw())
        else {
            self.forward_key(serial, time, key, true);
            return;
        };

        if self.sensitive_field() {
            self.forward_key(serial, time, key, true);
            return;
        }

        let combo = self.xkb_state.as_ref().is_some_and(|s| {
            s.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE)
                || s.mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE)
                || s.mod_name_is_active(xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE)
        });
        if combo {
            let outcome = self.bridge.flush_pending();
            self.commit_ops(outcome.ops);
            self.committed_word.clear();
            self.forward_key(serial, time, key, true);
            return;
        }

        if is_modifier_keysym(keysym) {
            self.forward_key(serial, time, key, true);
            return;
        }

        if is_break_keysym(keysym) {
            let outcome = self.bridge.flush_pending();
            self.commit_ops(outcome.ops);
            // Word boundary: the shadow only ever describes the CURRENT word.
            self.committed_word.clear();
            self.forward_key(serial, time, key, true);
            return;
        }

        let Some(ch) = keysym_to_char(keysym) else {
            self.forward_key(serial, time, key, true);
            return;
        };

        let outcome = if ch == '\x08' {
            self.bridge.backspace()
        } else {
            self.bridge.process_char(ch)
        };
        // Commit BEFORE any forward — same connection, so the committed word
        // lands in the app ahead of the re-injected key.
        self.commit_ops(outcome.ops);
        if outcome.handled {
            self.swallowed.insert(key);
        } else {
            // Forwarded keys still change the app-side word — mirror them in
            // the shadow so a later engine Replace deletes the right bytes
            // (no-preedit mode only; the shadow is idle under composition).
            if !self.applied_composition {
                if ch == '\x08' {
                    self.committed_word.pop();
                } else {
                    self.committed_word.push(ch);
                }
            }
            self.forward_key(serial, time, key, true);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for ImeV1State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            if interface == "zwp_input_method_v1" {
                state.im = Some(
                    registry.bind::<zwp_input_method_v1::ZwpInputMethodV1, _, _>(name, 1, qh, ()),
                );
            }
        }
    }
}

impl Dispatch<zwp_input_method_v1::ZwpInputMethodV1, ()> for ImeV1State {
    fn event(
        state: &mut Self,
        _im: &zwp_input_method_v1::ZwpInputMethodV1,
        event: zwp_input_method_v1::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use zwp_input_method_v1::Event;
        match event {
            Event::Activate { id } => {
                // A stale context (missed deactivate) must be destroyed, or
                // the compositor keeps it alive server-side.
                if let Some(old) = state.context.take() {
                    old.destroy();
                }
                state.keyboard = Some(id.grab_keyboard(qh, ()));
                state.context = Some(id);
                state.swallowed.clear();
                state.content_purpose = 0;
                // Surrounding-text support is a per-context fact — reassessed
                // for every text input (sync_use_preedit falls back to the
                // preedit model until this context proves support).
                state.saw_surrounding = false;
                state.committed_word.clear();
                state.bridge.discard();
                tracing::debug!("text input activated (v1 context created)");
            }
            Event::Deactivate { context } => {
                context.destroy();
                if state
                    .context
                    .as_ref()
                    .is_some_and(|current| *current == context)
                {
                    state.context = None;
                    state.keyboard = None;
                    // Focus left — the client inserts the preedit itself (the
                    // `commit` argument of preedit_string); only reset OUR
                    // side, emitting nothing.
                    state.bridge.discard();
                    state.swallowed.clear();
                    state.committed_word.clear();
                }
                tracing::debug!("text input deactivated (v1 context destroyed)");
            }
            _ => {}
        }
    }

    wayland_client::event_created_child!(ImeV1State, zwp_input_method_v1::ZwpInputMethodV1, [
        zwp_input_method_v1::EVT_ACTIVATE_OPCODE => (zwp_input_method_context_v1::ZwpInputMethodContextV1, ()),
    ]);
}

impl Dispatch<zwp_input_method_context_v1::ZwpInputMethodContextV1, ()> for ImeV1State {
    fn event(
        state: &mut Self,
        ctx: &zwp_input_method_context_v1::ZwpInputMethodContextV1,
        event: zwp_input_method_context_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Events from a retired context must not touch live state.
        if state.context.as_ref() != Some(ctx) {
            return;
        }
        use zwp_input_method_context_v1::Event;
        match event {
            Event::CommitState { serial } => state.serial = serial,
            Event::ContentType { purpose, .. } => {
                tracing::debug!("content_type purpose={purpose}");
                state.content_purpose = purpose;
            }
            // The client reset the field (cursor moved, text replaced): drop
            // the composition without emitting — the preedit is already gone
            // on the client side.
            Event::Reset => {
                state.bridge.discard();
                state.swallowed.clear();
                state.committed_word.clear();
            }
            // Only surrounding text WITH CONTENT unlocks the no-preedit
            // model (see the `saw_surrounding` field doc for why empty
            // events must not count — Konsole).
            Event::SurroundingText { text, .. } => {
                if !text.is_empty() {
                    state.saw_surrounding = true;
                }
            }
            Event::InvokeAction { .. } | Event::PreferredLanguage { .. } => {}
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for ImeV1State {
    fn event(
        state: &mut Self,
        _kb: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wl_keyboard::Event;
        match event {
            Event::Keymap { fd, size, .. } => {
                let keymap = unsafe {
                    xkb::Keymap::new_from_fd(
                        &state.xkb_context,
                        fd,
                        size as usize,
                        xkb::KEYMAP_FORMAT_TEXT_V1,
                        xkb::KEYMAP_COMPILE_NO_FLAGS,
                    )
                };
                match keymap {
                    Ok(Some(keymap)) => {
                        state.xkb_state = Some(xkb::State::new(&keymap));
                        tracing::debug!("keymap loaded ({size} bytes)");
                    }
                    Ok(None) => tracing::warn!("keymap compile failed (invalid keymap)"),
                    Err(e) => tracing::warn!("keymap read failed: {e}"),
                }
            }
            Event::Key {
                serial,
                time,
                key,
                state: key_state,
            } => {
                let pressed = matches!(key_state, WEnum::Value(wl_keyboard::KeyState::Pressed));
                state.handle_key(serial, time, key, pressed);
            }
            Event::Modifiers {
                serial,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } => {
                if let Some(xkb_state) = &mut state.xkb_state {
                    xkb_state.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
                // Forward so the app keeps an accurate modifier state for the
                // keys we re-inject (Ctrl+C must stay Ctrl+C).
                if let Some(ctx) = &state.context {
                    ctx.modifiers(serial, mods_depressed, mods_latched, mods_locked, group);
                }
            }
            _ => {}
        }
    }
}

/// Run the v1 engine on an ALREADY-OPEN connection (see module docs for why
/// the caller connects). Blocks for the process lifetime; returns
/// [`Unavailable`] when the compositor doesn't speak `zwp_input_method_v1`,
/// so `run_engine` can fall through to IBus.
pub(super) fn run_engine(conn: Connection) -> Result<()> {
    let display = conn.display();

    // Same deferral contract as the v2 backend: nothing that owns a thread
    // or an inotify watch may start before availability is confirmed.
    let method_state = MethodState::load();
    let macros = macro_sync::load_initial();
    let strict = macro_sync::load_initial_strict();
    // Loaded for the shared watcher signature only — v1 stays on the preedit
    // model, exactly like v2 (see mod.rs on `use_preedit`).
    let use_preedit = macro_sync::load_initial_use_preedit();

    let mut state = ImeV1State::new(
        method_state.clone(),
        macros.clone(),
        strict.clone(),
        use_preedit.clone(),
    );
    let mut queue = conn.new_event_queue::<ImeV1State>();
    let qh: QueueHandle<ImeV1State> = queue.handle();
    display.get_registry(&qh, ());
    queue.roundtrip(&mut state)?;

    if state.im.is_none() {
        return Err(anyhow!(Unavailable(
            "compositor lacks zwp_input_method_v1".into()
        )));
    }

    method_sync::spawn_watcher(method_state);
    macro_sync::spawn_watcher(macros, strict, use_preedit);

    tracing::info!("Wayland v1 (KDE) input method registered; waiting for activation");
    loop {
        queue.blocking_dispatch(&mut state)?;
    }
}
