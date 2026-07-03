use buttre_core::keyboard::{BackspaceMode, Config, Keyboard, KeyboardBuilder};
use buttre_core::state::learning::{LearningFile, LearningStore};
use buttre_engine::compose::Pref;
use buttre_engine::pipeline::presets::vni_config;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[test]
fn test_keyboard_creation() {
    let toml = r#"
[metadata]
id = "test"
name = "Test"
language = "vietnamese"

[transformations]
"aa" = "â"

[tones]
"s" = "acute"

[rules]
tone_position = "modern"
"#;
    
    // Test with old way converted to new way
    let config = Config::from_str(toml).unwrap();
    let pipeline_config = config.to_pipeline_config();
    let keyboard = Keyboard::new(pipeline_config);
    assert!(keyboard.is_ok());
}

#[test]
fn test_thuowr_via_keyboard() {
    let mut keyboard = KeyboardBuilder::telex().unwrap();
    
    for ch in "thuowr".chars() {
        keyboard.process(ch).unwrap();
    }
    
    assert_eq!(keyboard.buffer(), "thuở", "thuowr should produce thuở");
}

// ── Backspace: grapheme-aware, keeps the word editable, no desync ─────────────

#[test]
fn test_backspace_deletes_grapheme_keeps_tone() {
    use buttre_core::Action;
    let mut kb = KeyboardBuilder::telex().unwrap();

    // "vieetj" → "việt".  Backspace deletes the last grapheme 't' but KEEPS the
    // tone → "việ" (raw order ≠ display order: the tone key 'j' is typed last).
    for ch in "vieetj".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "việt");

    match kb.backspace().unwrap() {
        Action::Replace { backspace_count, text } => {
            assert_eq!(backspace_count, 1, "exactly one displayed char deleted");
            assert_eq!(text, "");
        }
        other => panic!("expected Replace{{1,\"\"}}, got {other:?}"),
    }
    assert_eq!(kb.buffer(), "việ", "tone preserved; only final consonant removed");

    // Composition stays alive: a tone key now re-tones the edited word.
    kb.process('s').unwrap();
    assert_eq!(kb.buffer(), "viế", "re-toning after backspace works");
}

#[test]
fn test_backspace_no_desync_then_fresh_word() {
    use buttre_core::Action;
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in "ngayf".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "ngày");
    // Each backspace deletes exactly one displayed grapheme — no over-deletion
    // reaching into a previous word.
    assert!(matches!(kb.backspace().unwrap(), Action::Replace { backspace_count: 1, .. }));
    assert_eq!(kb.buffer(), "ngà");
    assert!(matches!(kb.backspace().unwrap(), Action::Replace { backspace_count: 1, .. }));
    assert_eq!(kb.buffer(), "ng");
}

// ── Multi-word rolling window: edit/re-tone a previous word (Cách B) ───────────

#[test]
fn test_multiword_retone_previous_word() {
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in "ban cas".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "ban cá");
    // Backspace across the space, deleting the second word entirely.
    kb.backspace().unwrap(); // "ban c"
    kb.backspace().unwrap(); // "ban "
    kb.backspace().unwrap(); // "ban"
    assert_eq!(kb.buffer(), "ban");
    // The previous word is still composable: apply a tone to it.
    kb.process('f').unwrap();
    assert_eq!(kb.buffer(), "bàn", "must re-tone the previous word after backspace");
}

#[test]
fn test_multiword_window_cap_freezes_oldest() {
    // Window keeps the last 3 words; a 4th word scrolls the oldest into the
    // frozen prefix (still shown, no longer recomposed).
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in "mot hai ba bon".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "mot hai ba bon");
}

