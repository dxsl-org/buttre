// SPDX-License-Identifier: GPL-3.0-only
// Edit Sessions for TSF Text Modification
// 
// **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/platform_windows_tsf_tests.rs`.
// 
// Based on windows-chewing-tsf approach (4 edit sessions instead of 6)

use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ptr;
use std::rc::Rc;
use std::cell::RefCell;

use tracing::{debug, error};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::*;

/// Helper function to set selection in a context
/// 
/// This is used by all edit sessions to position the cursor
fn set_selection(
    context: &ITfContext,
    ec: u32,
    range: ITfRange,
    active_sel_end: TfActiveSelEnd,
) -> Result<()> {
    let mut selections = [TF_SELECTION::default(); 1];
    selections[0].range = ManuallyDrop::new(Some(range));
    selections[0].style.ase = active_sel_end;
    selections[0].style.fInterimChar = BOOL(0);
    
    let result = unsafe { context.SetSelection(ec, &selections) };
    
    // Cleanup
    let [TF_SELECTION { range, .. }] = selections;
    let _ = ManuallyDrop::into_inner(range);
    
    result
}

/// Edit Session 1: Insert Text at Selection
/// 
/// Simple text insertion without composition
#[implement(ITfEditSession)]
pub struct InsertText {
    context: ITfContext,
    text: HSTRING,
}

impl InsertText {
    pub fn new(context: ITfContext, text: HSTRING) -> Self {
        debug!("Creating InsertText edit session: text={}", text);
        Self { context, text }
    }
}

impl ITfEditSession_Impl for InsertText_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        debug!("InsertText::DoEditSession - ec={}", ec);
        
        // SAFETY:
        // 1. Called by TSF framework with valid edit cookie (ec)
        // 2. context is a valid ITfContext interface
        // 3. cast() to ITfInsertAtSelection is safe - same COM object
        // 4. InsertTextAtSelection is a COM method with proper error handling
        // 5. ec is the edit cookie provided by TSF - valid for this edit session
        // 6. All COM methods check for errors with ?
        unsafe {
            let insert_at_selection: ITfInsertAtSelection = self.this.context.cast()?;
            
            // Insert text at current selection
            let range = insert_at_selection.InsertTextAtSelection(
                ec,
                INSERT_TEXT_AT_SELECTION_FLAGS(0),
                &self.this.text,
            )?;
            
            // Collapse range to end (move cursor after inserted text)
            range.Collapse(ec, TF_ANCHOR_END)?;
            
            // Set selection at end
            set_selection(&self.this.context, ec, range, TF_AE_END)?;
            
            debug!("InsertText::DoEditSession - completed");
        }
        
        Ok(())
    }
}

use super::composition::{Composition, PendingComposition};

/// Edit Session 2: Set Composition String
/// 
/// This combines Start + Update + InlinePreedit from Weasel
/// Handles both starting new composition and updating existing one
#[implement(ITfEditSession)]
pub struct SetCompositionString {
    context: ITfContext,
    composition: Composition,
    composition_sink: ITfCompositionSink,
    da_atom: VARIANT,
    pending: Rc<RefCell<PendingComposition>>,
}

impl SetCompositionString {
    pub fn new(
        context: ITfContext,
        composition: Composition,
        composition_sink: ITfCompositionSink,
        da_atom: VARIANT,
        pending: Rc<RefCell<PendingComposition>>,
    ) -> Self {
        debug!("Creating SetCompositionString edit session");
        Self {
            context,
            composition,
            composition_sink,
            da_atom,
            pending,
        }
    }
}

