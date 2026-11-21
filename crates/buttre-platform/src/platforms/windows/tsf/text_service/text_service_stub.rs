// SPDX-License-Identifier: GPL-3.0-only
// TextService implementation for Windows TSF
// Using windows-rs 0.62 API

use windows::core::*;
use windows::Win32::UI::TextServices::*;

use super::candidate_ui::{CandidatePanel, PAGE_SIZE};
use super::composition::{Composition, PendingComposition};
use super::display_attribute::{
    DisplayAttributeEnum, DisplayAttributeInfo, GUID_DISPLAY_ATTRIBUTE_CONVERTED,
    GUID_DISPLAY_ATTRIBUTE_INPUT,
};
use super::edit_session::{
    EndComposition, InsertText, QueryTextExt, SetCompositionString, PANEL_GAP,
};
use super::langbar::ModeButton;
use super::switching::{self, SwitchCommand};
use super::vietnamese_engine::{CandidateMotion, VietnameseEngine};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use tracing::debug;

use crate::platforms::windows::tsf::com::{dll_add_ref, dll_release};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, HWND, LPARAM, WPARAM};
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
    ITfTextEditSink,
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
    /// Mirror of buttre's session-wide on/off compartment (`switching.rs`),
    /// refreshed at `Activate` and by `OnChange`. Off = claim no key.
    buttre_enabled: Cell<bool>,
    /// Advise cookie for that compartment's change sink.
    enabled_cookie: Cell<u32>,
    /// The input-mode button next to "ENG" (`langbar.rs`), registered with the
    /// thread's language bar from `Activate` to `Deactivate`.
    mode_button: RefCell<Option<ComObject<ModeButton>>>,
    pub(crate) key_busy: Cell<bool>,
    vietnamese_engine: Rc<RefCell<VietnameseEngine>>,
    /// Nôm candidate popup, created on first use and reused afterwards —
    /// creating a window per lookup would flicker and churn handles.
    candidate_panel: RefCell<Option<CandidatePanel>>,
    /// Last screen position the panel was placed at, reused when the host
    /// cannot report a fresh text extent (see `composition_screen_pos`).
    last_panel_pos: Cell<(i32, i32)>,
    /// `ITfTextEditSink` advise cookies, one per context we are watching.
    ///
    /// This is how the caret moving is detected WITHOUT eating the key that
    /// moved it — see `ITfTextEditSink_Impl::OnEndEdit`. Keyed by the context
    /// itself so a context pushed twice is not advised twice, and so
    /// `Deactivate` can unadvise every one.
    edit_cookies: RefCell<Vec<(ITfContext, u32)>>,
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
        // BEFORE anything reads a data file. `VietnameseEngine::new` below
        // resolves `buttre_nom.db` and `keyboards/*.toml`, and the default
        // resolver searches next to `current_exe()` — which in this process is
        // Word, Notepad or Chrome, not buttre. See `set_resource_dir`.
        pin_resource_dir();

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
            buttre_enabled: Cell::new(true),
            enabled_cookie: Cell::new(TF_INVALID_COOKIE),
            mode_button: RefCell::new(None),
            key_busy: Cell::new(false),
            vietnamese_engine: Rc::new(RefCell::new(VietnameseEngine::new())),
            candidate_panel: RefCell::new(None),
            last_panel_pos: Cell::new((100, 100)),
            edit_cookies: RefCell::new(Vec::new()),
        }
    }

    /// Is `selection` outside the open composition?
    ///
    /// The question that separates "the user moved the caret away" from "our own
    /// composition update moved it", and it is asked by POSITION rather than by
    /// timing on purpose: our edit sessions are `TF_ES_ASYNCDONTCARE`, so they
    /// can land after the keystroke that queued them has finished. Any flag
    /// meant to mark "this edit is ours" would already be cleared by then, and
    /// we would tear down our own composition mid-word.
    ///
    /// Returns `None` when the comparison cannot be made (no composition, or a
    /// range the host refuses to compare) — callers then leave the composition
    /// alone rather than guessing.
    fn selection_left_composition(
        &self,
        context: &ITfContext,
        ec: u32,
        composition_range: &ITfRange,
    ) -> Option<bool> {
        // SAFETY: `ec` is the read cookie TSF handed to `OnEndEdit`, valid for
        // the duration of that callback; `context` is the context it named.
        unsafe {
            let mut selection = [TF_SELECTION::default(); 1];
            let mut fetched = 0u32;
            context
                .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
                .ok()?;
            if fetched == 0 {
                return None;
            }
            // Take the range out of the ManuallyDrop wrapper so it is released
            // when this scope ends.
            let [TF_SELECTION { range, .. }] = selection;
            let selected = std::mem::ManuallyDrop::into_inner(range)?;

            let before = composition_range
                .CompareStart(ec, &selected, TF_ANCHOR_START)
                .ok()?;
            let after = composition_range
                .CompareEnd(ec, &selected, TF_ANCHOR_END)
                .ok()?;

            // `before > 0`: the composition starts after the selection does, so
            // the caret sits before our text. `after < 0`: the composition ends
            // before the selection does, so the caret ran past it.
            Some(before > 0 || after < 0)
        }
    }

    /// Queue `session` on `context`, synchronously when `sync` asks for it.
    ///
    /// Sync exists for one caller: finalizing the composition from
    /// `OnTestKeyDown` for a key the application is about to handle (Enter,
    /// arrows, shortcuts). That must land BEFORE the application acts on the
    /// key — deferred, the newline or paste would go inside the composition
    /// and the next composition update would overwrite it.
    ///
    /// Callers pass `sync = false` while an earlier `SetCompositionString` is
    /// still queued: it would run after this one, out of order. A refused
    /// lock (`TF_E_SYNCHRONOUS`) falls back to async — later than ideal,
    /// never lost.
    fn request_session(
        &self,
        context: &ITfContext,
        session: &ITfEditSession,
        sync: bool,
    ) -> Result<()> {
        // SAFETY: `context` and `session` are valid COM interfaces; client_id
        // is the id TSF handed us at Activate.
        unsafe {
            if sync {
                let granted = context.RequestEditSession(
                    self.client_id.get(),
                    session,
                    TF_ES_SYNC | TF_ES_READWRITE,
                )?;
                if granted != TF_E_SYNCHRONOUS {
                    return Ok(());
                }
                debug!("Synchronous edit session refused, deferring it");
            }
            context.RequestEditSession(
                self.client_id.get(),
                session,
                TF_ES_ASYNCDONTCARE | TF_ES_READWRITE,
            )?;
        }
        Ok(())
    }

    pub fn write_text(
        &self,
        context: &ITfContext,
        text: &str,
        cursor: usize,
        sink: ITfCompositionSink,
    ) -> Result<()> {
        self.write_text_with(context, text, cursor, sink, false)
    }

    /// [`Self::write_text`], optionally synchronous — see
    /// [`Self::request_session`].
    ///
    /// `cursor` counts characters, like the engine; this is where it becomes
    /// a TSF offset, which counts UTF-16 units. Most Nôm characters lie
    /// outside the BMP (𡗶 is two units), so passing the character count
    /// through put the caret between the two halves of the character, and
    /// the next keystroke split it.
    fn write_text_with(
        &self,
        context: &ITfContext,
        text: &str,
        cursor: usize,
        sink: ITfCompositionSink,
        sync: bool,
    ) -> Result<()> {
        debug!("TextService::write_text: {}", text);
        let cursor = utf16_offset(text, cursor);
        let text_len = text.encode_utf16().count();

        // Reuse in-flight session if TSF hasn't executed it yet.
        // Only update text/cursor — previous_length was captured at session-create
        // time and represents chars committed BEFORE this composition started;
        // clobbering it here would make the recovery path in DoEditSession
        // mis-count the chars to replace (Chrome-omnibox recovery, see edit_session.rs).
        if let Some(rc) = self.pending_edit.borrow().upgrade() {
            let mut p = rc.borrow_mut();
            p.text = text.into();
            p.cursor = cursor;
            self.last_text_len.set(text_len);
            return Ok(());
        }

        let previous_length = self.last_text_len.get();
        let pending = Rc::new(RefCell::new(PendingComposition {
            text: text.into(),
            cursor,
            previous_length,
        }));
        *self.pending_edit.borrow_mut() = Rc::downgrade(&pending);
        // UTF-16 units, like every TSF offset: DoEditSession's recovery path
        // shifts the range back by exactly this much.
        self.last_text_len.set(text_len);
        let da = VARIANT::from(self.da_atom_input.get() as i32);
        let session =
            SetCompositionString::new(context.clone(), self.composition.clone(), sink, da, pending);
        let session_interface: ITfEditSession = session.into();
        self.request_session(context, &session_interface, sync)
    }

    /// Insert finished text straight into the document, with no composition.
    ///
    /// The right shape for `Action::Commit` — a separator or a passthrough
    /// character is FINISHED text, not something the user is still editing.
    ///
    /// Routing it through [`Self::write_text`] instead was a race: that starts
    /// a composition, and the caller then had to end it by testing
    /// `composition.is_started()` on the very next line. When TSF ran the
    /// composition's edit session synchronously the test passed; when TSF
    /// DEFERRED it — measured at ~800 µs on Word against ~4 µs when inline —
    /// the test ran first, saw no composition, and skipped the end. The space
    /// was then left as an open composition, and the next keystroke's
    /// `SetText` overwrote it: the space visibly disappeared as the next word
    /// began.
    pub fn insert_text(&self, context: &ITfContext, text: &str) -> Result<()> {
        debug!("TextService::insert_text: {} char(s)", text.chars().count());

        let session = InsertText::new(context.clone(), HSTRING::from(text));
        let session_interface: ITfEditSession = session.into();
        self.request_session(context, &session_interface, false)
    }

    /// Helper to end composition via EndComposition edit session
    pub fn end_composition(&self, context: &ITfContext) -> Result<()> {
        self.end_composition_with(context, false)
    }

    /// [`Self::end_composition`], optionally synchronous — see
    /// [`Self::request_session`].
    fn end_composition_with(&self, context: &ITfContext, sync: bool) -> Result<()> {
        debug!("TextService::end_composition");
        // A write still queued must land first, so this cannot jump ahead.
        let sync = sync && self.pending_edit.borrow().upgrade().is_none();

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
            self.request_session(context, &session_interface, sync)?;

            // Forget the composition we just ended (CRITICAL). Nothing else
            // will: `OnCompositionTerminated` fires only when the APPLICATION
            // terminates a composition, never for one we end ourselves. Left
            // behind, the dead `ITfComposition` made `is_started()` keep
            // answering true, so every later keystroke skipped
            // `StartComposition` and called `GetRange` on a terminated
            // composition — which fails, aborting the edit session. The
            // symptom was a text service that composed the first word and
            // then went silent forever, with no error anywhere.
            //
            // Safe to clear now even though the session above is async: it
            // was handed its own reference to the composition when it was
            // built, so it does not read this slot.
            self.composition.clear();
            self.last_text_len.set(0);
        }

        Ok(())
    }

    /// Screen position just under the composition — or under the caret while
    /// no composition exists yet — for placing the candidate panel. `None`
    /// when the host cannot report one now.
    ///
    /// The extent has to be read inside an edit session — see [`QueryTextExt`].
    /// Requested synchronously; when the host refuses that, the session is
    /// deferred and moves `panel` itself once it runs.
    fn composition_screen_pos(&self, context: &ITfContext, panel: HWND) -> Option<(i32, i32)> {
        // SAFETY: `composition` is the live ITfComposition; GetRange takes no
        // edit cookie. RequestEditSession is a plain COM call on a valid
        // context with the client id TSF gave us at Activate.
        unsafe {
            let range = self.composition.get().and_then(|c| c.GetRange().ok());
            let out = Rc::new(Cell::new(None));
            let session: ITfEditSession =
                QueryTextExt::new(context.clone(), range.clone(), out.clone(), None).into();
            let granted = context
                .RequestEditSession(self.client_id.get(), &session, TF_ES_SYNC | TF_ES_READ)
                .inspect_err(|e| debug!("Text extent query failed: {:?}", e))
                .ok()?;
            if granted == TF_E_SYNCHRONOUS {
                debug!("Text extent query deferred; it will move the panel itself");
                let session: ITfEditSession =
                    QueryTextExt::new(context.clone(), range, out, Some(panel)).into();
                let _ = context.RequestEditSession(
                    self.client_id.get(),
                    &session,
                    TF_ES_ASYNC | TF_ES_READ,
                );
                return None;
            }
            let rect = out.get()?;
            Some((rect.left, rect.bottom + PANEL_GAP))
        }
    }

    /// Draw (or redraw) the candidate panel for the engine's current list,
    /// creating the window on first use. Hides it when nothing is offered.
    ///
    /// Panel creation failure is logged and swallowed: a missing popup makes
    /// Nôm harder to use, but an error propagated out of the key sink would
    /// make the key fall through and corrupt the composition.
    fn refresh_candidates(&self, context: &ITfContext) {
        // Copy the state out and RELEASE the engine borrow before the COM calls
        // below. Same rule as `Deactivate`: a borrow held across a call into the
        // application can be re-entered by a sink callback
        // (`OnCompositionTerminated` takes the engine mutably), and a RefCell
        // clash under `panic = "abort"` kills the host process, not just us.
        let state = self.vietnamese_engine.borrow().candidates().clone();

        if state.is_empty() {
            self.hide_candidates();
            return;
        }

        // The panel exists BEFORE positioning, so a deferred extent query has
        // a window to move. Its borrow ends here: positioning runs
        // application code, which can re-enter this object.
        let panel_hwnd = {
            let mut slot = self.candidate_panel.borrow_mut();
            // An owner takes its owned windows down with it: rebuild after that.
            if slot.as_ref().is_some_and(|panel| !panel.is_alive()) {
                *slot = None;
            }
            if slot.is_none() {
                match CandidatePanel::new() {
                    Ok(panel) => *slot = Some(panel),
                    Err(e) => {
                        debug!("Candidate panel unavailable: {:?}", e);
                        return;
                    }
                }
            }
            match slot.as_ref() {
                Some(panel) => panel.hwnd(),
                None => return,
            }
        };

        // Three sources, best first: the text extent (composition, or the
        // caret before the composition exists), the system caret (plain Win32
        // editors), the last position used.
        let extent = self.composition_screen_pos(context, panel_hwnd);
        let caret = if extent.is_none() {
            system_caret_pos()
        } else {
            None
        };
        let (x, y) = extent
            .or(caret)
            .unwrap_or_else(|| self.last_panel_pos.get());
        self.last_panel_pos.set((x, y));
        let owner = candidate_owner(context);
        debug!(
            "Candidate panel at ({x}, {y}) from {}, owner {:?}",
            if extent.is_some() {
                "text extent"
            } else if caret.is_some() {
                "system caret"
            } else {
                "last position"
            },
            owner
        );

        if let Some(panel) = self.candidate_panel.borrow().as_ref() {
            panel.show(&state, x, y, owner);
        }
    }

    /// Hide the panel, if one was ever created.
    fn hide_candidates(&self) {
        if let Some(panel) = self.candidate_panel.borrow().as_ref() {
            panel.hide();
        }
    }

    /// Commit `text` as the finished composition — the shape a chosen Nôm
    /// candidate takes: replace the reading with the character, close the
    /// composition, drop the popup.
    fn commit_composition_text(
        &self,
        context: &ITfContext,
        text: &str,
        sink: ITfCompositionSink,
    ) -> Result<()> {
        self.write_text(context, text, text.chars().count(), sink)?;
        self.end_composition(context)?;
        self.hide_candidates();
        Ok(())
    }

    /// Consume a key on behalf of the open candidate popup.
    ///
    /// Returns `false` when there is no list showing, or when the key means
    /// nothing to it — an out-of-range digit, say. The caller then continues
    /// with normal processing, so a stray key types itself instead of
    /// dismissing the list.
    ///
    /// This must run BEFORE `OnKeyDown`'s buffer-reset branch: Escape, Page Up
    /// and Page Down are all reset keys, and while candidates are up they mean
    /// dismiss and paginate, not "throw the word away".
    fn handle_candidate_key(
        &self,
        context: &ITfContext,
        vkey: u16,
        sink: &ITfCompositionSink,
    ) -> bool {
        if self.vietnamese_engine.borrow().candidates().is_empty() {
            return false;
        }

        if let Some(motion) = candidate_motion(vkey) {
            self.vietnamese_engine
                .borrow_mut()
                .move_candidate_cursor(motion, PAGE_SIZE);
            self.refresh_candidates(context);
            return true;
        }

        if vkey == VK_ESCAPE {
            // Dismiss only. The reading stays composed so the user can carry on
            // typing it, or commit it as Quốc ngữ.
            self.vietnamese_engine.borrow_mut().dismiss_candidates();
            self.hide_candidates();
            return true;
        }

        let chosen = match vkey {
            VK_DIGIT_1..=VK_DIGIT_9 => self
                .vietnamese_engine
                .borrow_mut()
                .select_candidate_at_page((vkey - VK_DIGIT_1) as usize, PAGE_SIZE),
            VK_SPACE | VK_RETURN => self
                .vietnamese_engine
                .borrow_mut()
                .select_current_candidate(),
            _ => None,
        };

        let Some(text) = chosen else {
            return false;
        };
        debug!("Candidate chosen: {}", text);
        if let Err(e) = self.commit_composition_text(context, &text, sink.clone()) {
            debug!("Failed to commit candidate: {:?}", e);
        }
        true
    }

    /// Switch the engine to the method `profile` stands for. Called with the
    /// active profile at `Activate` and on every profile switch.
    ///
    /// An open composition is committed first: it was composed by the
    /// previous method's keystrokes, and the next update under the new method
    /// would overwrite it with a recomposition of different keys.
    fn adopt_profile(&self, profile: &GUID) {
        let Some(method) = crate::platforms::windows::tsf::profiles::method_for_profile(profile)
        else {
            debug!("TSF profile {profile:?} is not one of ours");
            return;
        };
        debug!("TSF profile {profile:?} -> method '{method}'");
        if self.composition.is_started() {
            if let Some(context) = self.focused_context() {
                let _ = self.end_composition(&context);
            }
        }
        {
            let mut engine = self.vietnamese_engine.borrow_mut();
            engine.reset();
            engine.select_method(&method);
        }
        self.update_mode_button();
    }

    /// Bring the language-bar button in line with the current method and
    /// on/off state. Borrows are released before the button's COM callback.
    fn update_mode_button(&self) {
        let method = self.vietnamese_engine.borrow().method_id().to_string();
        let on = self.buttre_enabled.get();
        let button = self.mode_button.borrow().clone();
        if let Some(button) = button {
            button.set_state(&method, on);
        }
    }

    /// The context of the document that has focus in this thread, if any.
    fn focused_context(&self) -> Option<ITfContext> {
        let tm = self.thread_mgr.borrow().as_ref().cloned()?;
        // SAFETY: plain COM calls on the thread manager TSF gave us.
        unsafe { tm.GetFocus().ok()?.GetBase().ok() }
    }

    /// Re-read the session-wide on/off value. Turning off commits the word
    /// being typed — it stays in the document as typed, the way switching
    /// keyboards would leave it.
    fn refresh_enabled(&self) {
        let Some(tm) = self.thread_mgr.borrow().as_ref().cloned() else {
            return;
        };
        let on = switching::read_enabled(&tm);
        let was = self.buttre_enabled.replace(on);
        if was && !on {
            debug!("buttre turned off");
            if self.composition.is_started() {
                if let Some(context) = self.focused_context() {
                    let _ = self.end_composition(&context);
                }
            }
            self.vietnamese_engine.borrow_mut().reset();
            self.hide_candidates();
        } else if !was && on {
            debug!("buttre turned on");
        }
        self.update_mode_button();
    }

    /// Carry out `Ctrl+Shift+0…9` (see `switching.rs`).
    fn apply_switch(&self, command: SwitchCommand) {
        let Some(tm) = self.thread_mgr.borrow().as_ref().cloned() else {
            return;
        };
        let client_id = self.client_id.get();
        let on = !matches!(command, SwitchCommand::TurnOff);
        if let SwitchCommand::Activate(slot) = command {
            let Some(entry) = switching::installed_profile_for_slot(slot) else {
                debug!("Ctrl+Shift+{slot}: no buttre keyboard in that slot");
                return;
            };
            if active_buttre_profile() != Some(entry.guid) {
                // `OnActivated` adopts the method once Windows switches.
                if let Err(e) = switching::activate_profile(&entry.guid) {
                    debug!(
                        "Ctrl+Shift+{slot}: could not activate '{}' (not added in Settings?): {:?}",
                        entry.description, e
                    );
                }
            }
        }
        if let Err(e) = switching::write_enabled(&tm, client_id, on) {
            debug!(
                "Could not switch buttre {}: {:?}",
                if on { "on" } else { "off" },
                e
            );
        }
        // The compartment notifies every sink, ours included; refreshing here
        // too keeps this keystroke's own process right even if it does not.
        self.refresh_enabled();
    }

    /// buttre is on (`Ctrl+Shift+0…9`) AND has a keyboard loaded — the
    /// condition for claiming any typing key.
    fn is_typing_active(&self) -> bool {
        self.buttre_enabled.get() && self.vietnamese_engine.borrow().is_active()
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32) -> Result<()> {
        // First point in the DLL's life where file I/O is safe — `DllMain`
        // runs under the loader lock and must not touch the filesystem.
        // Idempotent, so the per-activation cost is one atomic load.
        crate::platforms::windows::tsf::logging::init_logging();
        debug!("TextService::Activate");

        let tm: ITfThreadMgr = ptim.ok()?.clone();
        *self.this.thread_mgr.borrow_mut() = Some(tm);
        self.this.client_id.set(tid);

        // The active keyboard profile IS the input method (`profiles.rs`).
        if let Some(profile) = active_buttre_profile() {
            self.this.adopt_profile(&profile);
        }

        // Session-wide on/off (Ctrl+Shift+0…9): read the current value, and
        // watch it so a switch made in another application lands here too.
        self.this.refresh_enabled();
        if let Some(tm) = self.this.thread_mgr.borrow().as_ref().cloned() {
            let sink: ITfCompartmentEventSink = {
                let r: InterfaceRef<'_, ITfCompartmentEventSink> = self.as_interface_ref();
                r.to_owned()
            };
            // SAFETY: plain COM calls on the compartment of a live thread
            // manager; the sink is this object, unadvised in Deactivate.
            unsafe {
                match switching::enabled_compartment(&tm)
                    .and_then(|c| c.cast::<ITfSource>())
                    .and_then(|source| source.AdviseSink(&ITfCompartmentEventSink::IID, &sink))
                {
                    Ok(cookie) => self.this.enabled_cookie.set(cookie),
                    Err(e) => debug!("Watching the on/off compartment failed: {:?}", e),
                }
            }
        }

        // The input-mode button next to "ENG": method icon while on, "E" while
        // off; a click toggles the same session-wide switch.
        if let Some(tm) = self.this.thread_mgr.borrow().as_ref().cloned() {
            let toggle_tm = tm.clone();
            let toggle = Box::new(move || {
                let on = switching::read_enabled(&toggle_tm);
                if let Err(e) = switching::write_enabled(&toggle_tm, tid, !on) {
                    debug!("Mode button could not toggle buttre: {:?}", e);
                }
            });
            let method = self.this.vietnamese_engine.borrow().method_id().to_string();
            let button =
                ModeButton::new(&method, self.this.buttre_enabled.get(), toggle).into_object();
            // SAFETY: plain COM calls on the thread manager TSF gave us.
            let added = unsafe {
                tm.cast::<ITfLangBarItemMgr>()
                    .and_then(|mgr| mgr.AddItem(&button.to_interface::<ITfLangBarItem>()))
            };
            match added {
                Ok(()) => *self.this.mode_button.borrow_mut() = Some(button),
                Err(e) => debug!("Adding the mode button to the language bar failed: {:?}", e),
            }
        }

        // Transport claim (phase 03): tell the hook this PROCESS hosts a live
        // text service. Process id, not thread id — in Chromium/Qt/Notepad the
        // activating thread is not the foreground window's thread, and the
        // mismatch made the hook type over the text service (garbled text).
        //
        // SAFETY: GetCurrentProcessId has no preconditions.
        let host_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        crate::platforms::windows::transport_claim::publish(host_pid);
        debug!("transport claim published for process {host_pid}");

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

        // Release the transport claim FIRST: from this moment the hook may
        // take over this window, and every line below only tears down COM
        // plumbing that no longer affects who types. Conditional release (not
        // publish(0)): another app's activation may have claimed since, and
        // wiping ITS claim would unguard a live text service.
        //
        // SAFETY: GetCurrentProcessId has no preconditions.
        let host_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        crate::platforms::windows::transport_claim::release(host_pid);

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
        if let Some(tm) = tm.as_ref() {
            let cookie = self.this.enabled_cookie.replace(TF_INVALID_COOKIE);
            if cookie != TF_INVALID_COOKIE {
                // SAFETY: the cookie came from AdviseSink on this compartment.
                unsafe {
                    if let Ok(source) =
                        switching::enabled_compartment(tm).and_then(|c| c.cast::<ITfSource>())
                    {
                        let _ = source.UnadviseSink(cookie);
                    }
                }
            }
        }
        // Take the button out BEFORE the COM call: RemoveItem unadvises its
        // sink, which runs this object's code.
        let button = self.this.mode_button.borrow_mut().take();
        if let (Some(tm), Some(button)) = (tm.as_ref(), button) {
            // SAFETY: plain COM calls on the thread manager stored at Activate.
            unsafe {
                if let Ok(mgr) = tm.cast::<ITfLangBarItemMgr>() {
                    let _ = mgr.RemoveItem(&button.to_interface::<ITfLangBarItem>());
                }
            }
        }
        self.unadvise_all_edit_sinks();

        self.this.composition.clear();
        // Destroy the panel outright, not just hide it: Deactivate means this
        // thread is done with us, and a window outliving it would be orphaned.
        *self.this.candidate_panel.borrow_mut() = None;
        *self.this.thread_mgr.borrow_mut() = None;
        self.this.client_id.set(0);
        self.this.da_atom_input.set(0);
        self.this.da_atom_converted.set(0);
        self.this.keystroke_tid.set(0);
        Ok(())
    }
}

