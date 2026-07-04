//! Vietnamese Syllable Structure Parser
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-engine/tests/pipeline_validation_tests.rs`.
//!
//! Parses Vietnamese syllables into components: Onset, Nucleus, Coda
//!
//! ## Vietnamese Syllable Structure
//!
//! Vietnamese syllables follow the pattern: (C₁)V(C₂)
//! - C₁: Optional initial consonant or consonant cluster
//! - V: Required vowel nucleus (single or cluster)
//! - C₂: Optional final consonant
//!
//! ## Examples
//!
//! - "a" → Onset: "", Nucleus: "a", Coda: ""
//! - "ba" → Onset: "b", Nucleus: "a", Coda: ""
//! - "ban" → Onset: "b", Nucleus: "a", Coda: "n"
//! - "thường" → Onset: "th", Nucleus: "ườ", Coda: "ng"
//!
//! ## Attested-syllable lookup
//!
//! [`is_attested`] / [`is_shape_attested`] test a syllable against the
//! embedded `attested_data` bitset — see `data/attested-syllables.txt` for
//! the data provenance. The (`onset_id`, `nucleus_id`, `coda_id`, `tone_id`)
//! decomposition is the single source of truth shared by the accessors here
//! and by `examples/gen_attested_syllables.rs`, which builds the bitset.

use std::collections::HashSet;

use crate::pipeline::attested_data;
use crate::pipeline::config::ToneMark;

/// Vietnamese syllable structure
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyllableStructure {
    /// Initial consonant(s): "", "b", "tr", "ngh"
    pub onset: String,
    
    /// Vowel nucleus: "a", "oa", "uye"
    pub nucleus: String,
    
    /// Final consonant: "", "n", "ng", "ch"
    pub coda: String,
}

impl SyllableStructure {
    /// Parse a Vietnamese syllable into components
    ///
    /// ## Algorithm
    ///
    /// 1. Normalize Vietnamese characters to base form (remove tones)
    /// 2. Extract onset (longest matching consonant cluster from start)
    /// 3. Extract coda (longest matching final consonant from end)
    /// 4. Remaining middle part is nucleus
    ///
    /// ## Example
    ///
    /// ```
    /// use buttre_engine::pipeline::validation::SyllableStructure;
    ///
    /// let structure = SyllableStructure::parse("thường");
    /// assert_eq!(structure.onset, "th");
    /// assert_eq!(structure.nucleus, "ươ");
    /// assert_eq!(structure.coda, "ng");
    /// ```
    pub fn parse(syllable: &str) -> Self {
        // Algorithm Step 0: Normalize to lowercase and remove tones
        let syllable_normalized = normalize_vietnamese(syllable);

        // Algorithm Steps 1-2: split into (onset, nucleus, coda) slices
        let (onset, nucleus, coda) = split_parts(&syllable_normalized);

        Self {
            onset: onset.to_string(),
            nucleus: nucleus.to_string(),
            coda: coda.to_string(),
        }
    }
    
    /// Check if this syllable structure is valid Vietnamese
    ///
    /// ## Algorithm
    ///
    /// Validates:
    /// 1. Onset is in valid onset list
    /// 2. Nucleus is in valid nucleus list
    /// 3. Coda is in valid coda list
    /// 4. Onset-Nucleus-Coda combination is valid
    pub fn is_valid(&self) -> bool {
        parts_are_valid(&self.onset, &self.nucleus, &self.coda)
    }
    
    /// Check if the onset-nucleus-coda combination is valid Vietnamese.
    ///
    /// ## Source
    ///
    /// Ported from Unikey `ukengine` `VCPairList` (the exhaustive vowel×coda
    /// table) plus the `isValidCVC` onset exceptions.  Three layers:
    ///
    /// 1. **Open syllable** (empty coda) → always valid.
    /// 2. **Onset exceptions** — an onset that rescues an otherwise-invalid VC:
    ///    `qu` + `y` + `n`/`nh` (quýnh, quynh); `gi` + `e`/`ê` + `n`/`ng`
    ///    (giếng — the `gi` onset absorbs the `i`).
    /// 3. **Per-nucleus allowed-coda set** — every nucleus that can take a coda
    ///    lists exactly which codas are legal; nuclei ending in a glide
    ///    (`i`/`o`/`u`/`y`) or otherwise open-only fall through to `false`.
    ///
    /// This makes invalid forms like `ưin`, `ưan`, `ơc`, `oem` correctly invalid
    /// while keeping `việt`, `tiếp`, `biếc`, `thường`, `quýnh`, `giếng` valid.
    ///
    /// ## Coda "k" (P6 — Đắk Lắk class place names)
    ///
    /// Coda `k` is legal ONLY for nuclei `u` and `ă` — derived from the 9
    /// previously-unembeddable dict entries (`búk`, `úk`; `lăk`, `lắk`, `măk`,
    /// `ăk`, `đăk`, `đắk`, `ắk` — see `data/attested-syllables.txt`'s header).
    /// This is deliberately NOT a blanket "k" allowance for every nucleus:
    /// `đik`/`đok` must stay invalid (no dict evidence for those shapes), so
    /// each nucleus arm below either includes `"k"` or does not, per the data.
    /// The structural coda TABLE (`VALID_CODAS`) accepts `k` unconditionally —
    /// this per-nucleus combination check is the only gate that limits it, and
    /// it is the reason attestation-lookup (`is_attested`/`decompose_ids`,
    /// which do not consult this function) can still embed `búk`/`đắk` while
    /// `could_be_vietnamese` (which DOES consult this function) keeps rejecting
    /// unattested k-coda shapes. Known accepted trade-off (red-team M1): the
    /// adjacent Telex `aw`→`ă` / tone-`r`→hook-on-`u` paths are deliberately
    /// ungated (same as `how`→`hơ`), so English `hawk`/`gawk`/`murk` now also
    /// pass this check (`hăk`/`găk`/`mủk`) — pinned as known behavior in
    /// `buttre-core`'s golden corpus, not fixed here.
    fn is_valid_combination(&self) -> bool {
        combination_is_valid(&self.onset, &self.nucleus, &self.coda)
    }
}

