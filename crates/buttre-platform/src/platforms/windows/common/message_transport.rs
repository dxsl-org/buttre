//! Posted-message text transport for editors that mis-read injected keyboard
//! input.
//!
//! ## The bug (measured 2026-11, Notepad 11.2609, Formatted and plain text)
//!
//! Windows 11 Notepad's editor (`RichEditD2DPT`, hosted in a XAML island)
//! loses `SendInput` text once its input queue backs up: every queued
//! `KEYEVENTF_UNICODE` (`VK_PACKET`) character arrives as the LAST character
//! of the batch (`tr 1a (trong)` → `))))))))`) and most `VK_BACK`s are
//! dropped. Short Telex edits usually survive; long ones — the inline Nôm
//! candidate list, word-boundary repairs while typing fast — never do.
//! Splitting the batch or pacing it at ≤5 ms per key does not help; 10 ms
//! does, at ~0.7 s per Nôm candidate refresh. EVKey and UniKey hook modes
//! have the same open bug.
//!
//! ## The fix
//!
//! Post `WM_KEYDOWN`/`WM_KEYUP(VK_BACK)` and `WM_CHAR` straight to the
//! focused editor window. The character travels in `wParam`, nothing depends
//! on asynchronous keyboard state — measured correct 8/8 runs, surrogate-pair
//! Nôm included, in ~1 ms.
//!
//! ## Ordering invariant
//!
//! Posted messages are dequeued BEFORE pending keyboard input, so a
//! passthrough keystroke still queued when a replacement is posted gets
//! overtaken (`va` + `â` → `âa`, measured). While this transport is active the
//! hook therefore must not pass text-editing keys through: it posts them on
//! this same channel, so every edit buttre makes or sees reaches the editor
//! in keystroke order. Keys the hook never handles (Enter, arrows, Ctrl
//! shortcuts) still use the input queue; a character typed within the
//! editor's processing lag after one of those can still overtake it.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_BACK;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, PostMessageW,
    GUITHREADINFO, WM_CHAR, WM_KEYDOWN, WM_KEYUP,
};

/// Window classes of focused controls that need this transport. Matched on
/// the control, not the executable, so it follows the editor wherever it is
/// hosted and leaves the rest of Notepad (find box, dialogs) on `SendInput`,
/// which works there.
const MESSAGE_TRANSPORT_CLASSES: &[&str] = &["RichEditD2DPT"];

/// `lParam` bits of a Backspace key message: repeat count 1, scan code 0x0E.
const BACK_KEYDOWN_LPARAM: u32 = 0x000E_0001;
/// Same, plus previous-state and transition bits, as on a real key release.
const BACK_KEYUP_LPARAM: u32 = 0xC00E_0001;
/// `lParam` of a `WM_CHAR`: repeat count 1.
const CHAR_LPARAM: u32 = 1;

/// The focused control of the foreground window when it needs posted-message
/// delivery, `None` otherwise (including on any query failure — the caller
/// then keeps the `SendInput` path, i.e. the behavior before this existed).
///
/// Queried fresh on every call, never cached: focus moves between the editor
/// and other controls of the same window, and a stale answer either way is
/// exactly the garbling or the overtaking this module exists to prevent.
pub fn target() -> Option<HWND> {
    // SAFETY: GetForegroundWindow takes no arguments and may return 0, which
    // is checked. GetWindowThreadProcessId accepts any window handle and a
    // null pid pointer is documented as allowed.
    let thread_id = unsafe {
        let foreground = GetForegroundWindow();
        if foreground == 0 {
            return None;
        }
        GetWindowThreadProcessId(foreground, std::ptr::null_mut())
    };
    if thread_id == 0 {
        return None;
    }

    // SAFETY: GUITHREADINFO is plain old data for which all-zero is valid;
    // cbSize is set before the call as the API requires, and the pointer is
    // to a live local.
    let focus = unsafe {
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(thread_id, &mut info) == 0 {
            return None;
        }
        info.hwndFocus
    };
    if focus == 0 {
        return None;
    }

    let mut class = [0u16; 64];
    // SAFETY: the buffer pointer and its length in u16 units describe a live
    // local array; GetClassNameW writes at most that many units.
    let len = unsafe { GetClassNameW(focus, class.as_mut_ptr(), class.len() as i32) };
    let class = class.get(..usize::try_from(len).ok()?)?;
    is_message_transport_class(class).then_some(focus)
}

/// Pure class-name check (unit-tested).
fn is_message_transport_class(class: &[u16]) -> bool {
    MESSAGE_TRANSPORT_CLASSES
        .iter()
        .any(|name| name.encode_utf16().eq(class.iter().copied()))
}

/// Delete `backspace_count` characters, then insert `text`, in order.
///
/// Backspace is posted as a key message, not `WM_CHAR(0x08)`: the editor
/// mangles the latter (measured). The key message is read against the
/// target thread's modifier state, so callers must not use this while the
/// user holds Ctrl/Alt — it would become delete-word.
pub fn post_replacement(hwnd: HWND, backspace_count: usize, text: &str) {
    let messages = std::iter::repeat_n(
        [
            (WM_KEYDOWN, VK_BACK as usize, BACK_KEYDOWN_LPARAM),
            (WM_KEYUP, VK_BACK as usize, BACK_KEYUP_LPARAM),
        ],
        backspace_count,
    )
    .flatten()
    .chain(
        text.encode_utf16()
            .map(|unit| (WM_CHAR, unit as usize, CHAR_LPARAM)),
    );

    for (message, wparam, lparam) in messages {
        // SAFETY: PostMessageW takes plain values; an invalid or destroyed
        // window makes it fail and return 0, which is handled below.
        if unsafe { PostMessageW(hwnd, message, wparam, lparam as isize) } == 0 {
            tracing::error!("PostMessageW to the editor failed; dropping the rest of the edit");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn notepad_editor_class_uses_message_transport() {
        assert!(is_message_transport_class(&utf16("RichEditD2DPT")));
    }

    /// Only the exact editor class: Notepad's other controls and classic
    /// RichEdit/Edit controls handle `SendInput` fine and must keep it.
    #[test]
    fn other_classes_keep_send_input() {
        for class in ["RichEditD2DPTX", "RichEditD2D", "RICHEDIT50W", "Edit", ""] {
            assert!(!is_message_transport_class(&utf16(class)), "{class}");
        }
    }
}
