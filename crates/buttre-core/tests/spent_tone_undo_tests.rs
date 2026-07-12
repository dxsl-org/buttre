//! Regression: interior spent-tone-undo fold (compose step 1.5).
//!
//! "reset" was untypeable: "ress" correctly undid the acute tone ("res"),
//! but the next 'e' recomposed raw "resse" from scratch — segment collected
//! both spent 's' keys as tones, last-tone-wins resurrected the removed
//! acute, and the non-adjacent `e..e` mark produced attested "rế", which
//! passed every un-latch condition and overwrote the display. The undo
//! event must stay folded for the rest of the word (Unikey multi-level
//! toggle: undo is final until the word boundary).

use buttre_core::KeyboardBuilder;

fn type_seq(seq: &str) -> String {
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in seq.chars() {
        kb.process(ch).unwrap();
    }
    kb.buffer().to_string()
}

#[test]
fn resset_types_reset_via_double_s_undo() {
    assert_eq!(type_seq("ress"), "res", "double-s undo must fire");
    assert_eq!(
        type_seq("resse"),
        "rese",
        "the 'e' after a spent undo must append literally, never resurrect the tone as 'rế'"
    );
    assert_eq!(type_seq("resset"), "reset");
}

#[test]
fn plain_reset_keeps_unikey_flexible_composition() {
    // No undo typed: the interior 's' is a live tone and the delayed 'e'
    // completes ê — same as Unikey. The escape hatch is the double-s above.
    assert_eq!(type_seq("reset"), "rết");
}

#[test]
fn spent_undo_fold_matches_display_for_sibling_words() {
    // Same class as "resset": deliberate undo mid-word, literal tail.
    assert_eq!(type_seq("tesst"), "test");
    assert_eq!(type_seq("dessign"), "design");
    // Tone never displayed ("glá" has an invalid onset): the visibility gate
    // must keep the fold out of English double-letter words.
    assert_eq!(type_seq("glasses"), "glasses");
}

#[test]
fn spent_transform_undo_is_final_too() {
    // Transform-toggle analog of "resset" (found by the 10k-word English
    // scan): "roww" undoes ơ, but a later tone key re-derived the whole raw
    // and resurrected the spent `w` mark — "rowws" became attested "rớ",
    // making rows/towns/owns/lows untypeable.
    assert_eq!(type_seq("roww"), "row", "double-w undo must fire");
    assert_eq!(
        type_seq("rowws"),
        "rows",
        "the 's' after a spent transform undo must append literally, never resurrect ơ as 'rớ'"
    );
    assert_eq!(type_seq("towwns"), "towns");
    assert_eq!(type_seq("owwns"), "owns");
    assert_eq!(type_seq("lowws"), "lows");
}

#[test]
fn tone_undo_restores_overridden_tone_keys_as_literal() {
    // "meters"/"donors" (10k-word scan): the trailing double-s undo used to
    // reconstruct the prefix by DISCARDING every tone-key char it contained,
    // eating the 'r' — "meterss" gave "metes". An undo returns the user's
    // literal keystrokes; no keystroke may vanish.
    assert_eq!(type_seq("meterss"), "meters");
    assert_eq!(type_seq("donorss"), "donors");
}

#[test]
fn yes_class_escape_still_works() {
    // The classic single-tone escape the users reach for first.
    assert_eq!(type_seq("yess"), "yes");
    assert_eq!(type_seq("boss"), "bos", "Unikey undo semantics unchanged");
}
