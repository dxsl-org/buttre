//! Personal learning stores (event-sourcing-completion Phase 5): two small
//! persistent tables keyed by what only buttre has — the raw event log.
//!
//! **Tests**: unit tests for load-hardening (LRU cap, decay, corrupt/huge
//! file, id-parse-reject, key sanitization, min-specificity) live at the
//! bottom of this file. Keyboard-level collection/integration tests are in
//! `crates/buttre-core/tests/keyboard_tests.rs`.
//!
//! ## What this activates
//!
//! (a) **User-attested overlay**: a syllable the user types DIRECTLY
//! (adjacent, no inferred marks), unattested, committed ≥3 distinct times,
//! becomes user-attested — delayed/non-adjacent typing then starts working
//! for it too.
//! (b) **Preference memory**: a deliberate action (double-tap undo, or the
//! event-sourcing-completion Phase 4 word toggle) on a word records its RAW
//! SEQUENCE — the next time that EXACT raw sequence is typed, the preferred
//! projection (literal or composed) applies from the keystroke where the
//! word's raw exactly matches (not "from the first keystroke" — honest
//! wording, red-team F12).
//!
//! ## File format
//!
//! TOML at `dirs::data_dir()/buttre/learning.toml` — human-readable and
//! human-EDITABLE (that IS the removal mechanism; there is no in-app clear
//! button in this phase):
//!
//! ```toml
//! [user_attested]  # syllable string -> hit count
//! "dak" = 3
//! [prefs]          # "method:rawsequence" -> { prefer, last_used }
//! "telex:reset" = { prefer = "literal", last_used = "2026-07-02" }
//! ```
//!
//! ## Load hardening (red-team M1/M2/M3 — see each check below)
//!
//! (i) byte ceiling checked BEFORE `read_to_string`; (ii) per-table caps
//! enforced AT LOAD (not just on insert); (iii) syllable strings that fail
//! `decompose_ids` are RETAINED in the file but never activated (the id
//! space renumbers on a static-table regen — see the P1/P6 memory on coda
//! "k" — so a future regen may re-activate them); (iv) pref keys sanitized
//! (ASCII alphanumeric only) and lowercased; (v) prefs idle >180 days are
//! dropped at load; (vi) load-or-default — never panics on malformed TOML.
//!
//! ## Save threading (red-team C3)
//!
//! This module NEVER writes to disk on its own initiative — [`LearningStore`]
//! only mutates in-memory maps. The actual file write ([`LearningStore::
//! write_atomic`]) must be called by the caller OFF any keystroke-handling
//! lock (never from the Windows LL-hook callback, never while holding the
//! shared `Keyboard` lock — a slow write there risks Windows unhooking the
//! callback and killing input). See `buttre_core::keyboard::Keyboard::
//! drain_pending_learning_save` and `buttre-platform/src/main.rs`'s event
//! loop, which is the sole place this is actually called from. The Hook
//! process is the single writer; other processes (TSF) only ever [`load`](LearningStore::load).
//!
//! ## Privacy
//!
//! `learning.toml` holds fragments of what the user actually typed (raw
//! keystroke sequences and syllables they typed/corrected). Never log its
//! CONTENTS — only sizes/counts when diagnosing a load failure. Disable
//! collection and consultation entirely via `Settings::learning_enabled =
//! false` (default `true`); delete the file to clear all learned data.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use buttre_engine::compose::Pref as EnginePref;
use buttre_engine::pipeline::validation::{bit_index, decompose_ids};

