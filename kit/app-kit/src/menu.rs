//! A menu's rows. Each row carries what it does, so the shell that draws
//! the menu knows nothing of what its rows are about.

use crate::chords::Run;
use gpui::{App, Window};
use std::rc::Rc;

#[derive(Clone)]
pub struct MenuRow {
    pub label: String,
    pub chord: Option<String>,
    pub run: Option<Run>,
    /// Destroys something: the first click arms it, the second runs it.
    pub danger: bool,
    pub checked: bool,
    /// Offered and refused; its hint says why.
    pub refused: bool,
    /// A line under the label: what the row does, or why it is refused.
    pub hint: Option<String>,
    /// The rows it opens in its place.
    pub sub: Vec<MenuRow>,
}

impl MenuRow {
    pub fn new(label: impl Into<String>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        let mut row = Self::note(label);
        row.run = Some(Rc::new(run));
        row
    }
    /// A row that opens `rows` in the menu's place; `state` reads beside it.
    pub fn sub(label: impl Into<String>, state: Option<String>, rows: Vec<MenuRow>) -> Self {
        let mut row = Self::note(label);
        row.chord = Some(match state {
            Some(s) => format!("{s} ›"),
            None => "›".into(),
        });
        row.sub = rows;
        row
    }
    /// A row that does nothing now, saying why.
    pub fn refused(mut self, why: impl Into<String>) -> Self {
        self.refused = true;
        self.hint = Some(why.into());
        self
    }
    pub fn hint(mut self, text: impl Into<String>) -> Self {
        self.hint = Some(text.into());
        self
    }
    /// A row that only says something.
    pub fn note(label: impl Into<String>) -> Self {
        let mut row = Self::rule();
        row.label = label.into();
        row
    }
    pub fn chord(mut self, c: &str) -> Self {
        self.chord = Some(c.into());
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }
    pub fn rule() -> Self {
        Self {
            label: String::new(),
            chord: None,
            run: None,
            danger: false,
            checked: false,
            refused: false,
            hint: None,
            sub: vec![],
        }
    }
    pub fn is_rule(&self) -> bool {
        self.run.is_none() && self.sub.is_empty() && self.label.is_empty()
    }
    /// A note, or a row refused now: it reads dim and takes no click.
    pub fn inert(&self) -> bool {
        self.refused || (self.run.is_none() && self.sub.is_empty())
    }
}

/// An open menu's rows, the danger row armed and the keyboard's row; a
/// popup ([`crate::popup::Popup::menu`]) says where it stands.
#[derive(Clone)]
pub struct Menu {
    pub rows: Vec<MenuRow>,
    pub armed: Option<usize>,
    /// The row the keyboard is on: Up and Down move it, Enter or Right
    /// picks it. None until a key moves it, or from a key that opened it.
    pub cursor: Option<usize>,
}

/// What a click on row `index` of a menu comes to.
pub enum Pick {
    /// Nothing: a note, a rule or a refused row.
    Nothing,
    /// The menu now shows the row's sub-rows, or its danger row is armed.
    Stays,
    /// The menu closes and the row runs.
    Run(Run),
}

impl Menu {
    pub fn new(rows: Vec<MenuRow>) -> Self {
        Self {
            rows,
            armed: None,
            cursor: None,
        }
    }

    /// Opened from a key: its first row it can pick is the keyboard's, so
    /// Enter runs it and the arrows move on.
    pub fn keyed(mut self) -> Self {
        self.step(1);
        self
    }

    /// Row `index`, a danger row, armed already: one click or Enter runs
    /// it, as when a key asked for the danger.
    pub fn armed(mut self, index: usize) -> Self {
        self.armed = Some(index);
        self.cursor = Some(index);
        self
    }

    /// Moves the keyboard's row `by` rows over the rows it can pick,
    /// round the ends; from none, Down takes the first and Up the last.
    pub fn step(&mut self, by: isize) {
        let live: Vec<usize> = (0..self.rows.len())
            .filter(|i| !self.rows[*i].inert())
            .collect();
        if live.is_empty() {
            self.cursor = None;
            return;
        }
        let n = live.len() as isize;
        let at = match self.cursor.and_then(|c| live.iter().position(|i| *i == c)) {
            Some(at) => (at as isize + by).rem_euclid(n),
            None if by < 0 => n - 1,
            None => 0,
        };
        self.cursor = Some(live[at as usize]);
    }

    pub fn pick(&mut self, index: usize) -> Pick {
        let Some(row) = self.rows.get(index).cloned() else {
            return Pick::Nothing;
        };
        if row.inert() {
            return Pick::Nothing;
        }
        if !row.sub.is_empty() {
            self.rows = row.sub;
            self.armed = None;
            // Opened from the keyboard, the keyboard goes on in it.
            if self.cursor.take().is_some() {
                self.step(1);
            }
            return Pick::Stays;
        }
        if row.danger && self.armed != Some(index) {
            self.armed = Some(index);
            return Pick::Stays;
        }
        match row.run {
            Some(run) => Pick::Run(run),
            None => Pick::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_danger_row_runs_on_the_second_click_and_a_sub_row_opens_in_place() {
        let mut m = Menu::new(vec![
            MenuRow::note("Says"),
            MenuRow::new("Kill", |_, _| {}).danger(),
            MenuRow::sub(
                "Windows",
                Some("2".into()),
                vec![MenuRow::new("1", |_, _| {})],
            ),
            MenuRow::new("Claim", |_, _| {}).refused("Nothing to claim."),
        ]);
        assert!(matches!(m.pick(0), Pick::Nothing));
        assert!(matches!(m.pick(3), Pick::Nothing), "refused");
        assert!(matches!(m.pick(1), Pick::Stays));
        assert_eq!(m.armed, Some(1));
        assert!(matches!(m.pick(1), Pick::Run(_)));
        assert!(matches!(m.pick(2), Pick::Stays));
        assert_eq!(m.rows.len(), 1);
        assert!(matches!(m.pick(0), Pick::Run(_)));
        assert!(MenuRow::rule().is_rule() && !MenuRow::note("x").is_rule());
    }

    #[test]
    fn the_keyboard_steps_over_rows_it_cannot_pick_and_goes_on_into_a_sub_menu() {
        let mut m = Menu::new(vec![
            MenuRow::note("Says"),
            MenuRow::new("Split", |_, _| {}),
            MenuRow::rule(),
            MenuRow::new("Claim", |_, _| {}).refused("Nothing to claim."),
            MenuRow::sub(
                "Windows",
                None,
                vec![MenuRow::note("Pick one"), MenuRow::new("1", |_, _| {})],
            ),
        ]);
        m.step(1);
        assert_eq!(m.cursor, Some(1));
        m.step(1);
        assert_eq!(m.cursor, Some(4), "past the rule and the refused row");
        m.step(1);
        assert_eq!(m.cursor, Some(1), "round the end");
        m.step(-1);
        assert_eq!(m.cursor, Some(4));
        assert!(matches!(m.pick(4), Pick::Stays));
        assert_eq!(m.cursor, Some(1), "on the sub-menu's first row");
        m.cursor = None;
        m.step(-1);
        assert_eq!(m.cursor, Some(1), "Up from none takes the last");
    }
}