/// Free-function core of the validity check — operates on borrowed slices so
/// the zero-alloc paths ([`is_valid_syllable_fast`], [`split_parts`] callers)
/// never need to build a `SyllableStructure` with three owned Strings.
pub(crate) fn parts_are_valid(onset: &str, nucleus: &str, coda: &str) -> bool {
    VALID_ONSETS.contains(&onset)
        && !nucleus.is_empty()
        && VALID_NUCLEI.contains(&nucleus)
        && VALID_CODAS.contains(&coda)
        && combination_is_valid(onset, nucleus, coda)
}

/// Slice-based body of [`SyllableStructure::is_valid_combination`] — see that
/// method's doc for the full Unikey `VCPairList` provenance and the coda-"k"
/// rules.
fn combination_is_valid(onset: &str, n: &str, c: &str) -> bool {
    {
        // Layer 1: open syllable is always structurally valid.
        if c.is_empty() {
            return true;
        }

        // Layer 2: onset-rescued exceptions (Unikey isValidCVC).
        if onset == "qu" && n == "y" && matches!(c, "n" | "nh") {
            return true;
        }
        if onset == "gi" && matches!(n, "e" | "ê") && matches!(c, "n" | "ng") {
            return true;
        }

        // Layer 3: per-nucleus allowed coda set (Unikey VCPairList).
        match n {
            "a" => matches!(c, "c" | "ch" | "m" | "n" | "ng" | "nh" | "p" | "t"),
            // "ă" alone (not "â") also takes "k" — see the coda-"k" doc above.
            "ă" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t" | "k"),
            "â" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
            "e" => matches!(c, "c" | "ch" | "m" | "n" | "ng" | "nh" | "p" | "t"),
            "ê" => matches!(c, "c" | "ch" | "m" | "n" | "nh" | "p" | "t"),
            "i" => matches!(c, "c" | "ch" | "m" | "n" | "nh" | "p" | "t"),
            "o" | "ô" | "oo" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
            "ơ" => matches!(c, "m" | "n" | "p" | "t"),
            // "u" also takes "k" — see the coda-"k" doc above.
            "u" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t" | "k"),
            "ư" => matches!(c, "c" | "m" | "n" | "ng" | "t"),
            "y" => c == "t",
            "iê" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
            "oa" => matches!(c, "c" | "ch" | "m" | "n" | "ng" | "nh" | "p" | "t"),
            "oă" => matches!(c, "c" | "m" | "n" | "ng" | "t"),
            "oe" => matches!(c, "n" | "t"),
            "uâ" | "ua" => matches!(c, "n" | "ng" | "t"),
            "uê" | "ue" => matches!(c, "c" | "ch" | "n" | "nh"),
            "uô" | "uo" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
            "ươ" | "ưo" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
            "uy" => matches!(c, "c" | "ch" | "n" | "nh" | "p" | "t"),
            "yê" | "ye" => matches!(c, "m" | "n" | "ng" | "p" | "t"),
            "uyê" | "uye" => matches!(c, "n" | "t"),
            // Every other nucleus is open-only; a non-empty coda makes it invalid.
            _ => false,
        }
    }
}

