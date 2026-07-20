//! Pipeline Configuration - Config-driven input method definitions
//!
//! This module defines the configuration structure for input methods.
//! Instead of hardcoding Telex/VNI logic, we define them as configurations.

use crate::pipeline::dictionary::DictionaryProvider;
use crate::vowel::{TonePositioningMode, VowelSeqTable};
use std::collections::HashMap;
use std::sync::Arc;

/// Tone mark types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToneMark {
    /// No tone (unmarked)
    None,
    /// Acute accent (sắc) - /
    Acute,
    /// Grave accent (huyền) - \
    Grave,
    /// Hook above (hỏi) - ?
    Hook,
    /// Tilde (ngã) - ~
    Tilde,
    /// Dot below (nặng) - .
    Dot,
}

/// Tone style (old vs new orthography)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneStyle {
    /// Old style: "hoà", "toà"
    Old,
    /// New style: "hòa", "tòa"
    New,
}

/// Unicode normalization form
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnicodeForm {
    /// NFC (Canonical Composition) - single codepoint: "â"
    NFC,
    /// NFD (Canonical Decomposition) - multiple codepoints: "a" + "^"
    NFD,
}

/// Pipeline Settings
///
/// Controls which optional stages are enabled in the pipeline.
#[derive(Debug, Clone, Default)]
pub struct PipelineSettings {
    /// List of enabled optional stages
    /// Possible values: "validation", "transform", "tone", "retrofix", "orthography", "lookup"
    /// Note: "normalization", "gatekeeper", "output" are always enabled
    pub enabled: Vec<String>,

    /// Enable TSF composition events (UpdateComposition) instead of Replace
    pub use_composition: bool,
}

/// Validation Stage Settings
#[derive(Debug, Clone)]
pub struct ValidationSettings {
    /// Syllable structure type: "vietnamese", "hmong", "custom", "none"
    pub syllable_structure: String,

    /// Allow invalid syllables to pass through
    pub allow_invalid: bool,

    /// Strict Vietnamese spelling control (the config window's "Kiểm soát
    /// gắt gao chính tả tiếng Việt" switch). `false` (default) keeps the
    /// Unikey-style leniency for deliberate-đ abbreviations: a vowel-less
    /// consonant cluster starting with `đ` ("đt", "đc", "đkkd") stays
    /// composed instead of reverting to its raw keystrokes — `đ` can only
    /// arise from an explicit transform ("dd"/"d9"), so the Vietnamese
    /// intent is unambiguous. `true` restores the strict revert.
    pub strict_spelling: bool,
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self {
            syllable_structure: "vietnamese".to_string(),
            allow_invalid: false,
            strict_spelling: false,
        }
    }
}

/// Orthography Stage Settings
#[derive(Debug, Clone)]
pub struct OrthographySettings {
    /// Tone style: "modern" or "traditional"
    pub tone_style: String,

    /// Unicode form: "nfc" or "nfd"
    pub unicode_form: String,
}

impl Default for OrthographySettings {
    fn default() -> Self {
        Self {
            tone_style: "modern".to_string(),
            unicode_form: "nfc".to_string(),
        }
    }
}

/// Lookup Stage Settings
#[derive(Debug, Clone)]
pub struct LookupSettings {
    /// Database file path
    pub database: String,

    /// Maximum number of candidates to return
    pub max_candidates: usize,

    /// Auto replace buffer with top candidate
    pub auto_replace: bool,

    /// Space behavior when candidates are shown
    /// - "auto_select_single": Auto-select if exactly 1 candidate, otherwise add to search
    /// - "always_select": Always select first candidate (if available)
    /// - "always_search": Always add space to search keywords
    /// - "passthrough": Let space pass through normally
    pub space_behavior: String,

    /// Enter behavior when candidates are shown
    /// - "select_first": Select first candidate
    /// - "select_current": Select currently highlighted candidate
    /// - "passthrough": Let enter pass through normally
    pub enter_behavior: String,
}

