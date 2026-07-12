//! Simple candidate window for Hook mode
//!
//! Displays a list of numbered candidates (1-5) using a simple Win32 window.

use buttre_engine::pipeline::Candidate;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static WINDOW_HANDLE: AtomicIsize = AtomicIsize::new(0);
static CANDIDATES: OnceLock<std::sync::Mutex<Vec<Candidate>>> = OnceLock::new();
static INPUT_TEXT: OnceLock<std::sync::Mutex<String>> = OnceLock::new();
static CANDIDATES_TEXT: OnceLock<std::sync::Mutex<String>> = OnceLock::new(); // Lưu text đã hiển thị
static IS_SHOWING: AtomicBool = AtomicBool::new(false);

const WINDOW_CLASS: &str = "buttreCandidateWindow";

/// Initialize candidate window (call once at startup)
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        // Register window class
        let class_name = WINDOW_CLASS
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: HINSTANCE::default(),
            hIcon: HICON::default(),
            hCursor: LoadCursorW(None, IDC_ARROW).ok().unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as _),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hIconSm: HICON::default(),
        };

        if RegisterClassExW(&wc) == 0 {
            let err = std::io::Error::last_os_error();
            tracing::warn!("Failed to register candidate window class: {}", err);
        }

        // Create hidden window
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR(class_name.as_ptr()),
            windows::core::w!("buttre Candidates"),
            WS_POPUP | WS_BORDER,
            0,
            0,
            300,
            200,
            None,
            None,
            None,
            None,
        )
        .unwrap_or_default();

        if !hwnd.is_invalid() {
            WINDOW_HANDLE.store(hwnd.0 as isize, Ordering::Release);
        }
    }

    CANDIDATES.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    INPUT_TEXT.get_or_init(|| std::sync::Mutex::new(String::new()));
    CANDIDATES_TEXT.get_or_init(|| std::sync::Mutex::new(String::new()));
    Ok(())
}

/// Window procedure for candidate window
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: BeginPaint is safe to call with valid HWND
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };

            // Draw candidates
            if let Some(cand_lock) = CANDIDATES.get() {
                if let Ok(cands) = cand_lock.lock() {
                    let mut y = 10;
                    for (i, candidate) in cands.iter().take(5).enumerate() {
                        let text = format!("{}. {}", i + 1, candidate.text);
                        let mut text_wide: Vec<u16> =
                            text.encode_utf16().chain(std::iter::once(0)).collect();

                        let mut rect = RECT {
                            left: 10,
                            top: y,
                            right: 290,
                            bottom: y + 30,
                        };

                        // SAFETY: DrawTextW is safe to call with valid HDC
                        unsafe {
                            DrawTextW(
                                hdc,
                                &mut text_wide,
                                &mut rect,
                                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                            );
                        }

                        y += 35;
                    }
                }
            }

            // SAFETY: EndPaint is safe to call with valid HWND
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        // SAFETY: DefWindowProcW is safe to call with valid parameters
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Show candidates inline (returns formatted text to display)
/// Format: "1天 2𡗶 (trời) 3霄 4𡗶 5天"
pub fn show_candidates(candidates: Vec<Candidate>, input: String) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    // Store candidates (full Candidate objects with display text and value)
    if let Some(cand_lock) = CANDIDATES.get() {
        if let Ok(mut cands) = cand_lock.lock() {
            *cands = candidates.clone();
        }
    }

    // Store input text (để biết cần xóa bao nhiêu ký tự)
    if let Some(input_lock) = INPUT_TEXT.get() {
        if let Ok(mut input_text) = input_lock.lock() {
            *input_text = input.clone();
        }
    }

    // Format candidates: " 1𡗶 (trời) 2天 3霄"
    // Display the full text (includes Vietnamese meaning in parentheses)
    // Note: main_action already committed/replaced the input text,
    // so we only need to append candidates
    // Add leading space for better readability: "tr 1𡨸 2𡧲 3𡷿"
    let mut formatted = String::from(" "); // Leading space
    for (i, candidate) in candidates.iter().take(5).enumerate() {
        if i > 0 {
            formatted.push(' ');
        }
        formatted.push_str(&format!("{}{}", i + 1, candidate.text));
    }

    // Store formatted text for later deletion
    if let Some(text_lock) = CANDIDATES_TEXT.get() {
        if let Ok(mut text) = text_lock.lock() {
            *text = formatted.clone();
        }
    }

    IS_SHOWING.store(true, Ordering::Release);

    tracing::info!(
        "Showing {} candidates inline: {}",
        candidates.len(),
        formatted
    );
    Some(formatted)
}