impl ITfEditSession_Impl for SetCompositionString_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        debug!("SetCompositionString::DoEditSession - ec={}", ec);
        
        // SAFETY:
        // 1. Called by TSF framework with valid edit cookie (ec)
        // 2. All context casts are safe - same COM object, different interfaces
        // 3. InsertTextAtSelection with TF_IAS_QUERYONLY just queries, doesn't modify
        // 4. StartComposition creates a composition with proper lifetime management
        // 5. SetText, SetValue, and other methods use edit cookie for synchronization
        // 6. ptr::null() for ShiftEnd/ShiftStart is valid (null GUID pointer)
        // 7. All COM methods check for errors with ?
        unsafe {
            // 1. Start composition if not already started
            if !self.this.composition.is_started() {
                // Recovery mode (Chrome-omnibox class): the app terminated our
                // previous composition while its chars were already committed to
                // the document. `previous_length` (char count) is how many chars
                // the app committed. We shift the current selection back by that
                // amount and overwrite in place, rather than starting a fresh
                // composition at the current caret — which would insert the
                // transformed char *after* the already-committed chars ("dđ").
                let previous_length = self.this.pending.borrow().previous_length;
                if previous_length > 0 {
                    debug!("Recovery: replacing {} committed chars without composition", previous_length);

                    let mut sel_buf = [TF_SELECTION::default(); 1];
                    let mut sel_len = 0u32;
                    let sel_ok = self.this.context.GetSelection(
                        ec,
                        TF_DEFAULT_SELECTION,
                        &mut sel_buf,
                        &mut sel_len,
                    );

                    // NOTE: every fallible COM call below is checked with
                    // `.is_ok()`/`if let Ok` rather than `?` — `?` would
                    // return from `DoEditSession` before the COM-reference
                    // cleanup two lines down runs, leaking `sel_buf`'s
                    // `ITfRange` on any failure (review MED).
                    let mut recovery_ok = false;
                    if sel_ok.is_ok() && sel_len > 0 {
                        if let Some(cur_range) = sel_buf[0].range.deref().as_ref() {
                            if let Ok(replace_range) = cur_range.Clone() {
                                let mut moved = 0i32;
                                // Extend backwards to cover the committed chars.
                                let _ = replace_range.ShiftStart(
                                    ec,
                                    -(previous_length as i32),
                                    &mut moved,
                                    ptr::null(),
                                );
                                let text_set = {
                                    let pending = self.this.pending.borrow();
                                    replace_range.SetText(ec, 0, &pending.text).is_ok()
                                };
                                if text_set
                                    && replace_range.Collapse(ec, TF_ANCHOR_END).is_ok()
                                    && set_selection(&self.this.context, ec, replace_range, TF_AE_END).is_ok()
                                {
                                    recovery_ok = true;
                                }
                            }
                        }
                    }

                    // Release the selection's COM reference — always reached,
                    // since no `?` runs above.
                    let [TF_SELECTION { range, .. }] = sel_buf;
                    let _ = ManuallyDrop::into_inner(range);

                    if recovery_ok {
                        debug!("Recovery: completed successfully");
                        return Ok(());
                    }
                    // Recovery failed — fall through to start a fresh composition.
                }

                debug!("Starting new composition");

                let context_composition: ITfContextComposition = self.this.context.cast()?;
                let insert_at_selection: ITfInsertAtSelection = self.this.context.cast()?;
                
                // Get insertion point
                let range = insert_at_selection.InsertTextAtSelection(
                    ec, 
                    TF_IAS_QUERYONLY, 
                    &[]
                )?;
                
                // Start composition
                // Note: MS docs say pSink is optional, but it fails if NULL
                let composition = context_composition.StartComposition(
                    ec, 
                    &range, 
                    &self.this.composition_sink
                )?;
                
                self.this.composition.set(composition);
                debug!("Composition started successfully");
            }
            
            // 2. Update composition text and cursor
            if let Some(composition) = self.this.composition.get() {
                let pending = self.this.pending.borrow();
                debug!("Updating composition: text={}, cursor={}", pending.text, pending.cursor);
                
                // Get composition range
                let range = composition.GetRange()?;
                
                // Set composition text
                if let Err(error) = range.SetText(ec, 0, &pending.text) {
                    error!("Failed to set composition text: {}", error);
                    return Err(error);
                }
                
                // Set display attribute (underline, etc.)
                let disp_attr_prop = self.this.context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
                if let Err(error) = disp_attr_prop.SetValue(ec, &range, &self.this.da_atom) {
                    error!("Failed to set display attribute: {}", error);
                    // Non-fatal, continue
                }
                
                // Set cursor position: collapse to start, shift end forward by cursor chars,
                // then shift start to match end so we get a zero-width range at the cursor.
                // Use the actual 'moved' value from ShiftEnd so ShiftStart never overshoots.
                let cursor_range = range.Clone()?;
                let mut moved = 0;
                cursor_range.Collapse(ec, TF_ANCHOR_START)?;
                cursor_range.ShiftEnd(ec, pending.cursor as i32, &mut moved, ptr::null())?;
                let end_offset = moved;
                cursor_range.ShiftStart(ec, end_offset, &mut moved, ptr::null())?;
                
                set_selection(&self.this.context, ec, cursor_range, TF_AE_END)?;
                
                debug!("Composition updated successfully");
            }
        }
        
        Ok(())
    }
}

