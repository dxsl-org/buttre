// SPDX-License-Identifier: GPL-3.0-only
// TextService implementation for Windows TSF
// Using windows-rs 0.62 API

use windows::core::*;
use windows::Win32::UI::TextServices::*;

use super::composition::{Composition, PendingComposition};
use super::display_attribute::{
    DisplayAttributeEnum, DisplayAttributeInfo, GUID_DISPLAY_ATTRIBUTE_CONVERTED,
    GUID_DISPLAY_ATTRIBUTE_INPUT,
};
use super::edit_session::{EndComposition, SetCompositionString};
use super::vietnamese_engine::VietnameseEngine;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use tracing::debug;

use crate::platforms::windows::tsf::com::{dll_add_ref, dll_release};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{CLSID_TF_CategoryMgr, ITfCategoryMgr};

/// TextService implementation
///
/// Full implementation is being built incrementally.
#[implement(
    ITfTextInputProcessor,
    ITfTextInputProcessorEx,
    ITfCompositionSink,
    ITfDisplayAttributeProvider,
    ITfKeyEventSink,
    ITfThreadMgrEventSink,
    ITfThreadFocusSink,
    ITfCompartmentEventSink,
    ITfActiveLanguageProfileNotifySink
)]
pub struct TextService {
    composition: Composition,
    pending_edit: RefCell<Weak<RefCell<PendingComposition>>>,
    last_text_len: Cell<usize>,
    thread_mgr: RefCell<Option<ITfThreadMgr>>,
    client_id: Cell<u32>,
    da_atom_input: Cell<u32>,
    da_atom_converted: Cell<u32>,
    keystroke_tid: Cell<u32>,
    thread_cookies: RefCell<Vec<u32>>,
    keyboard_openclose_cookie: Cell<u32>,
    pub(crate) key_busy: Cell<bool>,
    vietnamese_engine: Rc<RefCell<VietnameseEngine>>,
    candidate_ui: RefCell<Option<Rc<super::candidate_ui::NomCandidateUI>>>,
}
// ...
// ... existing impls ...

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        debug!("EnumDisplayAttributeInfo");
        Ok(DisplayAttributeEnum::new().into())
    }

    // Signature is fixed by the windows-rs-generated
    // `ITfDisplayAttributeProvider_Impl` trait (COM vtable contract) —
    // cannot be `unsafe fn`. The raw pointer read is scoped to an inner
    // `unsafe` block below.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        debug!("GetDisplayAttributeInfo");
        // SAFETY:
        // 1. guid pointer is provided by TSF framework - valid during call
        // 2. We check for null before dereferencing
        // 3. GUID is a POD type - safe to dereference and compare
        // 4. Pointer is only read, not modified
        unsafe {
            if guid.is_null() {
                return Err(E_INVALIDARG.into());
            }

            if *guid == GUID_DISPLAY_ATTRIBUTE_INPUT {
                Ok(DisplayAttributeInfo::create_input().into())
            } else if *guid == GUID_DISPLAY_ATTRIBUTE_CONVERTED {
                Ok(DisplayAttributeInfo::create_converted().into())
            } else {
                Err(E_INVALIDARG.into())
            }
        }
    }
}

impl Default for TextService {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        // Each live TextService holds one DLL refcount increment.
        // Release it so DllCanUnloadNow returns S_OK once the last instance is gone.
        dll_release();
    }
}

impl TextService {
    pub fn new() -> Self {
        use super::vietnamese_engine::VietnameseMode;

        // Increment DLL refcount: Windows should not unload the DLL while a
        // TextService instance exists. Balanced by Drop above.
        dll_add_ref();

        Self {
            composition: Composition::new(),
            pending_edit: RefCell::new(Weak::new()),
            last_text_len: Cell::new(0),
            thread_mgr: RefCell::new(None),
            client_id: Cell::new(0),
            da_atom_input: Cell::new(0),
            da_atom_converted: Cell::new(0),
            keystroke_tid: Cell::new(0),
            thread_cookies: RefCell::new(Vec::new()),
            keyboard_openclose_cookie: Cell::new(TF_INVALID_COOKIE),
            key_busy: Cell::new(false),
            vietnamese_engine: Rc::new(RefCell::new(VietnameseEngine::new(VietnameseMode::Telex))),
            candidate_ui: RefCell::new(None),
        }
    }

