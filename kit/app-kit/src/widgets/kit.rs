//! The shared kit: every control is drawn from here, at CHROTE's measures,
//! never improvised in a view.

use crate::theme::{Theme, art, size, ty};
use gpui::{
    AnimationExt, App, Div, ElementId, FontWeight, Hsla, SharedString, Stateful, div, img,
    prelude::*, px,
};

/// A field's or a section's label: the theme's group label, quieter.
pub fn label(t: &Theme, text: impl Into<SharedString>) -> Div {
    group_label(t, text).text_color(t.dim)
}

/// A group's name: the display face, small and weighted, in the theme's
/// case, its letters spaced by the theme's tracking. It clips rather than
/// wraps.
pub fn group_label(t: &Theme, text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    let words = t.label_case(&text);
    let d = div()
        .flex()
        .flex_none()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(t.ty.display)
        .font_weight(t.ty.display_weight)
        .text_size(px(t.ty.group))
        .text_color(t.text2);
    if t.ty.tracking <= 0. {
        return d.child(words);
    }
    // Text has no letter spacing of its own: each letter stands apart.
    d.gap(px(t.ty.tracking)).children(words.chars().map(|c| {
        if c == ' ' {
            div().flex_none().w(px(t.ty.group * 0.3))
        } else {
            div().flex_none().child(c.to_string())
        }
    }))
}

/// A page's name at the head of the page: every page draws it the same,
/// in the theme's display face.
pub fn page_title(t: &Theme, text: impl Into<SharedString>) -> Div {
    let text = text.into();
    let selector = format!("page-title-{text}");
    div()
        .debug_selector(move || selector)
        .flex_none()
        .font_family(t.ty.display)
        .font_weight(t.ty.display_weight)
        .text_size(px(ty::PAGE))
        .text_color(t.text)
        .child(text)
}

/// A number worth reading at a glance, in `colour` (the text's unless it
/// is a state), with its unit small after it.
pub fn glance(
    t: &Theme,
    value: impl Into<SharedString>,
    unit: impl Into<SharedString>,
    colour: Option<Hsla>,
) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(4.))
        .child(
            div()
                .font_family(t.ty.display)
                .font_weight(t.ty.display_weight)
                .text_size(px(ty::GLANCE))
                .line_height(px(ty::GLANCE * 1.1))
                .text_color(colour.unwrap_or(t.text))
                .child(value.into()),
        )
        .child(
            div()
                .font_family(t.ty.display)
                .text_size(px(ty::TITLE))
                .text_color(t.text2)
                .child(unit.into()),
        )
}

/// How many a group holds: a small number on a quiet plate.
pub fn count(t: &Theme, n: usize) -> Div {
    div()
        .flex_none()
        .min_w(px(size::COUNT))
        .h(px(size::COUNT))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size::RADIUS))
        .bg(t.raised)
        .text_size(px(size::SMALL - 1.))
        .text_color(t.text2)
        .child(n.to_string())
}

/// The app's mark, the theme's ravens.
pub fn mark(t: &Theme) -> gpui::Img {
    img(t.art.mark.clone()).flex_none().size(px(art::MARK))
}

/// The bar at a page's head, as high as the tab bar: its title, a note,
/// and its actions to the right.
pub fn page_bar(t: &Theme) -> Div {
    div()
        .flex_none()
        .h(px(size::TAB_BAR))
        .px(px(crate::theme::board::PAD))
        .flex()
        .items_center()
        .gap(px(12.))
        .border_b_1()
        .border_color(t.divider)
}

/// A small switch, reachable with Tab and flipped with Space or Enter.
pub fn toggle(t: &Theme, id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    let accent = t.accent;
    div()
        .id(id)
        .tab_index(0)
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .rounded_full()
        .p(px(2.))
        .flex()
        .when(on, |d| d.justify_end())
        .bg(if on { t.accent } else { t.raised })
        .border_1()
        .border_color(if on { t.accent } else { t.divider })
        .focus(move |s| s.border_color(accent))
        .cursor_pointer()
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(if on { t.on_accent() } else { t.text2 }),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The one button that acts: accent fill.
    Primary,
    /// Outlined.
    Plain,
    /// Words only, for a header or a row.
    Quiet,
}

/// How the tab bar draws its tabs: spaced out where the window has room
/// for every page's tab, close together where it has not. The shell sets
/// it as it draws the bar; every tab on the bar reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabStyle {
    pub compact: bool,
}

impl gpui::Global for TabStyle {}