/// Edit Session 3: End Composition
/// 
/// Terminates the composition and commits the text
#[implement(ITfEditSession)]
pub struct EndComposition {
    context: ITfContext,
    composition: ITfComposition,
}

impl EndComposition {
    pub fn new(context: ITfContext, composition: ITfComposition) -> Self {
        debug!("Creating EndComposition edit session");
        Self {
            context,
            composition,
        }
    }
}

impl ITfEditSession_Impl for EndComposition_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        debug!("EndComposition::DoEditSession - ec={}", ec);
        
        // SAFETY:
        // 1. Called by TSF framework with valid edit cookie (ec)
        // 2. composition is a valid ITfComposition interface
        // 3. GetRange returns the composition's text range
        // 4. GetProperty retrieves display attribute property (GUID_PROP_ATTRIBUTE is valid)
        // 5. Clear, Collapse, ShiftStart, EndComposition all use ec for synchronization
        // 6. All COM methods check for errors with ?
        unsafe {
            let range = self.this.composition.GetRange()?;
            
            // 1. Clear display attribute
            let disp_attr_prop = self.this.context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
            if let Err(error) = disp_attr_prop.Clear(ec, &range) {
                error!("Failed to clear display attribute: {}", error);
                // Non-fatal, continue
            }
            
            // 2. Collapse composition range to end
            let new_composition_start = range.Clone()?;
            new_composition_start.Collapse(ec, TF_ANCHOR_END)?;
            self.this.composition.ShiftStart(ec, &new_composition_start)?;
            
            // 3. Set selection at end
            set_selection(&self.this.context, ec, new_composition_start, TF_AE_END)?;
            
            // 4. End composition
            self.this.composition.EndComposition(ec)?;
            
            debug!("Composition ended successfully");
        }
        
        Ok(())
    }
}

/// Edit Session 4: Get Selection Rectangle
/// 
/// Gets the screen position of the current selection (for candidate window positioning)
#[implement(ITfEditSession)]
pub struct SelectionRect {
    context: ITfContext,
    rect: Cell<RECT>,
}

impl SelectionRect {
    pub fn new(context: ITfContext) -> Self {
        debug!("Creating SelectionRect edit session");
        Self {
            context,
            rect: Cell::default(),
        }
    }
    
    /// Get the rectangle after DoEditSession completes
    pub fn rect(&self) -> RECT {
        self.rect.get()
    }
}

impl ITfEditSession_Impl for SelectionRect_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        debug!("SelectionRect::DoEditSession - ec={}", ec);
        
        let mut selection = [TF_SELECTION::default(); 1];
        let mut selection_len = 0;
        
        // SAFETY:
        // 1. Called by TSF framework with valid edit cookie (ec)
        // 2. selection is a valid array on the stack
        // 3. GetSelection fills the array with current selection
        // 4. TF_DEFAULT_SELECTION is a valid constant
        // 5. GetActiveView returns the active view for the context
        // 6. RECT and BOOL are POD types - default() creates valid instances
        // 7. GetTextExt writes to rc and clipped - we provide valid mutable references
        // 8. ManuallyDrop::into_inner properly releases the COM interface
        unsafe {
            self.this.context.GetSelection(
                ec,
                TF_DEFAULT_SELECTION,
                &mut selection,
                &mut selection_len,
            )?;
            
            if let Some(sel_range) = &selection[0].range.deref() {
                let view = self.this.context.GetActiveView()?;
                let mut rc = RECT::default();
                let mut clipped = BOOL::default();
                
                if let Ok(()) = view.GetTextExt(ec, sel_range, &mut rc, &mut clipped) {
                    self.this.rect.set(rc);
                    debug!("Got selection rect: ({}, {}, {}, {})", 
                        rc.left, rc.top, rc.right, rc.bottom);
                } else {
                    error!("Failed to get text extent");
                }
            }
        }
        
        // Cleanup
        let [TF_SELECTION { range, .. }] = selection;
        let _ = ManuallyDrop::into_inner(range);
        
        Ok(())
    }
}

