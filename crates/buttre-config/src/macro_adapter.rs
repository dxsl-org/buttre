//! "Gõ tắt" tab adapter: CRUD over `macros.toml`'s `[macros]` table, plus
//! config-time validation and a non-blocking collision warning (research-02
//! finding #2 — no Vietnamese IME does this; it is buttre's differentiator).
//!
//! Validation happens HERE (before `write_atomic`) so the user sees a
//! message immediately; `MacroStore::write_atomic`'s own load-time hardening
//! (`MacroStore::from_file`, applied on the NEXT load) is the backstop that
//! makes a hand-edited-into-corruption file safe regardless of this UI.

use buttre_core::state::macros::{MacroEntry, MacroStore};
use buttre_engine::pipeline::validation::is_attested;

/// One editable row.
#[derive(Clone)]
pub struct MacroRow {
    pub trigger: String,
    pub expand: String,
    pub enabled: bool,
}

/// Load all macros as rows, sorted by trigger for a stable display order.
pub fn load_rows() -> Vec<MacroRow> {
    let store = MacroStore::load();
    let file = store.snapshot_for_save();
    let mut rows: Vec<MacroRow> = file
        .macros
        .into_iter()
        .map(|(trigger, e)| MacroRow {
            trigger,
            expand: e.expand,
            enabled: e.enabled,
        })
        .collect();
    rows.sort_by(|a, b| a.trigger.cmp(&b.trigger));
    rows
}

/// Validation failure — shown to the user verbatim (already Vietnamese,
/// end-user-facing text, not a debug message).
#[derive(Debug, PartialEq, Eq)]
pub struct ValidationError(pub String);

/// A non-blocking heads-up shown alongside a successful add/edit — buttre's
/// differentiator (research-02): no existing Vietnamese IME warns about
/// this collision class, they only offer a blunt global on/off.
#[derive(Debug, PartialEq, Eq)]
pub struct CollisionWarning(pub String);

/// Mirrors `MacroStore::from_file`'s own hardening floor — reject HERE with
/// a message instead of silently dropping the entry at the next load.
const MIN_TRIGGER_CHARS: usize = 2;
const MAX_EXPANSION_CHARS: usize = 256;

/// Validate a candidate trigger/expansion pair against the CURRENT rows
/// (excluding `editing`, the row being edited in place, if any — so
/// re-saving a row unchanged doesn't flag itself as a duplicate of itself).
fn validate(
    rows: &[MacroRow],
    editing: Option<&str>,
    trigger: &str,
    expand: &str,
) -> Result<(), ValidationError> {
    let trigger = trigger.trim();
    if trigger.chars().count() < MIN_TRIGGER_CHARS
        || !trigger.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(ValidationError(format!(
            "Chuỗi gõ tắt phải có ít nhất {MIN_TRIGGER_CHARS} ký tự, chỉ gồm chữ/số (a-z, 0-9)."
        )));
    }
    if expand.trim().is_empty() {
        return Err(ValidationError(
            "Văn bản thay thế không được để trống.".to_string(),
        ));
    }
    if expand.chars().count() > MAX_EXPANSION_CHARS {
        return Err(ValidationError(format!(
            "Văn bản thay thế quá dài (tối đa {MAX_EXPANSION_CHARS} ký tự)."
        )));
    }
    let trigger_lc = trigger.to_lowercase();
    let is_duplicate = rows
        .iter()
        .any(|r| r.trigger.to_lowercase() == trigger_lc && editing != Some(r.trigger.as_str()));
    if is_duplicate {
        return Err(ValidationError(format!(
            "Chuỗi gõ tắt \"{trigger}\" đã tồn tại — sửa mục cũ thay vì thêm trùng."
        )));
    }
    Ok(())
}

/// Non-blocking collision check: does `trigger` also happen to be a real,
/// attested Vietnamese syllable? Typing it will now always expand instead
/// of composing — a deliberate trade the user should see, not be blocked
/// from making (they may WANT to shadow it, e.g. an uncommon syllable).
fn check_collision(trigger: &str) -> Option<CollisionWarning> {
    is_attested(trigger).then(|| {
        CollisionWarning(format!(
            "\"{trigger}\" cũng là một âm tiết tiếng Việt thật — gõ nó sẽ luôn ra gõ tắt, không bao giờ ra tiếng Việt nữa."
        ))
    })
}