impl TextService_Impl {
    /// Commit the open composition as finished text and forget the word.
    ///
    /// The single commit path for everything that ends a word without the
    /// engine committing it itself: Enter, Escape, navigation and editing
    /// keys, shortcuts. The boundary repair is probed and learning collected
    /// BEFORE `reset` discards the raw keystrokes both read. `sync` is for
    /// keys the application acts on right after (see `request_session`).
    fn finalize_composition(&self, context: &ITfContext, sync: bool) {
        if self.this.composition.is_started() {
            // Bound to a local so the engine borrow ends before the COM calls
            // below, which can re-enter this object.
            let repaired = self.this.vietnamese_engine.borrow().boundary_repair();
            if let Some(repaired) = repaired {
                let sink: ITfCompositionSink = {
                    let r: InterfaceRef<'_, ITfCompositionSink> = self.as_interface_ref();
                    r.to_owned()
                };
                let len = repaired.chars().count();
                if let Err(e) = self
                    .this
                    .write_text_with(context, &repaired, len, sink, sync)
                {
                    debug!("Failed to write boundary-repair text: {:?}", e);
                }
            }
            if let Err(e) = self.this.end_composition_with(context, sync) {
                debug!("Failed to end composition: {:?}", e);
            }
            // The word stays in the document (ending finalizes, never
            // deletes), so it was accepted: collect before the reset below.
            self.this
                .vietnamese_engine
                .borrow_mut()
                .collect_pending_learning();
        }
        self.this.vietnamese_engine.borrow_mut().reset();
        self.this.hide_candidates();
    }

