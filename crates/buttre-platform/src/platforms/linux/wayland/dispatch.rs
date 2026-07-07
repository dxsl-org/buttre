//! Wayland event dispatch for the input-method backend.
//!
//! Key-routing contract (mirrors the IBus adapter, semantics from
//! `EngineBridge`): presses we consume are remembered in `swallowed` so
//! their releases are consumed too; everything else is re-injected through
//! the virtual keyboard. Commits are requested on the same connection
//! BEFORE a forwarded separator key, so the committed word always lands
//! first in the app.

use super::super::engine_bridge::{is_break_keysym, is_modifier_keysym, keysym_to_char};
use super::ImeState;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2, zwp_input_method_manager_v2, zwp_input_method_v2,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1, zwp_virtual_keyboard_v1,
};
use xkbcommon::xkb;

impl Dispatch<wl_registry::WlRegistry, ()> for ImeState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" => {
                    state.seat =
                        Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(4), qh, ()));
                }
                "zwp_input_method_manager_v2" => {
                    state.im_manager = Some(
                        registry
                            .bind::<zwp_input_method_manager_v2::ZwpInputMethodManagerV2, _, _>(
                                name,
                                1,
                                qh,
                                (),
                            ),
                    );
                }
                "zwp_virtual_keyboard_manager_v1" => {
                    state.vk_manager = Some(registry.bind::<
                        zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, _, _,
                    >(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<zwp_input_method_v2::ZwpInputMethodV2, ()> for ImeState {
    fn event(
        state: &mut Self,
        _im: &zwp_input_method_v2::ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use zwp_input_method_v2::Event;
        match event {
            // Activation state is double-buffered — applied on Done.
            Event::Activate => state.pending_active = true,
            Event::Deactivate => state.pending_active = false,
            Event::ContentType { purpose, .. } => {
                state.pending_purpose = match purpose {
                    WEnum::Value(p) => p as u32,
                    WEnum::Unknown(u) => u,
                };
            }
            Event::SurroundingText { .. } | Event::TextChangeCause { .. } => {}
            Event::Done => {
                state.serial = state.serial.wrapping_add(1);
                state.content_purpose = state.pending_purpose;
                if state.pending_active && !state.active {
                    state.active = true;
                    // Grab ONLY while a text input is active — holding it
                    // outside text entry would swallow global keystrokes.
                    if let Some(im) = &state.input_method {
                        state.grab = Some(im.grab_keyboard(qh, ()));
                    }
                    tracing::debug!("text input activated (serial {})", state.serial);
                } else if !state.pending_active && state.active {
                    state.active = false;
                    if let Some(grab) = state.grab.take() {
                        grab.release();
                    }
                    // Focus left: text-input-v3 has no commit-on-focus-loss
                    // mode — an uncommitted preedit is discarded (known v1
                    // limitation, see module docs).
                    state.bridge.discard();
                    state.swallowed.clear();
                    tracing::debug!("text input deactivated");
                }
            }
            Event::Unavailable => {
                tracing::warn!("input method unavailable: another IME owns the seat");
                state.unavailable = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2, ()> for ImeState {
    fn event(
        state: &mut Self,
        _grab: &zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
        event: zwp_input_method_keyboard_grab_v2::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use zwp_input_method_keyboard_grab_v2::Event;
        match event {
            Event::Keymap { format, fd, size } => {
                // Seed the virtual keyboard with the compositor's OWN keymap
                // so forwarded keycodes mean exactly what the user typed.
                if let Some(vk) = &state.virtual_kb {
                    use std::os::fd::AsFd;
                    let format_raw = match format {
                        WEnum::Value(f) => f as u32,
                        WEnum::Unknown(u) => u,
                    };
                    vk.keymap(format_raw, fd.as_fd(), size);
                }
                // xkbcommon consumes an OwnedFd — duplicate to keep ours.
                match fd.try_clone() {
                    Ok(dup) => {
                        let keymap = unsafe {
                            xkb::Keymap::new_from_fd(
                                &state.xkb_context,
                                dup,
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
                    Err(e) => tracing::warn!("keymap fd dup failed: {e}"),
                }
                state.keymap_fd = Some((fd, size));
            }
            Event::Key {
                time,
                key,
                state: key_state,
                ..
            } => {
                let pressed = matches!(
                    key_state,
                    WEnum::Value(wayland_client::protocol::wl_keyboard::KeyState::Pressed)
                );
                state.handle_key(time, key, pressed);
            }
            Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(xkb_state) = &mut state.xkb_state {
                    xkb_state.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
                // Mirror modifiers to the virtual keyboard so forwarded keys
                // carry the right state (Ctrl+C must stay Ctrl+C).
                if let Some(vk) = &state.virtual_kb {
                    vk.modifiers(mods_depressed, mods_latched, mods_locked, group);
                }
            }
            Event::RepeatInfo { .. } => {
                // v1 limitation: no key repeat inside the composition.
            }
            _ => {}
        }
    }
}

impl ImeState {
    /// Route one key event: engine-consumed keys update the composition,
    /// everything else is re-injected via the virtual keyboard.
    fn handle_key(&mut self, time: u32, key: u32, pressed: bool) {
        if !pressed {
            if self.swallowed.remove(&key) {
                return; // we consumed the press — consume the release
            }
            self.forward_key(time, key, false);
            return;
        }

        self.sync_method();

        let Some(keysym) = self
            .xkb_state
            .as_ref()
            .map(|s| s.key_get_one_sym(xkb::Keycode::new(key + 8)).raw())
        else {
            self.forward_key(time, key, true);
            return;
        };

        // Password/PIN fields bypass the engine entirely.
        if self.sensitive_field() {
            self.forward_key(time, key, true);
            return;
        }

        // Shortcuts: commit the pending word, then let the combo through.
        let combo = self.xkb_state.as_ref().is_some_and(|s| {
            s.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE)
                || s.mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE)
                || s.mod_name_is_active(xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE)
        });
        if combo {
            let outcome = self.bridge.flush_pending();
            self.commit_ops(outcome.ops);
            self.forward_key(time, key, true);
            return;
        }

        if is_modifier_keysym(keysym) {
            self.forward_key(time, key, true);
            return;
        }

        if is_break_keysym(keysym) {
            let outcome = self.bridge.flush_pending();
            self.commit_ops(outcome.ops);
            self.forward_key(time, key, true);
            return;
        }

        let Some(ch) = keysym_to_char(keysym) else {
            self.forward_key(time, key, true);
            return;
        };

        let outcome = if ch == '\x08' {
            self.bridge.backspace()
        } else {
            self.bridge.process_char(ch)
        };
        // Commit BEFORE any forward — same connection, so the compositor
        // applies the committed word ahead of the re-injected key.
        self.commit_ops(outcome.ops);
        if outcome.handled {
            self.swallowed.insert(key);
        } else {
            self.forward_key(time, key, true);
        }
    }
}

// Interfaces with no events we care about.
wayland_client::delegate_noop!(ImeState: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(ImeState: ignore zwp_input_method_manager_v2::ZwpInputMethodManagerV2);
wayland_client::delegate_noop!(ImeState: ignore zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1);
wayland_client::delegate_noop!(ImeState: ignore zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1);
