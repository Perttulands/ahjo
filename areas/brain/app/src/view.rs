//! The Brain drawn in the window's panel: the search in the panel's
//! header, under it what arrived lately or what the search found; or the
//! page read in their place, its title in the header.
use crate::page::Panel;
use crate::state::PageRef;
use crate::tokens::*;
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, size, theme};
use ahjo_app_kit::widgets::{kit, markdown, pane, select};
use ahjo_app_kit::zoom::Zoom;
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, HighlightStyle, KeyDownEvent, MouseButton,
    SharedString, StyledText, Window, div, prelude::*, px,
};

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let reading = self.reading();
        let body = if reading {
            self.reader(&t, window, cx).into_any_element()
        } else {
            self.list(&t, cx).into_any_element()
        };
        let selected = self.select.clone();
        // The panel decides its width.
        div()
            .id("brain-panel")
            .debug_selector(|| "brain-panel".into())
            .size_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            .font_family(FAMILY)
            .text_color(t.text)
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .on_key_down(cx.listener(move |panel, event: &KeyDownEvent, window, cx| {
                let k = &event.keystroke;
                if let Some(z) = ahjo_app_kit::zoom::from_key(k) {
                    panel.zoom_read(z, 1, cx);
                    cx.stop_propagation();
                } else if k.key == "left" && k.modifiers.alt && !k.modifiers.control {
                    panel.back(window, cx);
                    cx.stop_propagation();
                } else if k.modifiers.control
                    && k.key == "c"
                    && let Some(text) = selected.borrow().copied()
                {
                    select::copy(text, cx);
                    cx.stop_propagation();
                }
            }))
    }
}

