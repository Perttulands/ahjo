//! The one theme picker, in every app: the theme drawn now, as a control
//! ([`button`]), which opens a list of every theme in a popup beside it
//! ([`open`]), the app's own ravens first, then the well-known ones, each
//! with a swatch of its ground, its ink, its accent and its state colours.
//! Up and Down try a theme on the whole app, Enter keeps it, Escape (or a
//! press outside) closes the popup back in the one there was; a click on a
//! row keeps that one. An app that can follow Swarm's theme (Desk, Brain)
//! offers that first.

use crate::frame::AppFrame;
use crate::popup::{Popup, PopupId, Spot};
use crate::theme::{Direction, FAMILY, Theme, size, theme};
use crate::widgets::kit;
use gpui::{
    App, AppContext, Context, Div, Entity, FocusHandle, IntoElement, KeyDownEvent, Render,
    SharedString, Stateful, WeakEntity, Window, div, prelude::*, px,
};
use std::rc::Rc;

/// The list's width, and the swatch's.
const WIDTH: f32 = 300.;
const SWATCH: f32 = 76.;
/// A state's dot in a swatch, and the accent's bar.
const DOT: f32 = 6.;
const BAR: f32 = 12.;
/// What a row that follows Swarm's theme says.
const AS_SWARM: &str = "As in Ahjo Swarm";

/// A theme to choose: a direction, or none, following Ahjo Swarm's.
pub type Choice = Option<Direction>;

type Keep = Rc<dyn Fn(Choice, &mut Window, &mut App)>;

/// Where the picker is: the theme there was when it opened and the one
/// tried now, among the rows it offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Picker {
    pub was: Choice,
    pub at: Choice,
    rows: Vec<Choice>,
}

/// What a key in the open picker comes to.
#[derive(Debug, PartialEq, Eq)]
pub enum Key {
    /// Try this one.
    Try(Choice),
    Keep(Choice),
    Nothing,
}

impl Picker {
    /// At `now`, offering every theme, after following Swarm's when
    /// `follows_swarm`.
    pub fn new(now: Choice, follows_swarm: bool) -> Self {
        let rows = follows_swarm
            .then_some(None)
            .into_iter()
            .chain(Direction::ALL.iter().map(|d| Some(*d)))
            .collect();
        Self {
            was: now,
            at: now,
            rows,
        }
    }

    /// The key `key` (gpui's name for it) in the open picker.
    pub fn key(&mut self, key: &str) -> Key {
        let n = self.rows.len() as isize;
        let step = match key {
            "up" => -1,
            "down" => 1,
            "home" => -n,
            "end" => n,
            "enter" => return Key::Keep(self.at),
            _ => return Key::Nothing,
        };
        let i = self.rows.iter().position(|d| *d == self.at).unwrap_or(0) as isize;
        let to = match key {
            "home" | "end" => (i + step).clamp(0, n - 1),
            _ => (i + step).rem_euclid(n),
        };
        self.at = self.rows[to as usize];
        Key::Try(self.at)
    }
}

/// The theme a choice draws: none is the one Swarm keeps.
pub fn drawn(choice: Choice) -> Theme {
    match choice {
        Some(d) => Theme::new(d),
        None => crate::theme::chosen().0,
    }
}

/// The open picker, the popup's content.
pub struct ThemePicker {
    picker: Picker,
    /// Every theme, made once as the picker opens, for its swatch.
    themes: Vec<Theme>,
    focus: FocusHandle,
    frame: WeakEntity<AppFrame>,
    popup: Option<PopupId>,
    keep: Keep,
}

/// Opens `picker` ([`Picker::new`]) beside `spot` on `frame`, as `owner`'s
/// popup, the keyboard in it. A theme kept is handed to `keep`, which
/// keeps it; the picker draws the app in it meanwhile and closes. Closed
/// otherwise, the app is drawn in the theme there was.
pub fn open(
    frame: &Entity<AppFrame>,
    owner: &str,
    spot: Spot,
    picker: Picker,
    keep: impl Fn(Choice, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ThemePicker> {
    let view = cx.new(|cx| ThemePicker {
        picker,
        themes: Direction::ALL.iter().map(|d| Theme::new(*d)).collect(),
        focus: cx.focus_handle(),
        frame: frame.downgrade(),
        popup: None,
        keep: Rc::new(keep),
    });
    let focus = view.read(cx).focus.clone();
    let closed = view.downgrade();
    let popup = Popup::beside(spot, view.clone())
        .focus(focus.clone())
        .on_dismiss(move |_, cx| {
            let _ = closed.update(cx, |p, cx| p.closed(cx));
        });
    let id = frame.update(cx, |f, cx| f.open_popup(owner, popup, cx));
    view.update(cx, |p, _| p.popup = Some(id));
    window.focus(&focus, cx);
    view
}

impl ThemePicker {
    /// The whole app drawn in `choice` while the picker tries it; nothing
    /// kept.
    fn try_on(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let th = drawn(choice);
        if theme(cx).direction != th.direction {
            cx.set_global(th);
            cx.refresh_windows();
        }
        cx.notify();
    }

    /// Keeps `choice` and closes the picker.
    fn keep(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        self.picker.was = choice;
        self.picker.at = choice;
        (self.keep)(choice, window, cx);
        if let Some(id) = self.popup.take() {
            let _ = self.frame.update(cx, |f, cx| f.close_popup(id, cx));
        }
    }

    /// Closes the picker, the app drawn in the theme there was, as its
    /// page goes from view.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.popup.take() {
            let _ = self.frame.update(cx, |f, cx| f.close_popup(id, cx));
        }
    }

    /// The popup closed (Escape, a press outside, or kept): the app is
    /// drawn in the theme there was, or the one kept.
    fn closed(&mut self, cx: &mut Context<Self>) {
        self.popup = None;
        self.try_on(self.picker.was, cx);
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match self.picker.key(&e.keystroke.key) {
            Key::Try(d) => self.try_on(d, cx),
            Key::Keep(d) => self.keep(d, window, cx),
            Key::Nothing => return,
        }
        cx.stop_propagation();
    }
}

