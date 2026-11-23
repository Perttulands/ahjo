//! What the shell draws around and over the page: the tab bar, the status
//! line with its toast, the layers and popups on the stack, and the keys a
//! chord was typed with.

use crate::shell::Shell;
use ahjo_app_kit::frame::{self, Cover};
use ahjo_app_kit::hub::{Edge, SideOn};
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::menu::MenuRow;
use ahjo_app_kit::panel::{Act, EdgeColumn};
use ahjo_app_kit::theme::{self, Theme, size, theme};
use ahjo_app_kit::widgets::{boundary, kit};
use ahjo_app_kit::zoom::Zoom;
use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, Window, deferred, div, point, prelude::*, px,
};
use std::rc::Rc;

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
/// The keys a chord was typed with draw over everything, the frame's
/// notices and stack ([`frame::NOTICES`], [`frame::STACK`]) too.
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
            let hint = h
                .frame
                .read(cx)
                .status
                .as_ref()
                .filter(|s| s.text == KEYS_OFF_HINT);
            if let Some(at) = hint.filter(|_| on).map(|s| s.at) {
                h.withdraw(at, cx);
            }
            h.save(cx);
            h.toast(if on { "Shortcuts on" } else { "Shortcuts off" }, cx);
        });
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
        let share = self.panel_slide.share(open, window);
        let top = top?;
        let share = share?;
        let room = window.viewport_size().width / px(1.);
        let side = if hub.settings.side_open {
            size::SIDE
        } else {
            size::SIDE_SHUT
        };
        let most = size::PANEL_MAX.min(room - side - size::PANEL_PAGE_MIN);
        // A window too narrow for the width kept still leaves the page its
        // least: the panel gives way, the kept width stays for a wider window.
        let width = width.min(most);
        let shell = cx.entity().downgrade();
        let (back_to, close) = (shell.clone(), shell);
        let hub = self.hub.clone();
        let column = EdgeColumn {
            id: "panel",
            left: edge == Edge::Left,
            width,
            range: (size::PANEL_MIN.min(most), most),
            first_width: size::PANEL,
            head: top.head(window, cx),
            content: top.view.clone().into_any_element(),
            focus: self.panel_focus.clone(),
            back: back.then(|| {
                let back: Act = Rc::new(move |window, cx| {
                    let _ = back_to.update(cx, |s, cx| s.panel_back(window, cx));
                });
                back
            }),
            close: Rc::new(move |window, cx| {
                let _ = close.update(cx, |s, cx| s.put_panel_away(window, cx));
            }),
            close_keys: "Esc, Ctrl+B",
            sized: Rc::new(move |width, keep, _, cx| {
                hub.update(cx, |h, cx| h.set_panel_width(width, keep, cx));
            }),
        };
        Some(column.render(t, share, window, cx))
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
        let frame = self.hub.read(cx).frame.clone();
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
                            .children(frame::stack(&frame, Cover::Page, window, cx)),
                    )
                    .children(right),
            )
            .children(frame::notices(&frame, cx))
            .children(frame::stack(&frame, Cover::Window, window, cx))
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