/// A tab's padding either side, spaced out and compact.
pub const TAB_PAD: f32 = 16.;
pub const TAB_PAD_COMPACT: f32 = 10.;

fn compact(cx: &App) -> bool {
    cx.try_global::<TabStyle>().is_some_and(|s| s.compact)
}

/// A tab's label: its words as written.
pub fn tab_label(title: &str) -> Div {
    let selector = format!("tab-label-{title}");
    div()
        .debug_selector(move || selector)
        .child(title.to_string())
}

/// One tab on the tab bar: the active one in weight over a bar of the
/// accent, standing on the bar's rule; the others words. It never shrinks.
pub fn tab(t: &Theme, id: impl Into<ElementId>, active: bool, cx: &App) -> Stateful<Div> {
    let text = t.text;
    div()
        .id(id)
        .relative()
        .flex_none()
        .px(px(if compact(cx) {
            TAB_PAD_COMPACT
        } else {
            TAB_PAD
        }))
        .flex()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .text_size(px(size::TEXT))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(if active { t.text } else { t.text2 })
        .hover(move |d| d.text_color(text))
        .when(active, |d| {
            // The mark draws out from the middle as the tab is chosen.
            d.child(
                div()
                    .absolute()
                    .bottom_0()
                    .h(px(size::TAB_MARK))
                    .bg(t.accent)
                    .with_animation(
                        "tab-mark",
                        gpui::Animation::new(std::time::Duration::from_millis(
                            crate::theme::motion::TAB_IN,
                        ))
                        .with_easing(gpui::ease_out_quint()),
                        |d, x| {
                            let inset = size::TAB_MARK_INSET + size::TAB_MARK_DRAW * (1. - x);
                            d.left(px(inset)).right(px(inset)).opacity(x)
                        },
                    ),
            )
        })
}

/// The bar tabs sit on: its rule along the bottom, drawn first so the
/// active card covers it, and room above the cards that whatever else is
/// on the bar shares, so all of it stands on one line.
pub fn tab_bar(t: &Theme) -> Div {
    div()
        .relative()
        .flex()
        .items_stretch()
        .bg(t.bg)
        .pt(px(size::TAB_CARD_TOP))
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(1.))
                .bg(t.divider),
        )
}

pub fn button(
    t: &Theme,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    kind: Kind,
) -> Stateful<Div> {
    let b = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(size::CONTROL))
        .px(px(10.))
        .rounded(px(size::RADIUS))
        .text_size(px(size::TEXT))
        .cursor_pointer()
        .relative()
        .child(text.into())
        // A press here quiets its tooltip and places the menu it opens.
        .child(crate::pointer::watch());
    let accent = t.accent;
    let text = t.text;
    // A hairline every kind has, so focus (Tab) can colour it without a jump.
    let b = b.border_1();
    match kind {
        Kind::Primary => b
            .bg(t.accent)
            .border_color(t.accent)
            .text_color(t.on_accent())
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.opacity(crate::theme::fade::HOVER))
            .focus(move |s| s.border_color(text)),
        Kind::Plain => b
            .border_color(t.divider)
            .text_color(t.text)
            .hover(move |s| s.border_color(accent))
            .focus(move |s| s.border_color(accent)),
        Kind::Quiet => b
            .px(px(6.))
            .border_color(gpui::transparent_black())
            .text_color(t.text2)
            .hover(move |s| s.text_color(text))
            .focus(move |s| s.border_color(accent)),
    }
}

/// One option of a choice (Settings' choices, Work's layouts, a form's
/// sessions and schedule): the chosen one filled as the primary button
/// is, the others outlined, so what is chosen reads at a glance.
pub fn option(
    t: &Theme,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    chosen: bool,
) -> Stateful<Div> {
    button(
        t,
        id,
        text,
        if chosen { Kind::Primary } else { Kind::Plain },
    )
}

/// A key cap: filled for the modifier, outlined for the key.
pub fn cap(t: &Theme, text: &str, filled: bool) -> Div {
    let c = div()
        .px(px(6.))
        .h(px(22.))
        .flex()
        .items_center()
        .rounded(px(size::RADIUS_SMALL))
        .text_size(px(size::NOTE))
        .font_weight(FontWeight::BOLD)
        .child(text.to_string());
    if filled {
        c.bg(t.accent).text_color(t.on_accent())
    } else {
        c.border_1().border_color(t.text2).text_color(t.text)
    }
}

