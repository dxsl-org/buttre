//! Candidate list state (Nôm lookup), shared by every backend that shows one.
//!
//! IBus, the Wayland engine, the macOS FFI and the Windows TSF text service all
//! need the same three things — which candidates are offered, which one is
//! highlighted, and what a number key at position N on the current page means —
//! and they disagree only about how to DRAW them. Keeping that logic here means
//! a selection bug is fixed once instead of per platform, and it is testable on
//! any OS: nothing in this module touches a window, a socket, or COM.
//!
//! Paging is deliberately NOT stored. The page size belongs to whoever draws
//! the list (IBus asks the client, the TSF panel derives it from the window),
//! so it is passed in per call and the cursor alone determines which page is
//! current. Storing it here would let the two drift apart silently.

/// One candidate offered for the current composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateView {
    /// Shown in the candidate UI — the character plus its gloss, e.g.
    /// `"𡗶 (trời)"`.
    pub display: String,
    /// Committed when this candidate is chosen — the bare character, e.g.
    /// `"𡗶"` (the engine `Candidate`'s `get_value()`).
    pub value: String,
}

impl CandidateView {
    /// Convert the engine's candidates into views.
    ///
    /// Shared because getting the two fields the wrong way round fails
    /// QUIETLY: the panel still looks right, and the gloss only appears in the
    /// user's document when they pick something — `"𡗶 (trời)"` inserted where
    /// `"𡗶"` belonged.
    pub fn from_engine(candidates: &[buttre_engine::pipeline::Candidate]) -> Vec<Self> {
        candidates
            .iter()
            .map(|c| Self {
                display: c.text.clone(),
                value: c.get_value().to_string(),
            })
            .collect()
    }
}

/// The offered candidates plus the highlight position.
///
/// Empty is the resting state: no list showing, cursor meaningless. Every
/// mutator keeps the cursor inside the list, so callers never have to bounds-check
/// before rendering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateState {
    items: Vec<CandidateView>,
    cursor: usize,
}

impl CandidateState {
    /// Replace the list, highlighting the first entry.
    ///
    /// A fresh lookup always starts at the top: the engine ranks candidates, so
    /// carrying a previous cursor over would highlight an unrelated character.
    pub fn set(&mut self, items: Vec<CandidateView>) {
        self.items = items;
        self.cursor = 0;
    }

