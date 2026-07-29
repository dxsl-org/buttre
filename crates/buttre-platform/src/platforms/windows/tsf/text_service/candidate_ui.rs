//! Nôm candidate panel for the Windows TSF text service.
//!
//! **Tests**: `crates/buttre-platform/tests/platform_windows_tsf_tests.rs`.
//!
//! A borderless top-most window that lists the candidates for the open
//! composition. It is a PURE RENDERER: it never receives a keystroke.
//!
//! That is deliberate, not a gap. The window is `WS_EX_NOACTIVATE`, so it never
//! takes focus and Windows never routes a key to it; every key still arrives at
//! `ITfKeyEventSink::OnKeyDown` in `text_service_stub.rs`, which owns selection
//! and navigation. Giving this window a `WM_KEYDOWN` handler would create a
//! second, unreachable copy of that logic — a previous version had exactly that
//! and it had never run.
//!
//! Selection state lives in `VietnameseEngine`'s `CandidateState`, shared with
//! every other backend (`shared::candidates`); this file only turns it into
//! pixels.

use std::cell::RefCell;
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::candidate_render::{self, PanelContent};
use crate::shared::candidates::CandidateState;

/// Candidates shown at once — the digits 1..=9 that select them.
pub const PAGE_SIZE: usize = 9;

const CLASS_NAME: PCWSTR = windows::core::w!("buttreNomCandidatePanel");

/// Everything `WM_PAINT` needs, in one allocation the window procedure can
/// reach through `GWLP_USERDATA`. The font belongs here rather than in
/// `CandidatePanel` because the procedure has no other way to get it — asking
/// the DC would return the system default and every Nôm glyph would be a box.
struct PanelShared {
    content: RefCell<PanelContent>,
    font: HFONT,
}

/// The candidate window plus the content it draws.
///
/// `shared` is boxed so its address is stable: the window procedure holds a raw
/// pointer to it, which must stay valid even if the `CandidatePanel` itself is
/// moved into another owner.
pub struct CandidatePanel {
    hwnd: HWND,
    shared: Box<PanelShared>,
}

impl CandidatePanel {
    /// Create the panel hidden. Fails only if the window cannot be created,
    /// in which case the caller runs without a panel rather than without an IME.
    pub fn new() -> Result<Self> {
        register_class();

        // SAFETY: the class was just registered; all parameters are constants
        // or null. WS_EX_NOACTIVATE keeps focus in the host application, which
        // is what lets the user keep typing while the panel is up.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                CLASS_NAME,
                windows::core::w!(""),
                WS_POPUP | WS_BORDER,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )?
        };

        let shared = Box::new(PanelShared {
            content: RefCell::new(PanelContent::default()),
            font: candidate_render::create_font(),
        });
        // SAFETY: hwnd is valid; the pointer stored here is owned by `shared`,
        // which outlives the window (Drop destroys the window first).
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                shared.as_ref() as *const PanelShared as isize,
            );
        }

        Ok(Self { hwnd, shared })
    }

    /// Show `state`'s current page at screen position `(x, y)`, resizing to fit.
    ///
    /// `y` should already be the BOTTOM of the caret so the panel sits under the
    /// text rather than over it.
    pub fn show(&self, state: &CandidateState, x: i32, y: i32) {
        if state.is_empty() {
            self.hide();
            return;
        }

        let page_start = state.page_start(PAGE_SIZE);
        let page_count = state.page_count(PAGE_SIZE);
        let content = PanelContent {
            lines: state
                .page_items(PAGE_SIZE)
                .iter()
                .enumerate()
                .map(|(i, item)| format!("{}. {}", i + 1, item.display))
                .collect(),
            highlight: state.cursor() - page_start,
            footer: (page_count > 1)
                .then(|| format!("Trang {} / {}", state.cursor() / PAGE_SIZE + 1, page_count)),
        };

        let size = candidate_render::measure(self.hwnd, self.shared.font, &content);
        *self.shared.content.borrow_mut() = content;

        // SAFETY: hwnd is this panel's own window, valid until Drop.
        // SWP_NOACTIVATE keeps the host application's focus intact.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                size.cx,
                size.cy,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.hwnd), None, true);
            let _ = UpdateWindow(self.hwnd);
        }
    }

    pub fn hide(&self) {
        // SAFETY: hwnd is valid until Drop.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

impl Drop for CandidatePanel {
    fn drop(&mut self) {
        // SAFETY: clear GWLP_USERDATA BEFORE destroying the window so any
        // message pumped synchronously during DestroyWindow (WM_PAINT,
        // WM_ERASEBKGND) sees null and skips the content pointer — which is
        // about to be freed with `self`.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
            // Harmless on the stock fallback font: DeleteObject refuses stock
            // objects rather than corrupting them.
            let _ = DeleteObject(self.shared.font.into());
        }
    }
}

fn register_class() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();

    REGISTER.call_once(|| {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            lpszClassName: CLASS_NAME,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and CLASS_NAME is a static
        // null-terminated wide string. A duplicate registration would only be
        // possible if `Once` ran twice.
        unsafe {
            RegisterClassW(&class);
        }
    });
}

/// # Safety
/// Called by Windows with valid message parameters. A panic crossing this FFI
/// boundary is undefined behaviour, so painting is wrapped in `catch_unwind` —
/// the host process must survive a bug in our renderer.
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg != WM_PAINT {
        // SAFETY: parameters come straight from the OS.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    let mut ps = PAINTSTRUCT::default();
    // SAFETY: hwnd is valid; EndPaint below balances this call on every path.
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: GWLP_USERDATA holds the pointer stored by
        // `CandidatePanel::new`, and Drop zeroes it before destroying the
        // window, so a non-zero value here is a live `PanelShared`.
        let shared_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
        if shared_ptr == 0 {
            return;
        }
        let shared = unsafe { &*(shared_ptr as *const PanelShared) };
        candidate_render::paint(hwnd, hdc, shared.font, &shared.content.borrow());
    }));

    // SAFETY: balances BeginPaint above.
    let _ = unsafe { EndPaint(hwnd, &ps) };
    LRESULT(0)
}
