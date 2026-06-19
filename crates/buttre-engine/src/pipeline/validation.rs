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
        
        // Algorithm Step 1: Extract onset (initial consonant cluster)
        let onset = extract_onset(&syllable_normalized);
        let after_onset = &syllable_normalized[onset.len()..];
        
        // Algorithm Step 2: Extract coda (final consonant)
        let coda = extract_coda(after_onset);
        let nucleus_end = after_onset.len() - coda.len();
        let nucleus = &after_onset[..nucleus_end];
        
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
        self.is_valid_onset() && 
        self.is_valid_nucleus() && 
        self.is_valid_coda() &&
        self.is_valid_combination()
    }
    
    /// Check if onset is valid
    fn is_valid_onset(&self) -> bool {
        VALID_ONSETS.contains(&self.onset.as_str())
    }
    
    /// Check if nucleus is valid
    fn is_valid_nucleus(&self) -> bool {
        // Empty nucleus is invalid
        if self.nucleus.is_empty() {
            return false;
        }
        VALID_NUCLEI.contains(&self.nucleus.as_str())
    }
    
    /// Check if coda is valid
    fn is_valid_coda(&self) -> bool {
        VALID_CODAS.contains(&self.coda.as_str())
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
    fn is_valid_combination(&self) -> bool {
        let (n, c) = (self.nucleus.as_str(), self.coda.as_str());

        // Layer 1: open syllable is always structurally valid.
        if c.is_empty() {
            return true;
        }

        // Layer 2: onset-rescued exceptions (Unikey isValidCVC).
        if self.onset == "qu" && n == "y" && matches!(c, "n" | "nh") {
            return true;
        }
        if self.onset == "gi" && matches!(n, "e" | "ê") && matches!(c, "n" | "ng") {
            return true;
        }

        // Layer 3: per-nucleus allowed coda set (Unikey VCPairList).
        match n {
            "a" => matches!(c, "c" | "ch" | "m" | "n" | "ng" | "nh" | "p" | "t"),
            "ă" | "â" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
            "e" => matches!(c, "c" | "ch" | "m" | "n" | "ng" | "nh" | "p" | "t"),
            "ê" => matches!(c, "c" | "ch" | "m" | "n" | "nh" | "p" | "t"),
            "i" => matches!(c, "c" | "ch" | "m" | "n" | "nh" | "p" | "t"),
            "o" | "ô" | "oo" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
            "ơ" => matches!(c, "m" | "n" | "p" | "t"),
            "u" => matches!(c, "c" | "m" | "n" | "ng" | "p" | "t"),
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
    text.to_lowercase()
        .chars()
        .map(|c| match c {
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
        })
        .collect()
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
        if syllable.starts_with(onset) {
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

/// Valid 1-character codas
const VALID_CODAS_1CHAR: &[&str] = &[
    "c", "m", "n", "p", "t",
];

/// All valid codas (including empty)
const VALID_CODAS: &[&str] = &[
    "", // Empty coda (open syllable)
    // 1-char
    "c", "m", "n", "p", "t",
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
    "uao", "uây", "uôi", "uoi", "uou", "uơi", "uya", "uyê", "uyu",
    "ươi", "ươu",
    "yêu", "yeu",
];