    /// Drop the list. Returns whether anything was actually showing, so callers
    /// can emit a hide/repaint only when it means something.
    pub fn clear(&mut self) -> bool {
        let had = !self.items.is_empty();
        self.items.clear();
        self.cursor = 0;
        had
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Global index of the highlighted candidate. Always 0 when empty.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn items(&self) -> &[CandidateView] {
        &self.items
    }

    /// Global index where the page holding the cursor starts.
    ///
    /// # Panics
    /// Panics if `page` is 0 — a zero-sized page has no meaningful arithmetic
    /// and every caller derives it from a real UI, so this can only be a bug.
    pub fn page_start(&self, page: usize) -> usize {
        assert!(page > 0, "page size must be non-zero");
        (self.cursor / page) * page
    }

    /// The candidates on the page holding the cursor, for rendering.
    pub fn page_items(&self, page: usize) -> &[CandidateView] {
        let start = self.page_start(page);
        let end = (start + page).min(self.items.len());
        &self.items[start..end]
    }

    /// Total number of pages at this page size (0 when nothing is showing).
    pub fn page_count(&self, page: usize) -> usize {
        assert!(page > 0, "page size must be non-zero");
        self.items.len().div_ceil(page)
    }

    /// Move the highlight to the next candidate, wrapping at the end.
    /// Returns whether the cursor moved (false when empty, or a single item).
    pub fn move_next(&mut self) -> bool {
        self.move_cursor(|cur, n| (cur + 1) % n)
    }

    /// Move the highlight to the previous candidate, wrapping at the start.
    pub fn move_prev(&mut self) -> bool {
        self.move_cursor(|cur, n| (cur + n - 1) % n)
    }

    /// Advance the highlight by one page, clamped to the last candidate.
    pub fn page_down(&mut self, page: usize) -> bool {
        self.move_cursor(move |cur, n| (cur + page).min(n - 1))
    }

    /// Retreat the highlight by one page, clamped to the first candidate.
    pub fn page_up(&mut self, page: usize) -> bool {
        self.move_cursor(move |cur, _| cur.saturating_sub(page))
    }

    fn move_cursor(&mut self, f: impl FnOnce(usize, usize) -> usize) -> bool {
        let n = self.items.len();
        if n == 0 {
            return false;
        }
        let next = f(self.cursor, n).min(n - 1);
        let moved = next != self.cursor;
        self.cursor = next;
        moved
    }

    /// Take the value at global `index` and clear the list. `None` (and no
    /// state change) when out of range.
    pub fn take_at(&mut self, index: usize) -> Option<String> {
        let value = self.items.get(index)?.value.clone();
        self.clear();
        Some(value)
    }

    /// Take the candidate at `page_index` (0-based) WITHIN the page currently
    /// holding the cursor — the mapping for number keys 1..=9 and for a click
    /// on the panel, both of which are page-relative, not global.
    pub fn take_at_page(&mut self, page_index: usize, page: usize) -> Option<String> {
        self.take_at(self.page_start(page) + page_index)
    }

    /// Take the highlighted candidate (Space/Enter, panel double-click).
    pub fn take_current(&mut self) -> Option<String> {
        self.take_at(self.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views(n: usize) -> Vec<CandidateView> {
        (0..n)
            .map(|i| CandidateView {
                display: format!("c{i} (gloss)"),
                value: format!("c{i}"),
            })
            .collect()
    }

    fn state(n: usize) -> CandidateState {
        let mut s = CandidateState::default();
        s.set(views(n));
        s
    }

    #[test]
    fn empty_state_is_inert() {
        let mut s = CandidateState::default();
        assert!(s.is_empty());
        assert!(!s.clear(), "clearing nothing must not ask for a repaint");
        assert!(!s.move_next());
        assert!(!s.move_prev());
        assert_eq!(s.take_current(), None);
        assert_eq!(s.page_count(9), 0);
    }

    #[test]
    fn set_resets_cursor_to_top() {
        let mut s = state(5);
        s.move_next();
        s.move_next();
        assert_eq!(s.cursor(), 2);
        // A new lookup ranks its own candidates — keeping the old cursor would
        // highlight an unrelated character.
        s.set(views(3));
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn cursor_wraps_both_ways() {
        let mut s = state(3);
        assert!(s.move_prev());
        assert_eq!(s.cursor(), 2, "prev from the top wraps to the end");
        assert!(s.move_next());
        assert_eq!(s.cursor(), 0, "next from the end wraps to the top");
    }

    #[test]
    fn paging_clamps_instead_of_wrapping() {
        let mut s = state(12);
        assert!(s.page_down(5));
        assert_eq!(s.cursor(), 5);
        assert!(s.page_down(5));
        assert_eq!(s.cursor(), 10);
        // Clamped, not wrapped: a page jump past the end should land on the
        // last candidate, not back at the start.
        assert!(s.page_down(5));
        assert_eq!(s.cursor(), 11);
        assert!(!s.page_down(5), "already at the end");
        s.page_up(5);
        assert_eq!(s.cursor(), 6);
    }

    #[test]
    fn page_window_follows_the_cursor() {
        let mut s = state(12);
        assert_eq!(s.page_start(5), 0);
        assert_eq!(s.page_items(5).len(), 5);
        s.page_down(5);
        assert_eq!(s.page_start(5), 5);
        s.page_down(5);
        assert_eq!(s.page_start(5), 10);
        assert_eq!(s.page_items(5).len(), 2, "last page is short");
        assert_eq!(s.page_count(5), 3);
    }

    #[test]
    fn number_keys_are_page_relative() {
        let mut s = state(12);
        s.page_down(5); // cursor 5 — second page
                        // "2" on the second page must mean the 7th candidate, not the 2nd.
        assert_eq!(s.take_at_page(1, 5).as_deref(), Some("c6"));
    }

    #[test]
    fn selection_clears_the_list() {
        let mut s = state(4);
        assert_eq!(s.take_at(2).as_deref(), Some("c2"));
        assert!(s.is_empty(), "a chosen list must not stay live");
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn out_of_range_selection_leaves_the_list_alone() {
        let mut s = state(3);
        s.move_next();
        assert_eq!(s.take_at(9), None);
        assert_eq!(s.len(), 3, "a stray key must not dismiss the list");
        assert_eq!(s.cursor(), 1);
        // Same for a page-relative index that runs off the end of a short page.
        assert_eq!(s.take_at_page(4, 5), None);
        assert_eq!(s.len(), 3);
    }
}
