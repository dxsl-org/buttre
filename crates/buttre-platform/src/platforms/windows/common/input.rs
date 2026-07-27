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

/// Modifier keys that turn an injected keystroke into an application shortcut.
/// Both the left and right variant of each is listed: either one alone is
/// enough to change how an injected keystroke is interpreted.
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

/// True when any modifier that would turn an injected keystroke into an
/// application shortcut is currently held.
///
/// ## Why callers need this
///
/// Injected input composes with whatever modifiers are ACTUALLY down. The
/// ordinary typing path never has any (a Ctrl/Alt chord never reaches the
/// engine as text), but the `Ctrl+Shift+Z` word toggle is dispatched from the
/// tray's event loop up to 50 ms after the keypress — nobody lets go of a key
/// that fast. Injecting then makes the application see `Ctrl+Shift+Backspace`
/// instead of `Backspace` (delete-previous-WORD in Word and VS Code, silently
/// destroying text) and routes the `KEYEVENTF_UNICODE` payload to accelerators
/// instead of inserting it.
///
/// The fix is to WAIT for the chord to be released — see
/// `hook::dispatch_toggle_last_word`.
///
/// ## Why not synthesize a keyup/keydown pair around the injection
///
/// Tried and reverted: `SendInput` UPDATES the async key state this function
/// reads, so once we release Ctrl ourselves we can no longer tell whether the
/// user is still holding it. Both ways of restoring are wrong — doing it
/// unconditionally can strand a modifier DOWN (breaking every later
/// keystroke), and doing it only for keys that still read as held restores
/// nothing at all, leaving the system convinced the user let go. The latter is
/// what silently swallowed every repeat of the chord: the first press worked,
/// the second arrived as a bare `z`.
#[cfg(windows)]
pub fn any_shortcut_modifier_held() -> bool {
    SHORTCUT_MODIFIER_VKS.into_iter().any(is_physically_down)
}

#[cfg(not(windows))]
pub fn any_shortcut_modifier_held() -> bool {
    false
}

/// True when `vk` is down according to the async (physical) key state.
#[cfg(windows)]
fn is_physically_down(vk: u16) -> bool {
    // SAFETY: GetAsyncKeyState takes a plain VK code and cannot fail; the high
    // bit of the result indicates the key is currently down.
    let state = unsafe { GetAsyncKeyState(vk as i32) };
    state as u16 & 0x8000 != 0
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
/// the `Ctrl+Shift+Z` word toggle — defers its injection until the chord is
/// released (see [`any_shortcut_modifier_held`]), so no modifier is down by
/// the time this runs either. When Shift is ALREADY physically held (e.g.
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

    /// Every modifier that reinterprets an injected keystroke must be watched,
    /// in both left and right form: Ctrl and Alt turn Backspace into
    /// delete-WORD, Shift turns it into a selection extend, Win opens the
    /// shell. Missing one means the word toggle would inject through it.
    #[test]
    fn watches_both_variants_of_every_shortcut_modifier() {
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
            assert!(
                SHORTCUT_MODIFIER_VKS.contains(&vk),
                "missing modifier {vk:#x}"
            );
        }
    }

    /// Deliberately NOT the generic `VK_CONTROL`/`VK_SHIFT`/`VK_MENU`: those
    /// report either side, which is fine for a pure probe but would have made
    /// the list ambiguous for anything that needs to know WHICH key is down.
    #[test]
    fn watch_list_has_no_duplicates() {
        let mut seen = SHORTCUT_MODIFIER_VKS.to_vec();
        seen.sort_unstable();
        let len_before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), len_before);
    }
}
