//! Font selection and painting for the Nôm candidate panel.
//!
//! Split from `candidate_ui.rs` so the window's lifetime rules and its pixels
//! stay separately readable. Everything here runs on the UI thread of the host
//! application, inside `WM_PAINT`.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

/// What the panel draws. Owned by the panel, read by the window procedure.
#[derive(Default)]
pub struct PanelContent {
    /// One rendered line per candidate on the current page, already numbered.
    pub lines: Vec<String>,
    /// Index INTO `lines` of the highlighted candidate.
    pub highlight: usize,
    /// `"Trang 2 / 3"`, or `None` for a single-page list.
    pub footer: Option<String>,
}

pub const PADDING: i32 = 8;

/// Faces tried in order for the candidate text.
///
/// Nôm lives in CJK Extension B and beyond, which the default GUI font does
/// NOT cover — without a font that does, every candidate renders as a box and
/// the panel is useless. The first two are the fonts Nôm users typically
/// install; the rest ship with Windows' East Asian support.
const FACE_PREFERENCES: [&str; 5] = [
    "Nom Na Tong",
    "HAN NOM A",
    "SimSun-ExtB",
    "MingLiU-ExtB",
    "Microsoft JhengHei",
];

/// Create the panel font, preferring a face that can actually draw Nôm.
///
/// Falls back to the default GUI font when none of the preferred faces are
/// installed — the panel then shows boxes, which is still better than no panel
/// at all, and the numbers and Vietnamese glosses remain readable.
pub fn create_font() -> HFONT {
    // SAFETY: a null HWND gives the screen DC, which is valid for measuring and
    // for the face-name probe below; it is released before returning.
    unsafe {
        let hdc = GetDC(None);
        // 12pt in device units. A fixed pixel height would render the panel
        // unreadably small on a high-DPI display, where the surrounding text is
        // scaled up and ours would not be.
        let height = -(12 * GetDeviceCaps(Some(hdc), LOGPIXELSY) / 72);
        let font = FACE_PREFERENCES
            .iter()
            .find_map(|face| try_face(hdc, face, height))
            .unwrap_or_else(|| HFONT(GetStockObject(DEFAULT_GUI_FONT).0));
        ReleaseDC(None, hdc);
        font
    }
}

/// Build a font for `face` and keep it only if GDI did NOT substitute another
/// face. `CreateFontW` never fails on an unknown name — it silently picks the
/// closest match — so asking the DC what it actually selected is the only way
/// to tell a real hit from a substitution.
fn try_face(hdc: HDC, face: &str, height: i32) -> Option<HFONT> {
    let wide: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a null-terminated UTF-16 buffer alive for the call;
    // `hdc` is the screen DC from the caller. The font is deleted again when
    // the probe rejects it, so no handle leaks.
    unsafe {
        let font = CreateFontW(
            height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(wide.as_ptr()),
        );
        if font.is_invalid() {
            return None;
        }
        let old = SelectObject(hdc, font.into());
        let mut actual = [0u16; 64];
        let len = GetTextFaceW(hdc, Some(&mut actual)) as usize;
        SelectObject(hdc, old);

        let matched =
            len > 1 && String::from_utf16_lossy(&actual[..len - 1]).eq_ignore_ascii_case(face);
        if matched {
            Some(font)
        } else {
            let _ = DeleteObject(font.into());
            None
        }
    }
}

/// Pixel size the panel needs for `content` when drawn with `font`.
pub fn measure(hwnd: HWND, font: HFONT, content: &PanelContent) -> SIZE {
    // SAFETY: hwnd is the panel's own window; the DC is released before return
    // and the original font is restored before that.
    unsafe {
        let hdc = GetDC(Some(hwnd));
        let old = SelectObject(hdc, font.into());

        let mut metrics = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut metrics);
        let line_height = metrics.tmHeight + 6;

        let mut width = 0;
        for line in content.lines.iter().chain(content.footer.iter()) {
            let wide: Vec<u16> = line.encode_utf16().collect();
            let mut size = SIZE::default();
            if GetTextExtentPoint32W(hdc, &wide, &mut size).as_bool() {
                width = width.max(size.cx);
            }
        }

        SelectObject(hdc, old);
        ReleaseDC(Some(hwnd), hdc);

        let rows = content.lines.len() as i32 + i32::from(content.footer.is_some());
        SIZE {
            cx: (width + PADDING * 2).clamp(120, 900),
            cy: rows * line_height + PADDING * 2,
        }
    }
}

/// Paint the whole panel. Colors come from the system palette so the panel
/// follows the user's theme instead of hard-coding a light one.
pub fn paint(hwnd: HWND, hdc: HDC, font: HFONT, content: &PanelContent) {
    // SAFETY: hdc is the paint DC from BeginPaint, valid for the whole call;
    // the previous font is restored before returning.
    unsafe {
        let old = SelectObject(hdc, font.into());

        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        FillRect(
            hdc,
            &client,
            HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
        );

        let mut metrics = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut metrics);
        let line_height = metrics.tmHeight + 6;

        let mut y = PADDING;
        for (i, line) in content.lines.iter().enumerate() {
            let selected = i == content.highlight;
            if selected {
                let row = RECT {
                    left: 0,
                    top: y - 2,
                    right: client.right,
                    bottom: y + line_height - 2,
                };
                FillRect(
                    hdc,
                    &row,
                    HBRUSH((COLOR_HIGHLIGHT.0 + 1) as isize as *mut _),
                );
                SetBkColor(hdc, COLORREF(GetSysColor(COLOR_HIGHLIGHT)));
                SetTextColor(hdc, COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)));
            } else {
                SetBkColor(hdc, COLORREF(GetSysColor(COLOR_WINDOW)));
                SetTextColor(hdc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
            }
            let wide: Vec<u16> = line.encode_utf16().collect();
            let _ = TextOutW(hdc, PADDING, y, &wide);
            y += line_height;
        }

        if let Some(footer) = &content.footer {
            SetBkColor(hdc, COLORREF(GetSysColor(COLOR_WINDOW)));
            SetTextColor(hdc, COLORREF(GetSysColor(COLOR_GRAYTEXT)));
            let wide: Vec<u16> = footer.encode_utf16().collect();
            let _ = TextOutW(hdc, PADDING, y, &wide);
        }

        SelectObject(hdc, old);
    }
}
