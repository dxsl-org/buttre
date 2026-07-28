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
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());

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
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());

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
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
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
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
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

// ── Word toggle (Ctrl+Shift+Z) ───────────────────────────────────────────────
// The chord itself is intercepted in `text_service_stub::OnKeyDown`, which
// needs a live TSF context; these cover the engine seam that branch calls.

/// Latest composition text the TSF stub would write for these actions.
fn composition_text(actions: &[Action]) -> Option<String> {
    actions.iter().rev().find_map(|a| match a {
        Action::UpdateComposition { text, .. } => Some(text.clone()),
        _ => None,
    })
}

#[test]
fn test_toggle_composition_flips_and_returns_an_update() {
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    for ch in "dads".chars() {
        engine.process_key(ch);
    }
    assert_eq!(engine.buffer_content(), "đá");

    let action = engine
        .toggle_composition()
        .expect("an open composition must be toggleable");
    match action {
        Action::UpdateComposition { text, cursor } => {
            assert_eq!(text, "dads");
            assert_eq!(cursor, text.chars().count());
        }
        other => panic!("expected UpdateComposition, got {other:?}"),
    }

    engine
        .toggle_composition()
        .expect("toggle must be bidirectional");
    assert_eq!(engine.buffer_content(), "đá");
}

#[test]
fn test_toggle_composition_literal_reaches_the_commit() {
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    for ch in "dads".chars() {
        engine.process_key(ch);
    }
    engine.toggle_composition().expect("toggle acts");

    let actions = engine.process_key(' ');
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::ConfirmComposition(t) if t == "dads")),
        "the literal choice must be what the text service confirms: {actions:?}"
    );
}

#[test]
fn test_toggle_composition_freezes_continued_typing() {
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    for ch in "dad".chars() {
        engine.process_key(ch);
    }
    engine.toggle_composition().expect("toggle acts");
    let actions = engine.process_key('s');
    assert_eq!(composition_text(&actions).as_deref(), Some("dads"));
}

#[test]
fn test_toggle_composition_noop_when_nothing_is_composing() {
    // The stub relies on `None` here to fall through, leaving the host app's
    // own Ctrl+Shift+Z ("redo") working when we have no word to act on.
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    assert!(engine.toggle_composition().is_none());
}

// ── Backspace: the composition is rewritten whole, never from the delta ──────

#[test]
fn test_backspace_leaves_the_full_text_in_the_buffer() {
    // What the text service must write after a backspace is the WHOLE new
    // composition, and `buffer_content` is where that lives.
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    for ch in "tie".chars() {
        engine.process_key(ch);
    }
    assert_eq!(engine.buffer_content(), "tie");

    let action = engine.process_backspace();
    assert!(matches!(action, Action::Replace { .. }));
    assert_eq!(
        engine.buffer_content(),
        "ti",
        "the buffer holds the full post-backspace text"
    );
}

#[test]
fn test_backspace_action_is_a_delta_not_the_composition() {
    // Pins the mistake that broke Notepad: `Replace` means "delete N, insert
    // this tail". Deleting one plain letter carries NO text at all, so writing
    // the action's payload as the composition emptied it — and an empty
    // composition makes the application terminate it, resetting the engine
    // mid-word.
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    for ch in "tie".chars() {
        engine.process_key(ch);
    }
    match engine.process_backspace() {
        Action::Replace { text, .. } => assert!(
            text.is_empty(),
            "delta text was '{text}' — if this ever becomes the full string, \
             the stub's use of buffer_content() should be revisited"
        ),
        other => panic!("expected Replace, got {other:?}"),
    }
}

#[test]
fn test_backspacing_the_only_char_empties_the_buffer() {
    // The stub ends the composition instead of writing "" for this case.
    let mut engine = VietnameseEngine::new_with_macros(VietnameseMode::Telex, vn_macro_store());
    engine.process_key('t');
    engine.process_backspace();
    assert!(engine.buffer_content().is_empty());
}

// ── Method selection reaches the text service ───────────────────────────────

use buttre_platform::platforms::windows::tsf::text_service::vietnamese_engine::VietnameseMode as Mode;

/// The tray writes `Settings::input_method`; the text service parses it. A
/// mismatch here is invisible at runtime — the service just keeps typing the
/// wrong method, which is exactly what happened when nothing parsed it at all.
#[test]
fn test_settings_ids_map_to_modes() {
    assert_eq!(Mode::from_settings_id("telex"), Mode::Telex);
    assert_eq!(Mode::from_settings_id("vni"), Mode::VNI);
    assert_eq!(Mode::from_settings_id("nom"), Mode::Nom);
    assert_eq!(Mode::from_settings_id("english"), Mode::English);
}

#[test]
fn test_unknown_method_id_becomes_a_custom_lookup() {
    assert_eq!(
        Mode::from_settings_id("taynguyen"),
        Mode::Custom("taynguyen".to_string())
    );
}

#[test]
fn test_english_mode_passes_keys_through() {
    // No keyboard is loaded, so the host application receives the raw key.
    let mut engine = VietnameseEngine::new_with_macros(Mode::English, vn_macro_store());
    let actions = engine.process_key('a');
    assert!(
        actions.iter().all(|a| matches!(a, Action::DoNothing)),
        "english mode must not compose: {actions:?}"
    );
    assert!(engine.buffer_content().is_empty());
}

#[test]
fn test_switching_mode_rebuilds_the_keyboard() {
    // Telex 'as' -> "á"; VNI needs 'a1' for the same. Proves the switch
    // actually replaced the keyboard rather than relabelling it.
    let mut engine = VietnameseEngine::new_with_macros(Mode::Telex, vn_macro_store());
    engine.process_key('a');
    engine.process_key('s');
    assert_eq!(engine.buffer_content(), "á");

    engine.set_mode(Mode::VNI);
    engine.process_key('a');
    engine.process_key('1');
    assert_eq!(engine.buffer_content(), "á");
}