/// Tone Configuration (NEW - Flexible Typing)
///
/// This configuration enables flexible typing features for Vietnamese input:
/// - Free marking: Allow tone on any vowel (not just phonologically correct)
/// - Permutation: Support multiple typing orders (truongwf, truwongf → trường)
///
/// ## Learning from Unikey
///
/// This design is inspired by Unikey's `freeMarking` option:
/// - Reference: `.reference/fcitx5-unikey/unikey/keycons.h`
/// - Unikey allows users to toggle between strict phonology and free marking
///
/// ## Architecture
///
/// - Data (vowel_sequences) comes from buttre-core/keyboard config
/// - Algorithms use this config in pipeline stages (Stage 4, 5)
#[derive(Debug, Clone)]
pub struct ToneConfig {
    /// Allow free tone placement (tone on any vowel in cluster)
    ///
    /// When `false` (default): Follow Vietnamese phonology rules
    /// - Super vowels (ă, â, ê, ô, ơ) always receive tone
    /// - qu/gi patterns skip the u/i
    /// - Triple vowels: tone on middle
    ///
    /// When `true`: Allow tone on any vowel user selects
    /// - More flexible but may create non-standard orthography
    /// - Useful for typing non-standard words, names, or dialects
    pub free_marking: bool,

    /// Allow permutation matching (flexible typing order)
    ///
    /// When `false` (default): Strict input order
    /// - Must type: truongwf (standard order)
    ///
    /// When `true`: Flexible input order
    /// - Can type: truongwf, truwongf, truowfng → all produce "trường"
    /// - Stage 4 uses permutation matcher
    pub allow_permutation: bool,

    /// Maximum characters to search backward for marking (Unikey: MAX_MODIFY_LENGTH = 6)
    ///
    /// Controls how far back the algorithm searches for vowels to modify.
    /// - When `free_marking` is false: Only last vowel position is considered
    /// - When `free_marking` is true: Search up to this many positions back
    ///
    /// This prevents accidental modification of distant characters in long words.
    pub max_modify_length: usize,

    /// Auto-correct "uo" to "ươ" when applying tone (Unikey algorithm)
    ///
    /// When `true`: "nguoif" → "người" (not "nguòi")
    /// - Automatically adds horn marks when user applies tone to plain "uo"
    /// - Handles common typo of forgetting to add 'w' for ư/ơ
    ///
    /// When `false`: "nguoif" → "nguòi" (literal interpretation)
    pub auto_correct_uo: bool,

    /// Vowel sequence table (73 Vietnamese sequences)
    ///
    /// This table is populated by the keyboard config layer (buttre-core)
    /// and consumed by pipeline stages for tone positioning.
    ///
    /// Empty by default - config builders should populate this.
    pub vowel_sequences: VowelSeqTable,

    /// Tone positioning mode
    ///
    /// Determines the algorithm for finding where to place tone marks.
    /// - Phonology: Use Vietnamese orthography rules
    /// - Free: Use nearest vowel to input position
    pub positioning_mode: TonePositioningMode,
}

impl Default for LookupSettings {
    fn default() -> Self {
        Self {
            database: "buttre_nom.db".to_string(),
            max_candidates: 9,
            auto_replace: false,
            space_behavior: "auto_select_single".to_string(), // Nôm default
            enter_behavior: "passthrough".to_string(),        // Let Enter work normally (new line)
        }
    }
}

impl Default for ToneConfig {
    fn default() -> Self {
        Self {
            free_marking: false,                     // Default: strict phonology rules
            allow_permutation: false,                // Default: strict input order
            max_modify_length: 6,                    // Unikey's MAX_MODIFY_LENGTH = 6
            auto_correct_uo: false,                  // Default: disabled for backward compatibility
            vowel_sequences: VowelSeqTable::empty(), // Empty by default
            positioning_mode: TonePositioningMode::Phonology,
        }
    }
}