    /// Start watching `context` for edits, unless we already are.
    ///
    /// Advised per context rather than once per thread because
    /// `ITfTextEditSink` lives on `ITfContext` — a thread can hold several
    /// (a document plus its find bar, say) and each needs its own advise.
    fn advise_edit_sink(&self, context: &ITfContext) {
        if self
            .this
            .edit_cookies
            .borrow()
            .iter()
            .any(|(watched, _)| watched == context)
        {
            return;
        }
        // SAFETY: `context` is a live ITfContext from TSF; the sink reference is
        // this object, which outlives the advise (Deactivate unadvises).
        unsafe {
            let Ok(source) = context.cast::<ITfSource>() else {
                return;
            };
            let sink: ITfTextEditSink = {
                let r: InterfaceRef<'_, ITfTextEditSink> = self.as_interface_ref();
                r.to_owned()
            };
            match source.AdviseSink(&ITfTextEditSink::IID, &sink) {
                Ok(cookie) => {
                    self.this
                        .edit_cookies
                        .borrow_mut()
                        .push((context.clone(), cookie));
                }
                // Not fatal: without the sink we simply lose the caret-move
                // detection in this context, and the composition is still ended
                // on focus change and on commit.
                Err(e) => debug!("AdviseSink(ITfTextEditSink) failed: {:?}", e),
            }
        }
    }

