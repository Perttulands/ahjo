//! The one way to find a thing in a list (a session to send to, a folder,
//! a window's session): a search field over the list, keyboard first.
//! Typing narrows the list as it is typed, forgivingly ([`fuzzy`]), and
//! marks the letters that found each row; chips above it narrow by kind.
//! Up and Down move the lit row, Enter takes it, Tab marks it and moves on
//! where several can be taken at once, as fzf does; a click takes a row, a
//! click on its box marks it. The owner keeps what is taken and marked and
//! hears it as [`FinderEvent`]s; the finder keeps the words, the chip and
//! the lit row.

use crate::text::fuzzy;
use crate::theme::{Theme, size, theme};
use crate::widgets::field::{FieldEvent, TextField};
use crate::widgets::kit::{self, Kind};
use crate::widgets::pane;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FontWeight, HighlightStyle, InteractiveElement,
    IntoElement, ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, StyledText, Subscription, Window, div, prelude::*, px,
};

/// One thing the list offers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    /// What the owner knows it by.
    pub id: SharedString,
    /// Its name, as the row shows it and the search reads it first.
    pub label: String,
    /// Other words the search finds it by (its program, folder, device),
    /// each a field of its own; drawn only where the typed words found it
    /// there, its letters marked.
    pub also: Vec<String>,
    /// A few dim words at the row's end.
    pub detail: Option<String>,
    pub lead: Lead,
    /// The chips it is listed under.
    pub kinds: Vec<&'static str>,
}

/// What stands before a row's name.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Lead {
    #[default]
    None,
    /// The square of the user it runs as.
    User(String),
    /// Something waits there for the operator.
    Waiting,
}

/// What the operator did in the list.
#[derive(Clone, Debug, PartialEq)]
pub enum FinderEvent {
    /// Took this one: Enter on it, or a click.
    Take(SharedString),
    /// Marked or unmarked this one: Tab on it, or a click on its box.
    Mark(SharedString),
    /// Marked every one shown now.
    MarkAll(Vec<SharedString>),
    /// Enter or Tab with no row lit: the operator is done here.
    Leave,
}

pub struct Finder {
    pub query: Entity<TextField>,
    /// The field's words, as last typed.
    words: String,
    choices: Vec<Choice>,
    /// A row above the others, whatever is typed ("New agent…").
    first: Option<Choice>,
    /// The chips: the kind each narrows to, and its words.
    chips: Vec<(&'static str, &'static str)>,
    chip: Option<&'static str>,
    /// The rows taken or marked, drawn checked.
    chosen: Vec<SharedString>,
    /// Several can be marked.
    several: bool,
    /// The lit row, among the first row and the rows shown.
    cursor: usize,
    /// A row is lit only once the operator typed or moved: Enter on an
    /// untouched list takes nothing.
    moved: bool,
    /// How many rows stand before the list scrolls.
    rows: usize,
    scroll: ScrollHandle,
    /// The rows' debug selectors start so (`<name>-<id>`).
    name: &'static str,
    _query: Subscription,
}

impl EventEmitter<FinderEvent> for Finder {}

impl Finder {
    pub fn new(name: &'static str, placeholder: &str, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextField::new(cx, placeholder));
        let sub = cx.subscribe(&query, |f, _, ev: &FieldEvent, cx| f.field(ev, cx));
        Self {
            query,
            words: String::new(),
            choices: vec![],
            first: None,
            chips: vec![],
            chip: None,
            chosen: vec![],
            several: false,
            cursor: 0,
            moved: false,
            rows: 8,
            scroll: ScrollHandle::new(),
            name,
            _query: sub,
        }
    }

    /// Several rows can be marked (Tab, or a row's box).
    pub fn several(mut self) -> Self {
        self.several = true;
        self
    }

