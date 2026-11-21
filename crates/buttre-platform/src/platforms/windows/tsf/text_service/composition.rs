// SPDX-License-Identifier: GPL-3.0-only
// Composition State Management for buttre TSF
//
// **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/platform_windows_tsf_tests.rs`.

use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::UI::TextServices::ITfComposition;

/// Shared composition state
///
/// This holds the current TSF composition object.
/// It is shared between the TextService and EditSessions.
#[derive(Clone, Default)]
pub struct Composition(Rc<RefCell<Option<ITfComposition>>>);

impl Composition {
    /// Create a new empty composition state
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(None)))
    }

    /// Check if a composition is currently active
    pub fn is_started(&self) -> bool {
        self.0.borrow().is_some()
    }

    /// Get the inner composition object (cloned)
    pub fn get(&self) -> Option<ITfComposition> {
        self.0.borrow().clone()
    }

    /// Set the current composition object
    pub fn set(&self, composition: ITfComposition) {
        self.0.replace(Some(composition));
    }

    /// Clear the current composition (e.g. when ended)
    pub fn clear(&self) {
        self.0.replace(None);
    }

    /// Access the inner Rc/RefCell (for edit sessions)
    /// This exposes the internal implementation to allow edit sessions to modify it directly
    pub fn inner(&self) -> &Rc<RefCell<Option<ITfComposition>>> {
        &self.0
    }
}

use windows::core::HSTRING;

/// Pending composition data
///
/// Holds the text and cursor position that we want to apply to the document.
/// Both offsets are UTF-16 units — TSF's unit, not Rust `char`s.
#[derive(Clone, Default)]
pub struct PendingComposition {
    pub text: HSTRING,
    /// Caret position inside `text`, in UTF-16 units.
    pub cursor: usize,
    /// Length of the previous composition text in UTF-16 units, for the
    /// recovery path that rewrites already-committed text.
    pub previous_length: usize,
}