    /// Stop watching `context`.
    fn unadvise_edit_sink(&self, context: &ITfContext) {
        let cookie = {
            let mut cookies = self.this.edit_cookies.borrow_mut();
            let Some(index) = cookies.iter().position(|(watched, _)| watched == context) else {
                return;
            };
            cookies.remove(index).1
        };
        // SAFETY: cookie came from AdviseSink on this same context.
        unsafe {
            if let Ok(source) = context.cast::<ITfSource>() {
                let _ = source.UnadviseSink(cookie);
            }
        }
    }

    /// Release every edit-sink advise. Called from `Deactivate`, where the
    /// borrow is taken and released before any COM call — `UnadviseSink` can
    /// re-enter this object.
    fn unadvise_all_edit_sinks(&self) {
        let watched: Vec<(ITfContext, u32)> =
            self.this.edit_cookies.borrow_mut().drain(..).collect();
        // SAFETY: each cookie came from AdviseSink on its paired context.
        unsafe {
            for (context, cookie) in watched {
                if let Ok(source) = context.cast::<ITfSource>() {
                    let _ = source.UnadviseSink(cookie);
                }
            }
        }
    }
}

impl ITfTextEditSink_Impl for TextService_Impl {
    /// The caret moved, or the document changed — decide whether our
    /// composition still belongs where it is.
    ///
    /// This exists so navigation keys never have to be EATEN. `OnTestKeyDown`
    /// can only answer "will you consume this key", so the old way of noticing
    /// an arrow press was to consume it, which stopped the caret from moving at
    /// all. Here the app moves the caret normally and TSF tells us afterwards.
    fn OnEndEdit(
        &self,
        pic: Ref<'_, ITfContext>,
        ec: u32,
        peditrecord: Ref<'_, ITfEditRecord>,
    ) -> Result<()> {
        if !self.this.composition.is_started() {
            return Ok(());
        }
        let (Some(context), Some(record)) = ((*pic).clone(), (*peditrecord).clone()) else {
            return Ok(());
        };

        // SAFETY: both interfaces come from TSF and are valid for this callback;
        // `ec` is its read cookie.
        let moved = unsafe { record.GetSelectionStatus() };
        if !matches!(moved, Ok(status) if status.as_bool()) {
            return Ok(());
        }

        // SAFETY: the composition is open (checked above), so GetRange has a
        // live range to hand back. It takes no edit cookie.
        let range = unsafe {
            let Some(composition) = self.this.composition.get() else {
                return Ok(());
            };
            match composition.GetRange() {
                Ok(range) => range,
                Err(e) => {
                    debug!("OnEndEdit: composition range unavailable: {:?}", e);
                    return Ok(());
                }
            }
        };

        // Anything other than a definite "left it" — inside the composition (our
        // own update, or a click within the word), or not comparable — leaves
        // the composition alone.
        if self.this.selection_left_composition(&context, ec, &range) == Some(true) {
            debug!("OnEndEdit: caret left the composition, closing it");
            if let Err(e) = self.this.end_composition(&context) {
                debug!("Failed to end composition after caret move: {:?}", e);
            }
            {
                // The word stays in the document (end_composition finalizes,
                // never deletes) — a caret move away is acceptance, the same
                // standard the buffer-reset keys apply. Collect before reset
                // discards the raw.
                let mut engine = self.this.vietnamese_engine.borrow_mut();
                engine.collect_pending_learning();
                engine.reset();
            }
            self.this.hide_candidates();
        }
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
        // The popup describes a composition the application just tore down.
        self.this.hide_candidates();
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
        // Watch the newly focused document's context. `OnPushContext` covers
        // contexts pushed while we are active, but a document that already
        // existed when the user switched TO buttre never fires it — and that is
        // the common case, so without this the very first document typed in
        // would have no caret-move detection.
        if let Ok(focused) = pdimfocus.ok() {
            // SAFETY: `focused` is the document manager TSF just gave us.
            if let Ok(context) = unsafe { focused.GetBase() } {
                self.advise_edit_sink(&context);
            }

            // Re-publish the transport claim on every focus gain. `Activate`
            // fires once per app, but the claim cell is ONE value per session:
            // with buttre active in two apps, Alt+Tab-ing between them would
            // leave the claim pointing at the app focused LAST at activation
            // time — the hook would then see foreground ≠ claim in the other
            // app and type over its live text service. Focus is the event that
            // tracks "whose window is in front", so it is where the claim must
            // be refreshed.
            //
            // SAFETY: GetCurrentThreadId has no preconditions.
            let host_thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
            crate::platforms::windows::transport_claim::publish(host_thread);
        }

        if self.this.key_busy.get() {
            return Ok(());
        }
        // Any move of focus OFF the document holding the composition ends it —
        // focus lost entirely (`pdimfocus` null) or moved to another document
        // of this thread (a Notepad tab, a find box). Left open, the engine's
        // keystrokes carried over and the next word typed in the new
        // document was composed onto the old one's.
        let moved_away = (*pdimfocus).clone() != (*pdimprevfocus).clone();
        if moved_away && self.this.composition.is_started() {
            debug!("OnSetFocus: focus left the composing document, ending composition");
            // SAFETY: GetBase is a plain COM call on the document manager TSF
            // just handed us as the previously focused one.
            unsafe {
                if let Ok(prev) = pdimprevfocus.ok() {
                    if let Ok(context) = prev.GetBase() {
                        let _ = self.this.end_composition(&context);
                    }
                }
            }
            {
                // Focus loss finalizes the composed text into the document
                // too — acceptance by the same standard as every other
                // end_composition path here. Collect before reset.
                let mut engine = self.this.vietnamese_engine.borrow_mut();
                engine.collect_pending_learning();
                engine.reset();
            }
            self.this.hide_candidates();
        }
        Ok(())
    }