// ── Phase 5: Keyboard-level performance check (red-team M4) ──────────────────
//
// `compose_bench` (buttre-engine) measures a single `compose()` call in
// isolation. The Hook backend's real per-keystroke cost is higher:
// `Keyboard::process_multiword` recomposes the ENTIRE rolling window (up to
// `MAX_WINDOW_WORDS` words) on every keystroke, and each word inside the
// window may independently pay the attestation-gate's demote-and-recompose
// cost (see `compose::compose_internal`). A worst-case English input that
// repeatedly triggers BOTH the gate's demote path AND window scrolling
// exercises what compose_bench alone cannot see. Repeating the flagship
// `"data"` case is the most direct worst case: every occurrence independently
// fires the non-adjacent gate, demotes, and recomposes, while the 4-char word
// length keeps the window scrolling on almost every keystroke (>3 words).
//
// This is NOT a tight perf gate (see phase-05-regression-suite.md Risk
// Notes) — it records real numbers and asserts only a generous sanity
// ceiling, so a genuine algorithmic regression (e.g. an accidental
// O(n^2)/O(n^3) reintroduced upstream) fails loudly without making CI flaky
// on ordinary machine variance. See phase-05-regression-suite.md for the
// actual measured numbers on the reference machine.
#[test]
fn test_keyboard_multiword_worst_case_perf() {
    use std::time::Instant;

    // 8 repetitions of the flagship gate/demote case, space-separated: the
    // window (cap 3 words) scrolls on almost every subsequent word boundary.
    let typing_input = ["data"; 8].join(" ");
    let keystroke_count = typing_input.chars().count();

    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    let start = Instant::now();
    for ch in typing_input.chars() {
        kb.process(ch).expect("process must not error");
    }
    let typing_elapsed = start.elapsed();
    let per_keystroke = typing_elapsed / keystroke_count as u32;

    // Backspace storm: delete everything just typed, one displayed grapheme
    // at a time — the worst case for `find_window_backspace_raw`'s
    // remove-one-key subset search over the live window.
    let start = Instant::now();
    for _ in 0..keystroke_count {
        kb.backspace().expect("backspace must not error");
    }
    let backspace_elapsed = start.elapsed();
    let per_backspace = backspace_elapsed / keystroke_count as u32;

    eprintln!(
        "[perf] keyboard multiword worst-case ({keystroke_count} keystrokes of \"{typing_input}\"): \
         typing {typing_elapsed:?} total ({per_keystroke:?}/keystroke); \
         backspace storm {backspace_elapsed:?} total ({per_backspace:?}/backspace)"
    );

    // Loose sanity ceilings only — see the doc comment above.
    assert!(
        per_keystroke.as_millis() < 5,
        "typing got unexpectedly slow: {per_keystroke:?}/keystroke (budget: well under 1ms typical)"
    );
    assert!(
        per_backspace.as_millis() < 20,
        "backspace storm got unexpectedly slow: {per_backspace:?}/backspace"
    );
}

// ── Phase 3: word-boundary final repair — multiword (Hook) delivery ─────────
// Test Scenario Matrix from phase-03-boundary-repair.md, replayed through the
// REAL Hook-backend code path (`Keyboard::process` → `process_multiword` →
// `compose_window`/`compose_one_word`/`diff_to_action`) rather than a mock —
// this is the same mechanism `hook.rs` drives via `send_replacement`.

fn type_str(kb: &mut Keyboard, s: &str) {
    for ch in s.chars() {
        kb.process(ch).expect("process must not error");
    }
}

#[test]
fn boundary_repair_vni_nhat6_space_restores_literal() {
    let mut kb = KeyboardBuilder::vni().expect("vni keyboard");
    type_str(&mut kb, "nhat6 ");
    assert_eq!(kb.buffer(), "nhat6 ", "shape-only inferred mark must repair to literal raw at the boundary");
}

#[test]
fn boundary_repair_vni_nhat61_space_untouched_exact_attested() {
    let mut kb = KeyboardBuilder::vni().expect("vni keyboard");
    type_str(&mut kb, "nhat61 ");
    assert_eq!(kb.buffer(), "nhất ", "exact-attested word must be untouched");
}

#[test]
fn boundary_repair_telex_vietej_space_untouched_exact_path() {
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "vietej ");
    assert_eq!(kb.buffer(), "việt ", "Telex's exact-attestation path is already correct");
}

#[test]
fn boundary_repair_data_space_no_double_repair() {
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "data ");
    assert_eq!(kb.buffer(), "data ", "already-literal word must not be touched again");
}

#[test]
fn boundary_repair_reset_space_accepted_collision_untouched() {
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "reset ");
    assert_eq!(kb.buffer(), "rết ", "exact-attested collision must not be repaired");
}

