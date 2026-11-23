//! What the shell draws around and over the page: the tab bar, the status
//! line with its toast, the layers and popups on the stack, and the keys a
//! chord was typed with.

use crate::shell::Shell;
use ahjo_app_kit::host::{Cover, Status};
use ahjo_app_kit::hub::{Edge, SideOn, Surface, Toast};
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::menu::{Menu, MenuRow, Pick};
use ahjo_app_kit::popup::{self, Content, Outside, Popup};
use ahjo_app_kit::theme::{self, Theme, alpha, motion, size, theme};
use ahjo_app_kit::widgets::kit::{GripEdge, Phase};
use ahjo_app_kit::widgets::{boundary, kit, pane};
use ahjo_app_kit::zoom::Zoom;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, ElementId, MouseButton, MouseDownEvent, Window,
    deferred, div, ease_out_quint, point, prelude::*, px,
};
use std::time::{Duration, Instant};

/// The padding of the words at the bar's right end (Keys, a link that is
/// not open).
const END_PAD: f32 = 12.;
/// Room kept on the bar for each page that draws its own tabs (the
/// terminals' workspace tabs and their `+`): they shrink and scroll to it.
const OWN_TABS_LEAST: f32 = 130.;
/// The tab that holds the pages the bar has no room for.
const MORE: &str = "More";
/// What follows the shown page's name when More's place carries it.
const MORE_MARK: &str = " …";
/// How long words that are not a failure stay up.
const NOTICE_SHOWN_S: u64 = 8;
/// The order of what draws over the page: the notices, then everything
/// on the stack (each over what opened before it), then the keys a chord
/// was typed with.
const NOTICES: usize = 25;
const STACK: usize = 40;
const ECHO: usize = 1000;
/// What the leader says with keys off.
pub(crate) const KEYS_OFF_HINT: &str =
    "Shortcuts are off: Keys off, at the tab bar's right, turns them on.";

/// Which pages' tabs stand on the bar and which wait under More.
struct Fit {
    compact: bool,
    more: Vec<usize>,
}

/// How many tabs of these widths stand on a bar with this much room, in
/// order, when not all fit. The last place is More's; it carries the shown
/// page's name and More's mark while that page waits under it, so it is
/// kept as wide as the widest tab left out and the mark.
fn standing(widths: &[f32], more_w: f32, mark_w: f32, room: f32) -> usize {
    let mut used = 0.;
    for (n, w) in widths.iter().enumerate() {
        let last = widths[n..]
            .iter()
            .map(|w| w + mark_w)
            .fold(more_w, f32::max);
        if used + last > room {
            // This many no longer leave room for that place: one fewer.
            return n.saturating_sub(1);
        }
        used += w;
    }
    widths.len().saturating_sub(1)
}

impl Shell {
    /// The width of words on the bar, at the bar's text size.
    fn words_w(window: &mut Window, words: &str) -> f32 {
        let text = window.text_system();
        let font = text.resolve_font(&gpui::font(theme::FAMILY));
        let cell = text
            .advance(font, px(size::TEXT), 'M')
            .map_or(size::TEXT * 0.6, |s| f32::from(s.width));
        cell * words.chars().count() as f32
    }