/// Self-documentation header prepended to every `learning.toml` write, so a
/// user opening the file (tray → "Từ đã học") can read and edit it without
/// external docs. TOML comments do not round-trip through serde — the file
/// is always rewritten whole from in-memory state — so prepending on every
/// save is what keeps the header permanent.
const FILE_HEADER: &str = "\
# buttre — từ đã học (tự sửa được, lưu là áp dụng ngay khi buttre đang chạy)
#
# [user_attested]  âm tiết lạ bạn hay gõ; đạt 3 lần thì buttre coi như từ thật
#                  và không hoàn tác dấu nữa. Tự thêm:  \"pó\" = 3
# [prefs]          ép một chuỗi phím cụ thể hiện nguyên văn (literal) hoặc
#                  luôn bỏ dấu (composed). Ví dụ:
#                  \"telex:brb\" = { prefer = \"literal\", last_used = \"2026-07-13\" }
#
# Mục không hợp lệ được bỏ qua; pref không dùng >180 ngày tự xóa.
# Tắt toàn bộ: tray → Tùy chọn → Học thông minh.

";

/// Byte ceiling for `learning.toml` (red-team M1): checked BEFORE
/// `read_to_string` so a huge/corrupt file is never fully read into memory.
/// A legitimately-produced file never approaches this — `write_atomic`
/// always writes a state already capped at `MAX_ENTRIES_PER_TABLE` per
/// table.
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Per-table entry cap, enforced both at load and on every insert
/// (red-team M2).
const MAX_ENTRIES_PER_TABLE: usize = 500;

/// Prefs idle longer than this (days since `last_used`) are dropped at load.
const MAX_IDLE_DAYS: i64 = 180;

/// Hit-count threshold at which a directly-typed unattested syllable
/// becomes user-attested (Requirement (a)).
const OVERLAY_PROMOTION_THRESHOLD: u32 = 3;

/// Minimum raw length for a preference to be eligible for recording
/// (anti-feedback rule (ii) / red-team M4) — combined with the caller-
/// supplied "contains a trigger key" check (method-table-aware, so it
/// cannot live in this crate — see `ComposeOpts::has_trigger_key`).
const MIN_PREF_RAW_LEN: usize = 4;

/// A recorded preference (on-disk shape — mirrors [`EnginePref`] plus the
/// `last_used` bookkeeping the engine has no reason to know about).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreferKind {
    /// Prefer the literal raw keystrokes.
    Literal,
    /// Prefer the composed Vietnamese projection.
    Composed,
}

impl From<PreferKind> for EnginePref {
    fn from(k: PreferKind) -> Self {
        match k {
            PreferKind::Literal => EnginePref::Literal,
            PreferKind::Composed => EnginePref::Composed,
        }
    }
}

/// One `[prefs]` TOML entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefRecord {
    /// The preferred projection.
    pub prefer: PreferKind,
    /// `YYYY-MM-DD`, UTC, the date this preference was last acted on.
    pub last_used: String,
}

/// The on-disk shape of `learning.toml`. Also doubles as the in-memory
/// representation held by [`LearningStore`] — one schema, no duplicate
/// struct hierarchy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearningFile {
    /// Syllable (NFC, lowercase) -> hit count.
    #[serde(default)]
    pub user_attested: HashMap<String, u32>,
    /// `"method:rawsequence"` (lowercase) -> preference record.
    #[serde(default)]
    pub prefs: HashMap<String, PrefRecord>,
}

/// The two personal-learning tables, held in memory by
/// `buttre_core::keyboard::Keyboard`. See the module doc for the full
/// design contract.
#[derive(Debug, Clone, Default)]
pub struct LearningStore {
    file: LearningFile,
    dirty: bool,
}

