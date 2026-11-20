//! ABI-surface tests for the macOS FFI (v4). Composition semantics are
//! covered cross-platform in `shared_engine_bridge_tests.rs` — these tests
//! pin the C contract: handle lifecycle, string lifetime/ownership, and the
//! IMKit mapping (commit / preedit / handled / candidates / input mode).
#![cfg(platform_macos)]

use buttre_platform::platforms::macos::ffi::*;
use std::ffi::{CStr, CString};

/// Run `f` in a child process with a throwaway HOME (the engine's method /
/// settings files live under it) and the workspace root as cwd, so the
/// ambient resource lookup finds `buttre_nom.db` and `keyboards/` there.
fn isolated_test<F>(test_name: &str, f: F)
where
    F: FnOnce(),
{
    if std::env::var_os("BUTTRE_TEST_ISOLATED").is_some() {
        f();
        return;
    }

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp_home = std::env::temp_dir().join(format!(
        "buttre-macos-test-{}-{}-{}",
        std::process::id(),
        n,
        test_name
    ));
    let _ = std::fs::remove_dir_all(&temp_home);
    std::fs::create_dir_all(&temp_home).expect("create temp HOME");
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let current_exe = std::env::current_exe().expect("current_exe");
    let status = std::process::Command::new(current_exe)
        .arg(test_name)
        .arg("--exact")
        .arg("--nocapture")
        .env("HOME", &temp_home)
        .env("BUTTRE_TEST_ISOLATED", "1")
        .current_dir(workspace_root)
        .status()
        .expect("spawn isolated test child");

    let _ = std::fs::remove_dir_all(&temp_home);
    assert!(status.success(), "isolated child test {test_name} failed");
}

fn cstr(ptr: *const std::os::raw::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string())
}

/// Feed `text` one character at a time; returns the last result.
fn type_str(id: u64, text: &str) -> ButtreKeyResult {
    let mut last = None;
    for ch in text.chars() {
        last = Some(buttre_engine_process_char(id, ch as u32));
    }
    last.expect("non-empty input")
}

/// Report input mode `method` the way the IMKit controller does.
fn select(id: u64, method: &str) -> ButtreKeyResult {
    let method = CString::new(method).unwrap();
    // SAFETY: `method` is a live NUL-terminated CString for the whole call.
    unsafe { buttre_engine_select_method(id, method.as_ptr()) }
}

#[test]
fn engine_lifecycle_and_composition() {
    isolated_test("engine_lifecycle_and_composition", || {
        let id = buttre_engine_new();
        assert_ne!(id, 0);

        let mut last_preedit = String::new();
        for ch in "vieejt".chars() {
            let result = buttre_engine_process_char(id, ch as u32);
            assert!(result.handled, "letters must be handled");
            assert!(result.commit.is_null(), "no commit mid-word");
            last_preedit = cstr(result.preedit).unwrap();
        }
        assert_eq!(last_preedit, "việt");

        // Out-of-band flush (focus loss) commits the boundary-repaired word.
        let flush = buttre_engine_flush(id);
        assert_eq!(cstr(flush.commit).as_deref(), Some("việt"));
        assert_eq!(cstr(flush.preedit).as_deref(), Some(""));

        buttre_engine_free(id);
    });
}

#[test]
fn space_and_return_commit_word_and_pass_through() {
    isolated_test("space_and_return_commit_word_and_pass_through", || {
        let id = buttre_engine_new();
        type_str(id, "vieejt");
        let space = buttre_engine_process_char(id, ' ' as u32);
        assert!(!space.handled, "separator must reach the client itself");
        assert_eq!(cstr(space.commit).as_deref(), Some("việt"));
        assert_eq!(cstr(space.preedit).as_deref(), Some(""));

        // AppKit reports Return as '\r'.
        type_str(id, "nams");
        let ret = buttre_engine_process_char(id, '\r' as u32);
        assert!(!ret.handled);
        assert_eq!(cstr(ret.commit).as_deref(), Some("nám"));
        buttre_engine_free(id);
    });
}

