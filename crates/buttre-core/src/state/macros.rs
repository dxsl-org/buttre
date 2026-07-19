//! Shorthand/gõ tắt: a deterministic, user-AUTHORED raw→expansion table
//! (`"vn"` → `"Việt Nam"`), stored at `dirs::data_dir()/buttre/macros.toml`.
//!
//! **Deliberately a SEPARATE mechanism from [`crate::state::learning`]**
//! (ADR-0001, `docs/adr/0001-learning-additive-only-record-replay-invariant.md`):
//! personal learning may only ever WIDEN what the engine already accepts,
//! inferred from typing behavior. A macro REPLACES a raw sequence with
//! arbitrary text, entirely by the user's own hand — mixing the two into one
//! file/mechanism would let an inferred signal silently rewrite output the
//! same way the "yes" incident did. Do not merge this into `learning.toml`.
//!
//! **Tests**: unit tests for load-hardening and lookup live at the bottom of
//! this file. `Keyboard`-level expansion/collision/revert tests are in
//! `crates/buttre-core/tests/macros_tests.rs`.
//!
//! ## Collision safety
//!
//! A macro only ever fires on a CLOSED word run — the whole raw between two
//! separators exactly equals a trigger (see `Keyboard::expand_macros`). This
//! is the AutoHotkey "boundary required on both sides" default: `vn` never
//! fires as a fragment of `advnture`, and the currently-open trailing word
//! never expands mid-type. `Ctrl+Shift+Z` reverts an expansion to the literal
//! raw (reuses the existing toggle-freeze mechanism) and the global
//! `Settings::shorthand` switch turns expansion off entirely.
//!
//! ## File format
//!
//! ```toml
//! [macros]
//! vn = { expand = "Việt Nam", enabled = true }
//! brb = { expand = "be right back" }   # enabled defaults to true
//! ```
//!
//! Read-only at the keyboard layer: unlike `learning.toml`, nothing at the
//! typing layer ever writes this file — edits come from the config window or
//! a hand-edit, picked up by the platform layer's file watcher. No dirty
//! flag, no save channel, no off-thread write concern.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Self-documentation header prepended to every save the config window (or a
/// future tray action) performs — mirrors `learning.rs`'s `FILE_HEADER`.
const FILE_HEADER: &str = "\
# buttre — gõ tắt (tự sửa được, lưu là áp dụng ngay khi buttre đang chạy)
#
# [macros]  chuỗi gõ -> văn bản thay thế. Chỉ nổ khi gõ ĐÚNG NGUYÊN cả từ rồi
#           sang dấu cách/dấu câu — không nổ giữa chừng, không nổ khi là một
#           phần của từ khác. Tự thêm:
#           vn = { expand = \"Việt Nam\" }
#           brb = { expand = \"be right back\", enabled = false }  # tắt riêng
#
# Đảo một lần gõ tắt vừa nổ về nguyên văn: Ctrl+Shift+Z.
# Tắt toàn bộ: tray → Tùy chọn → Gõ tắt.

";

/// Byte ceiling checked BEFORE `read_to_string` (mirrors `learning.rs`'s
/// `MAX_FILE_BYTES` — same rationale: a huge/corrupt file is never read).
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Max number of macro entries retained (mirrors `learning.rs`'s
/// `MAX_ENTRIES_PER_TABLE`) — this is a user-authored table, so hitting this
/// cap means the file was hand-crafted or corrupted, not organically grown.
const MAX_ENTRIES: usize = 500;

/// Max expansion length in chars — bounds a runaway/malicious entry from
/// emitting an unbounded number of keystrokes.
const MAX_EXPANSION_CHARS: usize = 256;

/// Minimum trigger length — a 1-char trigger collides with too much real
/// typing to be a sane default; the config window can still author longer
/// ones, this only rejects degenerate cases at load.
const MIN_TRIGGER_CHARS: usize = 2;

/// One `[macros]` TOML entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEntry {
    /// The literal text to emit in place of the trigger.
    pub expand: String,
    /// Per-entry on/off without deleting the entry.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// The on-disk shape of `macros.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MacroFile {
    /// Trigger (lowercase) -> entry.
    #[serde(default)]
    pub macros: HashMap<String, MacroEntry>,
}