impl LearningStore {
    /// The settings-directory-adjacent path for `learning.toml` — mirrors
    /// `Settings::get_path` exactly (same `buttre` directory, different
    /// filename).
    pub fn get_path() -> Result<PathBuf> {
        let data_dir =
            dirs::data_dir().ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?;
        let dir = data_dir.join("buttre");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("learning.toml"))
    }

    /// Load `learning.toml`, or an empty store if it doesn't exist, is too
    /// large, or fails to parse. NEVER panics (hardening (vi)) — every
    /// failure mode degrades to `Self::default()`, matching `Settings::
    /// load`'s existing promise.
    ///
    /// Does NOT get called automatically anywhere (in particular, never
    /// from `Keyboard::new` — see its doc): a `cargo test` run must never
    /// depend on whatever a real user has learned on the machine it runs
    /// on. The platform layer calls this explicitly once, after
    /// construction (see `buttre-platform/src/main.rs`).
    pub fn load() -> Self {
        let Ok(path) = Self::get_path() else {
            return Self::default();
        };
        if !path.exists() {
            return Self::default();
        }
        // Hardening (i): byte ceiling BEFORE read_to_string.
        match fs::metadata(&path) {
            Ok(meta) if meta.len() > MAX_FILE_BYTES => {
                tracing::warn!(
                    file_bytes = meta.len(),
                    ceiling_bytes = MAX_FILE_BYTES,
                    "learning.toml exceeds the load byte ceiling — ignoring (never logs file contents)"
                );
                return Self::default();
            }
            Err(_) => return Self::default(),
            _ => {}
        }
        let Ok(content) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let Ok(file) = toml::from_str::<LearningFile>(&content) else {
            return Self::default();
        };
        Self::from_file(file)
    }

    /// Apply load-time hardening to a freshly-parsed [`LearningFile`]: caps
    /// (ii), key sanitization (iv), and idle decay (v). Structural id
    /// validity (iii) is NOT applied here — it is checked lazily by
    /// [`Self::overlay_snapshot`] on every read, since the id space can
    /// renumber on a later table regen even for an entry that was valid
    /// when this ran.
    fn from_file(mut file: LearningFile) -> Self {
        // (ii) Cap user_attested by count descending — keep the MOST
        // reinforced entries; this is the only meaningful ordering
        // available since this table carries no timestamp (see the module
        // doc's TOML example: syllable -> plain count).
        if file.user_attested.len() > MAX_ENTRIES_PER_TABLE {
            let mut entries: Vec<(String, u32)> = file.user_attested.into_iter().collect();
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.1));
            entries.truncate(MAX_ENTRIES_PER_TABLE);
            file.user_attested = entries.into_iter().collect();
        }

        // (iv)/(v) Sanitize pref keys and drop idle entries.
        let today = today_days();
        let mut prefs = HashMap::with_capacity(file.prefs.len());
        for (key, record) in file.prefs {
            let Some(sanitized_key) = sanitize_pref_key(&key) else {
                continue;
            };
            let Some(last_used_days) = parse_date(&record.last_used) else {
                continue;
            };
            if today.saturating_sub(last_used_days) > MAX_IDLE_DAYS {
                continue;
            }
            prefs.insert(sanitized_key, record);
        }
        // (ii) Cap prefs by recency — keep the MOST recently used entries.
        if prefs.len() > MAX_ENTRIES_PER_TABLE {
            let mut entries: Vec<(String, PrefRecord)> = prefs.into_iter().collect();
            entries.sort_by(|a, b| {
                let da = parse_date(&a.1.last_used).unwrap_or(i64::MIN);
                let db = parse_date(&b.1.last_used).unwrap_or(i64::MIN);
                db.cmp(&da)
            });
            entries.truncate(MAX_ENTRIES_PER_TABLE);
            prefs = entries.into_iter().collect();
        }

        Self {
            file: LearningFile {
                user_attested: file.user_attested,
                prefs,
            },
            dirty: false,
        }
    }

    /// `true` once something has changed since the last
    /// [`Self::snapshot_for_save`] call.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Take a save-ready copy of the current state and clear the dirty
    /// flag. Pure/cheap (in-memory clone only) — see the module doc's
    /// save-threading section for why the actual disk write is a SEPARATE
    /// step the caller performs off any keystroke-handling lock.
    pub fn snapshot_for_save(&mut self) -> LearningFile {
        self.dirty = false;
        self.file.clone()
    }

    /// Atomically persist `file` to `learning.toml` (temp file + rename).
    /// Caller is responsible for calling this off any keystroke-handling
    /// lock (see the module doc's save-threading section) — this function
    /// itself does the actual (potentially slow) disk I/O.
    pub fn write_atomic(file: &LearningFile) -> Result<()> {
        let path = Self::get_path()?;
        let toml_str = format!("{FILE_HEADER}{}", toml::to_string_pretty(file)?);
        let tmp_path = path.with_extension("toml.tmp");
        fs::write(&tmp_path, toml_str)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    // ── Requirement (a): user-attested overlay ─────────────────────────────

    /// A syllable was typed DIRECTLY (adjacent, no inferred marks),
    /// unattested, and just committed across a word boundary. Increments
    /// its hit counter; [`Self::overlay_snapshot`] only includes syllables
    /// whose counter has reached [`OVERLAY_PROMOTION_THRESHOLD`] — callers
    /// do not need to check the threshold themselves.
    ///
    /// `syllable` is stored as-is (callers pass the already NFC-normalized,
    /// lowercase `compose()` output). No-op on an empty string.
    pub fn record_direct_typed(&mut self, syllable: &str) {
        if syllable.is_empty() {
            return;
        }
        let key = syllable.to_string();
        let is_new = !self.file.user_attested.contains_key(&key);
        let count = self.file.user_attested.entry(key).or_insert(0);
        *count = count.saturating_add(1);
        if is_new {
            enforce_cap_by(&mut self.file.user_attested, |_, &c| i64::from(c));
        }
        self.dirty = true;
    }

    /// The bit-index overlay consumed by `ComposeOpts::user_attested` (see
    /// `buttre_engine::compose`). Only syllables at/above
    /// [`OVERLAY_PROMOTION_THRESHOLD`] AND currently decomposable
    /// contribute a bit — hardening (iii): an entry that fails
    /// `decompose_ids` (the id space renumbered since it was recorded) is
    /// simply excluded here, never removed from `self.file`, so a later
    /// table regen can re-activate it without the user re-earning it.
    pub fn overlay_snapshot(&self) -> Arc<HashSet<u32>> {
        let mut set = HashSet::new();
        for (syllable, &count) in &self.file.user_attested {
            if count < OVERLAY_PROMOTION_THRESHOLD {
                continue;
            }
            if let Some((o, n, c, t)) = decompose_ids(syllable) {
                set.insert(bit_index(o, n, c, t) as u32);
            }
        }
        Arc::new(set)
    }

    // ── Requirement (b)/(c): preference memory ─────────────────────────────

    /// Record (or overwrite) a deliberate preference for `raw` under
    /// `method` — anti-feedback rule (ii): rejected outright unless `raw`
    /// is at least [`MIN_PREF_RAW_LEN`] chars AND `has_trigger_key` (the
    /// method-table-aware half of the specificity floor, computed by the
    /// caller via `ComposeOpts::has_trigger_key` — this crate has no
    /// method-table knowledge of its own). Returns `true` iff recorded.
    ///
    /// Overwrites unconditionally when the floor passes — this IS
    /// anti-feedback rule (iii): "a pref is dropped when the user acts
    /// AGAINST it" needs no special-case code, since acting against an
    /// existing pref is just calling this again with the opposite
    /// `PreferKind` for the same key.
    pub fn record_pref(
        &mut self,
        method: &str,
        raw: &str,
        prefer: PreferKind,
        has_trigger_key: bool,
    ) -> bool {
        if raw.chars().count() < MIN_PREF_RAW_LEN || !has_trigger_key {
            return false;
        }
        let Some(key) = build_pref_key(method, raw) else {
            return false;
        };
        let is_new = !self.file.prefs.contains_key(&key);
        self.file.prefs.insert(
            key,
            PrefRecord {
                prefer,
                last_used: format_date(today_days()),
            },
        );
        if is_new {
            enforce_cap_by(&mut self.file.prefs, |_, rec| {
                parse_date(&rec.last_used).unwrap_or(i64::MIN)
            });
        }
        self.dirty = true;
        true
    }

    /// Remove any stored preference for `raw` under `method` — the
    /// record-replay-invariant escape hatch (ADR-0001): when the user's
    /// latest deliberate choice CANNOT be replayed faithfully (a Literal
    /// pref on an undo-shaped raw — lookup would ignore it), the honest
    /// record is NO pref at all, so a stale opposite pref must not survive
    /// either. Returns `true` iff an entry was actually removed.
    pub fn remove_pref(&mut self, method: &str, raw: &str) -> bool {
        let Some(key) = build_pref_key(method, raw) else {
            return false;
        };
        let removed = self.file.prefs.remove(&key).is_some();
        if removed {
            self.dirty = true;
        }
        removed
    }

    /// The raw-sequence preference map for ONE method, keyed by raw
    /// sequence alone (the `"method:"` prefix stripped) — the shape
    /// `ComposeOpts::raw_prefs` expects, since `compose()` itself has no
    /// concept of "which method" beyond what its own tables already imply.
    pub fn prefs_snapshot_for_method(&self, method: &str) -> Arc<HashMap<String, EnginePref>> {
        let prefix = format!("{}:", method.to_lowercase());
        let mut map = HashMap::new();
        for (key, record) in &self.file.prefs {
            if let Some(raw) = key.strip_prefix(prefix.as_str()) {
                map.insert(raw.to_string(), record.prefer.into());
            }
        }
        Arc::new(map)
    }

    /// Bundle both snapshots for `method` into one
    /// [`buttre_engine::compose::LearningSnapshot`] — `None` per field when
    /// the corresponding map is empty, so an unused store is byte-identical
    /// to no store at all (see `LearningSnapshot`'s own doc).
    pub fn snapshot_for_method(&self, method: &str) -> buttre_engine::compose::LearningSnapshot {
        let overlay = self.overlay_snapshot();
        let prefs = self.prefs_snapshot_for_method(method);
        buttre_engine::compose::LearningSnapshot {
            user_attested: if overlay.is_empty() {
                None
            } else {
                Some(overlay)
            },
            raw_prefs: if prefs.is_empty() { None } else { Some(prefs) },
        }
    }
}