/// With no marked text, `setMarkedText("")` replaces the client's selection —
/// so idle flushes and pass-through keys must leave `preedit` NULL, or Cmd+C
/// and Shift+arrow delete the selected text.
#[test]
fn idle_calls_leave_the_client_untouched() {
    isolated_test("idle_calls_leave_the_client_untouched", || {
        let id = buttre_engine_new();
        let flush = buttre_engine_flush(id);
        assert!(flush.commit.is_null());
        assert!(
            flush.preedit.is_null(),
            "idle flush must not touch marked text"
        );

        let arrow = buttre_engine_process_char(id, 0xF702); // NSLeftArrowFunctionKey
        assert!(!arrow.handled);
        assert!(arrow.preedit.is_null());
        let escape = buttre_engine_process_char(id, 0x1B);
        assert!(!escape.handled);
        assert!(escape.preedit.is_null());
        buttre_engine_free(id);
    });
}

/// A key that composes nothing must end the word before the app sees it —
/// otherwise the app's own insert replaces the marked text and the word is
/// lost while the engine keeps composing it.
#[test]
fn non_text_keys_commit_the_pending_word() {
    isolated_test("non_text_keys_commit_the_pending_word", || {
        let id = buttre_engine_new();
        type_str(id, "vieejt");
        let arrow = buttre_engine_process_char(id, 0xF703); // NSRightArrowFunctionKey
        assert!(!arrow.handled, "the app still receives the key");
        assert_eq!(cstr(arrow.commit).as_deref(), Some("việt"));
        assert_eq!(cstr(arrow.preedit).as_deref(), Some(""));

        // Not exactly one character (dead key, multi-char event) → 0.
        type_str(id, "ab");
        let none = buttre_engine_process_char(id, 0);
        assert_eq!(cstr(none.commit).as_deref(), Some("ab"));
        buttre_engine_free(id);
    });
}

#[test]
fn backspace_contract() {
    isolated_test("backspace_contract", || {
        let id = buttre_engine_new();
        // Nothing composing → the app handles deletion itself.
        let empty = buttre_engine_process_backspace(id);
        assert!(!empty.handled);
        assert!(empty.preedit.is_null());

        type_str(id, "ho");
        let bs = buttre_engine_process_backspace(id);
        assert!(bs.handled);
        assert_eq!(cstr(bs.preedit).as_deref(), Some("h"));
        buttre_engine_free(id);
    });
}

#[test]
fn reset_discards_without_committing() {
    isolated_test("reset_discards_without_committing", || {
        let id = buttre_engine_new();
        type_str(id, "vie");
        let reset = buttre_engine_reset(id);
        assert!(reset.commit.is_null());
        assert_eq!(cstr(reset.preedit).as_deref(), Some(""));
        buttre_engine_free(id);
    });
}

#[test]
fn invalid_handles_are_safe() {
    isolated_test("invalid_handles_are_safe", || {
        for id in [0, 99999] {
            let result = buttre_engine_process_char(id, 'a' as u32);
            assert!(!result.handled);
            assert!(result.commit.is_null());
            assert!(result.preedit.is_null());
            assert!(!buttre_engine_process_backspace(id).handled);
            assert!(!buttre_engine_flush(id).handled);
            assert!(!buttre_engine_reset(id).handled);
            assert_eq!(buttre_engine_candidate_count(id), 0);
            assert!(buttre_engine_candidate_at(id, 0).is_null());
            assert!(!buttre_engine_select_candidate(id, 0).handled);
            assert!(!select(id, "vni").handled);
            buttre_engine_free(id); // no crash
        }
    });
}

