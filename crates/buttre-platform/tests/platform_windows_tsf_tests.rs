//! Windows-only integration tests — the whole `platforms::windows` module
//! tree does not exist on other targets.
#![cfg(windows)]

use buttre_core::state::macros::{MacroEntry, MacroFile, MacroStore};
use buttre_core::Action;
use buttre_platform::platforms::windows::tsf::text_service::candidate_ui::{
    CandidateItem, NomCandidateUI,
};
use buttre_platform::platforms::windows::tsf::text_service::composition::{
    Composition, PendingComposition,
};
use buttre_platform::platforms::windows::tsf::text_service::display_attribute::{
    DisplayAttributeInfo, GUID_DISPLAY_ATTRIBUTE_CONVERTED, GUID_DISPLAY_ATTRIBUTE_INPUT,
};
use buttre_platform::platforms::windows::tsf::text_service::vietnamese_engine::{
    VietnameseEngine, VietnameseMode,
};
use buttre_platform::platforms::windows::tsf::{com, logging, CLSID_BUTTRE_TEXT_SERVICE};
use std::sync::{Arc, Mutex};
use windows::core::{GUID, HSTRING};
use windows::Win32::UI::TextServices::ITfDisplayAttributeInfo;

/// An in-memory store with `vn` -> "Việt Nam" — never touches
/// `%APPDATA%`/`macros.toml`, unlike `MacroStore::load`/`load_gated`.
fn vn_macro_store() -> Arc<Mutex<MacroStore>> {
    let mut macros = std::collections::HashMap::new();
    macros.insert(
        "vn".to_string(),
        MacroEntry {
            expand: "Việt Nam".to_string(),
            enabled: true,
        },
    );
    Arc::new(Mutex::new(MacroStore::from_file(MacroFile { macros })))
}

#[test]
fn test_engine_basic() {
    let mut engine = VietnameseEngine::new(VietnameseMode::Telex);

    // Test basic transformation
    let actions = engine.process_key('a');
    // First 'a' should update composition with 'a'
    assert!(actions.iter().all(|a| matches!(
        a,
        Action::UpdateComposition { .. } | Action::Commit(_) | Action::DoNothing
    )));
}

#[test]
fn test_mode_switch() {
    let mut engine = VietnameseEngine::new(VietnameseMode::Telex);

    // Test Telex: a + s -> á
    engine.process_key('a');
    let actions = engine.process_key('s');
    assert!(actions
        .iter()
        .any(|a| matches!(a, Action::UpdateComposition { .. })));
    assert_eq!(engine.buffer_content(), "á");

    // Switch to VNI
    engine.set_mode(VietnameseMode::VNI);
    assert_eq!(engine.buffer_content(), ""); // Should reset

    // Test VNI: a + 1 -> á
    engine.process_key('a');
    let actions = engine.process_key('1');
    assert!(actions
        .iter()
        .any(|a| matches!(a, Action::UpdateComposition { .. })));
    assert_eq!(engine.buffer_content(), "á");
}

/// Regression for issue #4: `Keyboard::process` can return
/// `[ConfirmComposition(word), Commit(separator)]` for a single keystroke
/// (a punctuation/space key that both closes the current word run AND is
/// itself the character typed). `process_key` must surface both actions —
/// dropping the second is exactly how "xin." lost its trailing dot.
#[test]
fn test_process_key_surfaces_confirm_and_trailing_separator() {
    let mut engine = VietnameseEngine::new(VietnameseMode::Telex);
    engine.process_key('x');
    engine.process_key('i');
    engine.process_key('n');
    let actions = engine.process_key('.');

    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::ConfirmComposition(_))),
        "expected a ConfirmComposition action, got {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Commit(text) if text == ".")),
        "the trailing separator must not be dropped, got {actions:?}"
    );
}

/// Phase 3 (wire-shorthand-tsf-linux) success criterion: a TSF engine with a
/// `vn` -> "Việt Nam" store wired in expands on the separator that closes the
/// word, and the separator itself is not swallowed (mirrors
/// `test_process_key_surfaces_confirm_and_trailing_separator` above).
#[test]
fn test_tsf_macro_expands_on_separator() {
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    engine.process_key('v');
    engine.process_key('n');
    let actions = engine.process_key(' ');

    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::ConfirmComposition(text) if text == "Việt Nam")),
        "expected ConfirmComposition(\"Việt Nam\"), got {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Commit(text) if text == " ")),
        "the separator that closed the word must still be committed, got {actions:?}"
    );
}