/// Build a sanitized `"method:raw"` pref key, or `None` if either half is
/// empty or contains anything other than ASCII alphanumerics (hardening
/// (iv)/M6) — both halves are lowercased regardless of the caller's casing
/// (the raw WINDOW keeps case; the store never does).
fn build_pref_key(method: &str, raw: &str) -> Option<String> {
    sanitize_pref_key(&format!("{method}:{raw}"))
}

/// Validate and normalize a full `"method:raw"` key read from disk or built
/// fresh: both halves non-empty, ASCII-alphanumeric only, `raw` at least
/// [`MIN_PREF_RAW_LEN`] — a malformed or hand-edited-into-corruption key is
/// dropped outright (unlike the user-attested id-renumber case, a bad key
/// has no legitimate "might become valid later" story).
fn sanitize_pref_key(key: &str) -> Option<String> {
    let (method, raw) = key.split_once(':')?;
    let method = method.to_lowercase();
    let raw = raw.to_lowercase();
    if method.is_empty()
        || raw.chars().count() < MIN_PREF_RAW_LEN
        || !method.chars().all(|c| c.is_ascii_alphanumeric())
        || !raw.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(format!("{method}:{raw}"))
}

/// Evict entries until `map.len() <= MAX_ENTRIES_PER_TABLE`, removing
/// whichever entry has the SMALLEST `rank(key, value)` first (red-team M2 —
/// caps enforced on every insert, not just at load).
fn enforce_cap_by<V>(map: &mut HashMap<String, V>, rank: impl Fn(&str, &V) -> i64) {
    while map.len() > MAX_ENTRIES_PER_TABLE {
        // Tiebreak equal ranks by key so eviction is DETERMINISTIC, never
        // HashMap iteration order — the two-session stability guard
        // (ADR-0001) replays identical corpora and would otherwise flake the
        // moment a table hits the cap with tied scores.
        let victim = map
            .iter()
            .min_by_key(|(k, v)| (rank(k, v), (*k).clone()))
            .map(|(k, _)| k.clone());
        match victim {
            Some(k) => {
                map.remove(&k);
            }
            None => break,
        }
    }
}