    /// Spaced tabs where every page's fits; else close together; else as
    /// many as fit in order, and the rest under More. Which stand depends
    /// on the width alone, never on the page shown, so no tab moves when
    /// the page changes.
    fn fit_tabs(&self, visible: &[(usize, bool)], end_w: f32, window: &mut Window) -> Fit {
        let room = f32::from(window.viewport_size().width) - end_w;
        let own = visible.iter().filter(|(_, own)| *own).count() as f32 * OWN_TABS_LEAST;
        let pages: Vec<usize> = visible
            .iter()
            .filter(|(_, own)| !own)
            .map(|(i, _)| *i)
            .collect();
        let widths = |pad: f32, window: &mut Window| -> Vec<f32> {
            pages
                .iter()
                .map(|&i| Self::words_w(window, self.pages[i].meta.title) + pad * 2.)
                .collect()
        };
        for (pad, compact) in [(kit::TAB_PAD, false), (kit::TAB_PAD_COMPACT, true)] {
            if own + widths(pad, window).iter().sum::<f32>() <= room {
                return Fit {
                    compact,
                    more: vec![],
                };
            }
        }
        let pad = kit::TAB_PAD_COMPACT;
        let more_w = Self::words_w(window, MORE) + pad * 2.;
        let mark_w = Self::words_w(window, MORE_MARK);
        let stand = standing(&widths(pad, window), more_w, mark_w, room - own);
        Fit {
            compact: true,
            more: pages[stand..].to_vec(),
        }
    }

