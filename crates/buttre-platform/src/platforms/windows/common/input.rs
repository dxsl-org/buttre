//! Input Simulation
//!
//! Uses SendInput to send keystrokes (backspace and Unicode characters).

use tracing::debug;

#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_BACK, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

/// Extra info flag to identify our own injected keys
pub const BUTTRE_INJECTED: usize = 0x564B4559; // "VKEY" in hex

/// Modifier keys that turn an injected keystroke into an application shortcut,
/// in the left/right-specific form `SendInput` needs. Both variants of each are
/// listed because only the one actually held may be released — releasing the
/// other would be a keyup for a key that was never down.
#[cfg(windows)]
const SHORTCUT_MODIFIER_VKS: [u16; 8] = [
    VK_LCONTROL,
    VK_RCONTROL,
    VK_LSHIFT,
    VK_RSHIFT,
    VK_LMENU,
    VK_RMENU,
    VK_LWIN,
    VK_RWIN,
];

/// Temporarily lifts the user's physically-held modifier keys for the duration
/// of an injection, restoring them on drop.
///
/// ## Why this exists
///
/// Injected input composes with whatever modifiers are ACTUALLY down. The
/// ordinary typing path never has any (a Ctrl/Alt chord never reaches the
/// engine as text), but the `Ctrl+Shift+Z` word toggle injects while the user
/// is still holding the chord — the tray polls the hotkey every 50 ms and
/// nobody releases a key that fast. Without this guard the application sees
/// `Ctrl+Shift+Backspace` instead of `Backspace` (delete-previous-WORD in Word
/// and VS Code, silently destroying text) and the `KEYEVENTF_UNICODE` payload
/// gets routed to accelerators instead of inserted.
///
/// ## Restore is conditional, and that asymmetry is deliberate
///
/// On drop, a modifier is re-pressed ONLY if it is still physically held at
/// that instant. Getting it wrong in that direction leaves the application
/// believing a held key is up — harmless, self-correcting on the next
/// keystroke. Re-pressing unconditionally could leave a modifier stuck DOWN
/// from the application's point of view, which breaks all subsequent typing;
/// never trade toward that failure.
///
/// Every synthetic event carries [`BUTTRE_INJECTED`], so our own low-level
/// hook skips them and no engine state is touched.
#[cfg(windows)]
pub struct ModifiersReleased {
    /// The VKs this guard actually released, in release order.
    released: Vec<u16>,
}

#[cfg(windows)]
impl ModifiersReleased {
    /// Release every currently-held shortcut modifier. Cheap no-op (no
    /// `SendInput` at all) when none is held — the normal case for every
    /// caller other than the word toggle.
    ///
    /// Deliberately not named `new`/`default`: constructing this value INJECTS
    /// input as a side effect, which neither of those names would lead a reader
    /// to expect.
    pub fn release_held() -> Self {
        let released = held_shortcut_modifiers(is_physically_down);
        if !released.is_empty() {
            debug!(
                "Releasing {} held modifier(s) for injection",
                released.len()
            );
            send_modifier_batch(&released, KEYEVENTF_KEYUP);
        }
        Self { released }
    }
}

#[cfg(windows)]
impl Drop for ModifiersReleased {
    fn drop(&mut self) {
        // Re-check: the user may have let go during the injection, and
        // re-pressing then would strand the modifier down (see the type doc).
        let still_held = filter_still_held(&self.released, is_physically_down);
        if !still_held.is_empty() {
            send_modifier_batch(&still_held, 0);
        }
    }
}

/// Which shortcut modifiers to release, given a "is this VK down" oracle.
///
/// Split out from [`ModifiersReleased::release_held`] so the left/right
/// selection is unit-testable without injecting real keystrokes into the
/// developer's session (a test that pressed a real Ctrl and then failed could
/// strand it down system-wide).
#[cfg(windows)]
fn held_shortcut_modifiers(is_down: impl Fn(u16) -> bool) -> Vec<u16> {
    SHORTCUT_MODIFIER_VKS
        .into_iter()
        .filter(|&vk| is_down(vk))
        .collect()
}

/// Which of the previously-released modifiers to press back. Same
/// testability rationale as [`held_shortcut_modifiers`].
#[cfg(windows)]
fn filter_still_held(released: &[u16], is_down: impl Fn(u16) -> bool) -> Vec<u16> {
    released.iter().copied().filter(|&vk| is_down(vk)).collect()
}

/// True when `vk` is down according to the async (physical) key state.
///
/// `SendInput` updates this state, so after [`ModifiersReleased::release_held`]
/// a released modifier reads as up here — which is exactly what the
/// `shift_already_held` probe in [`send_replacement_via_selection`] needs to
/// see.
#[cfg(windows)]
fn is_physically_down(vk: u16) -> bool {
    // SAFETY: GetAsyncKeyState takes a plain VK code and cannot fail; the high
    // bit of the result indicates the key is currently down.
    let state = unsafe { GetAsyncKeyState(vk as i32) };
    state as u16 & 0x8000 != 0
}

