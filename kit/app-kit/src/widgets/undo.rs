//! The undo ring a text widget keeps: what the text was before each step,
//! with its selection, and the steps undone since the last edit. Keys
//! typed in a run make one step until a pause, a move of the caret, or a
//! new word; anything larger (a paste, a cut, a word deleted, a block
//! indented) is a step of its own.

use std::ops::Range;
use std::time::{Duration, Instant};

/// Keys typed within this long of the last one undo together.
pub const UNDO_PAUSE: Duration = Duration::from_millis(700);
/// The oldest steps fall off past this many.
pub const UNDO_DEPTH: usize = 200;

/// A text as it stood, and where its selection was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextState {
    pub text: String,
    pub selected: Range<usize>,
    pub reversed: bool,
}

/// What an edit is, for whether it joins the step before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEdit {
    /// One key's character typed, leaving the caret at `caret`;
    /// `starts_word` when it begins a word, which begins a step.
    Type { caret: usize, starts_word: bool },
    /// One key's backspace or delete, leaving the caret at `caret`.
    Erase { caret: usize },
    /// Anything else: always a step of its own.
    Whole,
}

#[derive(Default)]
pub struct TextHistory {
    undo: Vec<TextState>,
    redo: Vec<TextState>,
    /// When the last key edit was made and where it left the caret, while
    /// another key edit there would join its step.
    open: Option<(Instant, usize)>,
}

impl TextHistory {
    /// Notes `before`, the state an edit is about to change, unless the
    /// edit joins the step still open.
    pub fn record(&mut self, before: TextState, edit: TextEdit, now: Instant) {
        let (caret, starts_word) = match edit {
            TextEdit::Type { caret, starts_word } => (Some(caret), starts_word),
            TextEdit::Erase { caret } => (Some(caret), false),
            TextEdit::Whole => (None, true),
        };
        let joins = !starts_word
            && self.open.is_some_and(|(at, left)| {
                now.saturating_duration_since(at) <= UNDO_PAUSE && before.selected == (left..left)
            });
        if !joins {
            self.undo.push(before);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.open = caret.map(|c| (now, c));
    }

    /// Ends the open step, so the next key edit starts one.
    pub fn seal(&mut self) {
        self.open = None;
    }

    /// Forgets every step: the text was replaced, not edited.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The state before the last step, given the one now; `None` with
    /// nothing to undo.
    pub fn undo(&mut self, now: TextState) -> Option<TextState> {
        Self::step(&mut self.undo, &mut self.redo, now, &mut self.open)
    }

    /// The state the last undo left, given the one now; `None` with
    /// nothing to redo.
    pub fn redo(&mut self, now: TextState) -> Option<TextState> {
        Self::step(&mut self.redo, &mut self.undo, now, &mut self.open)
    }

    fn step(
        from: &mut Vec<TextState>,
        to: &mut Vec<TextState>,
        now: TextState,
        open: &mut Option<(Instant, usize)>,
    ) -> Option<TextState> {
        let back = from.pop()?;
        to.push(now);
        *open = None;
        Some(back)
    }
}

/// Whether `typed` at `at` in `text` begins a word: a word character after
/// anything that is not one.
pub fn starts_word(text: &str, at: usize, typed: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    typed.chars().next().is_some_and(word) && !text[..at].chars().next_back().is_some_and(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Types `keys` into `text` one by one, `gap` apart, as a field does.
    fn typed(h: &mut TextHistory, text: &mut String, keys: &str, start: Instant, gap: Duration) {
        for (n, c) in keys.chars().enumerate() {
            let at = text.len();
            let before = TextState {
                text: text.clone(),
                selected: at..at,
                reversed: false,
            };
            let s = c.to_string();
            let edit = TextEdit::Type {
                caret: at + s.len(),
                starts_word: starts_word(text, at, &s),
            };
            h.record(before, edit, start + gap * n as u32);
            text.push(c);
        }
    }

    fn state(text: &str) -> TextState {
        TextState {
            text: text.into(),
            selected: text.len()..text.len(),
            reversed: false,
        }
    }

    #[test]
    fn typing_undoes_a_word_at_a_time() {
        let mut h = TextHistory::default();
        let mut text = String::new();
        typed(
            &mut h,
            &mut text,
            "hello, brave world",
            Instant::now(),
            Duration::ZERO,
        );
        let mut seen = vec![];
        let mut now = state(&text);
        while let Some(back) = h.undo(now.clone()) {
            seen.push(back.text.clone());
            now = back;
        }
        assert_eq!(seen, ["hello, brave ", "hello, ", ""]);
        let mut again = vec![];
        while let Some(on) = h.redo(now.clone()) {
            again.push(on.text.clone());
            now = on;
        }
        assert_eq!(again, ["hello, ", "hello, brave ", "hello, brave world"]);
    }

    #[test]
    fn a_pause_or_a_move_of_the_caret_starts_a_step() {
        let start = Instant::now();
        let mut h = TextHistory::default();
        let mut text = String::new();
        typed(&mut h, &mut text, "abcd", start, UNDO_PAUSE * 2);
        assert_eq!(h.undo.len(), 4, "each key after a pause is a step");

        let mut h = TextHistory::default();
        h.record(
            state(""),
            TextEdit::Type {
                caret: 1,
                starts_word: true,
            },
            start,
        );
        // The caret went back to the start before the next key.
        let moved = TextState {
            text: "a".into(),
            selected: 0..0,
            reversed: false,
        };
        h.record(
            moved,
            TextEdit::Type {
                caret: 1,
                starts_word: false,
            },
            start,
        );
        assert_eq!(h.undo.len(), 2);
    }

    #[test]
    fn keys_erased_join_the_word_but_a_whole_edit_stands_alone() {
        let start = Instant::now();
        let mut h = TextHistory::default();
        let mut text = String::new();
        typed(&mut h, &mut text, "helo", start, Duration::ZERO);
        h.record(state("helo"), TextEdit::Erase { caret: 3 }, start);
        text.pop();
        typed(&mut h, &mut text, "lo", start, Duration::ZERO);
        assert_eq!(h.undo.len(), 1, "a typo fixed is still the one word");
        h.record(state("hello"), TextEdit::Whole, start);
        h.record(state("hello there"), TextEdit::Whole, start);
        let typing = TextEdit::Type {
            caret: 12,
            starts_word: false,
        };
        h.record(state("hello there!"), typing, start);
        assert_eq!(h.undo.len(), 4, "a whole edit never joins, nor is joined");
    }

    #[test]
    fn an_edit_after_undo_drops_the_redo_and_the_ring_keeps_its_depth() {
        let start = Instant::now();
        let mut h = TextHistory::default();
        for n in 0..UNDO_DEPTH + 5 {
            h.record(state(&n.to_string()), TextEdit::Whole, start);
        }
        assert_eq!(h.undo.len(), UNDO_DEPTH);
        assert_eq!(h.undo[0].text, "5", "the oldest fell off");
        let back = h.undo(state("now")).unwrap();
        assert!(h.redo(back.clone()).is_some());
        let back = h.undo(state("now")).unwrap();
        h.record(back, TextEdit::Whole, start);
        assert_eq!(h.redo(state("x")), None);
    }
}