    fn render_tab_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let off = self.hub.read(cx).areas_off();
        let hub = self.hub.read(cx);
        let keys_on = hub.settings.keys;
        let here = hub.fleet.here();
        let link = match here.state {
            LinkState::Open => None,
            LinkState::Connecting => Some(("Connecting…", t.dim)),
            LinkState::Lost => Some(("Offline, retrying", t.error)),
            LinkState::Refused => Some(("Refused by the node", t.error)),
        };
        let address = hub.fleet.base().to_string();
        let keys_words = if keys_on { "Keys" } else { "Keys off" };
        let update = self.update_badge(cx);
        let end_w = Self::words_w(window, keys_words)
            + END_PAD * 2.
            + theme::art::MARK
            + theme::art::MARK_PAD * 2.
            + link.map_or(0., |(w, _)| Self::words_w(window, w) + END_PAD * 2.)
            + update
                .as_ref()
                .map_or(0., |b| Self::words_w(window, b.words) + END_PAD * 2.);
        // The pages with a tab, and whether each draws its own.
        let mut drawn: Vec<(usize, Option<gpui::AnyElement>)> = vec![];
        for (i, page) in self.pages.iter().enumerate() {
            if !page.meta.tab || page.meta.area.is_some_and(|a| off.iter().any(|o| o == a)) {
                continue;
            }
            drawn.push((i, page.tabs(self.shown == Some(i), window, cx)));
        }
        let visible: Vec<(usize, bool)> = drawn.iter().map(|(i, e)| (*i, e.is_some())).collect();
        let fit = self.fit_tabs(&visible, end_w, window);
        let style = kit::TabStyle {
            compact: fit.compact,
        };
        if cx.try_global::<kit::TabStyle>() != Some(&style) {
            cx.set_global(style);
        }
        let mut bar = div()
            .id("page-tabs")
            .flex_1()
            .min_w_0()
            .flex()
            .items_stretch()
            .child(
                div()
                    .flex_none()
                    .px(px(theme::art::MARK_PAD))
                    .pb(px(size::TAB_CARD_TOP))
                    .flex()
                    .items_center()
                    .debug_selector(|| "app-mark".into())
                    .child(kit::mark(&t)),
            );
        for (i, own) in drawn {
            if let Some(tabs) = own {
                bar = bar.child(tabs);
                continue;
            }
            if fit.more.contains(&i) {
                continue;
            }
            let page = &self.pages[i];
            bar = bar.child(
                kit::tab(&t, ("tab", i), self.shown == Some(i), cx)
                    .debug_selector(|| format!("tab-{}", page.meta.id))
                    .child(kit::tab_label(page.meta.title))
                    .on_click(cx.listener(move |s, _, window, cx| s.show(i, window, cx))),
            );
        }
        if !fit.more.is_empty() {
            let more = fit.more.clone();
            // The shown page, when it waits here: More's place carries its
            // name, and the tabs before it stay where they were.
            let shown = self.shown.filter(|s| more.contains(s));
            bar = bar.child(
                kit::tab(&t, "more-pages", shown.is_some(), cx)
                    .debug_selector(|| "tab-more".into())
                    .child(kit::tab_label(&shown.map_or(MORE.to_string(), |i| {
                        format!("{}{MORE_MARK}", self.pages[i].meta.title)
                    })))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let rows = more
                                .iter()
                                .map(|&i| {
                                    let shell = cx.entity().downgrade();
                                    MenuRow::new(s.pages[i].meta.title, move |window, cx| {
                                        let _ = shell.update(cx, |s, cx| s.show(i, window, cx));
                                    })
                                    .checked(s.shown == Some(i))
                                })
                                .collect();
                            // Under the bar, so the tabs stay in sight.
                            let at = point(e.position.x, px(size::TAB_BAR));
                            s.host.menu(rows, at, cx);
                        }),
                    ),
            );
        }
        let bar = bar
            .child(div().flex_1())
            .children(link.map(|(words, colour)| {
                // Said only while the link is not open: what the app shows
                // is then old.
                kit::tab(&t, "connection", false, cx)
                    .debug_selector(|| "connection".into())
                    .flex_none()
                    .px(px(END_PAD))
                    .text_color(colour)
                    .tooltip(kit::tip(format!(
                        "{address} does not answer: what this window shows may be out of date"
                    )))
                    .child(words)
            }))
            .children(update.map(|b| self.render_update(&t, b, END_PAD, cx)))
            .child(
                kit::tab(&t, "keys-toggle", false, cx)
                    .debug_selector(|| "keys-toggle".into())
                    .flex_none()
                    .px(px(END_PAD))
                    .text_color(if keys_on { t.text2 } else { t.accent })
                    .tooltip(kit::tip(if keys_on {
                        "Every shortcut  Alt+K"
                    } else {
                        "Ahjo Swarm's shortcuts are off: every key goes to the terminal. Click to turn them on."
                    }))
                    .child(keys_words)
                    .on_click(cx.listener(move |s, _, window, cx| {
                        if keys_on {
                            s.toggle_keys(window, cx);
                        } else {
                            s.set_keys(Some(true), cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|s, e: &MouseDownEvent, _, cx| {
                            let shell = cx.entity().downgrade();
                            let shell2 = shell.clone();
                            let on = s.hub.read(cx).settings.keys;
                            let rows = vec![
                                MenuRow::new("Every shortcut", move |window, cx| {
                                    let _ = shell.update(cx, |s, cx| s.toggle_keys(window, cx));
                                })
                                .chord("Alt+K"),
                                MenuRow::rule(),
                                MenuRow::new("Shortcuts on", move |_, cx| {
                                    let _ = shell2.update(cx, |s, cx| s.set_keys(Some(!on), cx));
                                })
                                .checked(on),
                            ];
                            s.host.menu(rows, e.position, cx);
                        }),
                    ),
            );
        kit::tab_bar(&t).flex_none().h(px(size::TAB_BAR)).child(bar)
    }

    /// Turns the keys on or off; `None` flips them.
    pub(crate) fn set_keys(&mut self, on: Option<bool>, cx: &mut Context<Self>) {
        self.hub.update(cx, |h, cx| {
            let on = on.unwrap_or(!h.settings.keys);
            h.settings.keys = on;
            if on && h.status.as_ref().is_some_and(|s| s.text == KEYS_OFF_HINT) {
                h.status = None;
            }
            h.save(cx);
            h.toast(if on { "Shortcuts on" } else { "Shortcuts off" }, cx);
        });
    }

    /// What the app has to say, at the window's bottom right over the
    /// page: the last report (a failure stays until it is closed or
    /// replaced, other words go after a while) and a toast. Nothing when
    /// all is well.
    fn render_notices(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = theme(cx).clone();
        let hub = self.hub.read(cx);
        let status = hub.status.clone().filter(|s| {
            s.failure
                || s.at
                    .elapsed()
                    .is_ok_and(|age| age < Duration::from_secs(NOTICE_SHOWN_S))
        });
        let toast = hub.toast.as_ref().map(|toast| render_toast(&t, toast));
        if let Some(s) = &status
            && !s.failure
            && self.status_expiry.as_ref().map(|(at, _)| *at) != Some(s.at)
        {
            let left = Duration::from_secs(NOTICE_SHOWN_S)
                .saturating_sub(s.at.elapsed().unwrap_or_default());
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(left).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            });
            self.status_expiry = Some((s.at, task));
        }
        if status.is_none() && toast.is_none() {
            return None;
        }
        let mut stack = div()
            .absolute()
            .right(px(size::HEAD_PAD))
            .bottom(px(size::HEAD_PAD))
            .max_w(px(size::NOTICE))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(size::HEAD_GAP));
        if let Some(Status {
            at,
            text,
            failure,
            action,
        }) = status
        {
            let mut notice = kit::panel(&t)
                .id("status")
                .px(px(size::HEAD_PAD))
                .py(px(size::HEAD_GAP))
                .flex()
                .items_center()
                .gap(px(size::HEAD_GAP))
                .child(
                    div()
                        .id("status-text")
                        .debug_selector(|| "status-text".into())
                        .min_w_0()
                        .text_color(if failure { t.error } else { t.text })
                        .child(text),
                );
            if let Some((label, run)) = action {
                notice = notice.child(
                    kit::button(&t, "status-action", label, kit::Kind::Plain).on_click(
                        move |_, window, cx| {
                            let run = run.clone();
                            window.defer(cx, move |window, cx| run(window, cx));
                        },
                    ),
                );
            }
            if failure {
                notice = notice.child(
                    kit::button(&t, "status-close", "×", kit::Kind::Quiet)
                        .tooltip(kit::tip("Close"))
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.hub.update(cx, |h, cx| h.withdraw(at, cx));
                        })),
                );
            }
            stack = stack.child(notice);
        }
        stack = stack.children(toast);
        Some(deferred(stack).with_priority(NOTICES).into_any_element())
    }

    /// The page shown, inside its error boundary: a page that fails to
    /// draw shows why, with Try again, and nothing else stops.
    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let Some(i) = self.shown else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(kit::note(&t, "No pages are registered.", Some(t.dim)))
                .into_any_element();
        };
        let meta = self.pages[i].meta;
        if let Some(why) = boundary::take_late() {
            self.failed.insert(meta.id, why);
        }
        if !self.failed.contains_key(meta.id) {
            let page = &self.pages[i];
            match boundary::guard(|| page.draw(window, cx)) {
                Ok(el) => return el,
                Err(why) => {
                    self.failed.insert(meta.id, why);
                }
            }
        }
        let why = self.failed.get(meta.id).cloned().unwrap_or_default();
        let retry = kit::button(&t, "retry", "Try again", kit::Kind::Primary).on_click(
            cx.listener(move |s, _, _, cx| {
                s.failed.remove(meta.id);
                cx.notify();
            }),
        );
        boundary::failed(&t, meta.title, &why, retry)
    }

    /// What is on the stack over `cover`, bottom first, each over what
    /// opened before it: the layers covering it, and with the window the
    /// popups.
    fn render_stack(
        &mut self,
        cover: Cover,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let stack: Vec<Surface> = self.hub.read(cx).stack.iter().cloned().collect();
        let mut out = vec![];
        for (depth, s) in stack.into_iter().enumerate() {
            match s {
                Surface::Layer(l) if l.cover == cover => {
                    let id = l.view.entity_id().as_u64() as usize;
                    out.push(
                        deferred(
                            div().absolute().inset_0().child(l.view).with_animation(
                                ("layer-in", id),
                                Animation::new(Duration::from_millis(motion::PANEL_IN))
                                    .with_easing(ease_out_quint()),
                                |d, x| d.opacity(x),
                            ),
                        )
                        .with_priority(STACK + depth)
                        .into_any_element(),
                    )
                }
                Surface::Popup(p) if cover == Cover::Window => {
                    out.push(self.render_popup(depth, p, window, cx))
                }
                _ => {}
            }
        }
        out
    }

    /// A popup beside what it was opened from ([`popup::beside`]), over a
    /// cover that takes every press outside it: one that closes it, or one
    /// that only keeps the press from what is under it.
    fn render_popup(
        &mut self,
        depth: usize,
        popup: Popup,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = popup.id();
        let content = match &popup.content {
            Content::Menu(menu) => self.render_menu(depth, menu, cx),
            Content::View(view) => view.clone().into_any_element(),
        };
        let viewport = window.viewport_size();
        let mut cover = div().absolute().inset_0().occlude();
        if popup.outside == Outside::Close {
            for button in [MouseButton::Left, MouseButton::Right] {
                cover = cover.on_mouse_down(
                    button,
                    cx.listener(move |s, _, window, cx| s.press_outside(id, window, cx)),
                );
            }
        }
        deferred(
            cover.child(
                popup::beside(popup.spot, viewport).child(
                    pane::scroll(id.element("popup"))
                        .debug_selector(|| "popup".into())
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        // Never larger than the window it is kept inside: a
                        // drawn popup gives way to it ([`popup::form`] keeps
                        // its actions in view), and anything else taller
                        // scrolls there.
                        .max_w(viewport.width - px(2. * size::POPUP_EDGE))
                        .max_h(viewport.height - px(2. * size::POPUP_EDGE))
                        .child(content)
                        .with_animation(
                            id.element("popup-in"),
                            Animation::new(Duration::from_millis(motion::PANEL_IN))
                                .with_easing(ease_out_quint()),
                            // A fade only: it stands where it will stay from
                            // its first frame.
                            |d, x| d.opacity(x),
                        ),
                ),
            ),
        )
        .with_priority(STACK + depth)
        .into_any_element()
    }

    /// A menu's rows.
    fn render_menu(&mut self, depth: usize, menu: &Menu, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let mut list = kit::panel(&t)
            .min_w(px(size::MENU))
            .py(px(4.))
            .flex()
            .flex_col();
        for (i, row) in menu.rows.iter().enumerate() {
            if row.is_rule() {
                list = list.child(div().my(px(4.)).child(kit::rule(&t)));
                continue;
            }
            let armed = menu.armed == Some(i);
            let keyed = menu.cursor == Some(i);
            let label = if armed {
                format!("{}? Click again", row.label)
            } else {
                row.label.clone()
            };
            let inert = row.inert();
            let colour = if inert {
                t.dim
            } else if row.danger {
                t.error
            } else {
                t.text
            };
            let line = div()
                .h(px(size::ROW))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().w(px(12.)).text_color(t.accent).child(if row.checked {
                    "✓"
                } else {
                    ""
                }))
                .child({
                    let name = format!("menu-label-{}", row.label);
                    div().flex_1().debug_selector(move || name).child(label)
                })
                .children(
                    row.chord
                        .clone()
                        .map(|c| div().text_size(px(size::SMALL)).text_color(t.dim).child(c)),
                );
            list = list.child(
                div()
                    .id(("menu", depth * 100 + i))
                    .debug_selector(|| format!("menu-row-{i}"))
                    .px(px(12.))
                    .flex()
                    .flex_col()
                    .text_color(colour)
                    .when(!inert, |d| {
                        d.cursor_pointer().hover(|d| d.bg(t.accent_light()))
                    })
                    .when(keyed && !armed, |d| d.bg(t.accent_light()))
                    .when(armed, |d| d.bg(alpha(t.error, 0.15)))
                    .child(line)
                    .children(row.hint.clone().map(|h| {
                        div()
                            .max_w(px(280.))
                            .pl(px(24.))
                            .pb(px(6.))
                            .text_size(px(size::SMALL))
                            .text_color(t.dim)
                            .child(h)
                    }))
                    .on_click(cx.listener(move |s, _, window, cx| s.menu_pick(i, window, cx))),
            );
        }
        list.into_any_element()
    }

    /// A click on row `i` of the menu on top, or Enter on its keyboard row.
    pub(crate) fn menu_pick(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let pick = self.hub.update(cx, |h, cx| {
            let pick = match h.stack.top_mut() {
                Some(Surface::Popup(Popup {
                    content: Content::Menu(m),
                    ..
                })) => m.pick(i),
                _ => Pick::Nothing,
            };
            match pick {
                Pick::Run(_) => h.close_menu(cx),
                _ => cx.notify(),
            }
            pick
        });
        if let Pick::Run(run) = pick {
            window.defer(cx, move |window, cx| run(window, cx));
        }
    }

    fn render_echo(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = theme(cx);
        let caps = self.echo.as_ref()?;
        Some(
            deferred(
                div()
                    .absolute()
                    .bottom(px(size::PAD))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        kit::panel(t)
                            .debug_selector(|| "key-echo".into())
                            .px(px(10.))
                            .py(px(6.))
                            .child(kit::caps(t, caps)),
                    ),
            )
            .with_priority(ECHO)
            .into_any_element(),
        )
    }
}