/// Normalize Vietnamese text to base form (remove tone marks)
///
/// ## Algorithm
///
/// Converts Vietnamese characters with tones to their base forms:
/// - á, à, ả, ã, ạ → a
/// - ế, ề, ể, ễ, ệ → ê
/// - etc.
///
/// This allows syllable structure parsing to work with toned text.
pub fn normalize_vietnamese(text: &str) -> String {
    // flat_map(char::to_lowercase) is exactly `text.to_lowercase()` streamed
    // per char — one allocation for the result instead of two.
    text.chars()
        .flat_map(char::to_lowercase)
        .map(strip_tone_char)
        .collect()
}

/// Per-char body of [`normalize_vietnamese`]: map an already-lowercased char
/// to its tone-stripped base form (diacritic transforms ă/â/ê/ô/ơ/ư/đ are
/// preserved — only the five tone marks are removed).
fn strip_tone_char(c: char) -> char {
    match c {
            // a variants
            'á' | 'à' | 'ả' | 'ã' | 'ạ' => 'a',
            'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' => 'ă',
            'ấ' | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' => 'â',
            
            // e variants
            'é' | 'è' | 'ẻ' | 'ẽ' | 'ẹ' => 'e',
            'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => 'ê',
            
            // i variants
            'í' | 'ì' | 'ỉ' | 'ĩ' | 'ị' => 'i',
            
            // o variants
            'ó' | 'ò' | 'ỏ' | 'õ' | 'ọ' => 'o',
            'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' => 'ô',
            'ớ' | 'ờ' | 'ở' | 'ỡ' | 'ợ' => 'ơ',
            
            // u variants
            'ú' | 'ù' | 'ủ' | 'ũ' | 'ụ' => 'u',
            'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => 'ư',
            
            // y variants
            'ý' | 'ỳ' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
            
            // đ
            'đ' => 'đ',

            // Keep everything else
            other => other,
    }
}

/// Split an ALREADY-NORMALIZED syllable (lowercase, tone-stripped) into
/// borrowed (onset, nucleus, coda) slices — the zero-alloc core that
/// [`SyllableStructure::parse`] wraps with owned Strings for its public
/// struct form.
pub(crate) fn split_parts(normalized: &str) -> (&str, &str, &str) {
    let onset = extract_onset(normalized);
    let after_onset = &normalized[onset.len()..];
    let coda = extract_coda(after_onset);
    let nucleus = &after_onset[..after_onset.len() - coda.len()];
    (onset, nucleus, coda)
}

/// [`normalize_vietnamese`] into a caller-provided stack buffer — zero heap
/// allocation for every real syllable (≤16 chars, ≤4 UTF-8 bytes each fits
/// 64 bytes comfortably). Returns `None` when the input does not fit; callers
/// fall back to the heap path, so overflow is a slow-path, never an error.
pub(crate) fn normalize_vietnamese_into<'a>(text: &str, buf: &'a mut [u8; 64]) -> Option<&'a str> {
    let mut len = 0usize;
    for c in text.chars().flat_map(char::to_lowercase) {
        let mapped = strip_tone_char(c);
        let mut tmp = [0u8; 4];
        let encoded = mapped.encode_utf8(&mut tmp);
        if len + encoded.len() > buf.len() {
            return None;
        }
        buf[len..len + encoded.len()].copy_from_slice(encoded.as_bytes());
        len += encoded.len();
    }
    // Concatenation of encode_utf8 outputs is valid UTF-8 by construction.
    std::str::from_utf8(&buf[..len]).ok()
}