    fn OnPushContext(&self, pic: Ref<'_, ITfContext>) -> Result<()> {
        if let Some(context) = (*pic).clone() {
            self.advise_edit_sink(&context);
        }
        Ok(())
    }

    fn OnPopContext(&self, pic: Ref<'_, ITfContext>) -> Result<()> {
        if let Some(context) = (*pic).clone() {
            self.unadvise_edit_sink(&context);
        }
        Ok(())
    }
}

impl ITfThreadFocusSink_Impl for TextService_Impl {
    /// The host thread gained input focus — the earliest, most reliable "this
    /// app is now in front" signal TSF gives us, and it fires BEFORE the first
    /// keystroke lands. The document-focus sink alone missed app switches in
    /// hosts that keep one document manager across windows, and the claim has
    /// to be in place before key one or the hook types that key.
    fn OnSetThreadFocus(&self) -> Result<()> {
        // SAFETY: GetCurrentProcessId has no preconditions.
        let host_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        crate::platforms::windows::transport_claim::publish(host_pid);
        Ok(())
    }

    /// Deliberately does NOT release the claim: the window gaining focus next
    /// either hosts its own text service (its OnSetThreadFocus overwrites the
    /// claim) or does not (its pid never matches, so the hook runs there
    /// regardless). Releasing here would only open a gap in which the hook
    /// could race a sibling window of THIS app.
    fn OnKillThreadFocus(&self) -> Result<()> {
        Ok(())
    }
}

impl ITfActiveLanguageProfileNotifySink_Impl for TextService_Impl {
    /// The user switched to one of buttre's profiles (Win+Space, Ctrl+Shift,
    /// the keyboard list): adopt the method that profile stands for.
    ///
    /// Deactivation needs nothing: once another keyboard is active, keys stop
    /// reaching this text service. It used to write `Settings::enabled = false`
    /// so a hook running alongside would stand down; the TSF build no longer
    /// runs a hook alongside, and that write leaked into the separately-run
    /// hook app's on/off state.
    // Signature is fixed by the windows-rs-generated trait (COM vtable
    // contract) — cannot be `unsafe fn`; the pointer reads are scoped below.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn OnActivated(
        &self,
        clsid: *const GUID,
        guidprofile: *const GUID,
        factivated: BOOL,
    ) -> Result<()> {
        // SAFETY: both pointers are supplied by TSF for the duration of the
        // call; null is checked before each read, and GUID is POD.
        let profile = unsafe {
            let ours = !clsid.is_null()
                && *clsid == crate::platforms::windows::tsf::CLSID_BUTTRE_TEXT_SERVICE;
            (ours && !guidprofile.is_null()).then(|| *guidprofile)
        };
        let Some(profile) = profile else {
            return Ok(());
        };

        debug!(
            "OnActivated(ours): profile={profile:?} factivated={}",
            factivated.as_bool()
        );
        if factivated.as_bool() {
            self.this.adopt_profile(&profile);
        }
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
        let changed = unsafe { (!rguid.is_null()).then(|| *rguid) };
        if changed == Some(switching::GUID_COMPARTMENT_BUTTRE_ENABLED) {
            self.this.refresh_enabled();
            return Ok(());
        }
        if changed != Some(GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) {
            return Ok(());
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
                self.this.hide_candidates();
            }
        }
        Ok(())
    }
}

// Helper functions for key handling
/// Virtual key of the word-toggle chord's trailing key (`Ctrl+Shift+Z`).
///
/// Must stay in sync with `buttre_core::hotkey::manager`'s `Code::KeyZ`
/// registration. The two are mutually exclusive at runtime, not duplicated:
/// under TSF the tray process SKIPS registering the global hotkey precisely so
/// the keystroke reaches this in-process text service (`RegisterHotKey` would
/// otherwise swallow it before the focused app's IME ever saw it) — see
/// `WindowsBackend::owns_word_toggle_chord`.
const VK_WORD_TOGGLE: u16 = 0x5A; // 'Z'

const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_PRIOR: u16 = 0x21; // Page Up
const VK_NEXT: u16 = 0x22; // Page Down
const VK_UP: u16 = 0x26;
const VK_DOWN: u16 = 0x28;
const VK_DIGIT_1: u16 = 0x31;
const VK_DIGIT_9: u16 = 0x39;

/// Clears `TextService::key_busy` when `OnKeyDown` returns, on every path.
struct KeyBusyGuard<'a>(&'a Cell<bool>);

impl Drop for KeyBusyGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// TSF offset (UTF-16 units) of the position `chars` characters into `text`.
/// Clamped to the end of `text`.
fn utf16_offset(text: &str, chars: usize) -> usize {
    text.chars().take(chars).map(char::len_utf16).sum()
}

/// The window the candidate panel must be owned by: the context's view
/// window, else the focused window — Microsoft's documented recipe for IME
/// candidate windows. `None` leaves the panel unowned, as before.
fn candidate_owner(context: &ITfContext) -> Option<HWND> {
    // SAFETY: plain COM calls on a live context TSF handed us; GetFocus has no
    // preconditions and returns a null handle when nothing has focus.
    unsafe {
        let view_window = context
            .GetActiveView()
            .and_then(|view| view.GetWnd())
            .ok()
            .filter(|hwnd| !hwnd.is_invalid());
        view_window.or_else(|| {
            let focus = windows::Win32::UI::Input::KeyboardAndMouse::GetFocus();
            (!focus.is_invalid()).then_some(focus)
        })
    }
}

/// The active keyboard profile, when it is one of buttre's.
fn active_buttre_profile() -> Option<GUID> {
    // SAFETY: CoCreateInstance on a system CLSID in a thread TSF has already
    // initialised COM for; GetActiveProfile writes into a local POD struct.
    unsafe {
        let manager: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut profile = TF_INPUTPROCESSORPROFILE::default();
        manager
            .GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile)
            .ok()?;
        (profile.clsid == crate::platforms::windows::tsf::CLSID_BUTTRE_TEXT_SERVICE)
            .then_some(profile.guidProfile)
    }
}

