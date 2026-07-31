//! Settings management for buttre application
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/state_tests.rs`.
//!
//! This module handles loading and saving application settings to disk.
//! Settings are stored in a platform-specific location using TOML format.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application settings
///
/// These settings are persisted to disk and loaded on application startup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Current input method ID: `"telex"`, `"vni"`, `"nom"`, or a custom
    /// method id (a `keyboards/<id>.toml` filename stem).
    ///
    /// NEVER `"english"`. Turning the IME off is [`Self::enabled`], a separate
    /// field — see its doc for why the two cannot share one field. A
    /// `settings.toml` written before the split is migrated on load.
    pub input_method: String,

    /// Is the input method ON at all?
    ///
    /// Separate from [`Self::input_method`] because the two answer different
    /// questions, and cramming both into one field is what made every
    /// tray↔system sync attempt fail: `"english"` was stored AS a method, but
    /// to an operating system "English" is not a state of buttre — it is the
    /// ABSENCE of buttre. One side held a value the other could not express,
    /// so no amount of mirroring could reconcile them (see ADR-0003).
    ///
    /// Consequences of the split, relied upon elsewhere:
    /// - turning off and on again preserves the chosen method (nothing
    ///   overwrites `input_method`, so nothing has to be restored)
    /// - several places may WRITE this flag (tray click, hotkey, the OS
    ///   telling us the user switched away); none of them MIRRORS another
    ///
    /// `serde(default)` is `true`, which is the right answer for the case it
    /// actually covers: a `settings.toml` that predates this field but names a
    /// real method — that user had the IME on.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Enable auto-correction features
    pub auto_correct: bool,

    /// Enable shorthand/macro expansion
    pub shorthand: bool,

    /// Launch buttre on system startup. Default `true`: a fresh install (no
    /// `settings.toml` yet) starts with autostart ON, and the tray registers
    /// the per-OS login entry on its first launch (see
    /// `buttre-platform/src/main.rs`) — matching the "always start with the
    /// OS" expectation of an input method. Existing users are unaffected:
    /// their saved `settings.toml` already carries an explicit choice, and
    /// `load()` reads it verbatim rather than this default.
    pub startup: bool,

    /// Backspace deletion granularity (event-sourcing-completion Phase 4):
    /// `"grapheme"` (default) deletes the last DISPLAYED character —
    /// unchanged pre-phase behavior. `"raw"` deletes the last RAW keystroke
    /// and recomposes — the event-sourced engine's trivially-correct
    /// inverse, at the cost of sometimes removing more or less than one
    /// visible glyph. Parsed via `buttre_core::keyboard::BackspaceMode::
    /// from_settings_str`, which falls back to `"grapheme"` for any unknown
    /// value (never fails to load).
    #[serde(default = "default_backspace_mode")]
    pub backspace_mode: String,

    /// Enable personal learning (event-sourcing-completion Phase 5): the
    /// user-attested syllable overlay and raw-sequence preference memory
    /// persisted to `learning.toml`. When `false`, no signals are collected
    /// and no snapshot is applied (behavior is byte-identical to no store).
    /// PRIVACY: `learning.toml` holds fragments of typed words (raw key
    /// sequences the user corrected); it is local-only, never logged, and is
    /// removed/reset by deleting the file. Default on — the feature silently
    /// improves typing over time; flip to `false` to disable and stop
    /// collection.
    #[serde(default = "default_learning_enabled")]
    pub learning_enabled: bool,

    /// Strict Vietnamese spelling control — the config window's "Kiểm soát
    /// gắt gao chính tả tiếng Việt" checkbox. `false` (default) keeps the
    /// Unikey-style deliberate-đ leniency ("ddt" → "đt"); `true` reverts
    /// vowel-less đ-clusters to their raw keystrokes like any other
    /// non-syllable. Maps to
    /// `buttre_engine::pipeline::ValidationSettings::strict_spelling` via
    /// `Keyboard::set_strict_spelling`.
    #[serde(default)]
    pub strict_spelling: bool,

    /// Windows only, EXPERIMENTAL: run the low-level hook ALONGSIDE the TSF
    /// text service, as a fallback for apps TSF cannot reach (raw-input
    /// readers, some terminals, elevated windows). The text service claims the
    /// foreground process it owns via shared memory and the hook stands down
    /// there (`transport_claim.rs`).
    ///
    /// Default OFF. Field testing showed the claim still misses some hosts —
    /// browsers glitched (caret jumps mid-word) and Telegram dropped words on
    /// space, both symptoms of the two layers touching one keystroke, while
    /// TSF alone served every tested app cleanly. Until the arbitration is
    /// proven per-host (suspected gap: TIPs activating inside
    /// TextInputHost.exe rather than the focused app), the union coverage is
    /// not worth risking garbled text in the most common apps. Opt-in for
    /// users who need the hook-only apps.
    #[serde(default = "default_hook_fallback")]
    pub hook_fallback: bool,

    /// Show the composition as underlined preedit (`true`, the long-standing
    /// behavior) or commit text as-you-go with NO underline (`false`,
    /// Unikey-style). Honored only by the Linux/macOS preedit backends —
    /// Windows already commits real text. Nôm always uses preedit regardless
    /// (its candidate popup needs it). `false` relies on the focused app
    /// supporting in-place text deletion; backends that can't do it for a given
    /// client fall back to preedit rather than corrupt input. Default `true` so
    /// an upgrade never changes a user's typing out from under them.
    #[serde(default = "default_use_preedit")]
    pub use_preedit: bool,
}

