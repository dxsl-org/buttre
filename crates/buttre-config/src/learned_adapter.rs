//! "Từ đã học" tab adapter: converts `LearningFile::user_attested` to/from
//! the Slint table row model. Deletion/clear rebuild the FULL
//! `LearningFile` and round-trip `prefs` untouched — a syllable-table edit
//! must never silently drop the separate preference-memory table.

use buttre_core::state::learning::LearningStore;

/// One row: a learned syllable and its hit count (only entries at/above the
/// promotion threshold are meaningfully "learned", but the raw count is
/// shown as-is — this is the raw store, not the compose-time overlay view).
#[derive(Clone)]
pub struct LearnedWordRow {
    pub word: String,
    pub count: u32,
}

/// Load the current `user_attested` table as sorted rows (highest count
/// first — the most reinforced words are the most likely to interest the
/// user first).
pub fn load_rows() -> Vec<LearnedWordRow> {
    let mut store = LearningStore::load();
    let file = store.snapshot_for_save();
    let mut rows: Vec<LearnedWordRow> = file
        .user_attested
        .into_iter()
        .map(|(word, count)| LearnedWordRow { word, count })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    rows
}

/// Remove one learned word and persist — `prefs` is read fresh from disk
/// and written back unedited (it goes through the SAME
/// `LearningStore::load` hardening any read does — e.g. a pref idle >180
/// days is dropped, same as it would be on the next real load — this
/// function itself never touches the `prefs` map).
pub fn delete_word(word: &str) -> anyhow::Result<()> {
    let mut store = LearningStore::load();
    let mut file = store.snapshot_for_save();
    file.user_attested.remove(word);
    LearningStore::write_atomic(&file)
}

/// Clear every learned word — `prefs` survives (see `delete_word`'s doc).
pub fn clear_all() -> anyhow::Result<()> {
    let mut store = LearningStore::load();
    let mut file = store.snapshot_for_save();
    file.user_attested.clear();
    LearningStore::write_atomic(&file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buttre_core::state::learning::{LearningFile, PrefRecord, PreferKind};

    /// Guards the design's whole point: a "Từ đã học" edit must never touch
    /// `prefs`. Writes directly to the REAL `learning.toml` path (these
    /// adapter functions have no injectable in-memory store — they are
    /// driven by the real config window) — accepted test-isolation risk,
    /// consistent with this workspace's existing `Settings`/`AppState` test
    /// patterns, and safe from corruption (not just clobbering) thanks to
    /// the per-call-unique atomic-write temp names.
    #[test]
    fn delete_word_preserves_prefs() {
        let mut seed = LearningFile::default();
        seed.user_attested
            .insert("marker_word_to_delete".to_string(), 5);
        seed.prefs.insert(
            "telex:markerpref".to_string(),
            PrefRecord {
                prefer: PreferKind::Literal,
                last_used: "2026-07-13".to_string(),
            },
        );
        LearningStore::write_atomic(&seed).expect("seed write must succeed");

        delete_word("marker_word_to_delete").expect("delete must succeed");

        let mut reloaded = LearningStore::load();
        let file = reloaded.snapshot_for_save();
        assert!(
            !file.user_attested.contains_key("marker_word_to_delete"),
            "the deleted word must be gone"
        );
        assert!(
            file.prefs.contains_key("telex:markerpref"),
            "prefs must survive a user_attested-only edit"
        );
    }

    #[test]
    fn rows_sort_by_count_descending_then_word() {
        let mut file = LearningFile::default();
        file.user_attested.insert("bbb".to_string(), 1);
        file.user_attested.insert("aaa".to_string(), 3);
        file.user_attested.insert("ccc".to_string(), 3);
        let mut rows: Vec<LearnedWordRow> = file
            .user_attested
            .into_iter()
            .map(|(word, count)| LearnedWordRow { word, count })
            .collect();
        rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
        let words: Vec<&str> = rows.iter().map(|r| r.word.as_str()).collect();
        assert_eq!(words, vec!["aaa", "ccc", "bbb"]);
    }
}