/// Caps joined with `+`, as the keys panel and the echo show them.
pub fn caps(t: &Theme, list: &[String]) -> Div {
    let mut row = div().flex().items_center().gap(px(6.));
    for (i, c) in list.iter().enumerate() {
        if i > 0 {
            row = row.child(div().text_color(t.dim).text_size(px(size::NOTE)).child("+"));
        }
        // Every cap but the key itself is a modifier (Alt, Shift, the
        // leader).
        row = row.child(cap(t, c, i + 1 < list.len()));
    }
    row
}

/// A user's square: their initial on their identity colour.
pub fn user_square(t: &Theme, user: &str) -> Div {
    let initial = user
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    div()
        .flex_none()
        .size(px(14.))
        .rounded(px(size::RADIUS_TAG))
        .bg(t.identity_of(user))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(INITIAL))
        .font_weight(FontWeight::BOLD)
        .text_color(t.text)
        .child(initial)
}

/// Where something runs: the device's name in the device's colour, then its
/// environment, dim.
pub fn device_label(t: &Theme, device: &str, environment: &str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.identity_of(device))
                .child(device.to_string()),
        )
        .child(note(t, environment.to_string(), Some(t.dim)))
}

/// The mark of a session with news for the operator: a dot in the colour
/// of what waits for them.
pub fn waiting_dot(t: &Theme) -> Div {
    state_dot(t.needs)
}

/// A state's dot: running, done, failed or waiting, in its colour.
pub fn state_dot(colour: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(size::DOT))
        .rounded_full()
        .bg(colour)
}

/// A popup's frame: raised surface, hairline, shadow.
pub fn panel(t: &Theme) -> Div {
    div()
        .bg(t.surface)
        .border_1()
        .border_color(t.divider)
        .rounded(px(size::RADIUS_PANEL))
        .shadow_lg()
        .text_color(t.text)
        .text_size(px(size::TEXT))
}

/// A thin rule.
pub fn rule(t: &Theme) -> Div {
    div().h(px(1.)).w_full().bg(t.divider)
}

/// A muted line of text.
pub fn note(t: &Theme, text: impl Into<SharedString>, c: Option<Hsla>) -> Div {
    div()
        .text_size(px(size::NOTE))
        .text_color(c.unwrap_or(t.text2))
        .child(text.into())
}

/// A small badge on a row or a tile: words in a hairline box.
pub fn badge(
    t: &Theme,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    why: String,
) -> Stateful<Div> {
    div()
        .id(id)
        .tooltip(move |_, cx| cx.new(|_| Tip(why.clone().into())).into())
        .flex_none()
        .px(px(4.))
        .h(px(16.))
        .flex()
        .items_center()
        .rounded(px(size::RADIUS_SMALL))
        .border_1()
        .border_color(t.divider)
        .text_size(px(size::TINY))
        .text_color(t.text2)
        .child(text.into())
}

/// A user's initial in its square ([`user_square`]).
const INITIAL: f32 = 9.;

/// A hover explanation, drawn like a menu: `.tooltip(kit::tip(why))`.
pub fn tip(
    why: impl Into<SharedString>,
) -> impl Fn(&mut gpui::Window, &mut App) -> gpui::AnyView + 'static {
    let why: SharedString = why.into();
    move |window, cx| {
        // Not over what its button just opened: a menu, a form, a title.
        let words = match crate::pointer::quiet(window, cx) {
            true => SharedString::default(),
            false => why.clone(),
        };
        cx.new(|_| Tip(words)).into()
    }
}

pub struct Tip(pub SharedString);

impl Render for Tip {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let t = crate::theme::theme(cx);
        if self.0.is_empty() {
            return div().into_any_element();
        }
        // A tooltip is drawn outside the page's tree, so it names the
        // app's face itself.
        panel(t)
            .font_family(crate::theme::FAMILY)
            .max_w(px(320.))
            .px(px(8.))
            .py(px(4.))
            .text_size(px(size::NOTE))
            .text_color(t.text)
            .child(self.0.clone())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Window};

    struct Labelled;

    impl Render for Labelled {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let t = Theme::default();
            div()
                .child(device_label(&t, "landmass", "wsl").debug_selector(|| "device-label".into()))
        }
    }

    #[gpui::test]
    fn a_device_label_draws_the_device_and_its_environment(cx: &mut gpui::TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Labelled);
        let bounds = cx.debug_bounds("device-label").expect("the label is drawn");
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
    }
}

/// Which edge of what it sizes a grip stands on. Dragged away from what it
/// sizes, it widens it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripEdge {
    Left,
    Right,
}