    pub fn write_text(
        &self,
        context: &ITfContext,
        text: &str,
        cursor: usize,
        sink: ITfCompositionSink,
    ) -> Result<()> {
        debug!("TextService::write_text: {}", text);

        // Reuse in-flight session if TSF hasn't executed it yet.
        // Only update text/cursor — previous_length was captured at session-create
        // time and represents chars committed BEFORE this composition started;
        // clobbering it here would make the recovery path in DoEditSession
        // mis-count the chars to replace (Chrome-omnibox recovery, see edit_session.rs).
        if let Some(rc) = self.pending_edit.borrow().upgrade() {
            let mut p = rc.borrow_mut();
            p.text = text.into();
            p.cursor = cursor;
            self.last_text_len.set(text.chars().count());
            return Ok(());
        }

        let previous_length = self.last_text_len.get();
        let pending = Rc::new(RefCell::new(PendingComposition {
            text: text.into(),
            cursor,
            previous_length,
        }));
        *self.pending_edit.borrow_mut() = Rc::downgrade(&pending);
        // Track char count (UTF-16 BMP units) so DoEditSession's ShiftStart is correct.
        self.last_text_len.set(text.chars().count());

        let da = VARIANT::from(self.da_atom_input.get() as i32);
        let session =
            SetCompositionString::new(context.clone(), self.composition.clone(), sink, da, pending);
        let session_interface: ITfEditSession = session.into();
        unsafe {
            context.RequestEditSession(
                self.client_id.get(),
                &session_interface,
                TF_ES_ASYNCDONTCARE | TF_ES_READWRITE,
            )?;
        }
        Ok(())
    }

    /// Helper to end composition via EndComposition edit session
    #[allow(unused_must_use)]
    pub fn end_composition(&self, context: &ITfContext) -> Result<()> {
        debug!("TextService::end_composition");

        // Invalidate the write-coalescing slot: `write_text` reuses
        // `pending_edit` while its `SetCompositionString` session is still
        // queued (TF_ES_ASYNCDONTCARE may defer it past this call). Once a
        // composition is ending, any LATER `write_text` call in this same
        // keystroke (e.g. a `Commit` separator immediately after a
        // `ConfirmComposition`, see issue #4) targets a NEW composition, not
        // the one being closed — without this, that later call would silently
        // overwrite the still-pending final text instead of queuing its own
        // session, losing the confirmed word entirely.
        *self.pending_edit.borrow_mut() = Weak::new();

        if let Some(composition) = self.composition.get() {
            let session = EndComposition::new(context.clone(), composition);
            let session_interface: ITfEditSession = session.into();

            unsafe {
                context.RequestEditSession(
                    self.client_id.get(),
                    &session_interface,
                    TF_ES_ASYNCDONTCARE | TF_ES_READWRITE,
                )?;
            }
        }

        Ok(())
    }

    /// Get cursor position from TSF context
    /// Returns (x, y) in screen coordinates, or None if unavailable
    fn get_cursor_position(&self, context: &ITfContext) -> Option<(i32, i32)> {
        // SAFETY:
        // 1. All TSF COM methods are properly declared in windows crate
        // 2. composition_obj is a valid ITfComposition interface if Some
        // 3. GetRange returns a valid ITfRange for the composition
        // 4. context.cast() to ITfContextView is safe - same COM object
        // 5. RECT is a POD struct - default() creates valid zero-initialized instance
        // 6. GetTextExt writes to rect and clipped - we provide valid mutable references
        // 7. All COM interface calls check for errors with ok()?
        unsafe {
            // Try to get position from composition if available
            if let Some(composition_obj) = self.composition.get() {
                // Get the range from composition
                let range = composition_obj.GetRange().ok()?;

                // Get context view
                let view: ITfContextView = context.cast().ok()?;

                // We need an edit cookie - use TF_DEFAULT_EDIT_COOKIE as fallback
                // In a real implementation, this should be called from within an edit session
                let ec = 0; // Placeholder - will be replaced with proper edit cookie

                let mut rect = windows::Win32::Foundation::RECT::default();
                let mut clipped = BOOL(0);

                // Try to get text extent
                if view.GetTextExt(ec, &range, &mut rect, &mut clipped).is_ok() {
                    let x = rect.left;
                    let y = rect.bottom + 2;
                    debug!("Cursor position from composition: ({}, {})", x, y);
                    return Some((x, y));
                }
            }

            debug!("Could not get cursor position, using default");
            None
        }
    }