/// Zero-alloc equivalent of `SyllableStructure::parse(text).is_valid()` —
/// the per-candidate validity probe on `compose()`'s hot path (the parse
/// form costs 4 allocations per call; this one costs none for any real
/// syllable).
pub(crate) fn is_valid_syllable_fast(text: &str) -> bool {
    let mut buf = [0u8; 64];
    match normalize_vietnamese_into(text, &mut buf) {
        Some(normalized) => {
            let (onset, nucleus, coda) = split_parts(normalized);
            parts_are_valid(onset, nucleus, coda)
        }
        // Longer than any real syllable: fall back to the heap path rather
        // than guessing (parse handles arbitrary input).
        None => SyllableStructure::parse(text).is_valid(),
    }
}

/// Extract onset (initial consonant cluster) from syllable
///
/// ## Algorithm
///
/// Try to match longest valid onset from the start of syllable.
/// Returns the matched onset string.
pub fn extract_onset(syllable: &str) -> &str {
    // Try 3-char onsets first (longest)
    for &onset in VALID_ONSETS_3CHAR {
        if syllable.starts_with(onset) {
            return onset;
        }
    }
    
    // Try 2-char onsets
    for &onset in VALID_ONSETS_2CHAR {
        if let Some(after) = syllable.strip_prefix(onset) {
            // "gi" is ambiguous: either the onset digraph followed by a
            // distinct nucleus vowel (già, giường — "i" is purely a marker of
            // the palatal onset), or the onset "g" where the lone "i" IS the
            // nucleus (gì, gìn, gích, gíp — Vietnamese spelling never doubles
            // the "i" to write both the onset marker and the nucleus vowel).
            // Re-split to onset "g" whenever keeping the full "gi" onset
            // would leave nothing for the nucleus to claim.
            if onset == "gi" && after.len() == extract_coda(after).len() {
                return "g";
            }
            return onset;
        }
    }
    
    // Try 1-char onsets
    for &onset in VALID_ONSETS_1CHAR {
        if syllable.starts_with(onset) {
            return onset;
        }
    }
    
    // No onset (vowel-initial syllable)
    ""
}

/// Extract coda (final consonant) from remaining syllable
///
/// ## Algorithm
///
/// Try to match longest valid coda from the end of syllable.
/// Returns the matched coda string.
pub fn extract_coda(remaining: &str) -> &str {
    // Try 2-char codas first (longest)
    for &coda in VALID_CODAS_2CHAR {
        if remaining.ends_with(coda) {
            return coda;
        }
    }
    
    // Try 1-char codas
    for &coda in VALID_CODAS_1CHAR {
        if remaining.ends_with(coda) {
            return coda;
        }
    }
    
    // No coda (open syllable)
    ""
}

// Vietnamese Phonology Constants

/// Valid 3-character onsets
const VALID_ONSETS_3CHAR: &[&str] = &[
    "ngh", // nghệ, nghĩa
];

/// Valid 2-character onsets.
/// `dz` is non-standard but common in informal/stylized writing (dzô, dzậy, dzui).
const VALID_ONSETS_2CHAR: &[&str] = &[
    "ch", "gh", "gi", "kh", "ng", "nh", "ph", "qu", "th", "tr", "dz",
];

/// Valid 1-character onsets.
/// `z` is non-standard but common in informal writing (zô, zui, zậy).
const VALID_ONSETS_1CHAR: &[&str] = &[
    "b", "c", "d", "đ", "g", "h", "k", "l", "m", "n", "p", "r", "s", "t", "v", "x", "z",
];

/// All valid onsets (including empty)
const VALID_ONSETS: &[&str] = &[
    "", // Empty onset (vowel-initial)
    // 1-char
    "b", "c", "d", "đ", "g", "h", "k", "l", "m", "n", "p", "r", "s", "t", "v", "x", "z",
    // 2-char
    "ch", "gh", "gi", "kh", "ng", "nh", "ph", "qu", "th", "tr", "dz",
    // 3-char
    "ngh",
];