#[test]
fn boundary_repair_adjacent_vieet_space_never_repaired() {
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "vieet ");
    assert_eq!(kb.buffer(), "viêt ", "direct/adjacent typing carries no inferred mark, never repaired");
}

#[test]
fn boundary_repair_case_masked_diff_vieejt_space() {
    // Red-team M2: the repair diff must be computed against the CASE-MASKED
    // display form. "Vieejt" is already exact-attested ("Việt"), so this
    // also serves as a case-preservation regression guard for the no-op path.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "Vieejt ");
    assert_eq!(kb.buffer(), "Việt ", "case must survive the boundary-repair probe");
}

#[test]
fn boundary_repair_disabled_flag_keeps_old_behavior() {
    let mut config = vni_config();
    config.boundary_repair = false;
    let mut kb = KeyboardBuilder::new()
        .with_pipeline_config(config)
        .build()
        .expect("vni keyboard with boundary_repair disabled");
    type_str(&mut kb, "nhat6 ");
    assert_eq!(kb.buffer(), "nhât ", "boundary_repair=false must reproduce the old shape-attested-only behavior exactly");
}

#[test]
fn boundary_repair_multiword_reopens_on_backspace_over_separator() {
    // Bidirectional projection: a closed word's repair is NOT a one-way
    // ratchet — backspacing the separator away re-opens the word and the
    // shape-attested intermediate reappears automatically (the very next
    // window recompute simply sees `closed = false` for it again).
    let mut kb = KeyboardBuilder::vni().expect("vni keyboard");
    type_str(&mut kb, "nhat6 xin");
    assert_eq!(kb.buffer(), "nhat6 xin", "word closed by the separator is repaired while composing the rest");

    // Backspace "xin" away, then backspace over the separator itself.
    kb.backspace().unwrap(); // "nhat6 xi"
    kb.backspace().unwrap(); // "nhat6 x"
    kb.backspace().unwrap(); // "nhat6 "
    assert_eq!(kb.buffer(), "nhat6 ");
    kb.backspace().unwrap(); // removes the separator — word re-opens
    assert_eq!(kb.buffer(), "nhât", "re-opened word must show the live per-keystroke (open) projection again");
}

#[test]
fn boundary_repair_noop_after_p2_unlatch_single_word() {
    // Interaction with Phase 2: "vietje" un-latches mid-word to the
    // exact-attested "việt" — boundary repair at the following separator
    // must be a complete no-op (no double-Replace, no flicker back to any
    // literal form).
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "vietj");
    // Still mid-word: "vietj" alone has not unlatched yet.
    type_str(&mut kb, "e ");
    assert_eq!(kb.buffer(), "việt ", "P2 un-latch result must survive the boundary-repair probe untouched");
}

// ── Phase 4: bidirectional word toggle + raw-space backspace ────────────────
// Test Scenario Matrix from phase-04-user-controls.md.

#[test]
fn toggle_last_word_is_bidirectional_and_repeatable() {
    // critical row: type `reset` (shows `rết`) → hotkey → `reset`;
    // hotkey again → `rết`.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "reset");
    assert_eq!(kb.buffer(), "rết", "reset composes to the attested collision rết");

    let action = kb.toggle_last_word().expect("toggle must act on the open trailing word");
    assert!(matches!(action, buttre_core::Action::Replace { .. }));
    assert_eq!(kb.buffer(), "reset", "first toggle renders literal(raw), case-preserved");

    let action = kb.toggle_last_word().expect("second toggle must still find the same word");
    assert!(matches!(action, buttre_core::Action::Replace { .. }));
    assert_eq!(kb.buffer(), "rết", "second toggle flips back to compose(raw) — bidirectional, not one-shot");

    // A third toggle proves it's genuinely repeatable, not just a 2-state latch.
    kb.toggle_last_word().expect("third toggle must still act");
    assert_eq!(kb.buffer(), "reset");
}

