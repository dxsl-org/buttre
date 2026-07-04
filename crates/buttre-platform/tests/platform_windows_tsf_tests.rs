//! Windows-only integration tests — the whole `platforms::windows` module
//! tree does not exist on other targets.
#![cfg(windows)]

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
use windows::core::{GUID, HSTRING};
use windows::Win32::UI::TextServices::ITfDisplayAttributeInfo;

#[test]
fn test_engine_basic() {
    let mut engine = VietnameseEngine::new(VietnameseMode::Telex);

    // Test basic transformation
    let action = engine.process_key('a');
    // First 'a' should update composition with 'a'
    assert!(matches!(
        action,
        Action::UpdateComposition { .. } | Action::Commit(_) | Action::DoNothing
    ));
}

#[test]
fn test_mode_switch() {
    let mut engine = VietnameseEngine::new(VietnameseMode::Telex);

    // Test Telex: a + s -> á
    engine.process_key('a');
    let action = engine.process_key('s');
    assert!(matches!(action, Action::UpdateComposition { .. }));
    assert_eq!(engine.buffer_content(), "á");

    // Switch to VNI
    engine.set_mode(VietnameseMode::VNI);
    assert_eq!(engine.buffer_content(), ""); // Should reset

    // Test VNI: a + 1 -> á
    engine.process_key('a');
    let action = engine.process_key('1');
    assert!(matches!(action, Action::UpdateComposition { .. }));
    assert_eq!(engine.buffer_content(), "á");
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