/// Valid 2-character codas
const VALID_CODAS_2CHAR: &[&str] = &[
    "ch", "ng", "nh",
];

/// Valid 1-character codas.
///
/// `k` (P6) has no independent Vietnamese phonemic value distinct from `c` —
/// it exists in the table only to embed the Đắk Lắk place-name class
/// (`búk`/`lăk`/`đắk`/…). Which NUCLEUS may combine with it is restricted in
/// `is_valid_combination` (per-nucleus rows: `u`/`ă` only), so accepting `k`
/// unconditionally HERE only affects `extract_coda`/attestation-lookup id
/// decomposition, never the structural plausibility check.
const VALID_CODAS_1CHAR: &[&str] = &[
    "c", "m", "n", "p", "t", "k",
];

/// All valid codas (including empty)
const VALID_CODAS: &[&str] = &[
    "", // Empty coda (open syllable)
    // 1-char
    "c", "m", "n", "p", "t", "k",
    // 2-char
    "ch", "ng", "nh",
];

/// Valid vowel nuclei — written base forms (lowercase, tones removed).
///
/// ## Source
///
/// Ported from Unikey `ukengine` `VSeqList` (the exhaustive vowel-sequence
/// table), cross-checked against Bamboo `vowelSeqs` and OpenKey `_vowelForMark`.
/// Includes the loanword monophthong `oo` (boong/soong/xoong — present in
/// Bamboo/OpenKey, absent from Unikey) and the diacritic-incomplete intermediate
/// forms (`uo`, `ưo`, …) so partially-typed buffers are not rejected mid-compose.
const VALID_NUCLEI: &[&str] = &[
    // Monophthongs
    "a", "ă", "â", "e", "ê", "i", "o", "ô", "ơ", "u", "ư", "y",
    // Loanword monophthong
    "oo",
    // Diphthongs (2 letters)
    "ai", "ao", "au", "ay", "âu", "ây",
    "eo", "êu",
    "ia", "ie", "iê", "iu",
    "oa", "oă", "oe", "oi", "ôi", "ơi",
    "ua", "uâ", "ue", "uê", "ui", "uo", "uô", "uơ", "uy",
    "ưa", "ưi", "ưo", "ươ", "ưu",
    "ye", "yê",
    // Triphthongs (3 letters) — including diacritic-incomplete bare transients
    // (ieu→iêu, uoi→uôi/ươi, yeu→yêu) so partial typing is not rejected.
    "iêu", "ieu",
    "oai", "oao", "oay", "oeo",
    "uao", "uây", "uôi", "uoi", "uou", "uơi", "uya", "uye", "uyê", "uyu",
    "ươi", "ươu",
    "yêu", "yeu",
];

// ── Attested-syllable id decomposition (shared: accessors + generator) ────────
//
// The attested-syllable bitset is indexed by (onset_id, nucleus_id, coda_id,
// tone_id). The dimension sizes and the id functions below are the ONLY place
// that knows the mapping from a parsed component to its bitset axis — both
// `is_attested`/`is_shape_attested` (below) and
// `examples/gen_attested_syllables.rs` (which builds the bitset) call these
// same functions, so the two sides can never encode/decode with different
// tables.

/// Number of structurally valid onsets, including the empty onset.
/// Dimension size for the attested-syllable bitset's onset axis.
pub const NUM_ONSETS: usize = VALID_ONSETS.len();

/// Number of structurally valid nuclei. Dimension size for the
/// attested-syllable bitset's nucleus axis.
pub const NUM_NUCLEI: usize = VALID_NUCLEI.len();

/// Number of structurally valid codas, including the empty coda.
/// Dimension size for the attested-syllable bitset's coda axis.
pub const NUM_CODAS: usize = VALID_CODAS.len();

/// Number of `ToneMark` variants (None/ngang, Acute/sắc, Grave/huyền,
/// Hook/hỏi, Tilde/ngã, Dot/nặng). Dimension size for the attested-syllable
/// bitset's tone axis.
pub const NUM_TONES: usize = 6;