/// `serde(default)` value for `Settings::backspace_mode` — also the fallback
/// `Settings::default()` uses, so both paths agree on one literal.
fn default_backspace_mode() -> String {
    "grapheme".to_string()
}

/// `serde(default)` value for `Settings::learning_enabled`.
fn default_learning_enabled() -> bool {
    true
}

/// The method a user lands on when nothing better is known — a fresh install,
/// or a pre-split `settings.toml` that only recorded "off".
fn default_method() -> String {
    "telex".to_string()
}

/// `serde(default)` value for `Settings::enabled`.
///
/// `true`, and deliberately DIFFERENT from what `Settings::default()` uses.
/// The two answer different questions:
///
/// - this one: "an existing `settings.toml` has no `enabled` field" — it was
///   written before the split, and if it names a real method then the IME was
///   on, so `true`.
/// - `Settings::default()`: "there is no file at all" — a fresh install, which
///   has always started with the IME off (the old default was
///   `input_method = "english"`). Kept off so installing buttre does not
///   silently start rewriting the user's keystrokes.
fn default_enabled() -> bool {
    true
}

/// `serde(default)` value for `Settings::hook_fallback` — OFF until the
/// transport arbitration is proven per-host (see the field's doc for the
/// observed failures). Flipping this default is the release gate for phase 03.
fn default_hook_fallback() -> bool {
    false
}

/// `serde(default)` value for `Settings::use_preedit` — preedit ON, matching
/// the behavior every prior release shipped so an old `settings.toml` (and a
/// fresh install) keeps the known-good underline model until the user opts out.
fn default_use_preedit() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Fresh install: a method is pre-selected but the IME is OFF, which
            // is exactly what `input_method = "english"` meant before the split.
            // See `default_enabled`'s doc for why this differs from the
            // `serde(default)`.
            input_method: "telex".to_string(),
            enabled: false,
            auto_correct: false,
            shorthand: false,
            startup: true,
            backspace_mode: default_backspace_mode(),
            learning_enabled: default_learning_enabled(),
            strict_spelling: false,
            // Same OFF as the serde default: this knob has one meaning
            // everywhere until the arbitration earns its default-on.
            hook_fallback: default_hook_fallback(),
            use_preedit: default_use_preedit(),
        }
    }
}

impl Settings {
    /// Get the settings file path
    ///
    /// Resolved under `dirs::data_dir()` — NOT the config dir. This is a
    /// separate store from `~/.config/buttre/` (the tray↔engine sync files):
    /// - Windows: %APPDATA%\buttre\settings.toml
    /// - macOS: ~/Library/Application Support/buttre/settings.toml
    /// - Linux: ~/.local/share/buttre/settings.toml
    pub fn get_path() -> Result<PathBuf> {
        let data_dir =
            dirs::data_dir().ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?;
        let buttre_dir = data_dir.join("buttre");
        fs::create_dir_all(&buttre_dir)?;
        Ok(buttre_dir.join("settings.toml"))
    }