/// Tell buttre-core that its data files sit next to THIS DLL, not next to the
/// host application's executable.
///
/// Idempotent (`set_resource_dir` keeps the first value), so every
/// `TextService` may call it. A failure to locate our own module is logged and
/// ignored: the default `current_exe()` search then applies, which is what
/// happened before this existed.
fn pin_resource_dir() {
    match crate::platforms::windows::tsf::registration::get_dll_path() {
        Ok(dll) => {
            if let Some(dir) = dll.parent() {
                buttre_core::vietnamese::set_resource_dir(dir.to_path_buf());
            }
        }
        Err(e) => tracing::warn!("could not locate the buttre DLL, data files may be missing: {e}"),
    }
}

/// Screen position under the system caret, as a fallback anchor for the
/// candidate panel.
///
/// `None` in the many modern applications that draw their own caret and never
/// create a real one (Word, Chrome) — there `hwndCaret` is null and there is
/// nothing to report. It still covers plain Win32 editors, which is exactly
/// where the composition extent is least likely to be ready in time.
fn system_caret_pos() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};

    // SAFETY: `info` is zero-initialised with its cbSize set, as GetGUIThreadInfo
    // requires; thread id 0 asks about the foreground thread. ClientToScreen is
    // called only after hwndCaret is confirmed non-null.
    unsafe {
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(0, &mut info).ok()?;
        if info.hwndCaret.is_invalid() {
            return None;
        }
        let mut point = POINT {
            x: info.rcCaret.left,
            y: info.rcCaret.bottom,
        };
        // BOOL, not Result: `.ok()` here is the BOOL->Result conversion.
        if windows::Win32::Graphics::Gdi::ClientToScreen(info.hwndCaret, &mut point)
            .ok()
            .is_err()
        {
            return None;
        }
        Some((point.x, point.y + 2))
    }
}

/// Which way this key moves the candidate highlight, if it moves it at all.
fn candidate_motion(vkey: u16) -> Option<CandidateMotion> {
    match vkey {
        VK_UP => Some(CandidateMotion::Prev),
        VK_DOWN => Some(CandidateMotion::Next),
        VK_PRIOR => Some(CandidateMotion::PageUp),
        VK_NEXT => Some(CandidateMotion::PageDown),
        _ => None,
    }
}

/// Keys the candidate popup claims while it is up. `OnTestKeyDown` must report
/// these as handled, or TSF never routes them to `OnKeyDown` and Enter/arrows
/// go to the application while the list sits there ignoring them.
fn is_candidate_key(vkey: u16) -> bool {
    matches!(
        vkey,
        VK_RETURN | VK_ESCAPE | VK_SPACE | VK_PRIOR | VK_NEXT | VK_UP | VK_DOWN
    ) || matches!(vkey, VK_DIGIT_1..=VK_DIGIT_9)
}

/// The `Ctrl+Shift+<digit>` switch command `vkey` forms with the modifiers
/// held right now, if any.
fn switch_command_for(vkey: u16) -> Option<SwitchCommand> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardState, VK_CONTROL, VK_MENU, VK_SHIFT,
    };
    let mut state = [0u8; 256];
    // SAFETY: GetKeyboardState fills a caller-owned 256-byte buffer.
    unsafe { GetKeyboardState(&mut state) }.ok()?;
    let down = |vk: u16| state[usize::from(vk)] & 0x80 != 0;
    switching::switch_command(vkey, down(VK_CONTROL.0), down(VK_SHIFT.0), down(VK_MENU.0))
}

