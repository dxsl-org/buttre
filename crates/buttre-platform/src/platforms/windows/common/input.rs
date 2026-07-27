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
/// the `Ctrl+Shift+Z` word toggle — never reaches this function at all: it goes
/// through [`send_replacement_under_held_modifiers`], which uses the plain
/// payload precisely because the `shift_already_held` probe below cannot see
/// its own pending release. When Shift is ALREADY physically held (e.g.
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

    let mut inputs = build_plain_replacement(backspace_count, text);
    send_batch(&mut inputs, "batch");
}

/// N backspaces + text as INPUT events, no `SendInput` call.
///
/// Split from [`send_replacement_plain`] so the word-toggle path can wrap the
/// same payload in a modifier release/restore pair INSIDE ONE BATCH — see
/// [`send_replacement_under_held_modifiers`].
#[cfg(windows)]
fn build_plain_replacement(backspace_count: usize, text: &str) -> Vec<INPUT> {
    let char_count = text.chars().count();
    // 2 inputs per backspace (down/up), 2 per char (down/up) + surrogate extras
    let mut inputs = Vec::with_capacity((backspace_count * 2) + (char_count * 2));

    for _ in 0..backspace_count {
        inputs.push(key_input(VK_BACK, 0));
        inputs.push(key_input(VK_BACK, KEYEVENTF_KEYUP));
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
    inputs
}

/// One keyboard INPUT event for `vk`, tagged so our own hook skips it.
#[cfg(windows)]
fn key_input(vk: u16, flags: u32) -> INPUT {
    INPUT {
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
    }
}

/// Send one prepared batch, logging a short-write.
#[cfg(windows)]
fn send_batch(inputs: &mut [INPUT], what: &str) {
    if inputs.is_empty() {
        return;
    }
    // SAFETY: `inputs` is a live slice of fully initialized INPUT structs; the
    // count matches its length and the size argument matches the struct.
    unsafe {
        let expected = inputs.len() as u32;
        let sent = SendInput(
            expected,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
        if sent != expected {
            tracing::error!("SendInput {what} failed: expected {expected}, sent {sent}");
        }
    }
}

/// Replace text while the user is HOLDING a modifier chord — the word toggle's
/// injection path (`Ctrl+Shift+Z`).
///
/// ## The problem
///
/// Injected input composes with whatever modifiers are actually down. Sent
/// naively under a held chord, the application reads `Ctrl+Shift+Backspace`
/// instead of `Backspace` — delete-previous-WORD in Word and VS Code, which
/// silently ate text instead of replacing it — and the `KEYEVENTF_UNICODE`
/// payload gets routed to accelerators rather than inserted.
///
/// ## Why one batch is what makes this safe
///
/// The release, the payload and the restore all go in a SINGLE `SendInput`
/// call. Windows documents that the events of one call are never interspersed
/// with other input — user keystrokes included — so no keyboard state can
/// change mid-batch and the restore needs no state query at all. That matters
/// because a query would be worthless here: `SendInput` UPDATES the async key
/// state, so once we release Ctrl ourselves we can no longer tell whether the
/// user is still holding it. An earlier attempt that released and restored in
/// SEPARATE calls restored nothing for exactly that reason, leaving the system
/// convinced the user had let go — the first press worked and every repeat of
/// the chord was swallowed as a bare `z`.
///
/// ## Residual window
///
/// The modifier state is sampled a few microseconds before the batch is sent.
/// A physical release inside that window would be followed by our restore,
/// leaving that modifier down from the application's point of view until the
/// user presses and releases it once. Pressing a chord and releasing it within
/// microseconds is not humanly reachable, and the recovery is a single
/// keypress.
#[cfg(windows)]
pub fn send_replacement_under_held_modifiers(backspace_count: usize, text: &str) {
    if backspace_count == 0 && text.is_empty() {
        return;
    }
    let held: Vec<u16> = SHORTCUT_MODIFIER_VKS
        .into_iter()
        .filter(|&vk| is_physically_down(vk))
        .collect();
    if held.is_empty() {
        // Nothing to work around — take the ordinary path, which keeps the
        // Chromium-omnibox selection variant available.
        send_replacement(backspace_count, text);
        return;
    }

    debug!(
        "Replacement under {} held modifier(s): {} backspaces + {} char(s)",
        held.len(),
        backspace_count,
        text.chars().count()
    );

    // Plain payload only: the omnibox selection variant probes the async Shift
    // state to decide whether to synthesize its own Shift+Left, and inside this
    // batch that probe would still see the user's Shift as down even though our
    // prefix is about to lift it — it would then skip the sandwich and select
    // nothing. Under-deleting by one in a Chromium omnibox while the toggle
    // chord is held is the far smaller failure.
    let mut inputs =
        wrap_in_modifier_release(&held, build_plain_replacement(backspace_count, text));
    send_batch(&mut inputs, "replacement under held modifiers");
}

/// Sandwich `payload` between a release and a re-press of `held`, as ONE event
/// sequence.
///
/// The ordering IS the safety property (see
/// [`send_replacement_under_held_modifiers`]): releases first so the payload is
/// interpreted plainly, re-presses last and in the same batch so no state query
/// is needed to restore them.
#[cfg(windows)]
fn wrap_in_modifier_release(held: &[u16], payload: Vec<INPUT>) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(held.len() * 2 + payload.len());
    inputs.extend(held.iter().map(|&vk| key_input(vk, KEYEVENTF_KEYUP)));
    inputs.extend(payload);
    inputs.extend(held.iter().map(|&vk| key_input(vk, 0)));
    inputs
}

/// Insert text while the user is holding a modifier chord. Same contract and
/// rationale as [`send_replacement_under_held_modifiers`], for the commit-only
/// shape of the toggle's output.
#[cfg(windows)]
pub fn send_string_under_held_modifiers(text: &str) {
    send_replacement_under_held_modifiers(0, text);
}

#[cfg(not(windows))]
pub fn send_replacement_under_held_modifiers(_backspace_count: usize, _text: &str) {}

#[cfg(not(windows))]
pub fn send_string_under_held_modifiers(_text: &str) {}

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

    /// `(wVk, is_keyup)` per event, for asserting batch shape.
    fn shape(inputs: &[INPUT]) -> Vec<(u16, bool)> {
        inputs
            .iter()
            .map(|i| {
                // SAFETY: every INPUT built by this module is INPUT_KEYBOARD,
                // so reading the `ki` arm of the union is the correct variant.
                let ki = unsafe { i.Anonymous.ki };
                (ki.wVk, ki.dwFlags & KEYEVENTF_KEYUP != 0)
            })
            .collect()
    }

    /// THE safety property: releases first, re-presses LAST AND IN THE SAME
    /// batch. `SendInput` guarantees one call's events are never interspersed
    /// with other input, which is what lets the restore skip any state query —
    /// splitting this across two calls is what silently swallowed every repeat
    /// of the chord (the restore read its own release and did nothing).
    #[test]
    fn modifier_wrap_releases_before_payload_and_restores_after() {
        let held = [VK_LCONTROL, VK_LSHIFT];
        let payload = vec![key_input(VK_BACK, 0), key_input(VK_BACK, KEYEVENTF_KEYUP)];
        let batch = wrap_in_modifier_release(&held, payload);

        assert_eq!(
            shape(&batch),
            vec![
                (VK_LCONTROL, true), // release
                (VK_LSHIFT, true),   // release
                (VK_BACK, false),    // payload, now read as a plain Backspace
                (VK_BACK, true),
                (VK_LCONTROL, false), // restore, same batch
                (VK_LSHIFT, false),
            ]
        );
    }

    #[test]
    fn modifier_wrap_with_nothing_held_is_the_payload_verbatim() {
        let payload = vec![key_input(VK_BACK, 0)];
        let batch = wrap_in_modifier_release(&[], payload);
        assert_eq!(shape(&batch), vec![(VK_BACK, false)]);
    }
}