    /// Load settings from file, or return default if file doesn't exist
    ///
    /// This method will never fail - if the settings file cannot be loaded,
    /// it will return default settings instead.
    pub fn load() -> Self {
        match Self::get_path() {
            Ok(path) => {
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(settings) = toml::from_str::<Self>(&content) {
                            return settings.migrated();
                        }
                    }
                }
            }
            Err(e) => eprintln!("Failed to get settings path: {:?}", e),
        }
        Self::default()
    }

    /// Read `settings.toml` distinguishing the cases [`Self::load`]
    /// deliberately flattens: `Ok(None)` = file absent (fresh machine),
    /// `Ok(Some)` = parsed, `Err` = file PRESENT but unreadable/unparseable.
    ///
    /// Load-modify-save writers (the config window, the engines' command
    /// paths) must use THIS and refuse to save on `Err` — saving the
    /// defaults `load()` hands back would rewrite the user's whole file
    /// over one typo. Read-only consumers keep the infallible `load()`.
    ///
    /// # Errors
    /// Path resolution, file I/O, or a present-but-unparseable file (the
    /// message names the file and the parse error — the log line IS the
    /// diagnosis).
    pub fn read_strict() -> Result<Option<Self>> {
        let path = Self::get_path()?;
        Self::read_strict_from(&path)
    }

    /// [`Self::read_strict`] against an explicit path (testable core).
    fn read_strict_from(path: &std::path::Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)?;
        let settings = toml::from_str::<Self>(&content).map_err(|e| {
            anyhow::anyhow!(
                "settings.toml exists but does not parse ({e}) — refusing to overwrite it"
            )
        })?;
        Ok(Some(settings.migrated()))
    }

    /// Startup self-heal for a corrupt `settings.toml`: rename it aside to
    /// `settings.toml.bad` so later saves are legitimate fresh writes
    /// instead of silent clobbers, and the user's original bytes survive
    /// for hand-recovery. Returns `true` when a quarantine happened.
    /// Call once at process startup (the tray does), never on a hot path.
    /// Racing processes are safe: rename is atomic, first one wins, the
    /// loser's rename fails on a path that no longer exists.
    pub fn quarantine_if_corrupt() -> bool {
        match Self::read_strict() {
            Ok(_) => false,
            Err(e) => {
                let Ok(path) = Self::get_path() else {
                    return false;
                };
                let bad = path.with_extension("toml.bad");
                match fs::rename(&path, &bad) {
                    Ok(()) => {
                        tracing::warn!(
                            "settings.toml không đọc được ({e}); đã cách ly sang {} — \
                             cấu hình quay về mặc định",
                            bad.display()
                        );
                        true
                    }
                    Err(rename_err) => {
                        tracing::warn!(
                            "settings.toml không đọc được ({e}) và cũng không cách ly được \
                             ({rename_err}) — các thao tác lưu sẽ bị từ chối"
                        );
                        false
                    }
                }
            }
        }
    }

    /// Fold a pre-split `settings.toml` into the `enabled` + `input_method`
    /// model, and write the result back so the next load needs no inference.
    ///
    /// `input_method = "english"` used to mean "IME off". It carried no record
    /// of which Vietnamese method to return to — `last_vietnamese_method` lived
    /// only in memory — so the method resets to Telex. That is the one thing
    /// this migration cannot preserve, and it is why it must happen ONCE and be
    /// persisted: re-deriving it on every load would re-apply the reset over a
    /// choice the user made after upgrading.
    ///
    /// A save failure is logged and ignored: the in-memory value is already
    /// correct, so the session behaves properly and the migration simply runs
    /// again next time.
    fn migrated(mut self) -> Self {
        if !Self::is_off_sentinel(&self.input_method) {
            return self;
        }
        self.enabled = false;
        self.input_method = default_method();
        if let Err(e) = self.save() {
            eprintln!("Failed to persist migrated settings: {:?}", e);
        }
        self
    }

    /// Did this `input_method` value mean "IME off" in the pre-split model?
    fn is_off_sentinel(method: &str) -> bool {
        method.eq_ignore_ascii_case("english")
    }

    /// Save settings to file — atomically (temp file + rename).
    ///
    /// Not just belt-and-suspenders: the config window (a separate process)
    /// and the tray both now touch this file — the tray watches it for
    /// live-reload (`buttre-platform/src/main.rs`'s settings watcher) — so a
    /// plain in-place write could race a concurrent read and hand back a
    /// half-written, unparseable file. Mirrors `LearningStore::write_atomic`
    /// / `MacroStore::write_atomic`'s existing pattern.
    ///
    /// The temp filename is unique per call (see
    /// `super::atomic_write::unique_temp_path`'s doc): with two independent
    /// PROCESSES (the config window and the tray) able to save this same
    /// file — plus, in tests, many THREADS in one process — a shared temp
    /// name would let one writer's `fs::write` truncate mid-write of
    /// another's, or one writer's rename consume the temp file out from
    /// under a concurrent one.
    ///
    /// # Errors
    /// Returns an error if the settings file cannot be written.
    pub fn save(&self) -> Result<()> {
        let path = Self::get_path()?;
        let content = toml::to_string_pretty(self)?;
        let tmp_path = super::atomic_write::unique_temp_path(&path, "toml");
        fs::write(&tmp_path, content)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_strict_distinguishes_absent_parsed_and_corrupt() {
        let dir = std::env::temp_dir().join("buttre-settings-strict-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");

        // Absent → Ok(None): a fresh machine is not an error.
        assert!(matches!(Settings::read_strict_from(&path), Ok(None)));

        // Parsed → Ok(Some) with the file's values, not defaults.
        fs::write(&path, "input_method = \"vni\"\nauto_correct = false\nshorthand = false\nstartup = false\nbackspace_mode = \"grapheme\"\nlearning_enabled = true\n").unwrap();
        let loaded = Settings::read_strict_from(&path)
            .unwrap()
            .expect("file present must parse");
        assert_eq!(loaded.input_method, "vni");

        // Corrupt → Err, NEVER silently defaults: this is the branch that
        // stops a load-modify-save writer from rewriting the user's file.
        fs::write(&path, "not valid toml {{{").unwrap();
        assert!(Settings::read_strict_from(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_backspace_mode_is_grapheme() {
        assert_eq!(Settings::default().backspace_mode, "grapheme");
    }

    #[test]
    fn backspace_mode_defaults_when_absent_from_toml() {
        // Old settings.toml files predate this field entirely — `load()`
        // promises to never fail, and a missing field must fall back to
        // "grapheme" (byte-identical pre-phase behavior), not an error.
        let toml_str = r#"
            input_method = "telex"
            auto_correct = false
            shorthand = false
            startup = false
        "#;
        let settings: Settings =
            toml::from_str(toml_str).expect("must deserialize without backspace_mode present");
        assert_eq!(settings.backspace_mode, "grapheme");
    }

    #[test]
    fn use_preedit_defaults_true_when_absent_from_toml() {
        // Every settings.toml written before this field existed must load with
        // preedit ON (the prior behavior), not fail or silently flip to the
        // no-underline model.
        let toml_str = r#"
            input_method = "telex"
            auto_correct = false
            shorthand = false
            startup = false
        "#;
        let settings: Settings =
            toml::from_str(toml_str).expect("must deserialize without use_preedit present");
        assert!(settings.use_preedit);
        assert!(Settings::default().use_preedit);
    }

    // ── enabled / input_method split (ADR-0003) ─────────────────────────────

    #[test]
    fn fresh_install_preselects_telex_but_stays_off() {
        // What `input_method = "english"` used to mean. Installing buttre must
        // not start rewriting keystrokes on its own.
        let fresh = Settings::default();
        assert!(!fresh.enabled);
        assert_eq!(fresh.input_method, "telex");
    }

    #[test]
    fn input_method_is_never_the_off_sentinel() {
        assert!(!Settings::is_off_sentinel(
            &Settings::default().input_method
        ));
    }

    #[test]
    fn pre_split_file_with_a_real_method_loads_as_on() {
        // The case `serde(default)` exists for: no `enabled` field, but a method
        // is named — that user had the IME running.
        let toml_str = r#"
            input_method = "vni"
            auto_correct = false
            shorthand = false
            startup = false
        "#;
        let settings: Settings = toml::from_str(toml_str).expect("deserialize");
        assert!(settings.enabled, "an named method meant the IME was on");
        assert_eq!(settings.input_method, "vni");
    }

    #[test]
    fn migration_turns_the_english_sentinel_into_off_plus_a_real_method() {
        let toml_str = r#"
            input_method = "english"
            auto_correct = false
            shorthand = false
            startup = false
        "#;
        // Deserialize + migrate WITHOUT touching the real settings file: the
        // in-memory fold is what `load()` applies, and it is the part worth
        // pinning (`migrated()` also persists, which needs a real path).
        let raw: Settings = toml::from_str(toml_str).expect("deserialize");
        assert!(raw.enabled, "serde default fires before migration");

        let mut folded = raw;
        folded.enabled = false;
        folded.input_method = default_method();
        assert!(!folded.enabled);
        assert_eq!(folded.input_method, "telex");
        assert!(!Settings::is_off_sentinel(&folded.input_method));
    }

    #[test]
    fn off_sentinel_recognised_whatever_the_case() {
        // Hand-edited files exist; "English" must not slip through as a method.
        for value in ["english", "English", "ENGLISH"] {
            assert!(Settings::is_off_sentinel(value), "{value}");
        }
        for value in ["telex", "vni", "nom", "cham", ""] {
            assert!(!Settings::is_off_sentinel(value), "{value}");
        }
    }

    #[test]
    fn enabled_round_trips_through_toml() {
        let settings = Settings {
            enabled: false,
            input_method: "nom".to_string(),
            ..Settings::default()
        };
        let restored: Settings =
            toml::from_str(&toml::to_string_pretty(&settings).expect("ser")).expect("deserialize");
        assert!(
            !restored.enabled,
            "an explicit false must survive the round trip"
        );
        assert_eq!(restored.input_method, "nom");
    }

    #[test]
    fn backspace_mode_round_trips_through_toml() {
        let settings = Settings {
            backspace_mode: "raw".to_string(),
            ..Settings::default()
        };
        let serialized = toml::to_string_pretty(&settings).expect("serialize");
        let restored: Settings = toml::from_str(&serialized).expect("deserialize");
        assert_eq!(restored.backspace_mode, "raw");
    }
}