fn is_hotkey(vkey: u16) -> bool {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetKeyboardState, VK_CONTROL, VK_MENU, VK_SHIFT,
        };
        let mut key_state = [0u8; 256];
        if GetKeyboardState(&mut key_state).is_ok() {
            let ctrl = key_state[VK_CONTROL.0 as usize] & (1 << 7) != 0;
            let shift = key_state[VK_SHIFT.0 as usize] & (1 << 7) != 0;
            let alt = key_state[VK_MENU.0 as usize] & (1 << 7) != 0;
            // Toggle key (Ctrl+Space)
            if vkey == 0x20 {
                return ctrl;
            }
            // Word toggle (Ctrl+Shift+Z) — Alt must NOT be held, so
            // Ctrl+Alt+Shift+Z stays the host app's shortcut.
            if vkey == VK_WORD_TOGGLE {
                return ctrl && shift && !alt;
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

/// Printable keys the engine transforms.
fn is_printable_key(vkey: u16) -> bool {
    matches!(vkey,
        0x41..=0x5A |  // A-Z
        0x30..=0x39 |  // 0-9
        0x20 |         // Space
        0xBA..=0xC0 |  // OEM punctuation
        0xDB..=0xDF    // More OEM keys
    )
}

/// Will the text service EAT this key?
///
/// That is the only question `OnTestKeyDown` answers — NOT "would the text
/// service like to look at it". A key claimed here never reaches the
/// application, whatever `OnKeyDown` decides afterwards, so the two must agree.
///
/// Kept pure and separate because getting it wrong is invisible in review and
/// obvious only to whoever is typing:
///
/// * Buffer-reset keys were claimed unconditionally, so the ARROW KEYS stopped
///   moving the caret anywhere in the editor. `OnKeyDown` returned `BOOL(0)`
///   for them, but that came too late to hand the key back.
/// * With no keyboard loaded (the tray's "english", or a custom layout that
///   failed to load) `OnKeyDown` declines EVERY key — so claiming any of them
///   would swallow the user's typing outright.
///
/// Navigation and editing keys are now claimed by NOTHING: the caret moves in
/// the application. Mid-word they first commit the composition
/// (`interrupts_composition`, from `OnTestKeyDown`). Escape and Backspace are
/// the exceptions while composing — Escape cancels like every IME does,
/// Backspace must edit the ENGINE's keystrokes: left to the application it
/// deleted a character the engine still held, and the next tone key brought
/// it back.
fn claims_key(vkey: u16, engine_active: bool, composing: bool, candidates_open: bool) -> bool {
    if !engine_active {
        return false;
    }
    is_printable_key(vkey)
        || (composing && matches!(vkey, VK_ESCAPE | VK_BACK))
        || (candidates_open && is_candidate_key(vkey))
}

/// Must the open composition be committed before the application handles
/// this (unclaimed) key?
///
/// Every key that reaches the application mid-word acts at the caret, which
/// sits inside or at the end of the composition: Enter's newline, a Tab, a
/// paste, a Delete would land inside it and be overwritten by the next
/// composition update, and the engine would carry the word's keystrokes into
/// the next one. Only keys that do nothing on their own are exempt —
/// modifiers (Shift must not commit mid-word), lock keys and IME-mode keys.
fn interrupts_composition(vkey: u16) -> bool {
    !matches!(vkey,
        0x10..=0x12 |  // VK_SHIFT, VK_CONTROL, VK_MENU
        0x14 |         // VK_CAPITAL
        0x15..=0x1A |  // IME mode keys (kana/hangul, IME on/off, junja, final, kanji)
        0x1C..=0x1F |  // VK_CONVERT .. VK_MODECHANGE
        0x5B | 0x5C |  // VK_LWIN, VK_RWIN
        0x90 | 0x91 |  // VK_NUMLOCK, VK_SCROLL
        0xA0..=0xA5 |  // left/right Shift, Ctrl, Alt
        0xE5           // VK_PROCESSKEY
    )
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> Result<()> {
        debug!("ITfKeyEventSink::OnSetFocus");
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        pic: Ref<'_, ITfContext>,
        wParam: WPARAM,
        _lParam: LPARAM,
    ) -> Result<BOOL> {
        let vkey = wParam.0 as u16;

        // Ctrl+Shift+0…9 is always ours, on or off — it is how buttre is
        // turned back on. Carried out in `OnKeyDown`, never here: this is a
        // question, and hosts may ask it more than once per key.
        if switch_command_for(vkey).is_some() {
            return Ok(BOOL(1));
        }

        let engine_active = self.this.is_typing_active();
        let composing = self.this.composition.is_started();
        let candidates_open = !self.this.vietnamese_engine.borrow().candidates().is_empty();
        // Ctrl/Alt chords belong to the application (except our own hotkeys).
        let eaten =
            !should_ignore(vkey) && claims_key(vkey, engine_active, composing, candidates_open);

        debug!(
            "OnTestKeyDown: vkey={:?}, active={}, composing={}, eaten={}",
            vkey, engine_active, composing, eaten
        );

        // This is the last moment before the application sees the key —
        // `OnKeyDown` never runs for a key not claimed here.
        if !eaten && composing && interrupts_composition(vkey) {
            if let Some(context) = (*pic).clone() {
                self.finalize_composition(&context, true);
            }
        }

        Ok(BOOL(eaten as i32))
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

        if let Some(command) = switch_command_for(vkey) {
            debug!("Switch command {:?}", command);
            self.this.apply_switch(command);
            return Ok(BOOL(1));
        }

        // The candidate popup gets first refusal — several of its keys (Escape,
        // Page Up/Down) are also buffer-reset keys, and while a list is up they
        // mean something else entirely.
        //
        // `should_ignore` still comes first: Ctrl+1 and Alt+Space belong to the
        // application even with a list open, and an IME that ate them would
        // break every shortcut the user has while typing Nôm.
        let candidates_open = !self.this.vietnamese_engine.borrow().candidates().is_empty();
        if candidates_open && !should_ignore(vkey) {
            if let Some(context) = (*pic).clone() {
                let sink: ITfCompositionSink = {
                    let r: InterfaceRef<'_, ITfCompositionSink> = self.as_interface_ref();
                    r.to_owned()
                };
                if self.this.handle_candidate_key(&context, vkey, &sink) {
                    return Ok(BOOL(1));
                }
            }
        }

        // Some hosts (Notepad, measured: 0 `OnTestKeyDown` against 459
        // `OnKeyDown`) never ask `OnTestKeyDown` and send EVERY key here, so
        // the return value is what decides whether the key is eaten. Apply the
        // same rule `OnTestKeyDown` would have: a key we do not claim goes back
        // to the application untouched — after committing the word if it acts
        // on the document. Eating it instead swallowed every Backspace typed
        // after a word had been committed. In hosts that do call
        // `OnTestKeyDown`, an unclaimed key never reaches this point.
        let engine_active = self.this.is_typing_active();
        let composing = self.this.composition.is_started();
        if should_ignore(vkey) || !claims_key(vkey, engine_active, composing, candidates_open) {
            if composing && interrupts_composition(vkey) {
                if let Some(context) = (*pic).clone() {
                    self.finalize_composition(&context, true);
                }
            }
            return Ok(BOOL(0));
        }

        // Escape while composing, or a popup key the candidate list declined:
        // commit the word, keeping its text in the document.
        if is_buffer_reset_key(vkey) {
            debug!("Buffer reset key (vkey={}), committing the word", vkey);
            match (*pic).clone() {
                Some(context) => self.finalize_composition(&context, false),
                None => {
                    self.this.vietnamese_engine.borrow_mut().reset();
                    self.this.hide_candidates();
                }
            }
            return Ok(BOOL(0));
        }

        let context: ITfContext = (*pic).clone().ok_or(E_FAIL)?;

        // Mark mid-keystroke so OnSetFocus doesn't misfire a spurious
        // doc-switch commit during this key event. Cleared when this call
        // returns, on every path: `OnKeyUp` never runs (`OnTestKeyUp` claims
        // nothing), and clearing there left the flag stuck after the first
        // key — which silently disabled ending the composition on focus loss.
        self.this.key_busy.set(true);
        let _key_busy = KeyBusyGuard(&self.this.key_busy);
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

        // Word toggle (Ctrl+Shift+Z): flip the open composition between the
        // literal keystrokes and the composed form. Handled HERE, in-process,
        // rather than via the tray's global hotkey — `RegisterHotKey` delivers
        // to the registering thread and withholds the key from the focused
        // app, so a TSF text service can only see the chord when the tray
        // leaves it unregistered (`WindowsBackend::owns_word_toggle_chord`).
        //
        // Falls through (BOOL(0)) when there is no composition to act on, so
        // the host app's own Ctrl+Shift+Z ("redo" in many editors) still works
        // whenever we have nothing to toggle.
        if ctrl_pressed && shift_pressed && vkey == VK_WORD_TOGGLE {
            match self
                .this
                .vietnamese_engine
                .borrow_mut()
                .toggle_composition()
            {
                Some(Action::UpdateComposition { text, cursor }) => {
                    debug!("Word toggle: {} char(s)", text.chars().count());
                    if let Err(e) = self.this.write_text(&context, &text, cursor, sink) {
                        debug!("Failed to write word-toggle text: {:?}", e);
                        return Ok(BOOL(0));
                    }
                    return Ok(BOOL(1));
                }
                _ => return Ok(BOOL(0)),
            }
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
                // `Replace`'s payload is a DELTA — "delete N, then insert this
                // tail" — which is the hook backend's contract, not this one.
                // A composition is rewritten whole, so the delta's `text` is
                // the wrong thing to write: deleting the last letter of "Tie"
                // yields `Replace { backspace_count: 1, text: "" }`, and
                // writing that emptied the composition. An empty composition
                // makes the application terminate it (Notepad does, reliably),
                // which fires `OnCompositionTerminated` and RESETS THE ENGINE
                // mid-word — after which a tone key composed against the
                // leftover keystrokes only, losing the letters before it.
                //
                // The engine's buffer already holds the full post-backspace
                // text. Use that.
                // The key is claimed (`claims_key`), so it is eaten whatever
                // this returns — every path must leave screen and engine in
                // agreement.
                let action = self.this.vietnamese_engine.borrow_mut().process_backspace();
                if !matches!(action, Action::Replace { .. }) {
                    // The engine holds nothing to delete while a composition
                    // is showing: they disagree. Commit what is on screen so
                    // the next Backspace goes to the application.
                    debug!("Backspace: engine had nothing to delete, committing the composition");
                    self.finalize_composition(&context, false);
                    return Ok(BOOL(1));
                }
                let text = self.this.vietnamese_engine.borrow().buffer_content();
                debug!(
                    "Backspace: composition now {} char(s)",
                    text.chars().count()
                );

                if text.is_empty() {
                    // The last character was deleted: clear it from the
                    // composition, then close it. Ending without clearing
                    // committed that character instead of deleting it. The
                    // application may terminate the emptied composition
                    // itself; that only resets an engine already empty.
                    if let Err(e) = self.this.write_text(&context, "", 0, sink.clone()) {
                        debug!("Failed to clear the composition: {:?}", e);
                    }
                    if let Err(e) = self.this.end_composition(&context) {
                        debug!("Failed to end emptied composition: {:?}", e);
                    }
                    return Ok(BOOL(1));
                }

                if let Err(e) =
                    self.this
                        .write_text(&context, &text, text.chars().count(), sink.clone())
                {
                    debug!("Failed to write text: {:?}", e);
                    return Ok(BOOL(0));
                }
                Ok(BOOL(1))
            }
            // Enter normally never gets here: unclaimed, it commits the word in
            // `OnTestKeyDown` and goes to the application. Only a candidate
            // popup with nothing to select routes it here.
            else if ch == '\r' {
                self.finalize_composition(&context, false);
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
                            self.this.hide_candidates();

                            handled = BOOL(1);
                        }
                        Action::Commit(text) => {
                            debug!("Commit: {} char(s)", text.chars().count());
                            // Close any composition FIRST — inserting at the
                            // selection while one is open would land inside it.
                            if self.this.composition.is_started() {
                                if let Err(e) = self.this.end_composition(&context) {
                                    debug!("Failed to end composition: {:?}", e);
                                }
                            }
                            // Insert directly, never as a composition: see
                            // `insert_text` for the race this avoids.
                            if let Err(e) = self.this.insert_text(&context, &text) {
                                debug!("Failed to insert committed text: {:?}", e);
                            }
                            self.this.vietnamese_engine.borrow_mut().reset();
                            self.this.hide_candidates();
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
                        // The engine already absorbed the payload into its
                        // `CandidateState` (see `VietnameseEngine::process_key`)
                        // — these arms only trigger the repaint, and the panel
                        // reads the state so the highlight survives.
                        Action::ShowCandidates { .. } => {
                            self.this.refresh_candidates(&context);
                            handled = BOOL(1);
                        }
                        Action::HideCandidates => {
                            self.this.hide_candidates();
                            handled = BOOL(1);
                        }
                    }
                }
                // The key was claimed, so the application will never see it:
                // if no action used it, type it ourselves rather than lose it.
                if handled == BOOL(0) {
                    debug!(
                        "Engine declined claimed key '{}', inserting it as typed",
                        ch
                    );
                    self.finalize_composition(&context, false);
                    if let Err(e) = self.this.insert_text(&context, ch.encode_utf8(&mut [0; 4])) {
                        debug!("Failed to insert declined key: {:?}", e);
                    }
                    handled = BOOL(1);
                }
                Ok(handled)
            }
        } else {
            // Not a printable key, pass through
            Ok(BOOL(0))
        }
    }

    fn OnKeyUp(&self, _pic: Ref<'_, ITfContext>, _wParam: WPARAM, _lParam: LPARAM) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnPreservedKey(&self, _pic: Ref<'_, ITfContext>, _rguid: *const GUID) -> Result<BOOL> {
        Ok(BOOL(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The two tables below decide which keys the candidate popup gets. A wrong
    // virtual-key code here fails SILENTLY: the key reaches the application
    // instead of the popup, and the only symptom is a list that ignores Enter.

    #[test]
    fn candidate_keys_and_motions_agree() {
        for vkey in [VK_UP, VK_DOWN, VK_PRIOR, VK_NEXT] {
            assert!(
                candidate_motion(vkey).is_some(),
                "vkey {vkey:#x} should move the highlight"
            );
            assert!(is_candidate_key(vkey), "vkey {vkey:#x} must be claimed");
        }
        for vkey in [VK_RETURN, VK_ESCAPE, VK_SPACE] {
            assert!(
                candidate_motion(vkey).is_none(),
                "vkey {vkey:#x} selects or dismisses, it does not navigate"
            );
            assert!(is_candidate_key(vkey), "vkey {vkey:#x} must be claimed");
        }
    }

    #[test]
    fn every_selection_digit_is_claimed() {
        for (offset, vkey) in (VK_DIGIT_1..=VK_DIGIT_9).enumerate() {
            assert!(is_candidate_key(vkey), "digit {} unclaimed", offset + 1);
        }
        // '0' is NOT a selection key — the page holds nine, numbered 1..9.
        assert!(!is_candidate_key(0x30));
    }

    #[test]
    fn ordinary_letters_are_left_alone() {
        // Typing must keep working while the list is up: a letter extends the
        // reading and re-runs the lookup.
        for vkey in [0x41u16, 0x5A, VK_WORD_TOGGLE] {
            assert!(!is_candidate_key(vkey), "vkey {vkey:#x} must fall through");
        }
    }

    // ── What the text service eats ───────────────────────────────────────────
    // The regression these pin: arrow keys stopped moving the caret ANYWHERE in
    // the editor, because a claimed key is gone whatever OnKeyDown answers.

    const ARROWS: [u16; 4] = [0x25, 0x26, 0x27, 0x28];

    #[test]
    fn navigation_keys_reach_the_application_when_nothing_is_composing() {
        for vkey in ARROWS
            .into_iter()
            .chain([0x23, 0x24, VK_PRIOR, VK_NEXT, 0x2E])
        {
            assert!(
                !claims_key(vkey, true, false, false),
                "vkey {vkey:#x} must move the caret when no composition is open"
            );
        }
    }

    #[test]
    fn navigation_keys_move_the_caret_even_mid_word() {
        // The composition is closed by `ITfTextEditSink::OnEndEdit` after the
        // app has moved the caret, so the key itself is never consumed — no
        // "press the arrow twice" surprise.
        for vkey in ARROWS.into_iter().chain([0x23, 0x24, VK_PRIOR, VK_NEXT]) {
            assert!(
                !claims_key(vkey, true, true, false),
                "vkey {vkey:#x} must reach the app even with a composition open"
            );
        }
    }

    #[test]
    fn editing_keys_are_never_claimed() {
        // Delete, Insert, Tab, F-keys: whatever they do to the document, the
        // edit sink reports it. Eating them broke the editor.
        for vkey in [0x2Eu16, 0x2D, 0x09, 0x70, 0x7B] {
            assert!(!claims_key(vkey, true, true, false), "vkey {vkey:#x}");
        }
    }

    #[test]
    fn escape_is_claimed_only_while_composing() {
        assert!(claims_key(VK_ESCAPE, true, true, false));
        assert!(
            !claims_key(VK_ESCAPE, true, false, false),
            "with nothing composing, Escape belongs to the application"
        );
    }

    /// 𡗶 is one character but two UTF-16 units: the caret after it must be
    /// offset 2, or it sits between the surrogate halves and the next key
    /// splits the character.
    #[test]
    fn caret_offsets_count_utf16_units() {
        assert_eq!(utf16_offset("𡗶", 1), 2);
        assert_eq!(utf16_offset("a𡗶b", 2), 3);
        assert_eq!(utf16_offset("trời", 4), 4);
        assert_eq!(utf16_offset("𡗶", 5), 2, "clamped to the end");
    }

    /// Backspace mid-word must edit the engine's keystrokes; left to the
    /// application it deleted a letter the engine still held, and the next
    /// tone key brought it back.
    #[test]
    fn backspace_is_claimed_only_while_composing() {
        assert!(claims_key(VK_BACK, true, true, false));
        assert!(
            !claims_key(VK_BACK, true, false, false),
            "with nothing composing, Backspace deletes text in the application"
        );
        assert!(!claims_key(VK_BACK, false, true, false));
    }

    /// Keys that act on the document must commit the word before the
    /// application gets them, or their effect lands inside the composition.
    #[test]
    fn acting_keys_interrupt_the_composition() {
        // Enter, Tab, Delete, arrows, Home/End, F5, 'V' (Ctrl+V), VK_PACKET
        for vkey in [
            VK_RETURN, 0x09, 0x2E, 0x25, 0x27, 0x24, 0x23, 0x74, 0x56, 0xE7,
        ] {
            assert!(interrupts_composition(vkey), "vkey {vkey:#x}");
        }
    }

    /// Shift held for a capital letter mid-word must not commit the word.
    #[test]
    fn modifier_and_lock_keys_do_not_interrupt_the_composition() {
        for vkey in [
            0x10u16, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA5, 0x14, 0x5B, 0x90, 0xE5,
        ] {
            assert!(!interrupts_composition(vkey), "vkey {vkey:#x}");
        }
    }

    #[test]
    fn an_inactive_engine_claims_nothing() {
        // English, or a custom layout that failed to load: OnKeyDown declines
        // every key, so claiming any would swallow the user's typing.
        for vkey in [0x41u16, 0x30, VK_SPACE, VK_ESCAPE]
            .into_iter()
            .chain(ARROWS)
        {
            assert!(
                !claims_key(vkey, false, true, true),
                "vkey {vkey:#x} must pass through when no keyboard is loaded"
            );
        }
    }

    #[test]
    fn letters_are_always_claimed_while_active() {
        for vkey in [0x41u16, 0x5A, 0x30, 0x39, VK_SPACE] {
            assert!(claims_key(vkey, true, false, false));
        }
    }

    #[test]
    fn the_popup_claims_its_keys_even_with_no_composition() {
        // Defensive: candidates should never outlive the composition, but if
        // they somehow do, the digits must still pick from the list rather than
        // type themselves into the document.
        assert!(claims_key(VK_DIGIT_1, true, false, true));
        assert!(claims_key(VK_ESCAPE, true, false, true));
        assert!(!claims_key(VK_ESCAPE, true, false, false));
    }

    #[test]
    fn the_popup_wins_over_the_reset_meaning_of_its_keys() {
        // Escape, Page Up and Page Down are also buffer-reset keys. While a
        // candidate list is up they must dismiss and paginate instead — so they
        // have to be claimed, and `handle_candidate_key` has to run before
        // OnKeyDown's reset branch.
        for vkey in [VK_ESCAPE, VK_PRIOR, VK_NEXT] {
            assert!(is_buffer_reset_key(vkey));
            assert!(is_candidate_key(vkey));
            assert!(claims_key(vkey, true, true, true), "vkey {vkey:#x}");
        }
    }
}