/// Position of `onset` within the valid-onset table — the id used as the
/// first axis of the attested-syllable bitset. `None` if `onset` is not a
/// structurally valid Vietnamese onset.
pub fn onset_id(onset: &str) -> Option<usize> {
    VALID_ONSETS.iter().position(|&o| o == onset)
}

/// Position of `nucleus` within the valid-nucleus table.
///
/// `nucleus` is the mark-bearing vowel cluster WITHOUT tone (e.g. "iê", "â") —
/// the id used as the second axis of the attested-syllable bitset. `None` if
/// `nucleus` is not a structurally valid Vietnamese nucleus.
pub fn nucleus_id(nucleus: &str) -> Option<usize> {
    VALID_NUCLEI.iter().position(|&n| n == nucleus)
}

/// Position of `coda` within the valid-coda table.
///
/// This is the id used as the third axis of the attested-syllable bitset.
/// `None` if `coda` is not a structurally valid Vietnamese coda.
pub fn coda_id(coda: &str) -> Option<usize> {
    VALID_CODAS.iter().position(|&c| c == coda)
}

/// Column index for `tone` — the id used as the fourth axis of the
/// attested-syllable bitset.
///
/// Matches `ToneMark`'s declaration order, the same order
/// `crate::tone::tables` uses internally (0=None … 5=Dot), so a syllable's
/// tone maps to the same column everywhere in the crate.
pub const fn tone_id(tone: ToneMark) -> usize {
    tone as usize
}

/// Flatten (`onset_id`, `nucleus_id`, `coda_id`, `tone_id`) into a single bit
/// index for the attested-syllable bitset (row-major, tone varying fastest).
///
/// Shared by the generator (which sets bits) and `attested_data::is_set`
/// (which reads them), so the two can never encode/decode with different
/// layouts.
pub const fn bit_index(onset_id: usize, nucleus_id: usize, coda_id: usize, tone_id: usize) -> usize {
    ((onset_id * NUM_NUCLEI + nucleus_id) * NUM_CODAS + coda_id) * NUM_TONES + tone_id
}

/// Scan `syllable` (already NFC + lowercase) for a tone diacritic. Vietnamese
/// syllables carry at most one tone; if two DIFFERENT tone marks are found —
/// malformed input, e.g. mashed-together text — this returns `None` rather
/// than guessing which one is authoritative.
fn extract_tone(syllable: &str) -> Option<ToneMark> {
    let mut found = ToneMark::None;
    for ch in syllable.chars() {
        let (_, tone) = crate::tone::strip(ch);
        if tone != ToneMark::None {
            if found != ToneMark::None && found != tone {
                return None;
            }
            found = tone;
        }
    }
    Some(found)
}

/// Decompose a Vietnamese syllable (any case, NFC or NFD input) into the
/// (`onset_id`, `nucleus_id`, `coda_id`, `tone_id`) tuple used to index the
/// attested-syllable bitset.
///
/// Returns `None` when the syllable's onset, nucleus, or coda falls outside
/// the structural phonology tables, or when it carries more than one distinct
/// tone mark — both cases mean "not a decomposable Vietnamese syllable", not
/// an error worth surfacing to the caller.
pub fn decompose_ids(syllable: &str) -> Option<(usize, usize, usize, usize)> {
    let normalized = nfc_lowercase(syllable);
    let tone = extract_tone(&normalized)?;
    let mut buf = [0u8; 64];
    let (o, n, c) = match normalize_vietnamese_into(&normalized, &mut buf) {
        Some(stripped) => {
            let (onset, nucleus, coda) = split_parts(stripped);
            (onset_id(onset)?, nucleus_id(nucleus)?, coda_id(coda)?)
        }
        None => {
            let structure = SyllableStructure::parse(&normalized);
            (
                onset_id(&structure.onset)?,
                nucleus_id(&structure.nucleus)?,
                coda_id(&structure.coda)?,
            )
        }
    };
    Some((o, n, c, tone_id(tone)))
}