/// Where a drag of a grip is: under way, let go (the width to keep), or a
/// double click asking for the width it first had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Moving,
    Kept,
    Reset,
}

/// The one grip for an edge dragged to size what it edges (a panel, a
/// list): caught [`size::GRIP_HIT`] wide across the edge, lit along it while
/// hovered or held. A press holds the width `now`; the drag hands `on` the
/// width it makes, within `range`, as [`Phase::Moving`], and the release,
/// caught wherever on the window it happens, as [`Phase::Kept`]; a double
/// click hands [`Phase::Reset`]. The caller keeps the width. It stands
/// absolutely at `edge` of the relative box it is put in, half over what
/// lies beyond the edge, so it is drawn after it (deferred) and caught on
/// both sides. `name` names it and is its debug selector.
#[allow(clippy::too_many_arguments)]
pub fn grip(
    t: &Theme,
    name: impl Into<SharedString>,
    edge: GripEdge,
    now: f32,
    range: (f32, f32),
    window: &mut gpui::Window,
    cx: &mut App,
    on: impl Fn(f32, Phase, &mut gpui::Window, &mut App) + 'static,
) -> gpui::Deferred {
    let name: SharedString = name.into();
    let id = ElementId::Name(name.clone());
    // Where the press was, and the width then, while held.
    let held = window.use_keyed_state(id.clone(), cx, |_, _| None::<(f32, f32)>);
    let is_held = held.read(cx).is_some();
    let on = std::rc::Rc::new(on);
    let width_at = move |from: f32, width: f32, x: f32| {
        let moved = if edge == GripEdge::Right {
            x - from
        } else {
            from - x
        };
        (width + moved).clamp(range.0, range.1.max(range.0))
    };
    let (press, moving, release) = (held.clone(), held.clone(), held);
    let (on_moving, on_release, on_reset) = (on.clone(), on.clone(), on);
    let line = div()
        .w(px(size::GRIP))
        .h_full()
        .group_hover("grip", |d| d.bg(t.accent_soft()))
        .when(is_held, |d| d.bg(t.accent_soft()));
    let grip = div()
        .id(id)
        .debug_selector(move || name.to_string())
        .group("grip")
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(size::GRIP_HIT))
        .when(edge == GripEdge::Left, |d| d.left(px(-size::GRIP_HIT / 2.)))
        .when(edge == GripEdge::Right, |d| {
            d.right(px(-size::GRIP_HIT / 2.))
        })
        .flex()
        .justify_center()
        .cursor_col_resize()
        .child(line)
        .on_mouse_down(gpui::MouseButton::Left, move |e, window, cx| {
            cx.stop_propagation();
            if e.click_count == 2 {
                press.update(cx, |h, _| *h = None);
                on_reset(now, Phase::Reset, window, cx);
                return;
            }
            let x = f32::from(e.position.x);
            press.update(cx, |h, cx| {
                *h = Some((x, now));
                cx.notify();
            });
        })
        // The drag and its release are the window's while held, so the
        // pointer may leave the thin edge without a cover over the window.
        .child(
            gpui::canvas(
                |_, _, _| (),
                move |_, _, window, cx| {
                    if moving.read(cx).is_none() {
                        return;
                    }
                    let (moving, release) = (moving.clone(), release.clone());
                    let (on_moving, on_release) = (on_moving.clone(), on_release.clone());
                    window.on_mouse_event(move |e: &gpui::MouseMoveEvent, phase, window, cx| {
                        let Some((from, width)) = *moving.read(cx) else {
                            return;
                        };
                        if phase == gpui::DispatchPhase::Bubble {
                            on_moving(
                                width_at(from, width, f32::from(e.position.x)),
                                Phase::Moving,
                                window,
                                cx,
                            );
                            window.refresh();
                        }
                    });
                    window.on_mouse_event(move |e: &gpui::MouseUpEvent, phase, window, cx| {
                        if phase != gpui::DispatchPhase::Bubble
                            || e.button != gpui::MouseButton::Left
                        {
                            return;
                        }
                        let Some((from, width)) = release.update(cx, |h, cx| {
                            cx.notify();
                            h.take()
                        }) else {
                            return;
                        };
                        on_release(
                            width_at(from, width, f32::from(e.position.x)),
                            Phase::Kept,
                            window,
                            cx,
                        );
                    });
                },
            )
            .absolute()
            .size_full(),
        );
    gpui::deferred(grip)
}
