use buttre_engine::pipeline::validation::{SyllableStructure, extract_onset, extract_coda};

#[test]
fn test_parse_simple() {
    let s = SyllableStructure::parse("a");
    assert_eq!(s.onset, "");
    assert_eq!(s.nucleus, "a");
    assert_eq!(s.coda, "");
}

#[test]
fn test_parse_with_onset() {
    let s = SyllableStructure::parse("ba");
    assert_eq!(s.onset, "b");
    assert_eq!(s.nucleus, "a");
    assert_eq!(s.coda, "");
}

#[test]
fn test_parse_with_coda() {
    let s = SyllableStructure::parse("an");
    assert_eq!(s.onset, "");
    assert_eq!(s.nucleus, "a");
    assert_eq!(s.coda, "n");
}

#[test]
fn test_parse_full() {
    let s = SyllableStructure::parse("ban");
    assert_eq!(s.onset, "b");
    assert_eq!(s.nucleus, "a");
    assert_eq!(s.coda, "n");
}

#[test]
fn test_parse_complex_onset() {
    let s = SyllableStructure::parse("thường");
    assert_eq!(s.onset, "th");
    assert_eq!(s.nucleus, "ươ"); // Normalized (tone removed)
    assert_eq!(s.coda, "ng");
}

#[test]
fn test_parse_ngh_onset() {
    let s = SyllableStructure::parse("nghệ");
    assert_eq!(s.onset, "ngh");
    assert_eq!(s.nucleus, "ê"); // Normalized (tone removed)
    assert_eq!(s.coda, "");
}

#[test]
fn test_parse_triphthong() {
    let s = SyllableStructure::parse("uyên");
    assert_eq!(s.onset, "");
    assert_eq!(s.nucleus, "uyê"); // Already normalized
    assert_eq!(s.coda, "n");
}

#[test]
fn test_is_valid_simple() {
    assert!(SyllableStructure::parse("a").is_valid());
    assert!(SyllableStructure::parse("ba").is_valid());
    assert!(SyllableStructure::parse("ban").is_valid());
}

#[test]
fn test_is_valid_complex() {
    assert!(SyllableStructure::parse("thường").is_valid());
    assert!(SyllableStructure::parse("nghệ").is_valid());
    assert!(SyllableStructure::parse("uyên").is_valid());
}

#[test]
fn test_is_valid_invalid_nucleus() {
    let s = SyllableStructure {
        onset: "b".to_string(),
        nucleus: "xyz".to_string(), // Invalid
        coda: "n".to_string(),
    };
    assert!(!s.is_valid());
}

#[test]
fn test_is_valid_invalid_combination() {
    // "ưi" is open-only — "ưin" must be invalid (this is what made English "win"
    // wrongly validate as Vietnamese before the VCPair table was added).
    let s = SyllableStructure {
        onset: "".to_string(),
        nucleus: "ưi".to_string(),
        coda: "n".to_string(),
    };
    assert!(!s.is_valid(), "ưin must be invalid (ưi is open-only)");
}

#[test]
fn test_iep_iec_are_valid() {
    // Regression: the old table wrongly rejected "iê"+"p"/"c". These are real
    // Vietnamese words — tiếp, hiếp (iếp) and biếc, tiếc (iếc).
    assert!(SyllableStructure::parse("tiếp").is_valid(), "tiếp must be valid");
    assert!(SyllableStructure::parse("biếc").is_valid(), "biếc must be valid");
}

#[test]
fn test_upgraded_combination_constraints() {
    // Valid forms across the expanded nucleus set.
    for w in ["thuê", "yên", "yêu", "quýnh", "giếng", "boong", "xoong", "tuần", "khuê"] {
        assert!(SyllableStructure::parse(w).is_valid(), "{w} should be valid");
    }
    // Invalid nucleus+coda pairs that the thin table used to let through.
    for (nucleus, coda) in [("ư", "p"), ("ơ", "c"), ("oe", "m"), ("ăm", "")] {
        let s = SyllableStructure {
            onset: "".to_string(),
            nucleus: nucleus.to_string(),
            coda: coda.to_string(),
        };
        // "ăm" as a nucleus is itself invalid (not a real nucleus); the others
        // are valid nuclei with illegal codas.
        assert!(!s.is_valid(), "{nucleus}+{coda} should be invalid");
    }
}

#[test]
fn test_extract_onset() {
    assert_eq!(extract_onset("thuong"), "th"); // Use normalized form
    assert_eq!(extract_onset("nghe"), "ngh");
    assert_eq!(extract_onset("ba"), "b");
    assert_eq!(extract_onset("a"), "");
}

#[test]
fn test_extract_coda() {
    assert_eq!(extract_coda("an"), "n");
    assert_eq!(extract_coda("ang"), "ng");
    assert_eq!(extract_coda("anh"), "nh");
    assert_eq!(extract_coda("a"), "");
}

#[test]
fn test_real_vietnamese_words() {
    // Common Vietnamese words
    assert!(SyllableStructure::parse("việt").is_valid());
    assert!(SyllableStructure::parse("nam").is_valid());
    assert!(SyllableStructure::parse("người").is_valid());
    assert!(SyllableStructure::parse("trời").is_valid());
    assert!(SyllableStructure::parse("hòa").is_valid());
    assert!(SyllableStructure::parse("bình").is_valid());
}

#[test]
fn test_invalid_english_words() {
    // English words should parse but may not be valid
    let s = SyllableStructure::parse("xyz");
    // "xyz" parses as onset="x", nucleus="y", coda="z"
    // But "y" alone is valid, "z" is not a valid coda
    assert!(!s.is_valid());
}

#[test]
fn test_duoc_variants() {
    // được (with ươ) should be valid
    let duoc_correct = SyllableStructure::parse("được");
    println!("được: onset='{}', nucleus='{}', coda='{}'", duoc_correct.onset, duoc_correct.nucleus, duoc_correct.coda);
    assert_eq!(duoc_correct.onset, "đ");
    assert_eq!(duoc_correct.nucleus, "ươ");
    assert_eq!(duoc_correct.coda, "c");
    assert!(duoc_correct.is_valid(), "được should be valid");

    // đuợc (with uơ) should be INVALID due to uơ+coda rule
    let duoc_incorrect = SyllableStructure::parse("đuợc");
    println!("đuợc: onset='{}', nucleus='{}', coda='{}'", duoc_incorrect.onset, duoc_incorrect.nucleus, duoc_incorrect.coda);
    assert_eq!(duoc_incorrect.onset, "đ");
    assert_eq!(duoc_incorrect.nucleus, "uơ");
    assert_eq!(duoc_incorrect.coda, "c");
    assert!(!duoc_incorrect.is_valid(), "đuợc should be INVALID (uơ + coda)");
}