/// In-memory, load-hardened macro table. Read-only after construction — see
/// the module doc's "read-only at the keyboard layer" note.
#[derive(Debug, Clone, Default)]
pub struct MacroStore {
    file: MacroFile,
}

impl MacroStore {
    /// Mirrors `LearningStore::get_path` exactly (same `buttre` directory,
    /// different filename).
    pub fn get_path() -> Result<PathBuf> {
        let data_dir =
            dirs::data_dir().ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?;
        let dir = data_dir.join("buttre");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("macros.toml"))
    }

    /// Load `macros.toml`, or an empty store if it doesn't exist, is too
    /// large, or fails to parse. Never panics — every failure mode degrades
    /// to `Self::default()` (byte-identical to shorthand being off).
    ///
    /// Does NOT get called automatically anywhere — the platform layer
    /// calls this explicitly, gated on `Settings::shorthand`, exactly like
    /// `LearningStore::load`.
    pub fn load() -> Self {
        let Ok(path) = Self::get_path() else {
            return Self::default();
        };
        if !path.exists() {
            return Self::default();
        }
        match fs::metadata(&path) {
            Ok(meta) if meta.len() > MAX_FILE_BYTES => {
                tracing::warn!(
                    file_bytes = meta.len(),
                    ceiling_bytes = MAX_FILE_BYTES,
                    "macros.toml exceeds the load byte ceiling — ignoring"
                );
                return Self::default();
            }
            Err(_) => return Self::default(),
            _ => {}
        }
        let Ok(content) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let Ok(file) = toml::from_str::<MacroFile>(&content) else {
            return Self::default();
        };
        Self::from_file(file)
    }

    /// [`Self::load`] gated on `Settings::shorthand` — the single helper
    /// every backend (tray, TSF, Linux engine processes) calls so the
    /// "toggle off == empty store == expansion never fires" equivalence has
    /// exactly one spelling.
    pub fn load_gated(shorthand: bool) -> Self {
        if shorthand {
            Self::load()
        } else {
            Self::default()
        }
    }

    /// Apply load-time hardening to a raw [`MacroFile`]: sanitize/lowercase
    /// keys, drop entries below the min trigger length or over the max
    /// expansion length, cap total count. A malformed entry is dropped
    /// outright — unlike `learning.toml`'s user-attested table, a bad macro
    /// key has no "might become valid later" story (nothing here is
    /// id-based). Public (not just internal to [`Self::load`]) so the
    /// config window can validate a user's edits with the exact same rules
    /// before persisting them, and so tests can build a store in-memory
    /// without touching disk.
    pub fn from_file(file: MacroFile) -> Self {
        let mut macros = HashMap::with_capacity(file.macros.len());
        for (key, entry) in file.macros {
            let key = key.trim().to_lowercase();
            if key.chars().count() < MIN_TRIGGER_CHARS
                || !key.chars().all(|c| c.is_ascii_alphanumeric())
                || entry.expand.chars().count() > MAX_EXPANSION_CHARS
                || entry.expand.is_empty()
            {
                continue;
            }
            macros.insert(key, entry);
        }
        if macros.len() > MAX_ENTRIES {
            let mut entries: Vec<(String, MacroEntry)> = macros.into_iter().collect();
            // Deterministic truncation (key order) — no recency/count data
            // to rank by for a hand-authored table.
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            entries.truncate(MAX_ENTRIES);
            macros = entries.into_iter().collect();
        }
        Self {
            file: MacroFile { macros },
        }
    }

    /// Atomically persist `file` to `macros.toml` (temp file + rename) —
    /// mirrors `LearningStore::write_atomic`. Used by the config window
    /// (P3), not by anything at the typing layer. Temp filename is unique
    /// per call (see `Settings::save`'s doc): this file already has two
    /// potential writers (tray's seed-if-missing, the future config window)
    /// in different processes.
    pub fn write_atomic(file: &MacroFile) -> Result<()> {
        let path = Self::get_path()?;
        let toml_str = format!("{FILE_HEADER}{}", toml::to_string_pretty(file)?);
        let tmp_path = super::atomic_write::unique_temp_path(&path, "toml");
        fs::write(&tmp_path, toml_str)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// Take a save-ready copy for the config window to edit and persist.
    pub fn snapshot_for_save(&self) -> MacroFile {
        self.file.clone()
    }

    /// Look up `raw` (case-insensitive) for an ENABLED expansion. `None` on
    /// no match or a disabled entry — the caller (`Keyboard::expand_macros`)
    /// never needs to check `enabled` itself.
    pub fn lookup(&self, raw: &str) -> Option<&str> {
        let key = raw.to_lowercase();
        self.file
            .macros
            .get(&key)
            .filter(|e| e.enabled)
            .map(|e| e.expand.as_str())
    }

    /// Number of loaded (post-hardening) entries — used by the config window
    /// and diagnostics; never consulted by the typing layer.
    pub fn len(&self) -> usize {
        self.file.macros.len()
    }

    /// `true` iff no macros are loaded.
    pub fn is_empty(&self) -> bool {
        self.file.macros.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(expand: &str) -> MacroEntry {
        MacroEntry {
            expand: expand.to_string(),
            enabled: true,
        }
    }

    #[test]
    fn lookup_hits_enabled_entry_case_insensitive() {
        let mut macros = HashMap::new();
        macros.insert("vn".to_string(), entry("Việt Nam"));
        let store = MacroStore::from_file(MacroFile { macros });
        assert_eq!(store.lookup("vn"), Some("Việt Nam"));
        assert_eq!(store.lookup("VN"), Some("Việt Nam"));
        assert_eq!(store.lookup("Vn"), Some("Việt Nam"));
    }

    #[test]
    fn lookup_misses_disabled_entry() {
        let mut macros = HashMap::new();
        macros.insert(
            "brb".to_string(),
            MacroEntry {
                expand: "be right back".to_string(),
                enabled: false,
            },
        );
        let store = MacroStore::from_file(MacroFile { macros });
        assert_eq!(store.lookup("brb"), None);
    }

    #[test]
    fn lookup_misses_unknown_trigger() {
        let store = MacroStore::default();
        assert_eq!(store.lookup("vn"), None);
    }

    #[test]
    fn from_file_drops_trigger_below_min_length() {
        let mut macros = HashMap::new();
        macros.insert("v".to_string(), entry("Việt"));
        let store = MacroStore::from_file(MacroFile { macros });
        assert!(store.is_empty(), "1-char trigger must be dropped at load");
    }

    #[test]
    fn from_file_drops_non_alphanumeric_trigger() {
        let mut macros = HashMap::new();
        macros.insert("v-n".to_string(), entry("Việt Nam"));
        let store = MacroStore::from_file(MacroFile { macros });
        assert!(store.is_empty(), "non-alphanumeric trigger must be dropped");
    }

    #[test]
    fn from_file_lowercases_trigger_key() {
        let mut macros = HashMap::new();
        macros.insert("VN".to_string(), entry("Việt Nam"));
        let store = MacroStore::from_file(MacroFile { macros });
        assert_eq!(store.lookup("vn"), Some("Việt Nam"));
    }

    #[test]
    fn from_file_drops_oversized_expansion() {
        let mut macros = HashMap::new();
        macros.insert(
            "huge".to_string(),
            entry(&"x".repeat(MAX_EXPANSION_CHARS + 1)),
        );
        let store = MacroStore::from_file(MacroFile { macros });
        assert!(store.is_empty(), "oversized expansion must be dropped");
    }

    #[test]
    fn from_file_drops_empty_expansion() {
        let mut macros = HashMap::new();
        macros.insert("empty".to_string(), entry(""));
        let store = MacroStore::from_file(MacroFile { macros });
        assert!(store.is_empty());
    }

    #[test]
    fn from_file_caps_total_entries_deterministically() {
        let mut macros = HashMap::new();
        for i in 0..MAX_ENTRIES + 10 {
            macros.insert(format!("trig{i:04}"), entry("x"));
        }
        let store = MacroStore::from_file(MacroFile { macros });
        assert_eq!(store.len(), MAX_ENTRIES);
        // Deterministic: lowest keys survive (sorted ascending, truncated).
        assert!(store.lookup("trig0000").is_some());
        assert!(store
            .lookup(&format!("trig{:04}", MAX_ENTRIES + 9))
            .is_none());
    }

    #[test]
    fn load_never_panics_when_path_unresolvable() {
        // `load()` degrades to default on any failure — smoke-test the
        // public entry point runs at all in a test environment.
        let _ = MacroStore::load();
    }
}