    /// At most `rows` rows stand before the list scrolls.
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows;
        self
    }

    /// What the list offers now, what is chosen of it, and the chips
    /// that narrow it. Called as the owner draws, so it never notifies.
    pub fn offer(
        &mut self,
        first: Option<Choice>,
        choices: Vec<Choice>,
        chips: Vec<(&'static str, &'static str)>,
        chosen: Vec<SharedString>,
    ) {
        if self
            .chip
            .is_some_and(|c| !chips.iter().any(|(k, _)| *k == c))
        {
            self.chip = None;
        }
        self.first = first;
        self.choices = choices;
        self.chips = chips;
        self.chosen = chosen;
        self.cursor = self.cursor.min(self.count().saturating_sub(1));
    }

    /// Starts again: no words, no chip, nothing lit.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.query.update(cx, |f, cx| f.set("", false, cx));
        self.words.clear();
        self.chip = None;
        self.cursor = self.top();
        self.moved = false;
        cx.notify();
    }

    /// The rows drawn, the first row with them.
    fn count(&self) -> usize {
        self.shown().len() + self.first.is_some() as usize
    }

    /// Where the cursor rests: on the best match, below the first row.
    fn top(&self) -> usize {
        usize::from(self.first.is_some() && !self.shown().is_empty())
    }

    /// The narrowing by chip changed: the best match is lit again.
    fn narrowed(&mut self) {
        self.cursor = self.top();
    }

    /// The rows shown, best first, each with the letters that matched in
    /// each of its fields: its name's first, then those of `also`.
    pub fn shown(&self) -> Vec<(&Choice, Vec<Vec<usize>>)> {
        let words = &self.words;
        let kept: Vec<&Choice> = self
            .choices
            .iter()
            .filter(|c| self.chip.is_none_or(|k| c.kinds.contains(&k)))
            .collect();
        let mut found: Vec<(usize, fuzzy::FieldsHit)> = kept
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let fields: Vec<&str> = std::iter::once(c.label.as_str())
                    .chain(c.also.iter().map(String::as_str))
                    .collect();
                fuzzy::find_each_field(words, &fields).map(|h| (i, h))
            })
            .collect();
        found.sort_by_key(|(i, h)| (std::cmp::Reverse(h.score), *i));
        found.into_iter().map(|(i, h)| (kept[i], h.at)).collect()
    }

    /// The id of the lit row.
    fn lit(&self) -> Option<SharedString> {
        if !self.moved && self.words.trim().is_empty() {
            return None;
        }
        let first = self.first.iter().map(|c| c.id.clone());
        let rest = self.shown().into_iter().map(|(c, _)| c.id.clone());
        first.chain(rest).nth(self.cursor)
    }

    fn field(&mut self, ev: &FieldEvent, cx: &mut Context<Self>) {
        if *ev == FieldEvent::Changed {
            self.words = self.query.read(cx).text.clone();
        }
        let n = self.count();
        let first = self.first.as_ref().map(|c| c.id.clone());
        match ev {
            FieldEvent::Changed => self.cursor = self.top(),
            // The first move lights the row the cursor rests on.
            FieldEvent::Up if self.lit().is_some() => self.cursor = self.cursor.saturating_sub(1),
            FieldEvent::Down if self.lit().is_some() => {
                self.cursor = (self.cursor + 1).min(n.saturating_sub(1))
            }
            FieldEvent::Up | FieldEvent::Down => self.moved = true,
            FieldEvent::Submit => match self.lit() {
                Some(id) => cx.emit(FinderEvent::Take(id)),
                None => cx.emit(FinderEvent::Leave),
            },
            FieldEvent::Tab => match self.lit().filter(|id| Some(id) != first.as_ref()) {
                Some(id) if self.several => {
                    cx.emit(FinderEvent::Mark(id));
                    self.cursor = (self.cursor + 1).min(n.saturating_sub(1));
                }
                Some(id) => cx.emit(FinderEvent::Take(id)),
                None => cx.emit(FinderEvent::Leave),
            },
            _ => return,
        }
        self.scroll.scroll_to_item(self.cursor);
        cx.notify();
    }

    fn row(
        &self,
        i: usize,
        c: &Choice,
        at: &[Vec<usize>],
        first: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chosen = self.chosen.contains(&c.id);
        let lit = i == self.cursor && (self.moved || !self.words.trim().is_empty());
        let selector = if first {
            format!("{}-first", self.name)
        } else {
            format!("{}-{}", self.name, c.id)
        };
        let id = c.id.clone();
        let take = cx.listener(move |_, _, _, cx| cx.emit(FinderEvent::Take(id.clone())));
        let id = c.id.clone();
        let mark = cx.listener(move |_, _, _, cx: &mut Context<Self>| {
            cx.stop_propagation();
            cx.emit(FinderEvent::Mark(id.clone()));
        });
        div()
            .id((self.name, i))
            .debug_selector(move || selector.clone())
            // Rows keep their height; the list scrolls instead.
            .flex_none()
            .h(px(size::ROW))
            .pl(px(6.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(size::RADIUS))
            .cursor_pointer()
            .when(lit, |d| d.bg(t.raised))
            .when(chosen, |d| d.bg(t.accent_light()))
            .hover(|d| d.bg(t.accent_soft()))
            .on_click(take)
            .when(self.several && !first, |d| {
                d.child(
                    div()
                        .id((self.name, i + 100_000))
                        .debug_selector({
                            let s = format!("{}-mark-{}", self.name, c.id);
                            move || s.clone()
                        })
                        .flex_none()
                        .size(px(14.))
                        .rounded(px(size::RADIUS_SMALL))
                        .border_1()
                        .border_color(if chosen { t.accent } else { t.text2 })
                        .when(chosen, |d| d.bg(t.accent))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(size::TINY))
                        .text_color(t.on_accent())
                        .when(chosen, |d| d.child("✓"))
                        .on_click(mark),
                )
            })
            .map(|d| match &c.lead {
                Lead::None => d,
                Lead::User(u) => d.child(kit::user_square(t, u)),
                Lead::Waiting => d.child(kit::waiting_dot(t)),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when(first, |d| d.text_color(t.text2))
                    .when(chosen, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .child(marked_letters(&c.label, at.first().map_or(&[], |a| a), t)),
            )
            .children(found_by(c, at, t))
            .into_any_element()
    }
}

/// What a row says at its end, each with its letters marked: the other
/// fields the typed words found it by, then its detail, marked too where
/// it is one of them.
fn row_end<'a>(c: &'a Choice, at: &'a [Vec<usize>]) -> Vec<(&'a str, &'a [usize])> {
    let mut detail: Option<(&str, &[usize])> = c.detail.as_deref().map(|d| (d, &[][..]));
    let mut out = vec![];
    for (field, marks) in c.also.iter().zip(at.iter().skip(1)) {
        if marks.is_empty() {
            continue;
        }
        match detail {
            Some((words, _)) if words == field => detail = Some((words, marks)),
            _ => out.push((field.as_str(), marks.as_slice())),
        }
    }
    out.extend(detail);
    out
}