#[test]
fn toggle_closes_the_word_so_continued_typing_starts_a_new_one() {
    // critical row: toggle → keep typing → literal projection persists for
    // that word; a NEW word composes independently (word-freezing per the
    // architecture — prevents the toggled-literal + tone-key junk cascade).
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "reset");
    kb.toggle_last_word().expect("toggle must act");
    assert_eq!(kb.buffer(), "reset");

    // Continue typing WITHOUT a separator: "gaf" (Telex 'f' = huyền on 'a')
    // composes to "gà" if — and only if — it's treated as an independent
    // word rather than glued onto the toggled raw span.
    type_str(&mut kb, "gaf");
    assert_eq!(
        kb.buffer(),
        "resetgà",
        "toggled word stays literal; new word composes on its own, even with no typed separator"
    );
}

#[test]
fn toggle_word_survives_scroll_out_into_committed_prefix() {
    // critical row: toggle word 2, then the word scrolls out → committed as
    // its toggled form; the map shifts (re-anchors) rather than losing state.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "xin reset");
    assert_eq!(kb.buffer(), "xin rết", "pre-toggle: reset composes normally");

    kb.toggle_last_word().expect("toggle acts on the open trailing word ('reset')");
    assert_eq!(kb.buffer(), "xin reset", "toggled word renders as raw literal");

    // Push enough further words that "xin" scrolls into the frozen prefix
    // while "reset" (toggled) is still live in the window — index shift.
    type_str(&mut kb, " kim nam");
    assert!(kb.buffer().contains("reset"), "toggle must survive the first scroll, re-anchored not lost");
    assert!(!kb.buffer().contains("rết"), "must not have silently reverted to compose after the shift");

    // Push further still so "reset" itself scrolls into `committed`.
    type_str(&mut kb, " lan van");
    assert!(kb.buffer().contains("reset"), "toggled word must be committed in its toggled (literal) form");
    assert!(!kb.buffer().contains("rết"), "scrolled-out toggled word must not silently revert to composed form");
}

#[test]
fn toggle_last_word_noop_when_window_empty() {
    // high row: hotkey with an empty window → no-op, no crash.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    assert!(kb.toggle_last_word().is_none());
    assert_eq!(kb.buffer(), "");
}

#[test]
fn toggle_last_word_noop_for_non_multiword_backend() {
    // high row: hotkey on a TSF/composition-mode keyboard → no-op, no crash.
    // Scope: Hook multiword backend only (TSF deferred, phase-04 note).
    let mut kb = KeyboardBuilder::telex_with_composition(true).expect("composition keyboard");
    type_str(&mut kb, "reset");
    assert!(kb.toggle_last_word().is_none());
}

#[test]
fn raw_backspace_pops_last_keystroke_not_last_grapheme() {
    // high row: raw-backspace on `việt` (raw `vietj`) → `viet` — a
    // keystroke-wise inverse, independent of display-grapheme counting.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_backspace_mode(BackspaceMode::Raw);
    type_str(&mut kb, "vietj");
    kb.backspace().expect("raw backspace must not error");
    assert_eq!(kb.buffer(), "viet", "raw mode pops the last KEYSTROKE ('j'), not a display grapheme");
}

#[test]
fn grapheme_backspace_mode_is_default_and_byte_identical_to_pre_phase() {
    // high row: grapheme mode (default) is byte-identical to pre-phase
    // behavior — a fresh Keyboard always starts in Grapheme mode, and the
    // existing `test_backspace_deletes_grapheme_keeps_tone` /
    // `test_backspace_no_desync_then_fresh_word` regression tests exercise
    // the unchanged code path. This test guards the DEFAULT explicitly.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "vieetj");
    assert_eq!(kb.buffer(), "việt");
    match kb.backspace().unwrap() {
        buttre_core::Action::Replace { backspace_count, text } => {
            assert_eq!(backspace_count, 1, "default mode deletes exactly one displayed grapheme");
            assert_eq!(text, "");
        }
        other => panic!("expected Replace{{1,\"\"}}, got {other:?}"),
    }
    assert_eq!(kb.buffer(), "việ");
}