    /// Show candidate UI with Nôm character candidates
    pub fn show_candidates(
        &self,
        context: &ITfContext,
        candidates: Vec<super::candidate_ui::CandidateItem>,
        sink: ITfCompositionSink,
    ) -> Result<()> {
        use super::candidate_ui::NomCandidateUI;

        debug!(
            "TextService::show_candidates: {} candidates",
            candidates.len()
        );

        if candidates.is_empty() {
            return Ok(());
        }

        let ui = Rc::new(NomCandidateUI::new(candidates));

        let context_clone = context.clone();
        let composition = self.composition.clone();
        let client_id = self.client_id.get();
        let da_atom = self.da_atom_input.get();

        ui.set_on_select(move |character, _reading| {
            // Guard: if composition was cleared by Deactivate while the window was open, bail.
            if !composition.is_started() {
                return;
            }
            debug!("Candidate selected: {}", character);

            let text = character.to_string();
            let cursor = text.chars().count();
            let pending = Rc::new(RefCell::new(PendingComposition {
                text: text.into(),
                cursor,
                previous_length: 0,
            }));

            let da = VARIANT::from(da_atom as i32);
            let session = SetCompositionString::new(
                context_clone.clone(),
                composition.clone(),
                sink.clone(),
                da,
                pending,
            );

            let session_interface: ITfEditSession = session.into();
            // SAFETY:
            // 1. context_clone is a valid ITfContext interface
            // 2. RequestEditSession is a COM method - safe to call
            // 3. client_id is our TSF client ID from Activate
            // 4. session_interface is a valid ITfEditSession implementation
            // 5. TF_ES_ASYNCDONTCARE | TF_ES_READWRITE are valid flags
            unsafe {
                let _ = context_clone.RequestEditSession(
                    client_id,
                    &session_interface,
                    TF_ES_ASYNCDONTCARE | TF_ES_READWRITE,
                );
            }
        });

        // Get cursor position or use default
        let (x, y) = self.get_cursor_position(context).unwrap_or_else(|| {
            debug!("Using default cursor position");
            (100, 100)
        });

        // Show window at cursor position
        if let Err(e) = ui.create_window(x, y) {
            debug!("Failed to create candidate window: {:?}", e);
            return Err(e);
        }

        // Store UI reference
        *self.candidate_ui.borrow_mut() = Some(ui);

        Ok(())
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32) -> Result<()> {
        debug!("TextService::Activate");

        let tm: ITfThreadMgr = ptim.ok()?.clone();
        *self.this.thread_mgr.borrow_mut() = Some(tm);
        self.this.client_id.set(tid);

        // Register Display Attributes
        // SAFETY:
        // 1. CoCreateInstance is properly declared in windows crate
        // 2. CLSID_TF_CategoryMgr is a valid Windows CLSID constant
        // 3. CLSCTX_INPROC_SERVER is a valid COM context flag
        // 4. RegisterGUID is a COM method - safe to call on valid interface
        // 5. GUID_DISPLAY_ATTRIBUTE_* are valid GUID constants we defined
        unsafe {
            let cat_mgr: ITfCategoryMgr =
                CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;

            let atom_input = cat_mgr.RegisterGUID(&GUID_DISPLAY_ATTRIBUTE_INPUT)?;
            self.this.da_atom_input.set(atom_input);

            let atom_converted = cat_mgr.RegisterGUID(&GUID_DISPLAY_ATTRIBUTE_CONVERTED)?;
            self.this.da_atom_converted.set(atom_converted);
        }

        // Register KeyEventSink
        // SAFETY:
        // 1. ptim is a valid Ref to ITfThreadMgr provided by TSF
        // 2. ok() safely extracts the interface reference
        // 3. cast() to ITfKeystrokeMgr is safe - same COM object, different interface
        // 4. AdviseKeyEventSink is a COM method - safe to call on valid interface
        // 5. tid is our client ID provided by TSF framework
        // 6. self.as_interface_ref() creates valid ITfKeyEventSink reference from our object
        unsafe {
            // Get thread manager from Ref
            let thread_mgr = ptim.ok()?;

            // Get ITfKeystrokeMgr from ITfThreadMgr
            let keystroke_mgr: ITfKeystrokeMgr = thread_mgr.cast()?;

            // Register ourselves as ITfKeyEventSink using as_interface_ref()
            if let Err(e) = keystroke_mgr.AdviseKeyEventSink(tid, self.as_interface_ref(), true) {
                debug!("Failed to register KeyEventSink: {:?}", e);
            } else {
                self.this.keystroke_tid.set(tid);
                debug!("KeyEventSink registered with tid={}", tid);
            }
        }

        // Register thread manager event sinks + compartment sink
        unsafe {
            let thread_mgr = ptim.ok()?;
            let source: ITfSource = thread_mgr.cast()?;
            {
                let mut cookies = self.this.thread_cookies.borrow_mut();
                let s: ITfThreadMgrEventSink = {
                    let r: InterfaceRef<'_, ITfThreadMgrEventSink> = self.as_interface_ref();
                    r.to_owned()
                };
                if let Ok(c) = source.AdviseSink(&ITfThreadMgrEventSink::IID, &s) {
                    cookies.push(c);
                }
                let s: ITfThreadFocusSink = {
                    let r: InterfaceRef<'_, ITfThreadFocusSink> = self.as_interface_ref();
                    r.to_owned()
                };
                if let Ok(c) = source.AdviseSink(&ITfThreadFocusSink::IID, &s) {
                    cookies.push(c);
                }
                let s: ITfActiveLanguageProfileNotifySink = {
                    let r: InterfaceRef<'_, ITfActiveLanguageProfileNotifySink> =
                        self.as_interface_ref();
                    r.to_owned()
                };
                if let Ok(c) = source.AdviseSink(&ITfActiveLanguageProfileNotifySink::IID, &s) {
                    cookies.push(c);
                }
            }

            let compartment_mgr: ITfCompartmentMgr = thread_mgr.cast()?;
            if let Ok(openclose) =
                compartment_mgr.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)
            {
                let enable = VARIANT::from(1i32);
                let _ = openclose.SetValue(tid, &enable);
                if let Ok(openclose_src) = openclose.cast::<ITfSource>() {
                    let s: ITfCompartmentEventSink = {
                        let r: InterfaceRef<'_, ITfCompartmentEventSink> = self.as_interface_ref();
                        r.to_owned()
                    };
                    if let Ok(c) = openclose_src.AdviseSink(&ITfCompartmentEventSink::IID, &s) {
                        self.this.keyboard_openclose_cookie.set(c);
                    }
                }
            }
        }