impl ahjo_app_kit::panel::PanelView for Panel {
    fn head(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        if self.reading() {
            return self.reader_head(&t).into_any_element();
        }
        // The search stands in the panel's header: Esc there clears what
        // is typed; with nothing typed it is the panel's, and puts it away.
        self.search_head(&t)
            .on_key_down(cx.listener(|panel, event: &KeyDownEvent, _, cx| {
                let k = &event.keystroke;
                if k.key == "escape"
                    && !k.modifiers.modified()
                    && !panel.field.read(cx).text.is_empty()
                {
                    panel.field.update(cx, |field, cx| field.set("", false, cx));
                    panel.typed("", cx);
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }
    fn can_go_back(&self, _: &gpui::App) -> bool {
        self.reading()
    }
    fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.back(window, cx);
    }
    fn focus(&self, _: &gpui::App) -> Option<gpui::FocusHandle> {
        Some(self.reader_focus.clone())
    }
}

impl Panel {
    /// A page is read, or failed to be, in the list's place.
    fn reading(&self) -> bool {
        self.state.page.is_some() || self.state.page_error.is_some()
    }

    /// The search and how many it found.
    fn search_head(&self, t: &Theme) -> Div {
        let count = self
            .state
            .count()
            .filter(|_| !self.state.query.is_empty())
            .map(|count| {
                let selector = format!("brain-count-{count}");
                pane::meta(t, count, None).debug_selector(move || selector)
            });
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(size::HEAD_GAP))
            .child(
                div()
                    .debug_selector(|| "brain-search".into())
                    .flex_1()
                    .min_w_0()
                    .child(self.field.clone()),
            )
            .children(count)
    }

    /// The page read: its title and Send.
    fn reader_head(&self, t: &Theme) -> Div {
        let head = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(size::HEAD_GAP));
        let Some(page) = self.state.page.as_ref() else {
            return head.child(div().flex_1());
        };
        head.child(pane::title(t, page.title.clone()).debug_selector(|| "brain-page-title".into()))
            .children(self.state.reference().map(|reference| {
                pane::send(t, "brain-send", &self.host, reference)
                    .debug_selector(|| "brain-send".into())
            }))
    }

    /// The list: what the search found while something is typed, else
    /// what arrived lately; the marked row is the one Enter opens.
    fn list(&mut self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (rows, chosen) = self.list_rows(t, cx);
        if std::mem::take(&mut self.reveal)
            && let Some(at) = chosen
        {
            self.list_scroll.scroll_to_item(at);
        }
        pane::scroll("brain-results")
            .flex_1()
            .py(px(size::ROW_INSET))
            .track_scroll(&self.list_scroll)
            .children(rows)
    }

    /// The list's rows, and which of them is marked (an index into the
    /// rows, past any label).
    fn list_rows(&self, t: &Theme, cx: &mut Context<Self>) -> (Vec<AnyElement>, Option<usize>) {
        let b = &self.state;
        let mut out: Vec<AnyElement> = Vec::new();
        let chosen = b.chosen();
        let note = |words: String, colour| pane::label(t, words).text_color(colour);
        if !b.query.is_empty() {
            if let Some(e) = &b.search_error {
                out.push(note(e.clone(), t.error).into_any_element());
            } else if b.results.is_empty() && !b.searching {
                out.push(
                    pane::empty(t, format!("Nothing found for “{}”.", b.query), None)
                        .into_any_element(),
                );
            }
            for (i, hit) in b.results.iter().enumerate() {
                out.push(self.result_row(t, i, hit, chosen == Some(i), cx));
            }
            return (out, chosen);
        }
        if let Some(e) = &b.git_error {
            out.push(note(e.clone(), t.error).into_any_element());
        }
        if !b.arrivals_read {
            out.push(pane::empty(t, "Reading the brain…", None).into_any_element());
        } else if b.arrivals.is_empty() && b.git_error.is_none() {
            out.push(pane::empty(t, "Nothing has arrived yet.", None).into_any_element());
        } else if !b.arrivals.is_empty() {
            out.push(pane::label(t, "Arrived lately").into_any_element());
        }
        let first = out.len();
        let now = jiff::Timestamp::now();
        for (j, a) in b.arrivals.iter().enumerate() {
            let path = a.page.path.clone();
            let title = format!("brain-arrived-{path}");
            let date = format!("brain-arrived-date-{path}");
            out.push(
                pane::row(t, ("brain-arrival", j), chosen == Some(j))
                    .debug_selector(move || format!("brain-arrival-{j}"))
                    .child(pane::cell(a.page.title.clone()).debug_selector(move || title))
                    .child(
                        pane::meta(t, crate::state::when(&a.updated, now), None)
                            .debug_selector(move || date),
                    )
                    .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                        panel.open(&path, window, cx)
                    }))
                    .into_any_element(),
            );
        }
        (out, chosen.map(|at| at + first))
    }

    /// A result: its title and kind on one line, the words that matched
    /// under it with the match marked.
    fn result_row(
        &self,
        t: &Theme,
        i: usize,
        hit: &crate::state::Hit,
        chosen: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut snippet = String::new();
        let mut highlights = Vec::new();
        for (part, marked) in crate::state::snippet_parts(&hit.snippet) {
            let start = snippet.len();
            snippet.push_str(&part.replace('\n', " "));
            if marked {
                highlights.push((
                    start..snippet.len(),
                    HighlightStyle {
                        color: Some(t.text),
                        background_color: Some(alpha(t.accent, MARK)),
                        ..Default::default()
                    },
                ));
            }
        }
        let snippet = snippet.trim_end().to_string();
        let path = hit.path.clone();
        let title = format!("brain-hit-{path}");
        let kind = (!hit.kind.is_empty()).then(|| {
            let selector = format!("brain-result-kind-{}", hit.kind);
            pane::meta(t, hit.kind.clone(), None).debug_selector(move || selector)
        });
        pane::row(t, ("brain-result", i), chosen)
            .debug_selector(move || format!("brain-result-{i}"))
            .flex_col()
            .items_start()
            .py(px(RESULT_PAD_Y))
            .gap(px(SNIPPET_GAP))
            .when(chosen, |d| {
                d.child(div().debug_selector(move || format!("brain-result-selected-{i}")))
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(size::HEAD_GAP))
                    .child(pane::cell(hit.title.clone()).debug_selector(move || title))
                    .children(kind),
            )
            .when(!snippet.is_empty(), |d| {
                d.child(
                    div()
                        .w_full()
                        .font_weight(FontWeight::NORMAL)
                        .text_size(px(size::SMALL))
                        .text_color(t.text2)
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(StyledText::new(snippet).with_highlights(highlights)),
                )
            })
            .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                panel.open(&path, window, cx);
            }))
            .into_any_element()
    }

    /// The page read: Back, its title and Send over the page, which
    /// scrolls and zooms on its own.
    fn reader(&mut self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let Some(page) = self.state.page.clone() else {
            let why = self.state.page_error.clone().unwrap_or_default();
            let body = div()
                .debug_selector(|| "brain-page-error".into())
                .child(pane::empty(t, why, None).text_color(t.error));
            return pane::panel_detail(t, "brain-reader", None, body);
        };
        let body = self.page_body(t, &page, window, cx);
        let me = cx.weak_entity();
        let catch = ahjo_app_kit::zoom::catch(self.zoom_wheel.clone(), move |n, _, cx| {
            let z = if n > 0 { Zoom::In } else { Zoom::Out };
            let _ = me.update(cx, |panel, cx| panel.zoom_read(z, n.abs(), cx));
        });
        pane::panel_detail(t, "brain-reader", Some(&self.scroll), body)
            .relative()
            .bg(t.bg)
            .child(catch)
            .track_focus(&self.reader_focus)
            .on_mouse_down(MouseButton::Left, {
                let focus = self.reader_focus.clone();
                move |_, window, cx| window.focus(&focus, cx)
            })
    }

    fn page_body(
        &mut self,
        t: &Theme,
        page: &crate::state::Page,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut content = div().w_full().flex().flex_col().gap(px(GAP));
        let subject = page.path.split('/').collect::<Vec<_>>();
        let subject = if subject.len() > 2 {
            subject[1]
        } else {
            subject.first().copied().unwrap_or("")
        };
        let subject = subject.replace(['-', '_'], " ");
        let changed = if page.updated.is_empty() {
            "never committed".to_string()
        } else {
            format!(
                "changed {} by {}",
                crate::state::when(&page.updated, jiff::Timestamp::now()),
                page.author
            )
        };
        let meta = if subject.is_empty() {
            changed
        } else {
            format!("{subject} · {changed}")
        };
        let selector = format!("brain-meta-{meta}");
        let page_selector = format!("brain-reading-{}", page.path);
        content = content.child(
            div()
                .debug_selector(move || page_selector)
                .text_size(px(size::SMALL))
                .text_color(t.dim)
                .child(div().debug_selector(move || selector).child(meta)),
        );
        if !page.summary.is_empty() {
            content = content.child(kit::note(t, page.summary.clone(), Some(t.text2)));
        }
        content = content.child(self.prose(t, page, cx));
        content
            .children(self.page_list(t, ["Related", "brain-related"], &page.related, cx))
            .children(self.page_list(t, ["Linked from", "brain-backlink"], &page.backlinks, cx))
    }

    /// Other pages under a label, each a row that follows to it; `name`
    /// names the section and, with each page's path, its rows.
    fn page_list(
        &self,
        t: &Theme,
        [label, name]: [&'static str; 2],
        pages: &[PageRef],
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if pages.is_empty() {
            return None;
        }
        let mut section = pane::section(t, label)
            .pt(px(SECTION_TOP))
            .debug_selector(move || format!("{name}-section"));
        for (i, p) in pages.iter().enumerate() {
            let path = p.path.clone();
            let selector = format!("{name}-{path}");
            section = section.child(
                pane::row(t, SharedString::from(format!("{name}-{i}")), false)
                    .debug_selector(move || selector)
                    .child(pane::cell(p.title.clone()))
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                        panel.follow(&path, None, cx)
                    })),
            );
        }
        Some(section)
    }

    /// The page's Markdown: wiki links work, a link to a heading scrolls to
    /// it, and what is selected copies.
    fn prose(
        &mut self,
        t: &Theme,
        page: &crate::state::Page,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let links = page.links.clone();
        let selection = self.select.clone();
        let on_link = std::rc::Rc::new(move |target: &str, _: &mut Window, cx: &mut gpui::App| {
            if !selection.borrow().selection.is_empty() {
                return;
            }
            if target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("mailto:")
            {
                cx.open_url(target);
                return;
            }
            let (target, anchor) = target
                .split_once('#')
                .map_or((target, None), |(target, anchor)| {
                    (target, Some(anchor.to_string()))
                });
            if target.is_empty() {
                if let Some(anchor) = anchor {
                    let _ = weak.update(cx, |panel, cx| {
                        *panel.anchor.borrow_mut() = Some(anchor);
                        cx.notify();
                    });
                }
                return;
            }
            if let Some(path) = links
                .iter()
                .find(|link| link.target == target)
                .and_then(|link| link.page.as_ref())
                .map(|page| page.path.clone())
            {
                let _ = weak.update(cx, |panel, cx| panel.follow(&path, anchor, cx));
            }
        });
        // Zoomed, the text and its measure grow together.
        let read = self.read_size(cx);
        let scale = read / t.ty.read;
        let mut zoomed = t.clone();
        zoomed.ty.read = read;
        let look = markdown::Look {
            t: &zoomed,
            refs: false,
            on_link: Some(on_link),
            select: Some(self.select.clone()),
        };
        self.select.borrow_mut().begin();
        let without_title = crate::prose::without_title(&page.body, &page.title);
        let text = crate::prose::wiki_links_named(&without_title, |target| {
            page.links
                .iter()
                .find(|link| link.target == target)
                .and_then(|link| link.page.as_ref())
                .map(|linked| linked.title.clone())
                .filter(|title| !title.is_empty())
        });
        let selection = self.select.clone();
        let blocks = markdown::parse(&text);
        let mut slugs = crate::prose::Slugger::default();
        if without_title.len() != page.body.len() {
            slugs.slug(&page.title);
        }
        let headings: Vec<(String, usize)> = blocks
            .iter()
            .enumerate()
            .filter_map(|(i, block)| match block {
                markdown::Block::Heading(_, heading) => {
                    Some((slugs.slug(&markdown::inline(heading, false).text), i))
                }
                _ => None,
            })
            .collect();
        let anchor = self.anchor.clone();
        let scroll = self.scroll.clone();
        let body = markdown::render(&look, "brain-body", &blocks)
            .max_w(px(MEASURE * scale))
            .debug_selector(|| "brain-prose".into())
            .on_children_prepainted(move |bounds, window, cx| {
                selection.borrow_mut().laid_out();
                let Some(want) = anchor.borrow_mut().take() else {
                    return;
                };
                let want = crate::prose::slug(&want);
                let Some((_, index)) = headings.iter().find(|(slug, _)| slug == &want) else {
                    return;
                };
                let Some(child) = bounds.get(*index) else {
                    return;
                };
                let offset = scroll.offset();
                scroll.set_offset(gpui::point(
                    offset.x,
                    offset.y + scroll.bounds().top() - child.top(),
                ));
                window.defer(cx, |window, _| window.refresh());
            });
        select::track(body, &self.select, cx.entity_id(), |_, _| {}).into_any_element()
    }
}