fn found_by(c: &Choice, at: &[Vec<usize>], t: &Theme) -> Vec<AnyElement> {
    row_end(c, at)
        .into_iter()
        .map(|(words, marks)| {
            div()
                .flex_none()
                .max_w(px(size::FINDER_FOUND_BY))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(size::SMALL))
                .text_color(t.dim)
                .child(marked_letters(words, marks, t))
                .into_any_element()
        })
        .collect()
}

/// `label` with the letters at `at` (character indices) in the accent.
pub fn marked_letters(label: &str, at: &[usize], t: &Theme) -> StyledText {
    let mut ranges = vec![];
    for (i, (b, ch)) in label.char_indices().enumerate() {
        if at.contains(&i) {
            ranges.push((
                b..b + ch.len_utf8(),
                HighlightStyle {
                    color: Some(t.accent),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            ));
        }
    }
    StyledText::new(label.to_string()).with_highlights(ranges)
}

impl Render for Finder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let shown: Vec<(Choice, Vec<Vec<usize>>)> = self
            .shown()
            .into_iter()
            .map(|(c, at)| (c.clone(), at))
            .collect();
        let words = self.words.clone();
        let mut list = pane::scroll((self.name, usize::MAX))
            .debug_selector({
                let s = format!("{}-list", self.name);
                move || s.clone()
            })
            .track_scroll(&self.scroll)
            .flex_none()
            // Whole rows, so the last one shown is never cut through.
            .max_h(px(self.rows as f32 * size::ROW));
        let mut i = 0;
        if let Some(first) = self.first.clone() {
            list = list.child(self.row(i, &first, &[], true, &t, cx));
            i += 1;
        }
        for (c, at) in &shown {
            list = list.child(self.row(i, c, at, false, &t, cx));
            i += 1;
        }
        // The chips, each with how many it holds; only when one of them
        // tells some rows from the others.
        let counts: Vec<usize> = self
            .chips
            .iter()
            .map(|(k, _)| self.choices.iter().filter(|c| c.kinds.contains(k)).count())
            .collect();
        let chips = counts
            .iter()
            .any(|n| *n > 0 && *n < self.choices.len())
            .then(|| {
                let mut row = div().flex().flex_wrap().items_center().gap(px(4.));
                let chip = |id: &'static str, words: String, on: bool, t: &Theme| {
                    div()
                        .id(SharedString::from(format!("{}-chip-{id}", self.name)))
                        .debug_selector({
                            let s = format!("{}-chip-{id}", self.name);
                            move || s.clone()
                        })
                        .h(px(22.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .border_1()
                        .text_size(px(size::SMALL))
                        .cursor_pointer()
                        .when(on, |d| {
                            d.bg(t.accent_light())
                                .border_color(t.accent)
                                .text_color(t.text)
                        })
                        .when(!on, |d| {
                            d.border_color(t.divider)
                                .text_color(t.text2)
                                .hover(|d| d.text_color(t.text))
                        })
                        .child(words)
                };
                row = row.child(chip("all", "All".into(), self.chip.is_none(), &t).on_click(
                    cx.listener(|f, _, _, cx| {
                        f.chip = None;
                        f.narrowed();
                        cx.notify();
                    }),
                ));
                for ((kind, words), n) in self.chips.clone().into_iter().zip(counts) {
                    if n == 0 {
                        continue;
                    }
                    row = row.child(
                        chip(kind, format!("{words} {n}"), self.chip == Some(kind), &t).on_click(
                            cx.listener(move |f, _, _, cx| {
                                f.chip = if f.chip == Some(kind) {
                                    None
                                } else {
                                    Some(kind)
                                };
                                f.narrowed();
                                cx.notify();
                            }),
                        ),
                    );
                }
                row
            });
        // Narrowed, every row shown can be marked at once.
        let narrowed = !words.trim().is_empty() || self.chip.is_some();
        let all: Vec<SharedString> = shown.iter().map(|(c, _)| c.id.clone()).collect();
        let mark_all = (self.several && narrowed && all.len() > 1).then(|| {
            kit::button(
                &t,
                (self.name, usize::MAX - 1),
                format!("Mark all {}", all.len()),
                Kind::Quiet,
            )
            .debug_selector({
                let s = format!("{}-mark-all", self.name);
                move || s.clone()
            })
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(FinderEvent::MarkAll(all.clone()));
            }))
        });
        let nothing = (shown.is_empty() && !words.trim().is_empty()).then(|| {
            kit::note(
                &t,
                format!("Nothing here matches “{}”", words.trim()),
                Some(t.dim),
            )
            .px(px(8.))
            .py(px(4.))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(self.query.clone())
            .when(chips.is_some() || mark_all.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().flex_1().children(chips))
                        .children(mark_all),
                )
            })
            .child(list)
            .children(nothing)
    }
}