        Ok(())
    }

    fn Deactivate(&self) -> Result<()> {
        debug!("TextService::Deactivate");

        // Clone the ITfThreadMgr out of the RefCell BEFORE any COM calls so the
        // borrow is released. COM callbacks triggered by Unadvise* could re-enter
        // this TextService and attempt a second borrow, causing a RefCell panic.
        let tm = self.this.thread_mgr.borrow().as_ref().cloned();

        if let Some(tm) = tm.as_ref() {
            // SAFETY:
            // 1. tm is a valid ITfThreadMgr interface we stored in Activate
            // 2. cast() to ITfKeystrokeMgr is safe - same COM object
            // 3. UnadviseKeyEventSink is a COM method - safe to call
            // 4. tid is the cookie we received from AdviseKeyEventSink
            unsafe {
                if let Ok(keystroke_mgr) = tm.cast::<ITfKeystrokeMgr>() {
                    let tid = self.this.keystroke_tid.get();
                    if tid != 0 {
                        let _ = keystroke_mgr.UnadviseKeyEventSink(tid);
                        debug!("KeyEventSink unregistered");
                    }
                }
            }
        }

        if let Some(tm) = tm.as_ref() {
            unsafe {
                if let Ok(source) = tm.cast::<ITfSource>() {
                    for cookie in self.this.thread_cookies.borrow_mut().drain(..) {
                        let _ = source.UnadviseSink(cookie);
                    }
                }
                if let Ok(compartment_mgr) = tm.cast::<ITfCompartmentMgr>() {
                    if let Ok(openclose) =
                        compartment_mgr.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)
                    {
                        if let Ok(openclose_src) = openclose.cast::<ITfSource>() {
                            let cookie = self.this.keyboard_openclose_cookie.get();
                            if cookie != TF_INVALID_COOKIE {
                                let _ = openclose_src.UnadviseSink(cookie);
                            }
                        }
                    }
                }
            }
        }
        self.this.keyboard_openclose_cookie.set(TF_INVALID_COOKIE);

        self.this.composition.clear();
        *self.this.thread_mgr.borrow_mut() = None;
        self.this.client_id.set(0);
        self.this.da_atom_input.set(0);
        self.this.da_atom_converted.set(0);
        self.this.keystroke_tid.set(0);
        Ok(())
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(
        &self,
        _ec: u32,
        _composition: Ref<'_, ITfComposition>,
    ) -> Result<()> {
        debug!("OnCompositionTerminated: resetting engine");
        self.this.composition.clear();
        self.this.last_text_len.set(0);
        self.this.vietnamese_engine.borrow_mut().reset();
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32, _dwflags: u32) -> Result<()> {
        self.Activate(ptim, tid)
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _pdim: Ref<'_, ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }
    fn OnUninitDocumentMgr(&self, _pdim: Ref<'_, ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnSetFocus(
        &self,
        pdimfocus: Ref<'_, ITfDocumentMgr>,
        pdimprevfocus: Ref<'_, ITfDocumentMgr>,
    ) -> Result<()> {
        if self.this.key_busy.get() {
            return Ok(());
        }
        if pdimfocus.is_null() && self.this.composition.is_started() {
            debug!("OnSetFocus: focus lost, ending composition");
            // SAFETY: pdimprevfocus is valid when pdimfocus is null per TSF contract
            unsafe {
                if let Ok(prev) = pdimprevfocus.ok() {
                    if let Ok(context) = prev.GetBase() {
                        let _ = self.this.end_composition(&context);
                    }
                }
            }
            self.this.vietnamese_engine.borrow_mut().reset();
        }
        Ok(())
    }

    fn OnPushContext(&self, _pic: Ref<'_, ITfContext>) -> Result<()> {
        Ok(())
    }
    fn OnPopContext(&self, _pic: Ref<'_, ITfContext>) -> Result<()> {
        Ok(())
    }
}

