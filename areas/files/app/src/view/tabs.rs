//! The open files as tabs over the viewer, the Ctrl+Tab switch, the find
//! inside the open file, and a code file's outline of its symbols.
use super::*;
use crate::store::tabs::Key;
use gpui::{App, Entity};
use parvi_app_kit::{menu::MenuRow, widgets::field::TextField};

/// The Ctrl+Tab switch: its width, and its top from the page's.
const SWITCH_W: f32 = 420.;
const SWITCH_TOP: f32 = 56.;
/// A tab's name gives way in the middle past this many letters.
const TAB_NAME: usize = 28;
/// The outline beside code: its width, and the narrowest the code gets to
/// leave room for it.
const OUTLINE_W: f32 = 220.;
const CODE_MIN: f32 = 640.;
/// An outline only for a file with this many symbols.
const OUTLINE_MIN: usize = 3;

impl FilesPage {
    /// The tabs, with Back and Forward before them and the list of every
    /// open file after.
    pub(super) fn tab_strip(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let shown = self
            .opened
            .as_ref()
            .filter(|_| self.place.is_none())
            .map(|o| o.key());
        // Two tabs of one name say which folder each is in.
        let names: Vec<&str> = self
            .tabs
            .open
            .iter()
            .map(|t| paths::base_name(&t.key.1))
            .collect();
        let mut strip = div()
            .id("files-tabs")
            .debug_selector(|| "files-tabs".into())
            .flex_1()
            .min_w_0()
            .flex()
            .items_stretch()
            .overflow_x_scroll()
            .track_scroll(&self.tabs_scroll);
        for (i, tab) in self.tabs.open.iter().enumerate() {
            let key = tab.key.clone();
            let name = names[i];
            let twin = names.iter().filter(|n| **n == name).count() > 1;
            let active = shown.as_ref() == Some(&key);
            let dirty = self.drafts.contains_key(&key);
            strip =
                strip.child(self.tab_card(t, i, key, name, twin, active, dirty, tab.preview, cx));
        }
        let mut bar = kit::tab_bar(t)
            .flex_none()
            .h(px(size::TAB_BAR))
            .debug_selector(|| "files-tab-bar".into());
        bar = bar.children(self.history_acts(t, cx));
        bar = bar.child(strip);
        if self.tabs.open.len() > 1 {
            bar = bar.child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .pr(px(size::HEAD_GAP))
                    .child(
                        pane::act(t, "files-open-list", "\u{25be}")
                            .debug_selector(|| "files-open-list".into())
                            .tooltip(kit::tip("Every file open (Ctrl+Tab goes to the last)"))
                            .on_click(cx.listener(|p, e: &gpui::ClickEvent, _, cx| {
                                p.open_list(e.position(), cx)
                            })),
                    ),
            );
        }
        bar.into_any_element()
    }

    /// Back and Forward through where the reader was, and up to the
    /// folder.
    fn history_acts(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        if self.beside {
            return None;
        }
        let mut acts = div()
            .flex_none()
            .flex()
            .items_center()
            .px(px(size::HEAD_GAP))
            .gap(px(2.));
        for (back, word, id, tip) in [
            (true, "\u{2190}", "files-go-back", "Back (Alt+Left)"),
            (false, "\u{2192}", "files-go-forward", "Forward (Alt+Right)"),
        ] {
            let can = self.can_travel(back);
            acts = acts.child(
                pane::act(t, id, word)
                    .debug_selector(move || id.into())
                    .tooltip(kit::tip(tip))
                    .when(!can, |a| a.opacity(parvi_app_kit::theme::fade::IDLE))
                    .on_click(cx.listener(move |p, _, w, cx| p.travel(back, w, cx))),
            );
        }
        acts = acts.child(
            pane::act(t, "files-go-up", "\u{2191}")
                .debug_selector(|| "files-go-up".into())
                .tooltip(kit::tip("Up to the folder (Alt+Up)"))
                .on_click(cx.listener(|p, _, w, cx| p.up(w, cx))),
        );
        Some(acts)
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_card(
        &self,
        t: &Theme,
        i: usize,
        key: Key,
        name: &str,
        twin: bool,
        active: bool,
        dirty: bool,
        preview: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = SharedString::from(format!("files-tab-{i}"));
        let folder = twin.then(|| paths::base_name(&paths::parent(&key.1)).to_owned());
        let selector = format!("files-tab-{name}");
        let (show, close, menu, keep) = (key.clone(), key.clone(), key.clone(), key.clone());
        let state = self.states.get(&key.1).copied();
        kit::tab(t, SharedString::from(format!("tab-{}", key.1)), active, cx)
            .group(group.clone())
            .debug_selector(move || selector.clone())
            .when(active, |d| d.bg(t.surface))
            .child(
                div()
                    .when(preview, |d| d.italic())
                    .when_some(state, |d, s| d.text_color(state_colour(t, s)))
                    .child(pane::middle(name, TAB_NAME)),
            )
            .children(folder.map(|f| div().text_size(px(size::SMALL)).text_color(t.dim).child(f)))
            .child(
                // Unsaved shows a dot, which a hover makes the close.
                div()
                    .id(SharedString::from(format!("tab-close-{}", key.1)))
                    .debug_selector({
                        let id = format!("files-tab-close-{name}");
                        move || id.clone()
                    })
                    .flex_none()
                    .w(px(size::COUNT))
                    .h(px(size::COUNT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(size::RADIUS))
                    .text_color(t.dim)
                    .hover(|s| s.bg(t.raised).text_color(t.text))
                    .when(!active && !dirty, |d| {
                        d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
                    })
                    .child(if dirty { "\u{25cf}" } else { "\u{00d7}" })
                    .tooltip(kit::tip(if dirty {
                        "Unsaved: close (Ctrl+W) asks"
                    } else {
                        "Close (Ctrl+W)"
                    }))
                    .on_click(cx.listener(move |p, _, w, cx| {
                        cx.stop_propagation();
                        p.close_tab(close.clone(), w, cx);
                    })),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |p, _, w, cx| p.close_tab(key.clone(), w, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |p, e: &gpui::MouseDownEvent, _, cx| {
                    p.tab_menu(menu.clone(), e.position, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |p, e: &gpui::ClickEvent, w, cx| {
                if e.click_count() > 1 {
                    p.tabs.keep(&keep);
                    p.keep(cx);
                }
                p.show_tab(show.clone(), w, cx);
                p.focus_file(w, cx);
            }))
            .into_any_element()
    }

    /// Every open file, in a menu, the one shown checked.
    fn open_list(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let shown = self.opened.as_ref().map(|o| o.key());
        let rows = self
            .tabs
            .open
            .iter()
            .map(|tab| {
                let me = self.me.clone();
                let key = tab.key.clone();
                let dirty = self.drafts.contains_key(&key);
                let name = format!(
                    "{}{}",
                    paths::base_name(&key.1),
                    if dirty { " \u{25cf}" } else { "" }
                );
                MenuRow::new(name, move |w, cx| {
                    let _ = me.update(cx, |p, cx| p.show_tab(key.clone(), w, cx));
                })
                .hint(paths::shown(&self.tree.roots, &paths::parent(&tab.key.1)))
                .checked(shown.as_ref() == Some(&tab.key))
            })
            .collect();
        self.host.menu(rows, at, cx);
    }

    /// Ctrl+Tab held: the files in the order last shown, the one it is on
    /// lit.
    pub(super) fn switcher(&self, t: &Theme) -> Option<Div> {
        let at = self.switching?;
        let mut card = div()
            .debug_selector(|| "files-switcher".into())
            .mt(px(SWITCH_TOP))
            .w(px(SWITCH_W))
            .max_w(gpui::relative(0.9))
            .py(px(size::HEAD_GAP))
            .flex()
            .flex_col()
            .bg(t.surface)
            .border_1()
            .border_color(t.divider)
            .rounded(px(size::RADIUS * 2.))
            .shadow_lg();
        for (i, key) in self.tabs.recent().iter().enumerate() {
            let lit = i == at;
            let name = paths::base_name(&key.1).to_owned();
            let selector = format!("files-switch-{name}{}", if lit { "-lit" } else { "" });
            card = card.child(
                div()
                    .debug_selector(move || selector.clone())
                    .h(px(size::ROW))
                    .px(px(size::ROW_PAD))
                    .flex()
                    .items_center()
                    .gap(px(size::HEAD_GAP))
                    .when(lit, |d| d.bg(t.raised))
                    .child(div().flex_none().text_color(t.text).child(name))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(size::SMALL))
                            .text_color(t.dim)
                            .child(paths::shown(&self.tree.roots, &paths::parent(&key.1))),
                    )
                    .when(self.drafts.contains_key(key), |d| {
                        d.child(pane::meta(t, "\u{25cf}", None))
                    }),
            );
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .child(card),
        )
    }

    /// The find inside the open file, under its head: the query, how many
    /// it found and which is shown, steps, and (replacing) what goes in.
    pub(super) fn infile_bar(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let find = self.infile.as_ref()?;
        let count = match (find.found.len(), find.at) {
            (0, _) if self.infile_query.read(cx).text.is_empty() => String::new(),
            (0, _) => "None".into(),
            (n, Some(a)) => format!("{} of {n}", a + 1),
            (n, None) => n.to_string(),
        };
        let field = |id: &'static str, f: Entity<TextField>| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .flex_1()
                .min_w_0()
                .max_w(px(size::NOTICE))
                .child(f)
        };
        let line = || {
            div()
                .flex()
                .items_center()
                .gap(px(size::HEAD_GAP))
                .min_h(px(size::CONTROL + size::HEAD_GAP))
        };
        let mut bar = div()
            .flex_none()
            .px(px(size::HEAD_PAD))
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(t.divider)
            .bg(t.surface)
            .debug_selector(|| "files-infile".into())
            .capture_key_down(cx.listener(|p, e: &gpui::KeyDownEvent, w, cx| {
                let k = &e.keystroke;
                if k.key == "enter" && k.modifiers.shift {
                    p.infile_step(false, cx);
                    cx.stop_propagation();
                } else if k.key == "escape" {
                    p.close_infile(w, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                line()
                    .child(field("files-infile-query", self.infile_query.clone()))
                    .child(
                        pane::meta(t, count, None).debug_selector(|| "files-infile-count".into()),
                    )
                    .child(
                        pane::act(t, "files-infile-up", "\u{2191}")
                            .tooltip(kit::tip("The one before (Shift+Enter)"))
                            .on_click(cx.listener(|p, _, _, cx| p.infile_step(false, cx))),
                    )
                    .child(
                        pane::act(t, "files-infile-down", "\u{2193}")
                            .tooltip(kit::tip("The next (Enter)"))
                            .on_click(cx.listener(|p, _, _, cx| p.infile_step(true, cx))),
                    )
                    .when(!find.replace, |d| {
                        d.child(
                            pane::act(t, "files-infile-replace", "Replace")
                                .debug_selector(|| "files-infile-replace".into())
                                .tooltip(kit::tip("Ctrl+H"))
                                .on_click(cx.listener(|p, _, w, cx| p.find_in_file(true, w, cx))),
                        )
                    })
                    .child(
                        pane::act(t, "files-infile-close", "\u{00d7}")
                            .tooltip(kit::tip("Esc"))
                            .on_click(cx.listener(|p, _, w, cx| p.close_infile(w, cx))),
                    ),
            );
        if find.replace {
            bar = bar.child(
                line()
                    .child(field("files-infile-with", self.infile_with.clone()))
                    .child(
                        pane::act(t, "files-replace-one", "Replace")
                            .debug_selector(|| "files-replace-one".into())
                            .tooltip(kit::tip("This one, and on to the next (Enter)"))
                            .on_click(cx.listener(|p, _, w, cx| p.replace_one(w, cx))),
                    )
                    .child(
                        pane::act(t, "files-replace-all", "All")
                            .debug_selector(|| "files-replace-all".into())
                            .tooltip(kit::tip("Every one, as one undo (Ctrl+Enter)"))
                            .on_click(cx.listener(|p, _, w, cx| p.replace_every(w, cx))),
                    ),
            );
        }
        Some(bar)
    }

    /// The symbol the reader is in, after the file's name.
    pub(super) fn symbol_crumb(&self, t: &Theme, cx: &App) -> Option<Div> {
        let s = self.symbol_here(cx)?;
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .text_color(t.text2)
                .debug_selector(|| "files-symbol-crumb".into())
                .child(div().px(px(4.)).text_color(t.dim).child("\u{203a}"))
                .child(pane::middle(&s.name, TAB_NAME)),
        )
    }

    /// Code with its outline beside it when it has symbols and room.
    pub(super) fn with_outline(
        &self,
        t: &Theme,
        code: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let room = self.body_room.get().unwrap_or(0.);
        let show = self.symbols.len() >= OUTLINE_MIN
            && room >= CODE_MIN + OUTLINE_W
            && !self
                .opened
                .as_ref()
                .is_some_and(|o| o.editing && self.infile.is_some());
        let body = div()
            .relative()
            .size_full()
            .flex()
            .child(measure(self.body_room.clone()))
            .child(div().flex_1().min_w_0().h_full().child(code));
        if !show {
            return body.into_any_element();
        }
        let here = self.symbol_here(cx).map(|s| s.line);
        let least = self.symbols.iter().map(|s| s.depth).min().unwrap_or(0);
        let marks = self.code.read(cx).marks().to_vec();
        let mut list = div()
            .id("files-code-outline")
            .debug_selector(|| "files-code-outline".into())
            .flex_none()
            .w(px(OUTLINE_W))
            .h_full()
            .py(px(size::HEAD_GAP))
            .pr(px(size::HEAD_PAD))
            .border_l_1()
            .border_color(t.divider)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .text_size(px(size::SMALL));
        for (n, s) in self.symbols.iter().enumerate() {
            let end = self.symbols.get(n + 1).map_or(usize::MAX, |s| s.line);
            let changed = marks
                .iter()
                .find(|(l, _)| *l >= s.line && *l < end)
                .map(|m| m.1);
            let lit = here == Some(s.line);
            let line = s.line;
            list = list.child(
                div()
                    .id(SharedString::from(format!("code-outline-{line}")))
                    .flex_none()
                    .flex()
                    .gap(px(4.))
                    .py(px(2.))
                    .pl(px(
                        size::HEAD_GAP + (s.depth - least) as f32 * size::HEAD_PAD
                    ))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .text_color(if lit { t.text } else { t.text2 })
                    .hover(|d| d.text_color(t.text))
                    .border_l_2()
                    .border_color(match (lit, changed) {
                        (true, _) => t.accent,
                        (false, Some(m)) => m.colour(t),
                        (false, None) => gpui::transparent_black(),
                    })
                    .child(div().flex_none().text_color(t.dim).child(s.kind))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(s.name.clone()),
                    )
                    .tooltip(kit::tip(format!(
                        "{} {} (line {})",
                        s.kind,
                        s.name,
                        line + 1
                    )))
                    .on_click(cx.listener(move |p, _, w, cx| p.go_to(line, w, cx))),
            );
        }
        body.child(list).into_any_element()
    }

    /// A tab with unsaved changes asked to close.
    pub(super) fn closing_line(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let key = self.closing.as_ref()?;
        Some(
            div()
                .flex_none()
                .min_h(px(size::TAB_BAR))
                .px(px(size::HEAD_PAD))
                .flex()
                .items_center()
                .gap(px(size::HEAD_GAP))
                .border_b_1()
                .border_color(t.divider)
                .debug_selector(|| "files-closing".into())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(format!("{} has unsaved changes.", paths::base_name(&key.1))),
                )
                .child(
                    kit::button(t, "files-close-save", "Save", Kind::Primary)
                        .debug_selector(|| "files-close-save".into())
                        .on_click(cx.listener(|p, _, w, cx| p.close_saving(true, w, cx))),
                )
                .child(
                    pane::act(t, "files-close-lose", "Don't save")
                        .debug_selector(|| "files-close-lose".into())
                        .on_click(cx.listener(|p, _, w, cx| p.close_saving(false, w, cx))),
                )
                .child(
                    pane::act(t, "files-close-keep", "Cancel")
                        .on_click(cx.listener(|p, _, _, cx| p.close_cancel(cx))),
                ),
        )
    }

    /// The side's two views as tabs at the head of the list, and Hide.
    pub(super) fn side_switch(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .flex_none()
            .h(px(size::CONTROL + size::HEAD_GAP))
            .px(px(size::HEAD_GAP))
            .flex()
            .items_center()
            .gap(px(2.))
            .border_b_1()
            .border_color(t.divider);
        for side in Side::SHOWN {
            let on = self.side == side;
            let id = format!("files-side-{}", side.word().to_lowercase());
            bar = bar.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .px(px(size::HEAD_GAP))
                    .h(px(size::CONTROL))
                    .flex()
                    .items_center()
                    .rounded(px(size::RADIUS))
                    .cursor_pointer()
                    .text_size(px(size::SMALL))
                    .text_color(if on { t.text } else { t.dim })
                    .when(on, |d| d.bg(t.raised))
                    .hover(|d| d.text_color(t.text))
                    .tooltip(kit::tip(side.what()))
                    .child(side.word().to_uppercase())
                    .on_click(cx.listener(move |p, _, w, cx| p.set_side(side, w, cx))),
            );
        }
        bar.child(div().flex_1()).child(
            pane::act(t, "files-side-hide", "\u{2190}")
                .debug_selector(|| "files-side-hide".into())
                .tooltip(kit::tip("Hide the side (Ctrl+B)"))
                .on_click(cx.listener(|p, _, w, cx| p.toggle_side(w, cx))),
        )
    }

    /// Hidden side: the act that shows it again, before the tabs.
    pub(super) fn side_show(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        (self.side == Side::Hidden && !self.beside && self.split_of.is_none()).then(|| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .pl(px(size::HEAD_GAP))
                .child(
                    pane::act(t, "files-side-show", "\u{2630}")
                        .debug_selector(|| "files-side-show".into())
                        .tooltip(kit::tip("Show the side (Ctrl+B)"))
                        .on_click(cx.listener(|p, _, w, cx| p.toggle_side(w, cx))),
                )
                .into_any_element()
        })
    }
}