/// The marks after `ev`, for an owner that keeps them as the finder's ids.
pub fn marks_after(ev: &FinderEvent, marked: &mut Vec<SharedString>) {
    match ev {
        FinderEvent::Take(_) | FinderEvent::Leave => {}
        FinderEvent::Mark(id) => {
            if let Some(i) = marked.iter().position(|m| m == id) {
                marked.remove(i);
            } else {
                marked.push(id.clone());
            }
        }
        FinderEvent::MarkAll(ids) => {
            for id in ids {
                if !marked.contains(id) {
                    marked.push(id.clone());
                }
            }
        }
    }
}

/// The finder's field has the keyboard.
pub fn focus(finder: &Entity<Finder>, window: &mut Window, cx: &mut App) {
    let q = finder.read(cx).query.clone();
    crate::widgets::field::focus(&q, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_found_by_another_field_shows_it_marked() {
        let c = Choice {
            label: "api-tests".into(),
            also: vec!["claude".into(), "harbour".into(), "box".into()],
            detail: Some("claude".into()),
            ..Default::default()
        };
        let none: Vec<usize> = vec![];
        // Found by its name: the detail alone, unmarked.
        let at = vec![vec![0], none.clone(), none.clone(), none.clone()];
        assert_eq!(row_end(&c, &at), vec![("claude", &[][..])]);
        // Found by its folder: the folder, marked, before the detail.
        let at = vec![none.clone(), none.clone(), vec![0, 2], none.clone()];
        assert_eq!(
            row_end(&c, &at),
            vec![("harbour", &[0, 2][..]), ("claude", &[][..])]
        );
        // Found by its program, which the detail says: the detail marked.
        let at = vec![none.clone(), vec![0, 1], none.clone(), none];
        assert_eq!(row_end(&c, &at), vec![("claude", &[0, 1][..])]);
    }
}