impl ITfThreadFocusSink_Impl for TextService_Impl {
    fn OnSetThreadFocus(&self) -> Result<()> {
        Ok(())
    }
    fn OnKillThreadFocus(&self) -> Result<()> {
        Ok(())
    }
}

impl ITfActiveLanguageProfileNotifySink_Impl for TextService_Impl {
    fn OnActivated(
        &self,
        _clsid: *const GUID,
        _guidprofile: *const GUID,
        _factivated: BOOL,
    ) -> Result<()> {
        Ok(())
    }
}

impl ITfCompartmentEventSink_Impl for TextService_Impl {
    // Signature is fixed by the windows-rs-generated
    // `ITfCompartmentEventSink_Impl` trait (COM vtable contract) — cannot
    // be `unsafe fn`. The raw pointer read is scoped to an inner `unsafe`
    // block below.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        // SAFETY: rguid is a valid pointer provided by TSF framework
        unsafe {
            if rguid.is_null() || *rguid != GUID_COMPARTMENT_KEYBOARD_OPENCLOSE {
                return Ok(());
            }
        }
        // Clone ITfThreadMgr out of the RefCell before COM calls so the borrow
        // is released. GetCompartment/GetValue may call back into this sink.
        let tm = self.this.thread_mgr.borrow().as_ref().cloned();
        let Some(tm) = tm else {
            return Ok(());
        };
        // SAFETY: tm is the ITfThreadMgr stored during Activate
        unsafe {
            let compartment_mgr: ITfCompartmentMgr = tm.cast()?;
            let openclose = compartment_mgr.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)?;
            let value = openclose.GetValue()?;
            use windows::Win32::System::Variant::VT_I4;
            if value.Anonymous.Anonymous.vt == VT_I4
                && value.Anonymous.Anonymous.Anonymous.lVal == 0
            {
                debug!("ITfCompartmentEventSink: IME disabled, resetting engine");
                self.this.composition.clear();
                self.this.vietnamese_engine.borrow_mut().reset();
            }
        }
        Ok(())
    }
}

// Helper functions for key handling
fn is_hotkey(vkey: u16) -> bool {
    // Toggle key (Ctrl+Space)
    if vkey == 0x20 {
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, VK_CONTROL};
            let mut key_state = [0u8; 256];
            if GetKeyboardState(&mut key_state).is_ok() {
                return key_state[VK_CONTROL.0 as usize] & (1 << 7) != 0;
            }
        }
    }
    false
}

fn should_ignore(vkey: u16) -> bool {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, VK_CONTROL, VK_MENU};
        let mut key_state = [0u8; 256];
        if GetKeyboardState(&mut key_state).is_ok() {
            let ctrl = key_state[VK_CONTROL.0 as usize] & (1 << 7) != 0;
            let alt = key_state[VK_MENU.0 as usize] & (1 << 7) != 0;

            // Ignore if Ctrl or Alt is pressed, UNLESS it's a specific hotkey we handle
            if (ctrl || alt) && !is_hotkey(vkey) {
                return true;
            }
        }
    }
    false
}