/// Pipeline Configuration
///
/// Defines all the rules and settings for an input method.
///
/// ## Enhanced Format
///
/// The new config format supports:
/// - Configurable pipeline stages (enable/disable)
/// - Language-specific settings
/// - Multi-language support (Vietnamese, Nôm, Bahnar, Hmong, etc.)
/// - **Context Rules**: Custom closures for complex logic (Telex W, OEO, etc.)
/// - **Conditional Rules**: Transform rules with conditions
///
/// ## Example
///
/// ```rust,ignore
/// let telex_config = PipelineConfig {
///     name: "telex".to_string(),
///     pipeline: PipelineSettings {
///         enabled: vec!["validation", "transform", "tone", "retrofix", "orthography"],
///         use_composition: false,
///     },
///     transform_rules: hashmap! {
///         "aa" => "â",
///         "aw" => "ă",
///     },
///     tone_map: hashmap! {
///         's' => ToneMark::Acute,
///         'f' => ToneMark::Grave,
///     },
///     context_rules: vec![
///         ContextRule::new("telex_w_after_ư",
///             RuleMatcher::And(vec![...]),
///             RuleAction::Skip),
///     ],
///     conditional_rules: vec![
///         ConditionalRule::with_condition("aa", "â",
///             RuleMatcher::Not(Box::new(RuleMatcher::StartsWith("q")))),
///     ],
///     validation: Some(ValidationSettings::default()),
///     orthography: Some(OrthographySettings::default()),
///     lookup: None,
///     // Legacy fields (for backward compatibility)
///     enable_lookup: false,
///     dictionary: None,
///     tone_style: ToneStyle::New,
///     unicode_form: UnicodeForm::NFC,
/// };
/// ```
#[derive(Clone)]
pub struct PipelineConfig {
    /// Name of the input method (e.g., "telex", "vni")
    pub name: String,

    /// Pipeline settings (NEW: enhanced format)
    pub pipeline: PipelineSettings,

    /// Transformation rules: sequence → result
    /// Example: "aa" → "â", "aw" → "ă", "dd" → "đ"
    pub transform_rules: HashMap<String, String>,

    /// Tone key mappings: key → tone mark
    /// Example: 's' → Acute, 'f' → Grave
    pub tone_map: HashMap<char, ToneMark>,

    // ========================================
    // Enhanced Rules (Phase 1)
    // ========================================
    /// Context rules with custom closures
    /// Used for complex logic like Telex W handling, OEO blocking, etc.
    /// These rules are evaluated during transformation stages
    ///
    /// Wrapped in Arc to allow cloning (closures can't be cloned)
    pub context_rules: Arc<Vec<crate::pipeline::rules::ContextRule>>,

    /// Conditional transformation rules
    /// Transform rules that only apply when certain conditions are met
    /// Example: "aa → â" but not after "q"
    ///
    /// Wrapped in Arc to allow cloning (closures can't be cloned)
    pub conditional_rules: Arc<Vec<crate::pipeline::rules::ConditionalRule>>,

    // ========================================
    // Stage Settings
    // ========================================
    /// Validation settings (NEW: enhanced format)
    pub validation: Option<ValidationSettings>,

    /// Orthography settings (NEW: enhanced format)
    pub orthography: Option<OrthographySettings>,

    /// Lookup settings (NEW: enhanced format)
    pub lookup: Option<LookupSettings>,

    /// Tone settings (NEW: flexible typing)
    /// Controls tone placement behavior and permutation matching
    pub tone: ToneConfig,

    /// Native script mode for direct mapping keyboards (Khmer, Cham)
    /// When true, enables single-char transforms, double-key patterns, etc.
    pub native_script_mode: bool,

    /// Word-boundary final repair (event-sourcing-completion Phase 3).
    ///
    /// At a word boundary (separator commit, Enter, or another reset-key
    /// commit), recompute the just-finished word with the CLOSED projection
    /// (`compose::compose_closed`) and adopt it when it differs from what's
    /// displayed — restores the literal raw for an inferred non-adjacent
    /// mark whose tone never arrived (VNI `"nhat6"` + space → `"nhat6 "`,
    /// not `"nhât "`).
    ///
    /// Default ON for BOTH backends (user decision 2026-07-02, overriding
    /// the red-team F7 recommendation to default the TSF backend OFF):
    /// shape-only inferred forms are rare, and retyping the affected word is
    /// the accepted escape on TSF until a TSF-side toggle exists (Phase 4,
    /// not yet implemented — TSF has no per-word undo affordance of its own
    /// today). Only takes effect when the compose stage's validator is
    /// `compose::Validator::Vietnamese` — there is no attested-syllable
    /// table to gate against for Hmong/Custom/None, so the flag is a no-op
    /// there regardless of this setting.
    pub boundary_repair: bool,

    // ========================================================================
    // Legacy fields (for backward compatibility)
    // ========================================================================
    /// Enable dictionary lookup (Stage 8)
    /// LEGACY: Use lookup.is_some() instead
    pub enable_lookup: bool,

    /// Dictionary provider for Stage 8
    /// LEGACY: Will be replaced by lookup settings
    pub dictionary: Option<Arc<dyn DictionaryProvider>>,