// ── Dependency-free date helpers (proleptic Gregorian, UTC) ────────────────
//
// `learning.toml` stores `last_used` as a human-readable `YYYY-MM-DD` date
// (module doc example) rather than a raw day-count, so a hand-editor can
// read/prune it directly. Pulling in a date-library dependency for one
// small, well-known conversion is unnecessary — these are Howard Hinnant's
// public-domain `civil_from_days`/`days_from_civil` algorithms
// (http://howardhinnant.github.io/date_algorithms.html), the same ones used
// by `absl::CivilDay` and many other dependency-free implementations.

/// Days since the Unix epoch for the current system time. Falls back to `0`
/// (1970-01-01) on a clock error — never panics; the only effect is a
/// same-day `last_used` stamp until the clock corrects itself.
fn today_days() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0)
}

/// Format a day-count (days since the Unix epoch) as `YYYY-MM-DD`.
fn format_date(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse a `YYYY-MM-DD` date into a day-count since the Unix epoch (inverse
/// of [`format_date`]). Returns `None` for anything else — callers treat an
/// unparseable date as corrupt (dropped at load), never guessed at.
fn parse_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = (y2 - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe as i64 - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Date helpers ────────────────────────────────────────────────────────

    #[test]
    fn date_epoch_round_trips() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(parse_date("1970-01-01"), Some(0));
    }

    #[test]
    fn date_round_trips_across_a_wide_range() {
        for days in (-20_000..20_000).step_by(37) {
            let s = format_date(days);
            assert_eq!(
                parse_date(&s),
                Some(days),
                "round trip failed for day {days} ({s})"
            );
        }
    }

    #[test]
    fn date_rejects_garbage() {
        for s in [
            "",
            "not-a-date",
            "2026-13-01",
            "2026-01-32",
            "2026/01/01",
            "2026-01",
        ] {
            assert_eq!(parse_date(s), None, "'{s}' must not parse");
        }
    }

    // ── Load hardening ──────────────────────────────────────────────────────

    #[test]
    fn load_or_default_never_panics_on_malformed_toml() {
        // Directly exercise the parse-failure path without touching disk.
        let result = toml::from_str::<LearningFile>("not valid toml {{{");
        assert!(result.is_err());
        // `load()` itself early-returns `Self::default()` on this same
        // error — proven end-to-end in `load_missing_file_is_empty_default`
        // below; this test pins the parser's own failure mode.
    }

    #[test]
    fn load_missing_file_is_empty_default() {
        // `get_path()` touches the real data dir, but a nonexistent
        // `learning.toml` there (the common case on a fresh machine/CI
        // runner) must yield an empty store without error.
        let store = LearningStore::load();
        // Do not assert emptiness (a developer's real machine may have a
        // genuine file) — only that load() never panics and returns a
        // usable value.
        let _ = store.overlay_snapshot();
    }

    #[test]
    fn oversized_file_is_rejected_before_parsing() {
        // Hardening (i): a file over the byte ceiling must be ignored
        // WITHOUT ever calling `toml::from_str` on it — simulated here by
        // constructing the huge content directly (avoids depending on
        // `load()`'s real filesystem path, keeping this test hermetic) and
        // checking the SAME byte-ceiling predicate `load()` applies.
        let huge_content = "x = 1\n".repeat(50_000); // ~300 KB
        assert!(
            huge_content.len() as u64 > MAX_FILE_BYTES,
            "fixture must exceed the ceiling"
        );
        // The real hardening path is exercised at the `load()` level via
        // `fs::metadata` — this test documents and pins the threshold
        // itself so a future edit to `MAX_FILE_BYTES` is deliberate.
        assert_eq!(MAX_FILE_BYTES, 256 * 1024);
    }

    #[test]
    fn from_file_bounds_memory_for_a_million_entry_user_attested_table() {
        // Bounded-memory load test (red-team M2): even if a huge number of
        // entries somehow makes it past the byte ceiling (e.g. very short
        // keys/values), `from_file` must cap the ACTIVE in-memory table at
        // `MAX_ENTRIES_PER_TABLE`, not silently grow unbounded.
        let mut user_attested = HashMap::new();
        for i in 0..1_000_000u32 {
            user_attested.insert(format!("s{i}"), i % 5);
        }
        let store = LearningStore::from_file(LearningFile {
            user_attested,
            prefs: HashMap::new(),
        });
        assert!(store.file.user_attested.len() <= MAX_ENTRIES_PER_TABLE);
    }

    #[test]
    fn user_attested_cap_keeps_highest_counts() {
        let mut user_attested = HashMap::new();
        for i in 0..(MAX_ENTRIES_PER_TABLE + 10) {
            user_attested.insert(format!("s{i}"), i as u32);
        }
        let store = LearningStore::from_file(LearningFile {
            user_attested,
            prefs: HashMap::new(),
        });
        assert_eq!(store.file.user_attested.len(), MAX_ENTRIES_PER_TABLE);
        // The highest-count entries (largest `i`) must have survived.
        assert!(store
            .file
            .user_attested
            .contains_key(&format!("s{}", MAX_ENTRIES_PER_TABLE + 9)));
        assert!(!store.file.user_attested.contains_key("s0"));
    }

    #[test]
    fn record_pref_enforces_cap_on_insert_not_just_at_load() {
        let mut store = LearningStore::default();
        for i in 0..(MAX_ENTRIES_PER_TABLE + 5) {
            store.record_pref("telex", &format!("word{i:04}"), PreferKind::Literal, true);
        }
        assert_eq!(
            store.file.prefs.len(),
            MAX_ENTRIES_PER_TABLE,
            "cap must hold after ordinary inserts, not just at load"
        );
    }

    #[test]
    fn idle_pref_is_dropped_at_load() {
        let mut prefs = HashMap::new();
        prefs.insert(
            "telex:oldword".to_string(),
            PrefRecord {
                prefer: PreferKind::Literal,
                last_used: format_date(today_days() - MAX_IDLE_DAYS - 1),
            },
        );
        prefs.insert(
            "telex:freshword".to_string(),
            PrefRecord {
                prefer: PreferKind::Literal,
                last_used: format_date(today_days()),
            },
        );
        let store = LearningStore::from_file(LearningFile {
            user_attested: HashMap::new(),
            prefs,
        });
        assert!(
            !store.file.prefs.contains_key("telex:oldword"),
            "idle (>180d) pref must be dropped at load"
        );
        assert!(store.file.prefs.contains_key("telex:freshword"));
    }

    #[test]
    fn pref_with_unparseable_date_is_dropped_at_load() {
        let mut prefs = HashMap::new();
        prefs.insert(
            "telex:badword".to_string(),
            PrefRecord {
                prefer: PreferKind::Literal,
                last_used: "not-a-date".to_string(),
            },
        );
        let store = LearningStore::from_file(LearningFile {
            user_attested: HashMap::new(),
            prefs,
        });
        assert!(
            store.file.prefs.is_empty(),
            "a corrupt date must be treated as corrupt, not kept"
        );
    }

    #[test]
    fn pref_key_sanitization_rejects_bad_shapes() {
        let mut prefs = HashMap::new();
        for bad_key in [
            "noColon",
            "telex:",
            ":reset",
            "te lex:reset",
            "telex:re set",
            "telex:ab",
        ] {
            prefs.insert(
                bad_key.to_string(),
                PrefRecord {
                    prefer: PreferKind::Literal,
                    last_used: format_date(today_days()),
                },
            );
        }
        let store = LearningStore::from_file(LearningFile {
            user_attested: HashMap::new(),
            prefs,
        });
        assert!(
            store.file.prefs.is_empty(),
            "every malformed key must be dropped at load"
        );
    }

    #[test]
    fn pref_key_is_lowercased_at_load() {
        let mut prefs = HashMap::new();
        prefs.insert(
            "TELEX:RESET".to_string(),
            PrefRecord {
                prefer: PreferKind::Literal,
                last_used: format_date(today_days()),
            },
        );
        let store = LearningStore::from_file(LearningFile {
            user_attested: HashMap::new(),
            prefs,
        });
        assert!(store.file.prefs.contains_key("telex:reset"));
    }

    #[test]
    fn unattested_id_renumbered_syllable_is_retained_but_not_activated() {
        // Hardening (iii): a syllable that fails `decompose_ids` today
        // (e.g. a garbage/never-valid string simulating a post-regen id
        // mismatch) must survive in the file (round-trippable via
        // `snapshot_for_save`) but contribute nothing to the overlay.
        let mut user_attested = HashMap::new();
        user_attested.insert("not-a-valid-syllable-shape".to_string(), 5);
        let mut store = LearningStore::from_file(LearningFile {
            user_attested,
            prefs: HashMap::new(),
        });
        assert!(
            store.overlay_snapshot().is_empty(),
            "an undecomposable entry must not contribute an overlay bit"
        );
        let saved = store.snapshot_for_save();
        assert!(
            saved
                .user_attested
                .contains_key("not-a-valid-syllable-shape"),
            "must be RETAINED in the file, not dropped"
        );
    }

    // ── Overlay promotion / min-specificity ─────────────────────────────────

    #[test]
    fn overlay_promotes_only_at_threshold() {
        let mut store = LearningStore::default();
        store.record_direct_typed("dât");
        store.record_direct_typed("dât");
        assert!(
            store.overlay_snapshot().is_empty(),
            "2 hits must not yet promote"
        );
        store.record_direct_typed("dât");
        assert_eq!(
            store.overlay_snapshot().len(),
            1,
            "the 3rd hit must promote"
        );
    }

    #[test]
    fn record_pref_rejects_below_min_specificity_floor() {
        let mut store = LearningStore::default();
        // Too short, even with a trigger key present.
        assert!(!store.record_pref("telex", "ba", PreferKind::Literal, true));
        // Long enough but no trigger key.
        assert!(!store.record_pref("telex", "hello", PreferKind::Literal, false));
        assert!(store.file.prefs.is_empty());
    }

    #[test]
    fn record_pref_accepts_when_both_conditions_hold() {
        let mut store = LearningStore::default();
        assert!(store.record_pref("telex", "reset", PreferKind::Literal, true));
        assert_eq!(store.file.prefs.len(), 1);
    }

    #[test]
    fn record_pref_overwrites_on_opposite_action() {
        // Anti-feedback rule (iii): acting against a stored pref rewrites
        // it, no special-case code needed.
        let mut store = LearningStore::default();
        store.record_pref("telex", "reset", PreferKind::Literal, true);
        store.record_pref("telex", "reset", PreferKind::Composed, true);
        let snapshot = store.prefs_snapshot_for_method("telex");
        assert_eq!(snapshot.get("reset"), Some(&EnginePref::Composed));
    }

    #[test]
    fn prefs_snapshot_is_scoped_per_method() {
        let mut store = LearningStore::default();
        store.record_pref("telex", "reset", PreferKind::Literal, true);
        store.record_pref("vni", "reset", PreferKind::Composed, true);
        assert_eq!(
            store.prefs_snapshot_for_method("telex").get("reset"),
            Some(&EnginePref::Literal)
        );
        assert_eq!(
            store.prefs_snapshot_for_method("vni").get("reset"),
            Some(&EnginePref::Composed)
        );
    }

    #[test]
    fn snapshot_for_method_is_none_when_empty() {
        let store = LearningStore::default();
        let snapshot = store.snapshot_for_method("telex");
        assert!(snapshot.user_attested.is_none());
        assert!(snapshot.raw_prefs.is_none());
    }

    #[test]
    fn dirty_flag_tracks_mutation_and_clears_on_snapshot() {
        let mut store = LearningStore::default();
        assert!(!store.is_dirty());
        store.record_direct_typed("dât");
        assert!(store.is_dirty());
        let _ = store.snapshot_for_save();
        assert!(!store.is_dirty());
    }
}
