//! Composition model of the `zwp_input_method_v2` backend: the underlined
//! preedit, or — "Gõ thẳng không gạch chân" (`Settings::use_preedit` off) —
//! commit-as-you-go, revising the word already in the field. The second
//! model exists for apps that never draw a preedit: COSMIC's own apps
//! (cosmic-edit, cosmic-term) show nothing until Space.
//!
//! A revision deletes the tail of the current word one of two ways:
//! - `delete_surrounding_text` when the field reports real surrounding text
//!   and is not a terminal (a client that reports none cannot be trusted to
//!   apply deletes; terminals report text yet cannot delete it);
//! - otherwise BackSpace key presses through the virtual keyboard — the
//!   Unikey-style fallback that works in any app, terminals included.
//!   COSMIC's apps report no surrounding text, so they land here.

use super::ImeState;
use xkbcommon::xkb;

/// text-input-v3 `content_purpose.terminal`.
const PURPOSE_TERMINAL: u32 = 13;
/// evdev `KEY_BACKSPACE` — the fallback when the keymap has no BackSpace.
const KEY_BACKSPACE: u32 = 14;

impl ImeState {
    /// Apply a pending preedit-model change. Never flips mid-word — the
    /// flush inside `set_use_composition` would commit half a word and break
    /// its tone placement; the next key after the word ends retries.
    pub(super) fn sync_use_preedit(&mut self) {
        // Passthrough: set_use_composition no-ops, so recording the model
        // would wedge this check until restart.
        if !self.bridge.is_enabled() {
            return;
        }
        let want_composition = self.use_preedit.load(std::sync::atomic::Ordering::Relaxed);
        if want_composition == self.applied_composition || !self.bridge.preedit().is_empty() {
            return;
        }
        let outcome = self.bridge.set_use_composition(want_composition);
        self.commit_ops(outcome.ops);
        self.applied_composition = want_composition;
        self.committed_word.clear();
        tracing::debug!("preedit model -> composition={want_composition}");
    }

    /// Delete the last `chars` characters of the current word in the app.
    /// `pending` holds commit text not yet sent this cycle: it is flushed
    /// first, since the compositor applies a cycle's delete BEFORE its
    /// commit string, and synthesized keys must follow the text they erase.
    /// Characters are counted against [`Self::committed_word`] — an underflow
    /// (word started before we were watching) deletes only what we tracked.
    pub(super) fn delete_word_chars(&mut self, chars: usize, pending: &mut String) {
        let Some(im) = self.input_method.clone() else {
            return;
        };
        if !pending.is_empty() {
            im.commit_string(std::mem::take(pending));
            im.commit(self.serial);
        }
        let mut bytes = 0usize;
        let mut count = 0u32;
        for _ in 0..chars {
            match self.committed_word.pop() {
                Some(c) => {
                    bytes += c.len_utf8();
                    count += 1;
                }
                None => break,
            }
        }
        if count == 0 {
            return;
        }
        let via_surrounding = self.saw_surrounding && self.content_purpose != PURPOSE_TERMINAL;
        tracing::debug!(
            "delete {count} chars ({bytes} bytes) via {}",
            if via_surrounding {
                "surrounding text"
            } else {
                "BackSpace"
            }
        );
        if via_surrounding {
            im.delete_surrounding_text(bytes as u32, 0);
        } else if let Some(vk) = &self.virtual_kb {
            let key = self.backspace_keycode();
            for _ in 0..count {
                vk.key(self.last_key_time, key, 1);
                vk.key(self.last_key_time, key, 0);
            }
        }
    }

    /// The evdev keycode that types BackSpace in the keymap the virtual
    /// keyboard carries — the one the compositor last sent the grab. A
    /// compositor may hand over a keymap of a virtual keyboard (`wtype`,
    /// remote input) where keycode 14 is something else entirely.
    fn backspace_keycode(&self) -> u32 {
        let Some(state) = &self.xkb_state else {
            return KEY_BACKSPACE;
        };
        let keymap = state.get_keymap();
        (keymap.min_keycode().raw()..=keymap.max_keycode().raw())
            .find(|&code| {
                keymap
                    .key_get_syms_by_level(xkb::Keycode::new(code), 0, 0)
                    .iter()
                    .any(|sym| sym.raw() == xkb::keysyms::KEY_BackSpace)
            })
            .and_then(|code| code.checked_sub(8))
            .unwrap_or(KEY_BACKSPACE)
    }

    /// Mirror a key that reached the app un-composed (no-preedit mode only)
    /// so a later revision deletes the right characters.
    pub(super) fn track_forwarded_char(&mut self, ch: char) {
        if self.applied_composition {
            return;
        }
        if ch == '\x08' {
            self.committed_word.pop();
        } else {
            self.committed_word.push(ch);
        }
    }
}