    /// Tone style (old vs new orthography)
    /// LEGACY: Use orthography.tone_style instead
    pub tone_style: ToneStyle,

    /// Unicode normalization form
    /// LEGACY: Use orthography.unicode_form instead
    pub unicode_form: UnicodeForm,
}

impl PipelineConfig {
    /// Create a new empty configuration
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pipeline: PipelineSettings::default(),
            transform_rules: HashMap::new(),
            tone_map: HashMap::new(),
            // Enhanced rules
            context_rules: Arc::new(Vec::new()),
            conditional_rules: Arc::new(Vec::new()),
            // Stage settings
            validation: None,
            orthography: None,
            lookup: None,
            tone: ToneConfig::default(), // NEW: flexible typing config
            native_script_mode: false,   // NEW: disabled by default for Telex/VNI
            boundary_repair: true,       // Phase 3: default ON for both backends
            // Legacy fields
            enable_lookup: false,
            dictionary: None,
            tone_style: ToneStyle::Old, // Default: kiểu cũ (óa, úy)
            unicode_form: UnicodeForm::NFC,
        }
    }

    /// Check if a stage is enabled
    pub fn is_stage_enabled(&self, stage: &str) -> bool {
        self.pipeline.enabled.iter().any(|s| s == stage)
    }

    /// Get tone style (from new or legacy field)
    pub fn get_tone_style(&self) -> ToneStyle {
        if let Some(ref ortho) = self.orthography {
            match ortho.tone_style.as_str() {
                "modern" | "new" => ToneStyle::New,
                _ => ToneStyle::Old, // Default: kiểu cũ
            }
        } else {
            self.tone_style
        }
    }

    /// Get unicode form (from new or legacy field)
    pub fn get_unicode_form(&self) -> UnicodeForm {
        if let Some(ref ortho) = self.orthography {
            match ortho.unicode_form.as_str() {
                "nfd" | "NFD" => UnicodeForm::NFD,
                _ => UnicodeForm::NFC,
            }
        } else {
            self.unicode_form
        }
    }

    /// Add a transformation rule
    ///
    /// ## Algorithm
    ///
    /// Adds a rule that maps a sequence of characters to a transformed result.
    /// Example: add_transform("aa", "â") means typing "aa" produces "â"
    pub fn add_transform(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.transform_rules.insert(from.into(), to.into());
    }

    /// Add a tone mapping
    ///
    /// ## Algorithm
    ///
    /// Maps a key to a tone mark. Example: add_tone('s', ToneMark::Acute)
    /// means typing 's' applies the acute accent (sắc)
    pub fn add_tone(&mut self, key: char, tone: ToneMark) {
        self.tone_map.insert(key, tone);
    }

    /// Check if a character is a tone key
    pub fn is_tone_key(&self, ch: char) -> bool {
        self.tone_map.contains_key(&ch)
    }

    /// Get the tone mark for a key
    pub fn get_tone(&self, ch: char) -> Option<ToneMark> {
        self.tone_map.get(&ch).copied()
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self::new("default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_config() {
        let config = PipelineConfig::new("test");
        assert_eq!(config.name, "test");
        assert!(config.transform_rules.is_empty());
        assert!(config.tone_map.is_empty());
    }

    #[test]
    fn test_add_transform() {
        let mut config = PipelineConfig::new("test");
        config.add_transform("aa", "â");

        assert_eq!(config.transform_rules.get("aa"), Some(&"â".to_string()));
    }

    #[test]
    fn test_add_tone() {
        let mut config = PipelineConfig::new("test");
        config.add_tone('s', ToneMark::Acute);

        assert!(config.is_tone_key('s'));
        assert_eq!(config.get_tone('s'), Some(ToneMark::Acute));
        assert!(!config.is_tone_key('x'));
    }

    #[test]
    fn test_tone_mark_equality() {
        assert_eq!(ToneMark::Acute, ToneMark::Acute);
        assert_ne!(ToneMark::Acute, ToneMark::Grave);
    }

    #[test]
    fn test_tone_style() {
        assert_eq!(ToneStyle::New, ToneStyle::New);
        assert_ne!(ToneStyle::Old, ToneStyle::New);
    }

    #[test]
    fn test_unicode_form() {
        assert_eq!(UnicodeForm::NFC, UnicodeForm::NFC);
        assert_ne!(UnicodeForm::NFC, UnicodeForm::NFD);
    }
}