/// A toast: a word that something happened, fading on its own.
fn render_toast(t: &Theme, toast: &Toast) -> AnyElement {
    let fade_at = Duration::from_millis(motion::TOAST_IN + motion::TOAST_SHOWN);
    let leaving = toast.at.elapsed() >= fade_at;
    let panel = kit::panel(t)
        .id("toast")
        .debug_selector(|| "toast".into())
        .px(px(size::HEAD_PAD))
        .py(px(size::HEAD_GAP))
        .child(toast.text.clone());
    // Each toast rises in, and fades as it goes.
    let id = ElementId::Name(format!("toast-{:?}-{leaving}", toast.at).into());
    if leaving {
        panel
            .with_animation(
                id,
                Animation::new(Duration::from_millis(motion::TOAST_OUT)),
                |d, x| d.opacity(1. - x),
            )
            .into_any_element()
    } else {
        panel
            .with_animation(
                id,
                Animation::new(Duration::from_millis(motion::TOAST_IN))
                    .with_easing(ease_out_quint()),
                |d, x| d.opacity(x).mt(px(motion::PANEL_DROP * (1. - x))),
            )
            .into_any_element()
    }
}

impl Shell {
    /// The side panel at the window's left, on every page: the sections
    /// pages lent it, in the pages' order, each with an even share of its
    /// height; put away, a narrow edge that brings it back. A page whose
    /// area is off lends nothing here, and a section lent for the other
    /// pages gives way on its own.
    fn render_side(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let hub = self.hub.read(cx);
        let off = hub.areas_off();
        let sections: Vec<gpui::AnyView> = self
            .pages
            .iter()
            .filter(|p| !p.meta.area.is_some_and(|a| off.iter().any(|o| o == a)))
            .filter_map(|p| {
                let (view, on) = hub.side(p.meta.id)?;
                let stands = match self.shown {
                    Some(shown) => on.stands(p.meta.id, self.pages[shown].meta.id),
                    None => on != SideOn::OwnPage,
                };
                stands.then_some(view)
            })
            .collect();
        if sections.is_empty() {
            return None;
        }
        let column = div()
            .id("side")
            .debug_selector(|| "side".into())
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            .border_r_1()
            .border_color(t.divider);
        if !hub.settings.side_open {
            let hub = self.hub.clone();
            return Some(
                column
                    .w(px(size::SIDE_SHUT))
                    .items_center()
                    .pt(px(8.))
                    .child(
                        kit::button(t, "side-open", "›", kit::Kind::Quiet)
                            .debug_selector(|| "side-open".into())
                            .tooltip(kit::tip("Show sessions  Alt+A"))
                            .on_click(move |_, _, cx| {
                                hub.update(cx, |h, cx| h.set_side_open(true, cx));
                            }),
                    )
                    .into_any_element(),
            );
        }
        let mut column = column.w(px(size::SIDE));
        // Each section starts from what it holds: room left over is shared,
        // and when both overflow the longer keeps the larger share, never
        // less than a few rows.
        for (n, view) in sections.into_iter().enumerate() {
            column = column.child(
                div()
                    .flex_auto()
                    .min_h(px(size::ROW * 4.))
                    .flex()
                    .flex_col()
                    .when(n > 0, |d| d.border_t_1().border_color(t.divider))
                    .child(view),
            );
        }
        Some(column.into_any_element())
    }
}

