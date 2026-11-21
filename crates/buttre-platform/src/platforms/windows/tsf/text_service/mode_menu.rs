// SPDX-License-Identifier: GPL-3.0-only
//! Right-click menu of the input-mode button (`langbar.rs`): the way into
//! buttre's settings now that the TSF build has no tray.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DestroyMenu, DestroyWindow, PostMessageW,
    SetForegroundWindow, TrackPopupMenuEx, MF_SEPARATOR, MF_STRING, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_NULL, WS_EX_TOOLWINDOW, WS_POPUP,
};

/// What the user picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuChoice {
    Settings,
    Toggle,
}

const ID_SETTINGS: u32 = 1;
const ID_TOGGLE: u32 = 2;

impl MenuChoice {
    fn from_id(id: u32) -> Option<Self> {
        match id {
            ID_SETTINGS => Some(Self::Settings),
            ID_TOGGLE => Some(Self::Toggle),
            _ => None,
        }
    }
}

/// Show the menu at screen point `at` and wait for the choice (`None` when
/// dismissed). `on` picks the wording of the on/off item.
pub fn show(at: POINT, on: bool) -> Option<MenuChoice> {
    let toggle_label = if on {
        w!("Tắt buttre\tCtrl+Shift+`")
    } else {
        w!("Bật buttre\tCtrl+Shift+1")
    };
    // SAFETY: every handle created here is destroyed before returning; the
    // label pointers are static wide strings. The owner is a throwaway popup
    // of THIS thread: a menu needs an owner on the calling thread, and the
    // window under the click (the taskbar) belongs to Explorer.
    unsafe {
        let owner = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("STATIC"),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )
        .ok()?;
        let menu = match CreatePopupMenu() {
            Ok(menu) => menu,
            Err(_) => {
                let _ = DestroyWindow(owner);
                return None;
            }
        };
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_SETTINGS as usize,
            w!("Cấu hình buttre…"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_TOGGLE as usize, toggle_label);

        // Foreground first, WM_NULL after: the documented recipe so the menu
        // closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(owner);
        let picked = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON).0,
            at.x,
            at.y,
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        let _ = DestroyWindow(owner);
        MenuChoice::from_id(picked.0 as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_choices() {
        assert_eq!(MenuChoice::from_id(ID_SETTINGS), Some(MenuChoice::Settings));
        assert_eq!(MenuChoice::from_id(ID_TOGGLE), Some(MenuChoice::Toggle));
        assert_eq!(MenuChoice::from_id(0), None, "dismissed");
    }
}