/// Send one keyup-or-keydown event per VK in a single batch.
///
/// `flags` is [`KEYEVENTF_KEYUP`] to release or `0` to press. Win keys get
/// [`KEYEVENTF_EXTENDEDKEY`] — they are extended-scancode keys and some
/// applications ignore the event without it.
#[cfg(windows)]
fn send_modifier_batch(vks: &[u16], flags: u32) {
    let mut inputs: Vec<INPUT> = vks
        .iter()
        .map(|&vk| {
            let extended = if vk == VK_LWIN || vk == VK_RWIN {
                KEYEVENTF_EXTENDEDKEY
            } else {
                0
            };
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: flags | extended,
                        time: 0,
                        dwExtraInfo: BUTTRE_INJECTED,
                    },
                },
            }
        })
        .collect();

    // SAFETY: `inputs` is a live Vec of properly initialized INPUT structs;
    // the count matches its length and the size argument matches the struct.
    unsafe {
        let expected = inputs.len() as u32;
        let sent = SendInput(
            expected,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
        if sent != expected {
            tracing::error!("SendInput (modifiers) failed: expected {expected}, sent {sent}");
        }
    }
}

/// Send backspace keys (optimized with batching)
#[cfg(windows)]
pub fn send_backspaces(count: usize) {
    if count == 0 {
        return;
    }

    debug!("Sending {} backspaces", count);

    // Batch backspaces for better performance
    let mut inputs = Vec::with_capacity(count * 2);

    for _ in 0..count {
        // Key down
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_BACK,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: BUTTRE_INJECTED,
                },
            },
        });

        // Key up
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_BACK,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: BUTTRE_INJECTED,
                },
            },
        });
    }

    // Send all at once with error checking
    // SAFETY:
    // 1. inputs is a valid Vec<INPUT> allocated on the stack
    // 2. as_mut_ptr() returns valid pointer to first element (or null if empty, but we check count == 0 above)
    // 3. expected count matches actual Vec length
    // 4. size_of::<INPUT>() is the correct structure size for SendInput
    // 5. SendInput is properly declared in windows_sys
    // 6. INPUT structs are properly initialized with valid VK codes and flags
    // 7. All memory is valid for the duration of the SendInput call
    unsafe {
        let expected = inputs.len() as u32;
        let sent = SendInput(
            expected,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );

        if sent != expected {
            tracing::error!("SendInput failed: expected {}, sent {}", expected, sent);
        }
    }
}

#[cfg(not(windows))]
pub fn send_backspaces(_count: usize) {}

/// Send a string as Unicode characters (optimized with batching)
#[cfg(windows)]
pub fn send_string(s: &str) {
    if s.is_empty() {
        return;
    }

    debug!("Sending string: '{}'", s);

    let char_count = s.chars().count();
    let mut inputs = Vec::with_capacity(char_count * 2);

    for ch in s.chars() {
        let code = ch as u32;

        // Handle characters that need surrogate pairs
        if code > 0xFFFF {
            let code = code - 0x10000;
            let high = ((code >> 10) + 0xD800) as u16;
            let low = ((code & 0x3FF) + 0xDC00) as u16;
            add_unicode_inputs(&mut inputs, high);
            add_unicode_inputs(&mut inputs, low);
        } else {
            add_unicode_inputs(&mut inputs, code as u16);
        }
    }

    if !inputs.is_empty() {
        // SAFETY:
        // 1. inputs is a valid Vec<INPUT> with unicode key events
        // 2. as_mut_ptr() returns valid pointer, validated non-empty above
        // 3. expected count matches Vec length
        // 4. size_of::<INPUT>() is correct for SendInput
        // 5. SendInput is properly declared in windows_sys
        // 6. INPUT structs contain valid Unicode scan codes (wScan field)
        // 7. KEYEVENTF_UNICODE flag properly set for unicode input
        unsafe {
            let expected = inputs.len() as u32;
            let sent = SendInput(
                expected,
                inputs.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );

            if sent != expected {
                tracing::error!("SendInput failed: expected {}, sent {}", expected, sent);
            }
        }
    }
}

/// Send backspaces and string in one batch.
///
/// In a Chromium omnibox (and ONLY there — see `omnibox_fix` for the
/// two-gate detection) a raw `VK_BACK` is consumed dismissing the inline
/// autocomplete selection instead of deleting the typed character, so the
/// plain batch under-deletes by one. In that context this switches to the
/// select-and-overwrite variant below; everywhere else the behavior is
/// byte-for-byte what it always was.
#[cfg(windows)]
pub fn send_replacement(backspace_count: usize, text: &str) {
    if backspace_count > 0 && super::omnibox_fix::should_apply_omnibox_fix() {
        debug!(
            "Omnibox fix: selection replacement, {} backspaces",
            backspace_count
        );
        send_replacement_via_selection(backspace_count, text);
        return;
    }
    send_replacement_plain(backspace_count, text);
}

