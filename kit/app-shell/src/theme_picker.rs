//! The theme picker on the Settings page: the theme drawn now, which opens
//! a list of every theme, the app's own ravens first, then the well-known
//! ones, each with a swatch of its ground, its ink, its accent and its
//! state colours. Up and Down try a theme on the whole app, Enter keeps
//! it, Escape (or a click outside) goes back to the one there was; a
//! click on a row keeps that one.

use crate::settings_page::SettingsPage;
use ahjo_app_kit::theme::{Direction, FAMILY, Theme, motion, size};
use ahjo_app_kit::widgets::kit;
use gpui::{
    Animation, AnimationExt, AnyElement, Div, FocusHandle, SharedString, Stateful, WeakEntity,
    anchored, deferred, div, ease_out_quint, prelude::*, px,
};
use std::time::Duration;

/// The list's width, and the swatch's.
const WIDTH: f32 = 300.;
const SWATCH: f32 = 76.;
/// A state's dot in a swatch, and the accent's bar.
const DOT: f32 = 6.;
const BAR: f32 = 12.;

/// An open picker: the theme there was when it opened, the one tried now.
pub(crate) struct Picker {
    pub was: Direction,
    pub at: Direction,
    /// Every theme, made once as the picker opens, for its swatch.
    themes: Vec<Theme>,
}

/// What a key in the open picker comes to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// Try this one.
    Try(Direction),
    Keep(Direction),
    Back(Direction),
    Nothing,
}

impl Picker {
    pub fn open(now: Direction) -> Self {
        Self {
            was: now,
            at: now,
            themes: Direction::ALL.iter().map(|d| Theme::new(*d)).collect(),
        }
    }

    /// The key `key` (gpui's name for it) in the open picker.
    pub fn key(&mut self, key: &str) -> Key {
        let step = match key {
            "up" => -1,
            "down" => 1,
            "home" => -(Direction::ALL.len() as isize),
            "end" => Direction::ALL.len() as isize,
            "enter" => return Key::Keep(self.at),
            "escape" => return Key::Back(self.was),
            _ => return Key::Nothing,
        };
        let i = Direction::ALL
            .iter()
            .position(|d| *d == self.at)
            .unwrap_or(0) as isize;
        let last = Direction::ALL.len() as isize - 1;
        let to = match key {
            "home" | "end" => (i + step).clamp(0, last),
            _ => (i + step).rem_euclid(last + 1),
        };
        self.at = Direction::ALL[to as usize];
        Key::Try(self.at)
    }
}

/// A theme's swatch: its ground with its ink, its accent and its four
/// states on it, in its own divider.
pub(crate) fn swatch(th: &Theme) -> Div {
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

/// The closed picker: the theme drawn now, as a control.
pub(crate) fn button(t: &Theme, id: &str) -> Stateful<Div> {
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
        .child(swatch(t))
        .child(t.direction.label())
        .child(div().text_color(t.dim).child("▾"))
        .tooltip(kit::tip("↑ ↓ try a theme, Enter keeps it, Esc goes back"))
}

/// The open list, over the page below its button: put it in the button's
/// place, after the button.
pub(crate) fn list(
    t: &Theme,
    picker: &Picker,
    focus: &FocusHandle,
    page: WeakEntity<SettingsPage>,
) -> AnyElement {
    let (keys, outside) = (page.clone(), page.clone());
    let mut rows = div().flex().flex_col().py(px(4.));
    let mut last_own = true;
    for (i, th) in picker.themes.iter().enumerate() {
        let d = th.direction;
        if i == 0 || d.own() != last_own {
            rows = rows.child(
                div()
                    .px(px(size::ROW_PAD))
                    .pt(px(if i == 0 { 4. } else { 10. }))
                    .pb(px(4.))
                    .child(kit::group_label(
                        t,
                        if d.own() { "Ravens" } else { "Well-known" },
                    )),
            );
        }
        last_own = d.own();
        let at = d == picker.at;
        let kept = d == picker.was;
        let pick = page.clone();
        let selector = format!("theme-{}", d.key());
        let raised = t.raised;
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
                .child(swatch(th))
                .child(div().flex_1().child(d.label()))
                .when(kept, |r| r.child(div().text_color(t.accent).child("✓")))
                .on_click(move |_, _, cx| {
                    pick.update(cx, |p, cx| p.picker_keep(d, cx)).ok();
                }),
        );
    }
    deferred(
        anchored().snap_to_window_with_margin(px(8.)).child(
            kit::panel(t)
                .id("theme-picker")
                .debug_selector(|| "theme-picker".into())
                .occlude()
                .track_focus(focus)
                .on_key_down(move |e, _, cx| {
                    keys.update(cx, |p, cx| p.picker_key(e, cx)).ok();
                })
                .on_mouse_down_out(move |_, _, cx| {
                    outside.update(cx, |p, cx| p.picker_back(cx)).ok();
                })
                .mt(px(4.))
                .w(px(WIDTH))
                .child(rows)
                .with_animation(
                    "theme-picker-in",
                    Animation::new(Duration::from_millis(motion::PANEL_IN))
                        .with_easing(ease_out_quint()),
                    |d, x| d.opacity(x),
                ),
        ),
    )
    .with_priority(20)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_try_keep_and_go_back() {
        let mut p = Picker::open(Direction::Night);
        assert_eq!(p.key("down"), Key::Try(Direction::Saga));
        assert_eq!(p.key("up"), Key::Try(Direction::Night));
        // Round at the ends, as a list one steps through.
        assert_eq!(p.key("up"), Key::Try(Direction::GithubLight));
        assert_eq!(p.key("home"), Key::Try(Direction::Night));
        assert_eq!(p.key("end"), Key::Try(Direction::GithubLight));
        assert_eq!(p.key("enter"), Key::Keep(Direction::GithubLight));
        assert_eq!(p.key("escape"), Key::Back(Direction::Night));
        assert_eq!(p.key("x"), Key::Nothing);
    }
}