impl Shell {
    /// The panel at the window's edge (the right unless the settings say
    /// left), while its content stands beside the page shown: one content
    /// at a time, under one header (Back where it applies, the content's
    /// head, Close), sized by its inner edge. It opens and goes with a
    /// slide, and the page beside takes its room or gives it back. Escape
    /// with the keyboard in it puts it away.
    fn render_panel(
        &mut self,
        edge: Edge,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let shown = self.shown_page().unwrap_or_default();
        let hub = self.hub.read(cx);
        if Edge::named(&hub.settings.panel_edge) != edge {
            return None;
        }
        let off = hub.areas_off();
        let width = hub.settings.panel_width;
        let top = hub.panel_on(shown).cloned().filter(|p| {
            !self.pages.iter().any(|m| {
                m.meta.id == p.page && m.meta.area.is_some_and(|a| off.iter().any(|o| o == a))
            })
        });
        let open = hub.panel.open && top.is_some();
        let back = top.as_ref().is_some_and(|p| p.can_go_back(cx)) || hub.panel.has_under();
        // A change of open since the last frame starts a slide; while one
        // runs, a panel put away is still drawn, narrowing.
        let slide = Duration::from_millis(motion::PANEL_SLIDE);
        if self.panel_drawn != Some(open) {
            if self.panel_drawn.is_some() {
                self.panel_moved = Some(Instant::now());
            }
            self.panel_drawn = Some(open);
        }
        let moving = self.panel_moved.filter(|at| at.elapsed() < slide);
        // Drawn again each frame until the slide ends, so a panel put away
        // is gone once it has slid out.
        if moving.is_some() {
            window.request_animation_frame();
        }
        let top = top?;
        if !open && moving.is_none() {
            return None;
        }
        let left = edge == Edge::Left;
        let head = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(size::HEAD_GAP))
            .child(top.head(window, cx));
        let column = div()
            .id("panel-content")
            .track_focus(&self.panel_focus)
            .key_context("Panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            .when(left, |d| d.border_r_1())
            .when(!left, |d| d.border_l_1())
            .border_color(t.divider)
            .on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && !e.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    s.put_panel_away(window, cx);
                }
            }))
            .child(
                pane::head(t)
                    .when(back, |d| {
                        d.child(
                            pane::act(t, "panel-back", "Back")
                                .debug_selector(|| "panel-back".into())
                                .on_click(cx.listener(|s, _, window, cx| s.panel_back(window, cx))),
                        )
                    })
                    .child(head)
                    .child(
                        pane::act(t, "panel-close", "Close")
                            .debug_selector(|| "panel-close".into())
                            .tooltip(kit::tip("Esc, Ctrl+B"))
                            .on_click(cx.listener(|s, _, window, cx| s.put_panel_away(window, cx))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(top.view.clone()),
            );
        // The grip lies outside the frame, which clips its panel as it
        // slides, and is drawn over the page beside.
        let room = window.viewport_size().width / px(1.);
        let side = if self.hub.read(cx).settings.side_open {
            size::SIDE
        } else {
            size::SIDE_SHUT
        };
        let most = size::PANEL_MAX.min(room - side - size::PANEL_PAGE_MIN);
        let hub = self.hub.clone();
        let grip = kit::grip(
            t,
            ("panel-grip", left as usize),
            if left {
                GripEdge::Right
            } else {
                GripEdge::Left
            },
            width,
            (size::PANEL_MIN, most),
            window,
            cx,
            move |width, phase, _, cx| {
                let (width, keep) = match phase {
                    Phase::Moving => (width, false),
                    Phase::Kept => (width, true),
                    Phase::Reset => (size::PANEL, true),
                };
                hub.update(cx, |h, cx| h.set_panel_width(width, keep, cx));
            },
        )
        .debug_selector(|| "panel-grip".into());
        let frame = div()
            .id(("panel", left as usize))
            .debug_selector(move || format!("panel-{}", if left { "left" } else { "right" }))
            .flex_none()
            .h_full()
            .relative()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(px(width))
                    .when(left, |d| d.right_0())
                    .when(!left, |d| d.left_0())
                    .child(column),
            );
        // The slide follows the clock on the wall, a frame at a time, so
        // it ends when its time is up wherever it is drawn.
        let shown = match moving {
            Some(at) => {
                let x = ease_out_quint()(at.elapsed().as_secs_f32() / slide.as_secs_f32());
                width * if open { x } else { 1. - x }
            }
            None => width,
        };
        let frame = frame.w(px(shown)).into_any_element();
        Some(
            div()
                .flex_none()
                .h_full()
                .relative()
                .child(frame)
                .child(deferred(grip))
                .into_any_element(),
        )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        self.view_changed(cx);
        self.veils.note(self.shown, &t);
        let page = self.render_page(window, cx);
        let side = self.render_side(&t, cx);
        let left = self.render_panel(Edge::Left, &t, window, cx);
        let right = self.render_panel(Edge::Right, &t, window, cx);
        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(theme::FAMILY)
            .text_size(px(size::TEXT))
            .capture_key_down(cx.listener(Self::capture_key))
            .on_key_down(cx.listener(Self::zoom_key))
            .child(self.render_tab_bar(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .children(side)
                    .children(left)
                    .child(
                        div()
                            .id("page")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .relative()
                            .child(page)
                            .children(self.veils.page())
                            .children(self.render_stack(Cover::Page, window, cx)),
                    )
                    .children(right),
            )
            .children(self.render_notices(cx))
            .children(self.render_stack(Cover::Window, window, cx))
            .children(self.render_echo(cx))
            .children(self.veils.theme())
            .child(ahjo_app_kit::zoom::app_catch(self.app_wheel.clone(), {
                let hub = self.hub.downgrade();
                move |n, _, cx| {
                    let z = if n > 0 { Zoom::In } else { Zoom::Out };
                    let _ = hub.update(cx, |h, cx| h.zoom_app(z, n.abs(), cx));
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::standing;

    #[test]
    fn the_tabs_that_stand_follow_from_the_width_alone() {
        // Six tabs and More; the widest left out is what More's place holds.
        let widths = [60., 80., 50., 90., 70., 60.];
        let fit = |room| standing(&widths, 40., 0., room);
        assert_eq!(fit(280.), 3, "60 + 80 + 50, and 90 at the end");
        assert_eq!(fit(279.), 2, "no room for the third and a 90");
        assert_eq!(fit(350.), 4, "the fourth, then 70 at the end");
        assert_eq!(fit(30.), 0, "More alone");
        // More's own words are the least its place takes.
        assert_eq!(standing(&[60., 20., 20.], 40., 0., 99.), 0);
        assert_eq!(standing(&[60., 20., 20.], 40., 0., 100.), 1);
        // The mark that follows a page's name there has room too.
        assert_eq!(standing(&widths, 40., 10., 289.), 2);
        assert_eq!(standing(&widths, 40., 10., 290.), 3);
    }
}