/// Omnibox variant (OpenKey's mechanism, verified against live Chrome):
/// `Shift+Left` pre-selects the last real character — collapsing any inline
/// autocomplete ghost selection in the process. With exactly one char to
/// delete and text to insert, no backspace is sent at all (the text types
/// over the selection — an atomic replace the autocomplete can't disturb);
/// otherwise all `backspace_count` backspaces are sent, the first consuming
/// the 1-char selection so the net deleted count is unchanged. Everything
/// ships in a single `SendInput` batch, all tagged `BUTTRE_INJECTED` so our
/// own hook ignores them.
///
/// Modifier note: transforms only ever fire on plain character keystrokes
/// (Ctrl/Alt chords never reach the engine as text), so injecting an
/// LSHIFT-down/LEFT/LSHIFT-up sandwich cannot compose with a held Ctrl/Alt
/// into a word-selection chord. The ONE caller for which that does not hold —
/// the `Ctrl+Shift+Z` word toggle, which injects while the chord is still down
/// — wraps its call in [`ModifiersReleased`], so by the time this runs the
/// modifiers are already lifted. When Shift is ALREADY physically held (e.g.
/// typing an all-caps word), the synthetic down/up is skipped — pressing it
/// anyway would still select correctly, but the synthetic keyup would lift
/// Shift out from under the user's still-held key, un-capitalizing whatever
/// they type next until they release and re-press it (review MED).
#[cfg(windows)]
fn send_replacement_via_selection(backspace_count: usize, text: &str) {
    debug_assert!(
        backspace_count > 0,
        "send_replacement_via_selection requires backspace_count > 0 — \
         the caller in send_replacement already guards this, but with \
         backspace_count == 0 this fires zero backspaces AND selects+\
         overwrites a real character the caller never asked to delete"
    );

    let key = |vk: u16, flags: u32| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: BUTTRE_INJECTED,
            },
        },
    };

    // SAFETY: GetAsyncKeyState takes a plain VK code and cannot fail; the
    // high bit of the result indicates the key is currently down.
    let shift_already_held = unsafe { GetAsyncKeyState(VK_SHIFT as i32) } as u16 & 0x8000 != 0;

    let char_count = text.chars().count();
    let mut inputs = Vec::with_capacity(6 + backspace_count.saturating_sub(1) * 2 + char_count * 2);

    // Shift+Left — KEYEVENTF_EXTENDEDKEY marks the real arrow key (not
    // numpad-4). Only synthesize the Shift chord ourselves when the user
    // isn't already holding it physically.
    if !shift_already_held {
        inputs.push(key(VK_LSHIFT, 0));
    }
    inputs.push(key(VK_LEFT, KEYEVENTF_EXTENDEDKEY));
    inputs.push(key(VK_LEFT, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP));
    if !shift_already_held {
        inputs.push(key(VK_LSHIFT, KEYEVENTF_KEYUP));
    }

    // OpenKey's counting rule: with exactly one char to delete AND text to
    // insert, send zero backspaces — the overwrite consumes the selection.
    // In every other case send ALL backspaces unchanged: the first one
    // deletes the 1-char selection (same net count as deleting one char),
    // the rest act on real text — the ghost suggestion is already collapsed.
    let backspaces_to_send = if backspace_count == 1 && !text.is_empty() {
        0
    } else {
        backspace_count
    };
    for _ in 0..backspaces_to_send {
        inputs.push(key(VK_BACK, 0));
        inputs.push(key(VK_BACK, KEYEVENTF_KEYUP));
    }

    for ch in text.chars() {
        let code = ch as u32;
        if code > 0xFFFF {
            let code = code - 0x10000;
            let high = ((code >> 10) + 0xD800) as u16;
            let low = ((code & 0x3FF) + 0xDC00) as u16;
            add_unicode_inputs(&mut inputs, high);
            add_unicode_inputs(&mut inputs, low);
        } else {
            add_unicode_inputs(&mut inputs, code as u16);
        }
    }

    // SAFETY:
    // 1. inputs is a valid Vec<INPUT>, non-empty (≥ the 4 selection events)
    // 2. as_mut_ptr()/len() form a valid array for SendInput
    // 3. size_of::<INPUT>() is the correct structure size
    // 4. All INPUT structs are fully initialized with valid VK codes/flags
    // 5. Batching preserves event order (selection → deletes → text)
    unsafe {
        let expected = inputs.len() as u32;
        let sent = SendInput(
            expected,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
        if sent != expected {
            tracing::error!(
                "SendInput selection-replacement failed: expected {}, sent {}",
                expected,
                sent
            );
        }
    }
}