#[test]
fn per_engine_string_storage_does_not_clobber() {
    isolated_test("per_engine_string_storage_does_not_clobber", || {
        let id1 = buttre_engine_new();
        let id2 = buttre_engine_new();
        assert_ne!(id1, id2);

        let r1 = buttre_engine_process_char(id1, 'a' as u32);
        let r2 = buttre_engine_process_char(id2, 's' as u32);

        // Both pointers remain valid and distinct.
        assert_eq!(cstr(r1.preedit).as_deref(), Some("a"));
        assert_eq!(cstr(r2.preedit).as_deref(), Some("s"));

        buttre_engine_free(id1);
        buttre_engine_free(id2);
    });
}

/// Selecting an input mode switches the method (committing the pending word
/// first) and persists it so other engines follow; re-selecting the current
/// mode is a no-op; ids that are not buildable methods change nothing.
#[test]
fn input_mode_selects_and_persists_method() {
    isolated_test("input_mode_selects_and_persists_method", || {
        let id = buttre_engine_new();
        type_str(id, "vieejt");
        let switch = select(id, "vni");
        assert_eq!(cstr(switch.commit).as_deref(), Some("việt"), "word kept");
        assert_eq!(buttre_platform::shared::method_sync::read_method(), "vni");

        type_str(id, "viet65");
        let space = buttre_engine_process_char(id, ' ' as u32);
        assert_eq!(cstr(space.commit).as_deref(), Some("việt"));

        // Other engines follow the persisted switch.
        let other = buttre_engine_new();
        type_str(other, "viet65");
        assert_eq!(
            cstr(buttre_engine_flush(other).commit).as_deref(),
            Some("việt")
        );

        // Activation re-reports the current mode: must not flush or rebuild.
        type_str(id, "vie");
        let same = select(id, "vni");
        assert!(same.commit.is_null() && same.preedit.is_null());
        assert_eq!(
            cstr(buttre_engine_flush(id).commit).as_deref(),
            Some("vie"),
            "word survives a same-mode report"
        );

        for bogus in ["english", "bogus", "../telex"] {
            assert!(!select(id, bogus).handled, "{bogus} rejected");
            assert_eq!(buttre_platform::shared::method_sync::read_method(), "vni");
        }
        // SAFETY: null is explicitly allowed (rejected, nothing read).
        assert!(!unsafe { buttre_engine_select_method(id, std::ptr::null()) }.handled);
        buttre_engine_free(id);
        buttre_engine_free(other);
    });
}

/// A method file left on the legacy "english" id (an old English-menu click)
/// silences new engines until the OS reports the selected mode — which then
/// brings typing back.
#[test]
fn selected_mode_overrides_legacy_english() {
    isolated_test("selected_mode_overrides_legacy_english", || {
        buttre_platform::shared::method_sync::write_method("english").unwrap();
        let id = buttre_engine_new();
        assert!(!buttre_engine_process_char(id, 'a' as u32).handled);
        select(id, "telex");
        assert!(buttre_engine_process_char(id, 'a' as u32).handled);
        buttre_engine_free(id);
    });
}

/// Nôm lookups surface through `candidates_changed` + count/at, and a
/// selection commits the bare character (not the display gloss) and hides
/// the list.
#[test]
fn nom_candidates_round_trip() {
    isolated_test("nom_candidates_round_trip", || {
        let id = buttre_engine_new();
        select(id, "nom");

        let mut shown = false;
        for ch in "trowif".chars() {
            let r = buttre_engine_process_char(id, ch as u32);
            if r.candidates_changed {
                shown = buttre_engine_candidate_count(id) > 0;
            }
        }
        assert!(shown, "a dictionary word must offer candidates");
        let count = buttre_engine_candidate_count(id);
        let display = cstr(buttre_engine_candidate_at(id, 0)).expect("first candidate");
        assert!(!display.is_empty());
        assert!(
            buttre_engine_candidate_at(id, count).is_null(),
            "out of range"
        );

        let pick = buttre_engine_select_candidate(id, 0);
        assert!(pick.handled);
        assert!(pick.candidates_changed);
        assert_eq!(buttre_engine_candidate_count(id), 0, "list hidden");
        let committed = cstr(pick.commit).expect("selection commits");
        assert!(
            display.starts_with(&committed),
            "commit {committed:?} is the bare value of {display:?}"
        );
        assert_eq!(cstr(pick.preedit).as_deref(), Some(""));
        buttre_engine_free(id);
    });
}