/// Check if this is a special key that should reset the typing buffer
/// Based on Unikey behavior: navigation and editing keys break the word boundary
fn is_buffer_reset_key(vkey: u16) -> bool {
    matches!(vkey,
        0x21 |         // VK_PRIOR (Page Up)
        0x22 |         // VK_NEXT (Page Down)
        0x23 |         // VK_END
        0x24 |         // VK_HOME
        0x25..=0x28 |  // VK_LEFT, VK_UP, VK_RIGHT, VK_DOWN (arrow keys)
        0x2D |         // VK_INSERT
        0x2E |         // VK_DELETE
        0x09 |         // VK_TAB
        0x1B |         // VK_ESCAPE
        0x70..=0x7B    // VK_F1 through VK_F12
    )
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> Result<()> {
        debug!("ITfKeyEventSink::OnSetFocus");
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        _pic: Ref<'_, ITfContext>,
        wParam: WPARAM,
        _lParam: LPARAM,
    ) -> Result<BOOL> {
        let vkey = wParam.0 as u16;

        // Check for modifiers first
        if should_ignore(vkey) {
            return Ok(BOOL(0));
        }

        // Inline key handling logic: handle printable keys (a-z, 0-9, space, punctuation)
        let should_handle = matches!(vkey,
            0x41..=0x5A |  // A-Z
            0x30..=0x39 |  // 0-9
            0x20 |         // Space
            0xBA..=0xC0 |  // OEM punctuation
            0xDB..=0xDF    // More OEM keys
        );

        // Also intercept buffer reset keys so we can reset the engine
        let should_intercept = should_handle || is_buffer_reset_key(vkey);

        debug!(
            "OnTestKeyDown: vkey={:?}, handle={}, intercept={}",
            vkey, should_handle, should_intercept
        );

        // Return TRUE if we want to handle this key
        Ok(BOOL(should_intercept as i32))
    }

    fn OnTestKeyUp(
        &self,
        _pic: Ref<'_, ITfContext>,
        _wParam: WPARAM,
        _lParam: LPARAM,
    ) -> Result<BOOL> {
        // We don't handle key up events
        Ok(BOOL(0))
    }

    fn OnKeyDown(&self, pic: Ref<'_, ITfContext>, wParam: WPARAM, _lParam: LPARAM) -> Result<BOOL> {
        use buttre_core::types::Action;

        let vkey = wParam.0 as u16;

        debug!("OnKeyDown: vkey={:?}", vkey);

        // Early exits before key_busy is set — these keys pass through immediately
        if is_buffer_reset_key(vkey) {
            debug!(
                "Buffer reset key detected (vkey={}), resetting engine",
                vkey
            );
            if self.this.composition.is_started() {
                if let Some(context) = (*pic).clone() {
                    // Word-boundary final repair (event-sourcing-completion
                    // Phase 3): this reset-key commit path ends the
                    // composition directly, bypassing process_key /
                    // ConfirmComposition — probe BEFORE resetting the engine
                    // below (reset() clears the state the probe reads) and
                    // fold the correction in, same as the Enter branch.
                    if let Some(repaired) = self.this.vietnamese_engine.borrow().boundary_repair() {
                        let sink: ITfCompositionSink = {
                            let r: InterfaceRef<'_, ITfCompositionSink> = self.as_interface_ref();
                            r.to_owned()
                        };
                        if let Err(e) = self.this.write_text(
                            &context,
                            &repaired,
                            repaired.chars().count(),
                            sink,
                        ) {
                            debug!("Failed to write boundary-repair text: {:?}", e);
                        }
                    }
                    let _ = self.this.end_composition(&context);
                }
            }
            self.this.vietnamese_engine.borrow_mut().reset();
            return Ok(BOOL(0));
        }
        if should_ignore(vkey) {
            return Ok(BOOL(0));
        }

        // Extract context before setting key_busy: if this fails we return early, and since
        // OnKeyUp may not be called after an error return, key_busy must stay false.
        let context: ITfContext = (*pic).clone().ok_or(E_FAIL)?;

        // Mark mid-keystroke only after we have a valid context so OnSetFocus doesn't
        // misfire a spurious doc-switch reset during this key event.
        self.this.key_busy.set(true);
        let sink: ITfCompositionSink = {
            let r: InterfaceRef<'_, ITfCompositionSink> = self.as_interface_ref();
            r.to_owned()
        };

        // Check modifiers for processing
        let (shift_pressed, ctrl_pressed) = unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                GetKeyboardState, VK_CONTROL, VK_SHIFT,
            };
            let mut key_state = [0u8; 256];
            if GetKeyboardState(&mut key_state).is_ok() {
                let shift = key_state[VK_SHIFT.0 as usize] & (1 << 7) != 0;
                let ctrl = key_state[VK_CONTROL.0 as usize] & (1 << 7) != 0;
                (shift, ctrl)
            } else {
                (false, false)
            }
        };

        // Handle Ctrl+Space to show Nôm candidates
        if ctrl_pressed && vkey == 0x20 {
            debug!("Ctrl+Space pressed - showing Nôm candidates");

            // Get current buffer text
            let buffer_text = self.this.vietnamese_engine.borrow().buffer_content();

            if !buffer_text.is_empty() {
                // Generate candidates
                let candidates = self
                    .this
                    .vietnamese_engine
                    .borrow()
                    .generate_candidates(&buffer_text);

                if !candidates.is_empty() {
                    if let Err(e) = self
                        .this
                        .show_candidates(&context, candidates, sink.clone())
                    {
                        debug!("Failed to show candidates: {:?}", e);
                    }
                    return Ok(BOOL(1)); // Handled
                }
            }

            return Ok(BOOL(0)); // Pass through if no candidates
        }

        // Convert vkey to char using ToUnicode
        let ch = unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                GetKeyboardState, MapVirtualKeyW, ToUnicode, MAPVK_VK_TO_VSC,
            };

            // Get current keyboard state
            let mut key_state = [0u8; 256];
            if GetKeyboardState(&mut key_state).is_ok() {
                let mut buff = [0u16; 8];
                let sc = MapVirtualKeyW(vkey.into(), MAPVK_VK_TO_VSC);
                let ret = ToUnicode(vkey.into(), sc, Some(&key_state), &mut buff, 0);

                if ret > 0 {
                    // Convert UTF-16 buffer to char
                    // We only care about the first complete char for now
                    String::from_utf16_lossy(&buff[0..ret as usize])
                        .chars()
                        .next()
                } else {
                    // Fallbacks for non-printable keys that return 0 from ToUnicode
                    match vkey {
                        0x08 => Some('\x08'), // Backspace
                        0x0D => Some('\r'),   // Enter
                        0x20 => Some(' '), // Space (usually handled by ToUnicode but just in case)
                        _ => None,
                    }
                }
            } else {
                None
            }
        };

        if let Some(ch) = ch {
            // Handle backspace specially
            if ch == '\x08' {
                let action = self.this.vietnamese_engine.borrow_mut().process_backspace();

                match action {
                    Action::Replace {
                        backspace_count,
                        text,
                    } => {
                        debug!("Backspace: backspace={}, text={}", backspace_count, text);

                        if let Err(e) = self.this.write_text(
                            &context,
                            &text,
                            text.chars().count(),
                            sink.clone(),
                        ) {
                            debug!("Failed to write text: {:?}", e);
                            return Ok(BOOL(0));
                        }

                        Ok(BOOL(1))
                    }
                    _ => Ok(BOOL(0)),
                }
            }
            // Handle space/enter - finalize composition
            // Note: We might want engine to handle punctuation too, so we pass punctuation through
            else if ch == '\r' {
                // Enter always ends composition
                if self.this.composition.is_started() {
                    // Word-boundary final repair (event-sourcing-completion
                    // Phase 3): Enter ends the composition directly,
                    // bypassing process_key/ConfirmComposition entirely —
                    // without this probe a shape-only inferred word (e.g.
                    // VNI "nhat6") commits unrepaired (red-team finding).
                    // Probe BEFORE end_composition/reset and fold the
                    // correction into the composition text if it differs.
                    if let Some(repaired) = self.this.vietnamese_engine.borrow().boundary_repair() {
                        if let Err(e) = self.this.write_text(
                            &context,
                            &repaired,
                            repaired.chars().count(),
                            sink.clone(),
                        ) {
                            debug!("Failed to write boundary-repair text: {:?}", e);
                        }
                    }
                    if let Err(e) = self.this.end_composition(&context) {
                        debug!("Failed to end composition: {:?}", e);
                    }
                    self.this.vietnamese_engine.borrow_mut().reset();
                }
                Ok(BOOL(0))
            }
            // Normal character (including space and punctuation)
            else {
                debug!("Processing normal key: '{}'", ch);
                let actions = self.this.vietnamese_engine.borrow_mut().process_key(ch);
                debug!("Engine returned {} action(s): {:?}", actions.len(), actions);

                // Apply EVERY action in order, not just the first. A closed
                // word run followed by a separator produces
                // [ConfirmComposition(word), Commit(separator)] — stopping
                // after the first action drops the separator on the floor
                // (issue #4: "xin." -> "xin"). `handled` is the OR of every
                // action's outcome: once any action is handled, TSF must not
                // let the raw key fall through to the app too.
                let mut handled = BOOL(0);
                for action in actions {
                    match action {
                        Action::Replace {
                            backspace_count,
                            text,
                        } => {
                            debug!(
                                "Vietnamese engine: backspace={}, text={}",
                                backspace_count, text
                            );

                            if let Err(e) = self.this.write_text(
                                &context,
                                &text,
                                text.chars().count(),
                                sink.clone(),
                            ) {
                                debug!("Failed to write text: {:?}", e);
                                return Ok(BOOL(0));
                            }

                            handled = BOOL(1);
                        }
                        Action::UpdateComposition { text, cursor } => {
                            debug!("UpdateComposition: text={}, cursor={}", text, cursor);
                            if let Err(e) =
                                self.this.write_text(&context, &text, cursor, sink.clone())
                            {
                                debug!("Failed to update composition: {:?}", e);
                                return Ok(BOOL(0));
                            }
                            handled = BOOL(1);
                        }
                        Action::ConfirmComposition(text) => {
                            debug!("ConfirmComposition: text={}", text);
                            if let Err(e) = self.this.write_text(
                                &context,
                                &text,
                                text.chars().count(),
                                sink.clone(),
                            ) {
                                debug!("Failed to write final text: {:?}", e);
                            }
                            if let Err(e) = self.this.end_composition(&context) {
                                debug!("Failed to end composition: {:?}", e);
                            }

                            // Reset engine
                            self.this.vietnamese_engine.borrow_mut().reset();

                            handled = BOOL(1);
                        }
                        Action::Commit(text) => {
                            debug!("Commit: text={}", text);
                            if let Err(e) = self.this.write_text(
                                &context,
                                &text,
                                text.chars().count(),
                                sink.clone(),
                            ) {
                                debug!("Failed to write text: {:?}", e);
                            }
                            if self.this.composition.is_started() {
                                if let Err(e) = self.this.end_composition(&context) {
                                    debug!("Failed to end composition: {:?}", e);
                                }
                            }
                            self.this.vietnamese_engine.borrow_mut().reset();
                            handled = BOOL(1);
                        }
                        Action::DoNothing => {
                            // If engine says DoNothing, but we are inside a composition,
                            // we might need to commit the current composition and pass the key?
                            // Or just pass the key and let TSF/App handle it.
                            // However, if we are in composition, passing the key might insert it *inside* the composition
                            // or corrupt state if the app doesn't know about composition.

                            if self.this.composition.is_started() {
                                // If we have an active composition and get a character that doesn't affect it (e.g. strange symbol),
                                // we probably want to commit the composition first.
                                // But usually buttre-core returns Commit or Confirm in that case.
                                // If it returns DoNothing, it ignores it.

                                debug!("Engine returned DoNothing inside composition - passing through key '{}'", ch);
                            } else {
                                debug!("Engine returned DoNothing - passing through key '{}'", ch);
                            }
                        }
                        // TODO: Implement candidate UI for TSF mode
                        Action::ShowCandidates { .. } => {
                            debug!("ShowCandidates - not yet implemented in TSF");
                            handled = BOOL(1);
                        }
                        Action::HideCandidates => {
                            debug!("HideCandidates - not yet implemented in TSF");
                            handled = BOOL(1);
                        }
                    }
                }
                Ok(handled)
            }
        } else {
            // Not a printable key, pass through
            Ok(BOOL(0))
        }
    }

    fn OnKeyUp(&self, _pic: Ref<'_, ITfContext>, _wParam: WPARAM, _lParam: LPARAM) -> Result<BOOL> {
        self.this.key_busy.set(false);
        Ok(BOOL(0))
    }

    fn OnPreservedKey(&self, _pic: Ref<'_, ITfContext>, _rguid: *const GUID) -> Result<BOOL> {
        Ok(BOOL(0))
    }
}