/// The original path: N backspaces + text, one batch.
#[cfg(windows)]
fn send_replacement_plain(backspace_count: usize, text: &str) {
    if backspace_count == 0 && text.is_empty() {
        return;
    }

    debug!(
        "Sending replacement: {} backspaces + '{}'",
        backspace_count, text
    );

    // Calculate total capacity needed
    let char_count = text.chars().count();
    // 2 inputs per backspace (down/up), 2 inputs per char (down/up) + extras for surrogates
    let capacity = (backspace_count * 2) + (char_count * 2);

    let mut inputs = Vec::with_capacity(capacity);

    // 1. Add Backspaces
    for _ in 0..backspace_count {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_BACK,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: BUTTRE_INJECTED,
                },
            },
        });

        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_BACK,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: BUTTRE_INJECTED,
                },
            },
        });
    }

    // 2. Add Text
    for ch in text.chars() {
        let code = ch as u32;
        if code > 0xFFFF {
            let code = code - 0x10000;
            let high = ((code >> 10) + 0xD800) as u16;
            let low = ((code & 0x3FF) + 0xDC00) as u16;
            add_unicode_inputs(&mut inputs, high);
            add_unicode_inputs(&mut inputs, low);
        } else {
            add_unicode_inputs(&mut inputs, code as u16);
        }
    }

    // 3. Send all at once
    if !inputs.is_empty() {
        // SAFETY:
        // 1. inputs contains both backspace and unicode key events in single batch
        // 2. as_mut_ptr() returns valid pointer, validated non-empty above
        // 3. expected count matches Vec length
        // 4. size_of::<INPUT>() is correct for SendInput
        // 5. SendInput is properly declared in windows_sys
        // 6. Batching improves performance and timing consistency
        // 7. All INPUT structs properly initialized with correct flags
        unsafe {
            let expected = inputs.len() as u32;
            let sent = SendInput(
                expected,
                inputs.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );

            if sent != expected {
                tracing::error!(
                    "SendInput batch failed: expected {}, sent {}",
                    expected,
                    sent
                );
            }
        }
    }
}

#[cfg(not(windows))]
pub fn send_string(_s: &str) {}

#[cfg(not(windows))]
pub fn send_replacement(_backspace_count: usize, _text: &str) {}

/// Add Unicode key down/up to inputs vector
#[cfg(windows)]
fn add_unicode_inputs(inputs: &mut Vec<INPUT>, scan: u16) {
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: BUTTRE_INJECTED,
            },
        },
    });

    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: BUTTRE_INJECTED,
            },
        },
    });
}

/// Send a single Unicode character
#[cfg(windows)]
pub fn send_unicode_char(ch: char) {
    send_string(&ch.to_string());
}

#[cfg(not(windows))]
pub fn send_unicode_char(_ch: char) {}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Only the variant actually held is released — emitting a keyup for a key
    /// that was never down would tell the application a key was let go that it
    /// never saw pressed.
    #[test]
    fn releases_only_the_held_variant_of_each_modifier() {
        let held = held_shortcut_modifiers(|vk| vk == VK_LCONTROL || vk == VK_RSHIFT);
        assert_eq!(held, vec![VK_LCONTROL, VK_RSHIFT]);
    }

    #[test]
    fn nothing_held_releases_nothing() {
        assert!(held_shortcut_modifiers(|_| false).is_empty());
    }

    #[test]
    fn covers_every_shortcut_modifier() {
        // Ctrl and Alt turn Backspace into delete-word; Shift turns it into a
        // selection extend; Win opens the shell. All four must be lifted, in
        // both left and right form.
        let held = held_shortcut_modifiers(|_| true);
        assert_eq!(held.len(), SHORTCUT_MODIFIER_VKS.len());
        for vk in [
            VK_LCONTROL,
            VK_RCONTROL,
            VK_LSHIFT,
            VK_RSHIFT,
            VK_LMENU,
            VK_RMENU,
            VK_LWIN,
            VK_RWIN,
        ] {
            assert!(held.contains(&vk), "missing modifier {vk:#x}");
        }
    }

    /// The restore step re-presses a SUBSET of what was released, never
    /// anything else — a modifier the user let go of during the injection must
    /// stay up (stranding one DOWN breaks all subsequent typing).
    #[test]
    fn restore_skips_modifiers_released_during_the_injection() {
        let released = vec![VK_LCONTROL, VK_LSHIFT];
        let still = filter_still_held(&released, |vk| vk == VK_LCONTROL);
        assert_eq!(still, vec![VK_LCONTROL]);
    }

    #[test]
    fn restore_presses_nothing_when_everything_was_let_go() {
        assert!(filter_still_held(&[VK_LCONTROL, VK_LSHIFT], |_| false).is_empty());
    }
}