impl Render for ThemePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let mut rows = div().flex().flex_col().py(px(4.));
        let mut group = None;
        for choice in self.picker.rows.clone() {
            let label = match choice {
                None => "Ahjo Swarm's",
                Some(d) if d.own() => "Ravens",
                Some(_) => "Well-known",
            };
            if group != Some(label) {
                rows = rows.child(
                    div()
                        .px(px(size::ROW_PAD))
                        .pt(px(if group.is_none() { 4. } else { 10. }))
                        .pb(px(4.))
                        .child(kit::group_label(&t, label)),
                );
                group = Some(label);
            }
            let th = match choice {
                Some(d) => self.themes.iter().find(|th| th.direction == d).cloned(),
                None => Some(drawn(None)),
            }
            .unwrap_or_default();
            let at = choice == self.picker.at;
            let kept = choice == self.picker.was;
            let key = choice.map_or("swarm", |d| d.key());
            let selector = format!("theme-{key}");
            let raised = t.raised;
            let name = match choice {
                Some(d) => d.label().to_string(),
                None => AS_SWARM.to_string(),
            };
            rows = rows.child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .mx(px(size::ROW_INSET))
                    .px(px(size::ROW_PAD - size::ROW_INSET))
                    .h(px(size::ROW + 2.))
                    .flex()
                    .items_center()
                    .gap(px(size::ROW_PAD))
                    .rounded(px(size::RADIUS))
                    .cursor_pointer()
                    .text_color(if at { t.text } else { t.text2 })
                    .when(at, |r| r.bg(t.raised))
                    .hover(move |r| r.bg(raised))
                    .child(swatch(&th))
                    .child(div().flex_1().child(name))
                    .when(kept, |r| r.child(div().text_color(t.accent).child("✓")))
                    .on_click(cx.listener(move |p, _, window, cx| p.keep(choice, window, cx))),
            );
        }
        kit::panel(&t)
            .id("theme-picker")
            .debug_selector(|| "theme-picker".into())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(px(WIDTH))
            .child(rows)
            // How the keys work, said where they work: under the list, not
            // over it.
            .child(
                div()
                    .px(px(size::ROW_PAD))
                    .py(px(6.))
                    .border_t_1()
                    .border_color(t.divider)
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child("↑ ↓ try  ·  Enter keeps  ·  Esc goes back"),
            )
    }
}

/// A theme's swatch: its ground with its ink, its accent and its four
/// states on it, in its own divider.
pub fn swatch(th: &Theme) -> Div {
    let dot = |c| div().size(px(DOT)).rounded_full().bg(c);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .w(px(SWATCH))
        .h(px(20.))
        .px(px(5.))
        .rounded(px(size::RADIUS))
        .bg(th.bg)
        .border_1()
        .border_color(th.divider)
        .font_family(FAMILY)
        .text_size(px(size::SMALL))
        .text_color(th.text)
        .child("Aa")
        .child(div().w(px(BAR)).h(px(4.)).rounded_full().bg(th.accent))
        .child(dot(th.running))
        .child(dot(th.done))
        .child(dot(th.needs))
        .child(dot(th.error))
}

/// The closed picker: the theme drawn now, as a control. Its press is
/// reported, so the picker opens beside it ([`Spot::here`]).
pub fn button(t: &Theme, id: &str) -> Stateful<Div> {
    let selector = id.to_string();
    let accent = t.accent;
    div()
        .id(SharedString::from(id.to_string()))
        .debug_selector(move || selector)
        .flex()
        .items_center()
        .gap(px(size::HEAD_GAP))
        .h(px(size::CONTROL + 4.))
        .pl(px(4.))
        .pr(px(10.))
        .rounded(px(size::RADIUS))
        .border_1()
        .border_color(t.divider)
        .cursor_pointer()
        .text_color(t.text)
        .hover(move |s| s.border_color(accent))
        .relative()
        .child(crate::pointer::watch())
        .child(swatch(t))
        .child(t.direction.label())
        .child(div().text_color(t.dim).child("▾"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_try_and_keep() {
        let mut p = Picker::new(Some(Direction::Night), false);
        assert_eq!(p.key("down"), Key::Try(Some(Direction::Saga)));
        assert_eq!(p.key("up"), Key::Try(Some(Direction::Night)));
        // Round at the ends, as a list one steps through.
        assert_eq!(p.key("up"), Key::Try(Some(Direction::GithubLight)));
        assert_eq!(p.key("home"), Key::Try(Some(Direction::Night)));
        assert_eq!(p.key("end"), Key::Try(Some(Direction::GithubLight)));
        assert_eq!(p.key("enter"), Key::Keep(Some(Direction::GithubLight)));
        assert_eq!(p.key("x"), Key::Nothing);
    }

    #[test]
    fn an_app_that_follows_swarm_offers_that_first() {
        let mut p = Picker::new(None, true);
        assert_eq!(p.key("down"), Key::Try(Some(Direction::ALL[0])));
        assert_eq!(p.key("home"), Key::Try(None));
        assert_eq!(p.key("enter"), Key::Keep(None));
    }
}