#[test]
fn raw_backspace_precisely_prunes_only_the_popped_boundary() {
    // Map invalidation, precise case (raw mode = pure tail truncation): a
    // raw-mode backspace entirely inside word 2 must not disturb word 1's
    // toggle, unlike grapheme mode's conservative whole-map clear below.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_backspace_mode(BackspaceMode::Raw);
    type_str(&mut kb, "reset");
    kb.toggle_last_word().expect("toggle must act");
    assert_eq!(kb.buffer(), "reset");

    type_str(&mut kb, " h");
    assert_eq!(kb.buffer(), "reset h");
    kb.backspace().expect("raw backspace must not error"); // pops 'h'
    assert_eq!(kb.buffer(), "reset ", "word 1's toggle survives a raw-mode edit entirely inside word 2");
}

#[test]
fn toggle_map_conservatively_cleared_by_any_backspace_even_in_a_different_word() {
    // medium row: toggle + backspace over the word — parity map consistent
    // with raw edits. Map invalidation intentionally errs conservative (see
    // `Keyboard::backspace_multiword`): a grapheme-mode backspace ANYWHERE
    // in the live window clears ALL toggle state, even for a word untouched
    // by the edit, rather than risk a stale offset reattaching to the wrong
    // word after an arbitrary raw rewrite. This is the documented trade-off,
    // asserted explicitly so a future change to it is a deliberate decision.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "reset");
    kb.toggle_last_word().expect("toggle must act"); // word 1 -> literal
    assert_eq!(kb.buffer(), "reset");

    type_str(&mut kb, " hai");
    assert_eq!(kb.buffer(), "reset hai", "toggle survives pure-append typing of a new word");

    kb.backspace().expect("backspace must not error"); // edits word 2 only ("hai" -> "ha")
    assert_eq!(
        kb.buffer(),
        "rết ha",
        "a backspace anywhere in the window conservatively clears ALL toggle state, \
         so word 1 reverts to its natural composed form even though only word 2 changed"
    );
}

#[test]
fn toggle_emits_learning_signal_for_p5_collector() {
    // medium row: toggle emits a learning event — collector receives
    // (raw, direction). Phase 5 seam only; not consumed here.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "reset");

    kb.toggle_last_word().expect("toggle must act");
    let signals = kb.drain_toggle_signals();
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].raw_sequence, "reset");
    assert!(signals[0].literal, "first toggle direction is literal");

    // Draining must actually clear the queue (no duplicate delivery).
    assert!(kb.drain_toggle_signals().is_empty());

    kb.toggle_last_word().expect("second toggle must act");
    let signals = kb.drain_toggle_signals();
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].raw_sequence, "reset");
    assert!(!signals[0].literal, "second toggle flips back to compose");
}

// ── Phase 5: personal learning — overlay + preference memory ────────────────
// Test Scenario Matrix from phase-05-learning-stores.md. All tests wire a
// fresh in-memory `LearningStore` directly via `Keyboard::set_learning` — no
// real file I/O (no test ever touches the real data dir / `learning.toml`),
// and a discarded `mpsc::Receiver` is a legitimate no-op consumer of the
// save channel (see `collect_and_refresh_learning`'s doc: a full/disconnected
// receiver is never treated as an error).

/// A fresh in-memory store + a channel whose receiver is intentionally
/// dropped/ignored — every test here asserts on the STORE directly, never on
/// what would have been written to disk.
fn fresh_learning() -> (Arc<Mutex<LearningStore>>, mpsc::Sender<LearningFile>) {
    let store = Arc::new(Mutex::new(LearningStore::default()));
    let (tx, _rx) = mpsc::channel();
    (store, tx)
}

#[test]
fn learning_disabled_by_default_no_collection_behavior_unchanged() {
    // `Keyboard::new` never wires a learning store unless `set_learning` is
    // called (mirrors the platform layer gating this on
    // `Settings::learning_enabled`) — behavior must be BYTE-IDENTICAL to
    // pre-Phase-5: the flagship non-adjacent gate/demote case ("data") must
    // still demote every time, even after repeats that WOULD have promoted
    // the overlay had a store been wired (see
    // `automatic_demote_records_nothing_even_after_many_repeats` below for
    // the same fixture WITH a store wired, proving the difference is the
    // wiring, not the fixture).
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "daat b c daat d e daat f g h");
    type_str(&mut kb, " data");
    assert!(
        kb.buffer().ends_with("data"),
        "without a wired learning store, the non-adjacent case still demotes to literal: {}",
        kb.buffer()
    );
}