/// Hide candidates (clear inline state)
pub fn hide_candidates() {
    IS_SHOWING.store(false, Ordering::Release);

    if let Some(cand_lock) = CANDIDATES.get() {
        if let Ok(mut cands) = cand_lock.lock() {
            cands.clear();
        }
    }

    if let Some(input_lock) = INPUT_TEXT.get() {
        if let Ok(mut input_text) = input_lock.lock() {
            input_text.clear();
        }
    }

    if let Some(text_lock) = CANDIDATES_TEXT.get() {
        if let Ok(mut text) = text_lock.lock() {
            text.clear();
        }
    }

    tracing::debug!("Candidates hidden");
}

/// Check if candidates are showing
pub fn is_showing() -> bool {
    IS_SHOWING.load(Ordering::Acquire)
}

/// Get length of currently displayed candidates text (for deletion)
pub fn get_candidates_text_len() -> usize {
    if let Some(text_lock) = CANDIDATES_TEXT.get() {
        if let Ok(text) = text_lock.lock() {
            text.chars().count()
        } else {
            0
        }
    } else {
        0
    }
}

/// Get input text length (for backspace calculation)
pub fn get_input_text_len() -> usize {
    if let Some(input_lock) = INPUT_TEXT.get() {
        if let Ok(input) = input_lock.lock() {
            input.chars().count()
        } else {
            0
        }
    } else {
        0
    }
}

/// Get the number of currently displayed candidates
pub fn get_candidates_count() -> usize {
    if let Some(cand_lock) = CANDIDATES.get() {
        if let Ok(cands) = cand_lock.lock() {
            cands.len()
        } else {
            0
        }
    } else {
        0
    }
}

/// Get candidate by index (0-based)
/// Returns the Candidate object if found
pub fn get_candidate(index: usize) -> Option<Candidate> {
    if let Some(cand_lock) = CANDIDATES.get() {
        if let Ok(cands) = cand_lock.lock() {
            cands.get(index).cloned()
        } else {
            None
        }
    } else {
        None
    }
}

/// Select candidate by number (1-5)
/// Returns (selected_value, backspace_count) if valid
/// selected_value is the actual Nôm character (without Vietnamese meaning in parentheses)
/// backspace_count = input_text.len() + candidates_text.len()
pub fn select_candidate(number: u8) -> Option<(String, usize)> {
    if !(1..=5).contains(&number) {
        return None;
    }

    let index = (number - 1) as usize;
    let candidate = get_candidate(index)?;

    // Extract the actual value to insert (just the Nôm character, not the display text)
    let selected_value = candidate.get_value().to_string();

    // Calculate total backspace count:
    // 1. Input text (e.g., "troi")
    // 2. Candidates text (e.g., " 1𡗶 (trời) 2天 3霄")
    let input_len = if let Some(input_lock) = INPUT_TEXT.get() {
        if let Ok(input_text) = input_lock.lock() {
            input_text.chars().count()
        } else {
            0
        }
    } else {
        0
    };

    let candidates_len = if let Some(text_lock) = CANDIDATES_TEXT.get() {
        if let Ok(text) = text_lock.lock() {
            text.chars().count()
        } else {
            0
        }
    } else {
        0
    };

    let backspace_count = input_len + candidates_len;

    hide_candidates();
    Some((selected_value, backspace_count))
}