/// Success criterion: `"vn"` + Enter/boundary commit also expands. TSF's own
/// Enter/reset-key handling in `text_service_stub.rs` queries
/// `boundary_repair()` BEFORE ending the composition, bypassing
/// `process_key`/`ConfirmComposition` entirely — this must independently
/// apply the same macro lookup (see `Keyboard::boundary_repair`).
#[test]
fn test_tsf_macro_expands_on_boundary_repair() {
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    engine.process_key('v');
    engine.process_key('n');

    assert_eq!(
        engine.boundary_repair(),
        Some("Việt Nam".to_string()),
        "Enter-path boundary_repair must expand the still-open \"vn\" run"
    );
}

/// Method switch (Telex<->VNI) must keep expansion working: `set_mode`
/// rebuilds the `Keyboard` but must re-inject the SAME shared macros store.
#[test]
fn test_tsf_macro_survives_mode_switch() {
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    engine.set_mode(VietnameseMode::VNI);

    engine.process_key('v');
    engine.process_key('n');
    let actions = engine.process_key(' ');

    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::ConfirmComposition(text) if text == "Việt Nam")),
        "expansion must survive a Telex->VNI method switch, got {actions:?}"
    );
}

/// No store (shorthand off) must be byte-identical to today: composed
/// passthrough, never an expansion.
#[test]
fn test_tsf_no_macro_store_passes_through() {
    let store = Arc::new(Mutex::new(MacroStore::default()));
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, store);
    engine.process_key('v');
    engine.process_key('n');
    let actions = engine.process_key(' ');

    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::ConfirmComposition(text) if text == "Việt Nam")),
        "an empty/unwired store must never expand, got {actions:?}"
    );
}

#[test]
fn test_reset() {
    let mut engine = VietnameseEngine::new(VietnameseMode::Telex);
    engine.process_key('a');
    engine.reset();
    assert_eq!(engine.buffer_content(), "");
}

#[test]
fn test_pending_composition() {
    let pending = PendingComposition {
        text: HSTRING::from("test"),
        cursor: 2,
        previous_length: 0,
    };
    assert_eq!(pending.cursor, 2);
}

#[test]
fn test_create_attributes() {
    let input: ITfDisplayAttributeInfo = DisplayAttributeInfo::create_input().into();
    // Use GUID comparison
    assert_eq!(
        unsafe { input.GetGUID() }.unwrap(),
        GUID_DISPLAY_ATTRIBUTE_INPUT
    );

    let converted: ITfDisplayAttributeInfo = DisplayAttributeInfo::create_converted().into();
    assert_eq!(
        unsafe { converted.GetGUID() }.unwrap(),
        GUID_DISPLAY_ATTRIBUTE_CONVERTED
    );
}

#[test]
fn test_composition_state() {
    let comp = Composition::new();
    assert!(!comp.is_started());
    assert!(comp.get().is_none());

    comp.clear();
    assert!(!comp.is_started());
}

#[test]
fn test_pending_composition_defaults() {
    let pending = PendingComposition::default();
    assert!(pending.text.is_empty());
    assert_eq!(pending.cursor, 0);
}

fn create_test_candidates() -> Vec<CandidateItem> {
    vec![
        CandidateItem {
            character: '𡦂',
            reading: "người".to_string(),
            meaning: Some("person".to_string()),
            frequency: 1000,
        },
        CandidateItem {
            character: '𠊛',
            reading: "người".to_string(),
            meaning: Some("person (variant)".to_string()),
            frequency: 500,
        },
    ]
}

#[test]
fn test_candidate_ui_creation() {
    let candidates = create_test_candidates();
    let ui = NomCandidateUI::new(candidates);

    // Test basic page info
    assert_eq!(ui.page_count(), 1);
}

#[test]
fn test_page_navigation() {
    let mut candidates = Vec::new();
    for i in 0..20 {
        candidates.push(CandidateItem {
            character: '𡦂',
            reading: format!("test{}", i),
            meaning: None,
            frequency: 100,
        });
    }

    let ui = NomCandidateUI::new(candidates);
    assert_eq!(ui.page_count(), 3); // 20 candidates, 9 per page = 3 pages

    assert!(ui.next_page());
    assert!(ui.prev_page());
}

#[test]
fn test_candidate_selection() {
    let candidates = create_test_candidates();
    let ui = NomCandidateUI::new(candidates);

    let selected = ui.select(0);
    assert!(selected.is_some());
    assert_eq!(selected.unwrap().character, '𡦂');
}

#[test]
fn test_clsid() {
    // Just verify CLSID is valid
    assert_ne!(CLSID_BUTTRE_TEXT_SERVICE, GUID::zeroed());
}

#[test]
fn test_ref_counting() {
    // Note: This modifies global state, but should be safe in test environment
    let initial = com::dll_get_ref_count();
    com::dll_add_ref();
    assert_eq!(com::dll_get_ref_count(), initial + 1);
    com::dll_release();
    assert_eq!(com::dll_get_ref_count(), initial);
}

#[test]
fn test_init_logging() {
    logging::init_logging();
}

#[test]
fn test_log_debug() {
    logging::log_debug("test message");
}