#[test]
fn direct_typed_unattested_syllable_promotes_overlay_after_three_commits() {
    // critical row: type an unattested-but-structurally-valid syllable
    // DIRECTLY (adjacent marks only) 3 distinct times, each across a real
    // word boundary → the overlay promotes it → a LATER non-adjacent
    // (delayed) typing of the exact same syllable now composes instead of
    // demoting to literal.
    //
    // "daat" (d-a-a-t, ADJACENT doubling) composes to "dât" exactly like the
    // engine's own `high_adjacent_typing_unchanged` fixture family
    // ("vieet"->"viêt"). "dât" is a confirmed decomposable-but-UNATTESTED
    // shape (`buttre-engine/tests/attested_lookup_tests.rs`), and "data"
    // (d-a-t-a, NON-adjacent) is the flagship gate/demote case that collapses
    // to literal "data" for the exact same reason
    // (`compose::tests::critical_data_stays_literal`) — a perfectly matched
    // pair for this scenario.
    let (store, tx) = fresh_learning();
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_learning(store.clone(), tx);

    // Filler single-char words force each "daat" occurrence through a real
    // word-boundary commit via the window's 4th-word overflow (see
    // `Keyboard::scroll_out_overflow`) — 3 occurrences, 3 commits.
    type_str(&mut kb, "daat b c daat d e daat f g h");
    assert!(
        store.lock().unwrap().overlay_snapshot().len() == 1,
        "3 direct-typed commits of the same unattested syllable must promote exactly one overlay entry"
    );

    // Continuing to type the NON-adjacent "data" form in the SAME session
    // now composes — the live executor's compose stage was refreshed at
    // every commit boundary (Combined Contract: repair → collect → refresh).
    type_str(&mut kb, " data");
    assert!(
        kb.buffer().ends_with("dât"),
        "overlay-promoted syllable must now survive the non-adjacent gate: {}",
        kb.buffer()
    );
}

#[test]
fn automatic_demote_records_nothing_even_after_many_repeats() {
    // critical row (anti-feedback rule (i)): an AUTOMATIC demote — the gate
    // stripping an inferred non-adjacent mark — is never a deliberate act,
    // however many times it repeats. `ComposeResult::demoted` is exactly
    // what distinguishes this from the direct-typed case above (both leave
    // `applied_marks` empty at the point the collector inspects them).
    let (store, tx) = fresh_learning();
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_learning(store.clone(), tx);

    type_str(&mut kb, &"data ".repeat(100));

    assert!(
        store.lock().unwrap().overlay_snapshot().is_empty(),
        "an automatic demote must never feed the overlay, however many times it repeats"
    );
}

#[test]
fn undo_shaped_commit_records_literal_pref_recalled_by_new_keyboard_sharing_the_store() {
    // critical row: a double-tap undo/toggle SHAPE, sitting at the exact
    // raw the word is committed with, records a literal preference for that
    // raw sequence. "ress" (r-e-s-s) is the same trailing-double-tone-key
    // undo shape as the engine's own `check_tone_toggle` fixtures
    // ("ass"->"as", "seess"->"sês") — `is_last_event_undo(['r','e','s','s'])`
    // is true for the word's raw exactly as committed.
    let (store, tx) = fresh_learning();
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_learning(store.clone(), tx.clone());

    type_str(&mut kb, "ress b c d"); // forces "ress" through a real commit
    assert_eq!(
        store.lock().unwrap().prefs_snapshot_for_method("telex").get("ress"),
        Some(&Pref::Literal),
        "an undo-shaped word-commit must record a literal preference for its exact raw"
    );

    // A brand-new Keyboard sharing the SAME store recalls it immediately —
    // no re-learning needed, and the recall is LITERAL (all 4 raw chars
    // verbatim), not the 3-char undo-collapsed form ("res") normal typing
    // would otherwise produce (honest wording, red-team F12).
    let mut kb2 = KeyboardBuilder::telex().expect("telex keyboard");
    kb2.set_learning(store, tx);
    type_str(&mut kb2, "ress");
    assert_eq!(kb2.buffer(), "ress", "pref recall renders the literal raw verbatim from the very first matching keystroke sequence");
}