/// Add a new macro. Returns the collision warning (if any) on success.
pub fn add(
    trigger: &str,
    expand: &str,
    enabled: bool,
) -> Result<Option<CollisionWarning>, ValidationError> {
    let rows = load_rows();
    validate(&rows, None, trigger, expand)?;
    let mut store_file = MacroStore::load().snapshot_for_save();
    store_file.macros.insert(
        trigger.trim().to_lowercase(),
        MacroEntry {
            expand: expand.trim().to_string(),
            enabled,
        },
    );
    MacroStore::write_atomic(&store_file).map_err(|e| ValidationError(e.to_string()))?;
    Ok(check_collision(trigger.trim()))
}

/// Edit an existing macro identified by its CURRENT trigger (`old_trigger`).
/// Allows renaming the trigger itself.
pub fn edit(
    old_trigger: &str,
    new_trigger: &str,
    expand: &str,
    enabled: bool,
) -> Result<Option<CollisionWarning>, ValidationError> {
    let rows = load_rows();
    validate(&rows, Some(old_trigger), new_trigger, expand)?;
    let mut store_file = MacroStore::load().snapshot_for_save();
    store_file.macros.remove(&old_trigger.to_lowercase());
    store_file.macros.insert(
        new_trigger.trim().to_lowercase(),
        MacroEntry {
            expand: expand.trim().to_string(),
            enabled,
        },
    );
    MacroStore::write_atomic(&store_file).map_err(|e| ValidationError(e.to_string()))?;
    Ok(check_collision(new_trigger.trim()))
}

/// Flip a single row's `enabled` flag without touching anything else.
pub fn set_enabled(trigger: &str, enabled: bool) -> anyhow::Result<()> {
    let mut store_file = MacroStore::load().snapshot_for_save();
    if let Some(entry) = store_file.macros.get_mut(&trigger.to_lowercase()) {
        entry.enabled = enabled;
    }
    MacroStore::write_atomic(&store_file)
}

/// Delete one macro.
pub fn delete(trigger: &str) -> anyhow::Result<()> {
    let mut store_file = MacroStore::load().snapshot_for_save();
    store_file.macros.remove(&trigger.to_lowercase());
    MacroStore::write_atomic(&store_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(trigger: &str) -> MacroRow {
        MacroRow {
            trigger: trigger.to_string(),
            expand: "x".to_string(),
            enabled: true,
        }
    }

    #[test]
    fn validate_rejects_short_trigger() {
        assert!(validate(&[], None, "v", "Việt Nam").is_err());
    }

    #[test]
    fn validate_rejects_non_alphanumeric_trigger() {
        assert!(validate(&[], None, "v-n", "Việt Nam").is_err());
    }

    #[test]
    fn validate_rejects_empty_expansion() {
        assert!(validate(&[], None, "vn", "").is_err());
    }

    #[test]
    fn validate_rejects_oversized_expansion() {
        let huge = "x".repeat(MAX_EXPANSION_CHARS + 1);
        assert!(validate(&[], None, "vn", &huge).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_trigger_case_insensitive() {
        let rows = vec![row("vn")];
        assert!(validate(&rows, None, "VN", "Việt Nam").is_err());
    }

    #[test]
    fn validate_allows_editing_the_same_row_unchanged() {
        let rows = vec![row("vn")];
        assert!(validate(&rows, Some("vn"), "vn", "Việt Nam mới").is_ok());
    }

    #[test]
    fn validate_allows_new_distinct_trigger() {
        let rows = vec![row("vn")];
        assert!(validate(&rows, None, "brb", "be right back").is_ok());
    }

    #[test]
    fn collision_flags_real_vietnamese_syllable() {
        // "cha" is a real attested Vietnamese syllable (father/prefix particle).
        assert!(check_collision("cha").is_some());
    }

    #[test]
    fn collision_silent_for_non_vietnamese_trigger() {
        assert!(check_collision("brb").is_none());
    }
}
