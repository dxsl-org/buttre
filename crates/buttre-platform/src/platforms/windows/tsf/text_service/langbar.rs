// SPDX-License-Identifier: GPL-3.0-only
//! buttre's input-mode button next to the taskbar's "ENG" indicator.
//!
//! Windows 11 shows only the language abbreviation for every keyboard of a
//! language; an IME's own state appears only through a language-bar item
//! with `GUID_LBI_INPUTMODE` — the same slot Microsoft's Chinese and Japanese
//! IMEs use. Ours shows the active method's icon while buttre is on and the
//! "E" icon while it is off; a left click toggles on/off, exactly like
//! `` Ctrl+Shift+` `` / `Ctrl+Shift+1`, and a right click opens a menu with
//! the way into buttre's settings.

use std::cell::RefCell;

use windows::core::{implement, Error, Interface, Ref, Result, BSTR, GUID};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, POINT, RECT};
use windows::Win32::System::Ole::{CONNECT_E_ADVISELIMIT, CONNECT_E_CANNOTCONNECT};
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::HICON;

use super::mode_icon;
use super::mode_menu::{self, MenuChoice};
use crate::platforms::windows::tsf::settings_launcher;
use crate::platforms::windows::tsf::CLSID_BUTTRE_TEXT_SERVICE;

/// The only sink cookie: TSF advises one `ITfLangBarItemSink` per item.
const SINK_COOKIE: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ButtonState {
    method_id: String,
    on: bool,
}

/// The language-bar button. `toggle` runs on a left click.
#[implement(ITfLangBarItem, ITfLangBarItemButton, ITfSource)]
pub struct ModeButton {
    state: RefCell<ButtonState>,
    sink: RefCell<Option<ITfLangBarItemSink>>,
    toggle: Box<dyn Fn()>,
}

impl ModeButton {
    pub fn new(method_id: &str, on: bool, toggle: Box<dyn Fn()>) -> Self {
        Self {
            state: RefCell::new(ButtonState {
                method_id: method_id.to_string(),
                on,
            }),
            sink: RefCell::new(None),
            toggle,
        }
    }

    /// Show `method_id` / `on`; asks Windows to redraw only when it changed.
    pub fn set_state(&self, method_id: &str, on: bool) {
        let next = ButtonState {
            method_id: method_id.to_string(),
            on,
        };
        if *self.state.borrow() == next {
            return;
        }
        *self.state.borrow_mut() = next;
        // Clone out before the call: OnUpdate re-enters GetIcon/GetText.
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            // SAFETY: plain COM call on the sink TSF advised.
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_TEXT | TF_LBI_TOOLTIP) };
        }
    }

    /// "buttre: Telex" / "buttre: tắt" — tooltip and accessible text.
    fn label(&self) -> String {
        let state = self.state.borrow();
        if !state.on {
            return "buttre: tắt (Ctrl+Shift+1 để bật)".to_string();
        }
        let name = match state.method_id.as_str() {
            "telex" => "Telex",
            "vni" => "VNI",
            "nom" => "Chữ Nôm",
            other => other,
        };
        format!("buttre: {name}")
    }
}

impl ITfLangBarItem_Impl for ModeButton_Impl {
    // Signature fixed by the windows-rs-generated trait (COM vtable
    // contract) — cannot be `unsafe fn`; the write is scoped below.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if pinfo.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let mut description = [0u16; 32];
        for (slot, unit) in description.iter_mut().zip("buttre".encode_utf16()) {
            *slot = unit;
        }
        // SAFETY: TSF passes a writable TF_LANGBARITEMINFO; null checked above.
        unsafe {
            *pinfo = TF_LANGBARITEMINFO {
                clsidService: CLSID_BUTTRE_TEXT_SERVICE,
                guidItem: GUID_LBI_INPUTMODE,
                dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
                ulSort: 0,
                szDescription: description,
            };
        }
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    fn Show(&self, _fshow: windows::core::BOOL) -> Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::from(self.label()))
    }
}

impl ITfLangBarItemButton_Impl for ModeButton_Impl {
    fn OnClick(&self, click: TfLBIClick, pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        if click == TF_LBI_CLK_LEFT {
            (self.toggle)();
        } else if click == TF_LBI_CLK_RIGHT {
            let on = self.state.borrow().on;
            match mode_menu::show(*pt, on) {
                Some(MenuChoice::Settings) => settings_launcher::open_settings(),
                Some(MenuChoice::Toggle) => (self.toggle)(),
                None => {}
            }
        }
        Ok(())
    }

    fn InitMenu(&self, _pmenu: Ref<'_, ITfMenu>) -> Result<()> {
        Ok(())
    }

    fn OnMenuSelect(&self, _wid: u32) -> Result<()> {
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        let state = self.state.borrow().clone();
        mode_icon::icon_for(&state.method_id, state.on).ok_or_else(|| Error::from(E_FAIL))
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(self.label()))
    }
}

impl ITfSource_Impl for ModeButton_Impl {
    // Signature fixed by the windows-rs-generated trait (COM vtable
    // contract) — cannot be `unsafe fn`; the read is scoped below.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<'_, windows::core::IUnknown>) -> Result<u32> {
        // SAFETY: TSF passes a valid IID pointer for the duration of the call.
        if riid.is_null() || unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(CONNECT_E_CANNOTCONNECT.into());
        }
        if self.sink.borrow().is_some() {
            return Err(CONNECT_E_ADVISELIMIT.into());
        }
        let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
        *self.sink.borrow_mut() = Some(sink);
        Ok(SINK_COOKIE)
    }

    fn UnadviseSink(&self, dwcookie: u32) -> Result<()> {
        if dwcookie != SINK_COOKIE || self.sink.borrow_mut().take().is_none() {
            return Err(CONNECT_E_CANNOTCONNECT.into());
        }
        Ok(())
    }
}