#[test]
fn toggle_commit_records_pref_recalled_by_new_keyboard_sharing_the_store() {
    // critical row: a P4 word toggle (the strongest deliberate signal) on a
    // word records a preference once that word is committed — recalled the
    // same way as the undo-shaped case above.
    let (store, tx) = fresh_learning();
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_learning(store.clone(), tx.clone());

    type_str(&mut kb, "reset");
    kb.toggle_last_word().expect("toggle must act on the open trailing word");
    assert_eq!(kb.buffer(), "reset", "toggle renders literal(raw), case-preserved, and freezes the word");

    type_str(&mut kb, " b c d"); // forces the toggled, frozen word through a real commit
    assert_eq!(
        store.lock().unwrap().prefs_snapshot_for_method("telex").get("reset"),
        Some(&Pref::Literal),
        "a toggle-to-literal must record a literal preference once the word commits"
    );

    let mut kb2 = KeyboardBuilder::telex().expect("telex keyboard");
    kb2.set_learning(store, tx);
    type_str(&mut kb2, "reset");
    assert_eq!(kb2.buffer(), "reset", "toggled preference recalled by a fresh Keyboard sharing the store");
}

#[test]
fn toggle_against_a_stored_pref_overwrites_it_self_correction() {
    // high row: the user later toggles AGAINST a stored preference — the
    // pref is overwritten (anti-feedback rule (iii)), not left stale.
    let (store, tx) = fresh_learning();
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_learning(store.clone(), tx.clone());

    type_str(&mut kb, "reset");
    kb.toggle_last_word().expect("first toggle: literal"); // reset
    kb.toggle_last_word().expect("second toggle: back to composed"); // rết
    assert_eq!(kb.buffer(), "rết");

    type_str(&mut kb, " b c d"); // commit the word in its FINAL (composed) toggle state
    assert_eq!(
        store.lock().unwrap().prefs_snapshot_for_method("telex").get("reset"),
        Some(&Pref::Composed),
        "acting against the (never yet persisted) literal direction must record the LATEST choice"
    );

    let mut kb2 = KeyboardBuilder::telex().expect("telex keyboard");
    kb2.set_learning(store, tx);
    type_str(&mut kb2, "reset");
    assert_eq!(kb2.buffer(), "rết", "composed preference recalled: same collision the syllable naturally produces, now via the pref path");
}

#[test]
fn collection_fires_once_at_commit_not_per_keystroke() {
    // medium row (red-team M7): the collector must never fire mid-word —
    // multiword recomputes the whole window every keystroke, so collecting
    // anywhere but the boundary would record the same event repeatedly.
    let (store, tx) = fresh_learning();
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_learning(store.clone(), tx);

    // Mid-word: composing the undo shape "ress" as the OPEN trailing word —
    // not yet committed, so the store must stay untouched.
    type_str(&mut kb, "res");
    assert!(store.lock().unwrap().prefs_snapshot_for_method("telex").is_empty());
    kb.process('s').expect("process must not error"); // completes "ress", still open
    assert!(
        store.lock().unwrap().prefs_snapshot_for_method("telex").is_empty(),
        "still the open trailing word — no commit yet, no collection"
    );

    type_str(&mut kb, " b c d"); // force the word boundary
    assert!(
        store.lock().unwrap().prefs_snapshot_for_method("telex").contains_key("ress"),
        "collection fires exactly once the word crosses the boundary"
    );
}

#[test]
fn short_undo_below_min_specificity_floor_records_nothing() {
    // high row (anti-feedback rule (ii), red-team M4): a raw shorter than
    // the min-specificity floor must never become a permanent literal
    // preference, however it was produced — "ass" (3 chars) is the engine's
    // own canonical tone-undo fixture, one character short of the floor.
    let (store, tx) = fresh_learning();
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_learning(store.clone(), tx);

    type_str(&mut kb, "ass b c d");
    assert!(
        store.lock().unwrap().prefs_snapshot_for_method("telex").is_empty(),
        "a raw shorter than the min-specificity floor must never be recorded, even from a genuine undo shape"
    );
}
