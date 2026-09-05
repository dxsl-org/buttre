//! ABI-surface tests for the macOS FFI (v2). Composition semantics are
//! covered cross-platform in `shared_engine_bridge_tests.rs` — these tests
//! pin the C contract: handle lifecycle, string lifetime/ownership, and the
//! IMKit mapping (commit / preedit / handled).
#![cfg(platform_macos)]

use buttre_platform::platforms::macos::ffi::*;
use std::ffi::CStr;

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

    let current_exe = std::env::current_exe().expect("current_exe");
    let status = std::process::Command::new(current_exe)
        .arg(test_name)
        .arg("--exact")
        .arg("--nocapture")
        .env("HOME", &temp_home)
        .env("BUTTRE_TEST_ISOLATED", "1")
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

#[test]
fn engine_lifecycle_and_composition() {
    isolated_test("engine_lifecycle_and_composition", || {
        let id = buttre_engine_new();
        assert_ne!(id, 0);

        // vieejt: v(9) i(34) e(14) e(14) j(38) t(17) — preedit builds to "việt"
        let mut last_preedit = String::new();
        for keycode in [9u16, 34, 14, 14, 38, 17] {
            let result = buttre_engine_process_key(id, keycode, false, false);
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
fn space_commits_word_and_passes_through() {
    isolated_test("space_commits_word_and_passes_through", || {
        let id = buttre_engine_new();
        for keycode in [9u16, 34, 14, 14, 38, 17] {
            buttre_engine_process_key(id, keycode, false, false);
        }
        let space = buttre_engine_process_key(id, 49, false, false);
        assert!(!space.handled, "separator must reach the client itself");
        assert_eq!(cstr(space.commit).as_deref(), Some("việt"));
        assert_eq!(cstr(space.preedit).as_deref(), Some(""));
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

        buttre_engine_process_key(id, 4, false, false); // h
        buttre_engine_process_key(id, 31, false, false); // o
        let bs = buttre_engine_process_backspace(id);
        assert!(bs.handled);
        assert_eq!(cstr(bs.preedit).as_deref(), Some("h"));
        buttre_engine_free(id);
    });
}

#[test]
fn invalid_handles_are_safe() {
    isolated_test("invalid_handles_are_safe", || {
        let result = buttre_engine_process_key(0, 0, false, false);
        assert!(!result.handled);
        assert!(result.commit.is_null());
        assert!(result.preedit.is_null());

        let result = buttre_engine_process_key(99999, 0, false, false);
        assert!(!result.handled);

        assert!(!buttre_engine_process_backspace(0).handled);
        assert!(!buttre_engine_flush(99999).handled);
        assert!(!buttre_engine_set_method(0, 1));
        buttre_engine_reset(0); // no crash
        buttre_engine_free(0); // no crash
        buttre_engine_free(99999); // no crash
    });
}

#[test]
fn per_engine_string_storage_does_not_clobber() {
    isolated_test("per_engine_string_storage_does_not_clobber", || {
        let id1 = buttre_engine_new();
        let id2 = buttre_engine_new();
        assert_ne!(id1, id2);

        let r1 = buttre_engine_process_key(id1, 0, false, false); // 'a'
        let r2 = buttre_engine_process_key(id2, 1, false, false); // 's'

        // Both pointers remain valid and distinct — the old global LAST_RESULT
        // would have clobbered r1's text with r2's.
        assert_eq!(cstr(r1.preedit).as_deref(), Some("a"));
        assert_eq!(cstr(r2.preedit).as_deref(), Some("s"));

        buttre_engine_free(id1);
        buttre_engine_free(id2);
    });
}

#[test]
fn set_method_switches_to_vni() {
    isolated_test("set_method_switches_to_vni", || {
        let id = buttre_engine_new();
        assert!(buttre_engine_set_method(id, 1)); // vni
                                                  // viet65: v(9) i(34) e(14) t(17) 6(22) 5(23)
        for keycode in [9u16, 34, 14, 17, 22, 23] {
            buttre_engine_process_key(id, keycode, false, false);
        }
        let space = buttre_engine_process_key(id, 49, false, false);
        assert_eq!(cstr(space.commit).as_deref(), Some("việt"));

        assert!(!buttre_engine_set_method(id, 9)); // unknown method rejected
        buttre_engine_free(id);
    });
}

#[test]
fn disabled_engine_passes_everything() {
    isolated_test("disabled_engine_passes_everything", || {
        let id = buttre_engine_new();
        buttre_engine_set_enabled(id, false);
        let result = buttre_engine_process_key(id, 0, false, false);
        assert!(!result.handled);
        buttre_engine_set_enabled(id, true);
        let result = buttre_engine_process_key(id, 0, false, false);
        assert!(result.handled);
        buttre_engine_free(id);
    });
}