/// Write `settings.toml` into the isolated HOME before the first engine is
/// created (the host loads its settings mirrors once, on that call).
fn write_settings(toml: &str) {
    let dir = dirs::data_dir().expect("data dir").join("buttre");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.toml"), toml).unwrap();
}

/// A minimal IMKit client: applies results to `doc` the way the controller
/// does (replacement checked against the text before the caret, unhandled
/// keys inserted by the "app").
fn feed(id: u64, doc: &mut String, ch: char) -> ButtreKeyResult {
    let r = buttre_engine_process_char(id, ch as u32);
    assert!(r.preedit.is_null(), "commit-as-you-go never marks text");
    let commit = cstr(r.commit).unwrap_or_default();
    match cstr(r.replaced) {
        Some(replaced) => {
            assert!(
                doc.ends_with(&replaced),
                "{doc:?} must end with {replaced:?}"
            );
            doc.truncate(doc.len() - replaced.len());
            doc.push_str(&commit);
        }
        None => doc.push_str(&commit),
    }
    if !r.handled {
        doc.push(ch);
    }
    r
}

/// "Gõ thẳng không gạch chân": with `use_preedit = false` and a client that
/// can edit its text, Telex commits as you go — every result edits the
/// client's text directly and never marks it.
#[test]
fn no_preedit_commits_as_you_go() {
    isolated_test("no_preedit_commits_as_you_go", || {
        write_settings("input_method = \"telex\"\nenabled = true\nauto_correct = false\nshorthand = false\nstartup = false\nuse_preedit = false\n");
        let id = buttre_engine_new();
        assert!(buttre_engine_sync_client(id, true).preedit.is_null());
        let mut doc = String::new();
        for ch in "vieejt nam".chars() {
            feed(id, &mut doc, ch);
        }
        assert_eq!(doc, "việt nam");
        // An arrow key ends the run: the next tone key must not reach back
        // over "nam".
        buttre_engine_process_char(id, 0xF702);
        let r = feed(id, &mut doc, 's');
        assert!(r.replaced.is_null());
        buttre_engine_free(id);
    });
}

/// A client that can't report its selection keeps the preedit model even
/// with the setting on, and switching models mid-word commits the word.
#[test]
fn no_preedit_falls_back_without_client_support() {
    isolated_test("no_preedit_falls_back_without_client_support", || {
        write_settings("input_method = \"telex\"\nenabled = true\nauto_correct = false\nshorthand = false\nstartup = false\nuse_preedit = false\n");
        let id = buttre_engine_new();
        buttre_engine_sync_client(id, false);
        let r = type_str(id, "vieejt");
        assert_eq!(cstr(r.preedit).as_deref(), Some("việt"));
        let switched = buttre_engine_sync_client(id, true);
        assert_eq!(cstr(switched.commit).as_deref(), Some("việt"));
        assert_eq!(cstr(switched.preedit).as_deref(), Some(""));
        buttre_engine_free(id);
    });
}

/// "Xóa lùi theo phím thô": Backspace drops the last raw key (here the tone
/// key) instead of the last displayed character.
#[test]
fn raw_backspace_drops_last_keystroke() {
    isolated_test("raw_backspace_drops_last_keystroke", || {
        write_settings("input_method = \"telex\"\nenabled = true\nauto_correct = false\nshorthand = false\nstartup = false\nbackspace_mode = \"raw\"\n");
        let id = buttre_engine_new();
        assert_eq!(
            cstr(type_str(id, "vieetj").preedit).as_deref(),
            Some("việt")
        );
        let bs = buttre_engine_process_backspace(id);
        assert_eq!(cstr(bs.preedit).as_deref(), Some("viêt"));
        buttre_engine_free(id);
    });
}