/// NFC + lowercase with zero allocation on the hot path: `compose()`'s
/// attestation-gate texts are always already NFC (built from precomposed
/// table entries) and already lowercase (the case mask is applied AFTER the
/// gate runs), so both conversions borrow in the overwhelmingly common case.
fn nfc_lowercase(syllable: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    use unicode_normalization::{is_nfc_quick, IsNormalized};

    let nfc: Cow<'_, str> = if matches!(is_nfc_quick(syllable.chars()), IsNormalized::Yes) {
        Cow::Borrowed(syllable)
    } else {
        Cow::Owned(crate::unicode::normalize_nfc(syllable))
    };
    if nfc.chars().any(char::is_uppercase) {
        match nfc {
            Cow::Borrowed(s) => Cow::Owned(s.to_lowercase()),
            Cow::Owned(s) => Cow::Owned(s.to_lowercase()),
        }
    } else {
        nfc
    }
}

/// Test whether `syllable` is an attested Vietnamese syllable, exact tone
/// match (no tone counts as ngang). Input may be any case, NFC or NFD.
///
/// Fails open: any input that cannot be decomposed into (onset, nucleus,
/// coda, tone) — because it is not shaped like a Vietnamese syllable at all —
/// returns `false` rather than panicking. This is a lookup gate, not a
/// validator: an unparseable candidate should be demoted to a literal, never
/// crash the pipeline.
///
/// ## Examples
///
/// ```
/// use buttre_engine::pipeline::validation::is_attested;
///
/// assert!(is_attested("việt"));
/// assert!(is_attested("hoà"));
/// assert!(is_attested("HÒA")); // case-insensitive
/// assert!(!is_attested("fâllb"));
/// ```
pub fn is_attested(syllable: &str) -> bool {
    match decompose_ids(syllable) {
        Some((o, n, c, t)) => attested_data::is_set(o, n, c, t),
        None => false,
    }
}

/// Test whether `syllable`'s (onset, nucleus, coda) SHAPE is attested under
/// ANY tone, ignoring whatever tone `syllable` itself carries (if any).
///
/// Used to gate non-alphabetic transform triggers where the tone has not
/// been typed yet: e.g. `is_shape_attested("nhât")` is `true` because
/// "nhất" (nh + â + t + sắc) is attested, even though "nhât" itself carries
/// no tone.
///
/// Fails open exactly like [`is_attested`]: an unparseable shape returns
/// `false`, never panics.
pub fn is_shape_attested(syllable: &str) -> bool {
    let normalized = nfc_lowercase(syllable);
    let mut buf = [0u8; 64];
    let ids = match normalize_vietnamese_into(&normalized, &mut buf) {
        Some(stripped) => {
            let (onset, nucleus, coda) = split_parts(stripped);
            (onset_id(onset), nucleus_id(nucleus), coda_id(coda))
        }
        None => {
            let structure = SyllableStructure::parse(&normalized);
            (
                onset_id(&structure.onset),
                nucleus_id(&structure.nucleus),
                coda_id(&structure.coda),
            )
        }
    };
    let (Some(o), Some(n), Some(c)) = ids else {
        return false;
    };
    (0..NUM_TONES).any(|t| attested_data::is_set(o, n, c, t))
}

/// Overlay-aware attested check (event-sourcing-completion Phase 5): ORs
/// the static bitset with a user-attested overlay — bit-indices (via
/// [`bit_index`]) for syllables the user typed directly and committed ≥3
/// distinct times (see `buttre_core::state::learning::LearningStore`).
///
/// `overlay: None` (no store wired, or nothing learned yet) is
/// BYTE-IDENTICAL to [`is_attested`] — every golden/regression test that
/// never wires a store exercises exactly that path.
///
/// This is the SINGLE consult point both P3's word-boundary closed gate
/// (`compose::passes_attestation_gate`) and P2's evidence-based un-latch
/// probe (`pipeline::stages::compose_stage::should_unlatch`) call through —
/// do not duplicate this OR-check elsewhere.
///
/// Fails open exactly like [`is_attested`]: an unparseable `syllable`
/// returns `false`, never panics.
pub fn is_attested_overlay(syllable: &str, overlay: Option<&HashSet<u32>>) -> bool {
    match decompose_ids(syllable) {
        Some((o, n, c, t)) => {
            attested_data::is_set(o, n, c, t)
                || overlay.is_some_and(|set| set.contains(&(bit_index(o, n, c, t) as u32)))
        }
        None => false,
    }
}

