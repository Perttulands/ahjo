//! What stands over the sky and beside it: the search and its results,
//! the replay bar, the actions, the inbox, the sheet a page is read in and
//! the note.

use super::*;

// ---- Overlays over the sky.

impl BrainApp {
    /// The search, and under it what it found, while a page is not being
    /// read over it.
    pub(super) fn search_panel(
        &self,
        t: &Theme,
        typing: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let recent = typing && self.query.is_empty() && self.recent_shown();
        let width = if recent {
            self.recent_width()
        } else {
            self.search_width()
        };
        let mut panel = div()
            .absolute()
            .left(px(chrome::INSET))
            .top(px(chrome::INSET))
            .w(px(width))
            .flex()
            .flex_col()
            .gap(px(search::LIST_GAP))
            .occlude()
            .child(
                div()
                    .id("search")
                    .debug_selector(|| "search".into())
                    .w(px(self.search_width()))
                    .h(px(search::FIELD_HEIGHT))
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .child(self.field.clone()),
            );
        let Some(data) = &self.data else { return panel };
        if self.query.is_empty() {
            if recent {
                panel = panel.child(self.recent(data, t, cx));
            }
            return panel;
        }
        if let Some(e) = &self.away {
            panel = panel.child(
                div()
                    .id("away")
                    .flex()
                    .gap(px(8.))
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.text2)
                    .tooltip(kit::tip(e.message.clone()))
                    .child(
                        div()
                            .flex_none()
                            .mt(px(4.))
                            .size(px(7.))
                            .rounded(px(7.))
                            .bg(t.error),
                    )
                    .child(
                        div().flex_1().min_w_0().child(match &self.place.from {
                            Some(r) => format!(
                                "{}'s search is not answering, so only titles and aliases \
                             are matched here.",
                                r.name
                            ),
                            None => "Citadel is not answering, so only titles and aliases are \
                                 matched here."
                                .to_string(),
                        }),
                    ),
            );
        }
        if self.results.is_empty() && self.answered == self.asked {
            return panel.child(
                div()
                    .h(px(art::EMPTY_ROOM))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .child(pane::vacant(t, "Nothing in the brain matches.", None)),
            );
        }
        let aside = self.aside();
        if self.results.is_empty() || (self.reading.is_some() && !typing && !aside) {
            return panel;
        }
        // Every result is reachable: the list scrolls in the room down to
        // the counts, and the wheel over it is its own, not the sky's.
        let mut room = self.size.1
            - search::FIELD_HEIGHT
            - chrome::INSET * 3.
            - chrome::FOOT
            - search::LIST_GAP;
        if self.away.is_some() {
            room -= search::AWAY;
        }
        let rows = self
            .results
            .iter()
            .enumerate()
            .map(|(i, f)| self.result_row(i, f, data, t, aside, cx).into_any_element())
            .collect::<Vec<_>>();
        let list = pane::scroll("results")
            .py(px(4.))
            .max_h(px(room.max(search::ASIDE_ROW * 3.)))
            .track_scroll(&self.results_scroll)
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .children(rows);
        panel = panel.child(list);
        panel
    }

    /// What changed last, as a way in before anything is asked.
    pub(super) fn recent(
        &self,
        data: &Data,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut list = div()
            .flex()
            .flex_col()
            .py(px(4.))
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .child(
                kit::label(t, "Changed lately")
                    .px(px(search::ROW_PAD_X))
                    .pt(px(search::ROW_PAD_Y))
                    .pb(px(2.)),
            );
        for &p in &data.recent {
            let page = &data.corpus.pages[p];
            let hue = data.hues[page.subject];
            list = list.child(
                div()
                    .id(("recent", p))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y - 2.))
                    .cursor_pointer()
                    .hover(|s| s.bg(alpha(hue, 0.08)))
                    .on_click(cx.listener(move |me, _, _, cx| me.open(p, cx)))
                    .on_hover(cx.listener(move |me, on: &bool, _, cx| {
                        let want = on.then_some(p);
                        if me.hover != want && (*on || me.hover == Some(p)) {
                            me.hover = want;
                            cx.notify();
                        }
                    }))
                    .child(div().flex_none().size(px(7.)).rounded(px(7.)).bg(hue))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(search::TITLE))
                            .text_color(alpha(t.text, 0.9))
                            .child(page.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(search::SNIPPET))
                            .text_color(t.dim)
                            .child(reader::since(page.modified, now)),
                    ),
            );
        }
        list
    }

    pub(super) fn result_row(
        &self,
        i: usize,
        f: &Found,
        data: &Data,
        t: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let page = &data.corpus.pages[f.page];
        let hue = data.hues[page.subject];
        let p = f.page;
        // Beside an orbit the row marked is the page read.
        let picked = match self.reading {
            Some(r) => r == p,
            None => self.pick == Some(i),
        };
        let snippet = snippet_text(&f.snippet, &page.summary, hue, t);
        div()
            .id(("result", i))
            // Its own height in the scrolling list, a wrapped title too.
            .flex_none()
            .flex()
            .gap(px(10.))
            .px(px(search::ROW_PAD_X))
            .py(px(search::ROW_PAD_Y))
            // Beside an orbit a title wraps onto a second line before it
            // is cut.
            .when(compact, |d| {
                d.min_h(px(search::ASIDE_ROW))
                    .py(px(search::ASIDE_ROW_PAD))
                    .items_center()
            })
            .when(picked, |d| d.bg(alpha(hue, 0.1)))
            .cursor_pointer()
            .hover(|s| s.bg(alpha(hue, 0.08)))
            .on_click(cx.listener(move |me, _, _, cx| {
                me.pick = Some(i);
                me.open(p, cx);
            }))
            .on_hover(cx.listener(move |me, on: &bool, _, cx| {
                let want = on.then_some(p);
                if me.hover != want && (*on || me.hover == Some(p)) {
                    me.hover = want;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex_none()
                    .when(!compact, |d| d.mt(px(5.)))
                    .size(px(7.))
                    .rounded(px(7.))
                    .bg(hue),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .when(compact, |d| d.line_clamp(2).text_ellipsis())
                                    .when(!compact, |d| d.truncate())
                                    .text_size(px(search::TITLE))
                                    .text_color(if picked { t.text } else { alpha(t.text, 0.9) })
                                    .when(picked, |d| d.font_weight(FontWeight::BOLD))
                                    .child(page.title.clone()),
                            )
                            .when(!compact, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(search::SNIPPET))
                                        .text_color(alpha(hue, 0.85))
                                        .child(page.kind.clone()),
                                )
                            }),
                    )
                    .when(!compact, |d| {
                        d.child(
                            div()
                                .text_size(px(search::SNIPPET))
                                .line_height(px(search::SNIPPET * 1.45))
                                .text_color(t.dim)
                                .truncate()
                                .child(snippet),
                        )
                    }),
            )
    }

    /// The replay's button, or the replay itself: the day it stands on, the
    /// days worked on as a strip to pick from, and play and pause.
    /// How far from the window's right Replay ends: beside the sheet while
    /// a page is read.
    pub(super) fn replay_right(&self) -> f32 {
        if self.reading.is_some() {
            reader::sheet_width(self.size.0, self.reader_k()) + chrome::INSET
        } else {
            chrome::INSET
        }
    }

    /// Replay's button says `words`, or is left out when there is no room.
    pub(super) fn replay_bar(
        &self,
        t: &Theme,
        words: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let right = self.replay_right();
        let button = |id: &'static str, words: String| {
            div()
                .id(id)
                .flex_none()
                .px(px(chrome::SWITCH_PAD_X))
                .h(px(chrome::SWITCH_HEIGHT))
                .flex()
                .items_center()
                .rounded(px(chrome::RADIUS))
                .cursor_pointer()
                .text_size(px(chrome::TEXT))
                .text_color(t.text2)
                .hover(|s| s.text_color(t.text).bg(t.raised))
                .child(words)
        };
        let Some(state) = &self.replay else {
            let words = words?;
            return Some(
                div()
                    .absolute()
                    .right(px(right))
                    .bottom(px(chrome::INSET - 6.))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .occlude()
                    .child(
                        button("replay", words.into())
                            .when(words != REPLAY[0], |b| b.tooltip(kit::tip(REPLAY[0])))
                            .on_click(cx.listener(|me, _, _, cx| me.toggle_replay(cx))),
                    )
                    .into_any_element(),
            );
        };
        let close = button("end-replay", "×".into())
            .on_click(cx.listener(|me, _, _, cx| me.toggle_replay(cx)));
        let bar = div()
            .absolute()
            .left(px(chrome::INSET))
            .right(px(right))
            .bottom(px(chrome::INSET + chrome::FOOT))
            .p(px(chrome::INSET))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .occlude();
        let r = match state {
            Replaying::Reading => {
                return Some(
                    bar.child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .text_color(t.text2)
                            .child("Reading the brain's history…")
                            .child(close),
                    )
                    .into_any_element(),
                );
            }
            Replaying::Failed(e) => {
                return Some(
                    bar.child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .gap(px(12.))
                            .child(div().text_color(t.error).child(e.message.clone()))
                            .child(close),
                    )
                    .into_any_element(),
                );
            }
            Replaying::Ready(r) => r,
        };
        let Some(&(today, ref pages)) = r.days.get(r.at.floor() as usize) else {
            return Some(
                bar.child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .text_color(t.text2)
                        .child("The brain's history names none of its pages.")
                        .child(close),
                )
                .into_any_element(),
            );
        };
        let most = r.days.iter().map(|d| d.1.len()).max().unwrap_or(1).max(1) as f32;
        let now = r.at.floor() as usize;
        let mut strip = div()
            .h(px(tokens::replay::STRIP))
            .flex()
            .items_end()
            .gap(px(tokens::replay::GAP));
        for (i, (_, touched)) in r.days.iter().enumerate() {
            let share = (touched.len() as f32).ln_1p() / most.ln_1p();
            let color = match i.cmp(&now) {
                std::cmp::Ordering::Less => alpha(t.text2, 0.7),
                std::cmp::Ordering::Equal => t.accent,
                std::cmp::Ordering::Greater => alpha(t.dim, 0.5),
            };
            strip = strip.child(
                div()
                    .id(("day", i))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_end()
                    .cursor_pointer()
                    .on_click(cx.listener(move |me, _, _, cx| {
                        if let Some(r) = me.replay_mut() {
                            r.seek(i);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w_full()
                            .h(px((tokens::replay::STRIP * share).max(2.)))
                            .bg(color),
                    ),
            );
        }
        let first = r.days.first().map(|d| d.0).unwrap_or(today);
        let last = r.days.last().map(|d| d.0).unwrap_or(today);
        let play = button(
            "play",
            if r.playing.is_some() { "Pause" } else { "Play" }.into(),
        )
        .on_click(cx.listener(|me, _, _, cx| {
            if let Some(r) = me.replay_mut() {
                r.play_pause();
            }
            cx.notify();
        }));
        let mut note = format!("{} worked on", self::pages(pages.len()));
        if r.reshuffles > 0 {
            note.push_str(&format!(
                " · {} reorganisation{} left out",
                r.reshuffles,
                if r.reshuffles == 1 { "" } else { "s" }
            ));
        }
        Some(
            bar.child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(16.))
                    .child(
                        div()
                            .font_family(t.ty.display)
                            .font_weight(t.ty.display_weight)
                            .text_size(px(ty::GLANCE))
                            .line_height(px(ty::GLANCE * 1.1))
                            .text_color(t.text)
                            .child(date(today)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .pb(px(4.))
                            .text_size(px(chrome::TEXT))
                            .text_color(t.text2)
                            .child(note),
                    )
                    .child(play)
                    .child(close),
            )
            .child(strip)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(chrome::TEXT))
                    .text_color(t.dim)
                    .child(date(first))
                    .child(format!("{} days worked on", r.days.len()))
                    .child(date(last)),
            )
            .into_any_element(),
        )
    }

    /// In a page's orbit, its evidence: how much there is, folded out of
    /// the ring or shown in it, and the switch between.
    pub(super) fn evidence_chip(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let data = self.data.as_ref()?;
        let page = self.reading.filter(|_| self.orbiting())?;
        let n = data
            .corpus
            .neighbours(page)
            .into_iter()
            .filter(|&p| data.corpus.pages[p].is_evidence())
            .count();
        if n == 0 {
            return None;
        }
        let free = self.free();
        let (words, act) = if self.fold_evidence {
            (format!("{n} evidence folded away"), "Show")
        } else {
            (format!("{n} evidence in the ring"), "Fold")
        };
        Some(
            div()
                .id("evidence")
                .absolute()
                .left(px(free.x + chrome::INSET))
                .top(px(free.y))
                .h(px(chrome::SWITCH_HEIGHT))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(chrome::SWITCH_PAD_X))
                .rounded(px(chrome::RADIUS))
                .bg(veil(t, chrome::PANEL_ALPHA))
                .occlude()
                .cursor_pointer()
                .text_size(px(chrome::TEXT))
                .text_color(t.text2)
                .on_click(cx.listener(|me, _, _, cx| me.toggle_evidence(cx)))
                .child(words)
                .child(div().text_color(t.accent).child(act)),
        )
    }

    /// Whose brain is read, as the switch says it.
    pub(super) fn source_label(&self) -> String {
        match (&self.place.from, &self.away_from) {
            (Some(r), Some(_)) => format!("{}'s brain, as last copied", r.name),
            (Some(r), None) => format!("{}'s brain", r.name),
            (None, _) => "This machine's brain".to_string(),
        }
    }

    /// Whose brain is read, at the foot of the search column: this
    /// machine's, or one of the others Ahjo Swarm knows, in a list it opens.
    pub(super) fn source_switch(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.source_label();
        let tip = match &self.away_from {
            Some(e) => format!("{}. Read another machine's brain", e.message),
            None => "Read another machine's brain".to_string(),
        };
        div()
            .id("from")
            .debug_selector(|| "from".into())
            .relative()
            .occlude()
            .h(px(chrome::SWITCH_HEIGHT))
            .px(px(chrome::SWITCH_PAD_X))
            .flex()
            .items_center()
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .cursor_pointer()
            .text_color(t.text2)
            .hover(|s| s.text_color(t.text).bg(t.raised))
            .tooltip(kit::tip(tip))
            .child(pointer::watch())
            .when(self.away_from.is_some(), |d| {
                d.child(div().mr(px(8.)).size(px(7.)).rounded(px(7.)).bg(t.error))
            })
            .on_click(cx.listener(|me, _, window, cx| me.choose_from(window, cx)))
            .child(label)
    }

    /// Update, beside whose brain is read, when the node this app was
    /// installed from hands out another build: a pill as the switch is,
    /// its words in the action colour.
    pub(super) fn update_pill(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        use ahjo_app_kit::update::Tone;
        let badge = self.updates.read(cx).badge()?;
        let updates = self.updates.clone();
        Some(
            div()
                .id("update")
                .h(px(chrome::SWITCH_HEIGHT))
                .px(px(chrome::SWITCH_PAD_X))
                .flex()
                .items_center()
                .rounded(px(chrome::RADIUS))
                .bg(veil(t, chrome::PANEL_ALPHA))
                .occlude()
                .text_color(match badge.tone {
                    Tone::Action => t.accent,
                    Tone::Busy => t.dim,
                    Tone::Failed => t.error,
                })
                .tooltip(kit::tip(badge.tip))
                .when(badge.acts, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(t.raised))
                        .on_click(move |_, window, cx| {
                            updates.update(cx, |u, cx| u.act(window, cx))
                        })
                })
                .child(badge.words)
                .into_any_element(),
        )
    }

    /// The inbox's keys while its list has them: Up and Down stand on a
    /// row, Enter picks it. Escape is the frame's, closing the list.
    pub(super) fn inbox_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (key, m) = (e.keystroke.key.as_str(), &e.keystroke.modifiers);
        if m.control || m.alt || m.platform {
            return;
        }
        let rows = self.inbox_rows();
        let n = rows.len();
        match key {
            "down" if n > 0 => self.inbox = self.inbox.map(|i| (i + 1) % n),
            "up" if n > 0 => self.inbox = self.inbox.map(|i| (i + n - 1) % n),
            "enter" => {
                if let Some(row) = self.inbox.and_then(|i| rows.get(i).cloned()) {
                    self.pick_inbox(row, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The notes caught here, in the inbox's popup: those waiting in the
    /// inbox for an agent, and those agents filed, with where each went.
    pub(super) fn inbox_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.inbox_panel(&theme(cx).clone(), cx).into_any_element()
    }

    pub(super) fn inbox_panel(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let today = time::OffsetDateTime::now_utc().date().to_julian_day();
        let at = self.inbox.unwrap_or(0);
        let rows = self.inbox_rows();
        let waiting = rows
            .iter()
            .filter(|r| matches!(r, InboxRow::Waiting(_)))
            .count();
        let filed = rows.len() - waiting;
        let mut list = pane::scroll("inbox")
            .debug_selector(|| "inbox-list".into())
            .track_focus(&self.inbox_focus)
            .on_key_down(cx.listener(Self::inbox_key))
            .w(px(self.recent_width().min(search::INBOX_W)))
            .max_h(px(self.size.1 - chrome::INSET * 3. - search::FIELD_HEIGHT))
            .py(px(4.))
            .rounded(px(chrome::RADIUS))
            // A popup over the page read as well as the sky: its own ground.
            .bg(t.surface)
            .border_1()
            .border_color(t.divider)
            .on_scroll(|_, _, cx| cx.stop_propagation());
        let label = |words: String| {
            kit::label(t, words)
                .px(px(search::ROW_PAD_X))
                .pt(px(search::ROW_PAD_Y))
                .pb(px(2.))
        };
        let Some(data) = self.data.clone() else {
            return list;
        };
        let pending = self.place.from.is_none() && self.filed.is_none();
        if rows.is_empty() && !pending {
            return list.child(
                div()
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y * 2.))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.text2)
                    .child("No notes yet. Ctrl+N catches one in the inbox, and an agent files it into the brain."),
            );
        }
        let row = |i: usize, id: &'static str| {
            div()
                .id((id, i))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(search::ROW_PAD_X))
                .py(px(search::ROW_PAD_Y - 2.))
                .cursor_pointer()
                .when(i == at, |d| d.bg(alpha(t.accent, 0.14)))
                .hover(|s| s.bg(alpha(t.text, 0.06)))
        };
        let age = |words: String| {
            div()
                .flex_none()
                .text_size(px(search::SNIPPET))
                .text_color(t.dim)
                .child(words)
        };
        if waiting > 0 {
            list = list.child(label(format!("Waiting for an agent · {waiting}")));
        }
        for (i, r) in rows.iter().enumerate() {
            let pick = r.clone();
            let el = match r {
                InboxRow::Waiting(p) => {
                    let page = &data.corpus.pages[*p];
                    row(i, "waiting")
                        .child(div().flex_none().size(px(7.)).rounded(px(7.)).bg(t.dim))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(search::TITLE))
                                .text_color(t.text)
                                .child(page.title.clone()),
                        )
                        .child(age(caught(&page.origin, {
                            // A file's time says when it was fetched as
                            // much as written: the note's day says older.
                            match page.updated {
                                Some(day) if day < today => reader::age(day, today),
                                _ => reader::since(page.modified, now),
                            }
                        })))
                }
                InboxRow::Filed(f) => {
                    let named: Vec<(String, Hsla)> =
                        f.to.iter()
                            .filter_map(|to| data.corpus.by_path(to))
                            .map(|p| {
                                let page = &data.corpus.pages[p];
                                (page.title.clone(), data.hues[page.subject])
                            })
                            .collect();
                    let hue = named.first().map_or(t.dim, |n| n.1);
                    let went = match (f.moved, named.as_slice()) {
                        (_, []) if f.to.is_empty() => "Taken out of the inbox".to_string(),
                        (_, []) => "Filed in a page since gone".to_string(),
                        (true, [(title, _)]) if *title == f.title => {
                            let p = f.to.first().and_then(|to| data.corpus.by_path(to));
                            let subject = p
                                .map(|p| {
                                    data.corpus.subjects[data.corpus.pages[p].subject]
                                        .name
                                        .clone()
                                })
                                .unwrap_or_default();
                            format!("Filed in {subject}")
                        }
                        (true, [(title, _)]) => {
                            let p = f.to.first().and_then(|to| data.corpus.by_path(to));
                            let subject = p
                                .map(|p| {
                                    data.corpus.subjects[data.corpus.pages[p].subject]
                                        .name
                                        .clone()
                                })
                                .unwrap_or_default();
                            format!("Filed in {subject} as {title}")
                        }
                        (_, [(title, _)]) => format!("Folded into {title}"),
                        (_, [(title, _), rest @ ..]) => {
                            format!("Folded into {title} and {} more", rest.len())
                        }
                    };
                    row(i, "filed")
                        .child(div().flex_none().size(px(7.)).rounded(px(7.)).bg(hue))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(search::TITLE))
                                        .text_color(alpha(t.text, 0.9))
                                        .child(f.title.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(search::SNIPPET))
                                        .text_color(t.text2)
                                        .child(went),
                                ),
                        )
                        .child(age(caught(
                            &f.origin,
                            reader::since(f.when.max(0) as u64 * 1_000_000_000, now),
                        )))
                }
            };
            if i == waiting && filed > 0 {
                list = list.child(label("Filed by agents".into()).mt(px(search::ROW_PAD_Y)));
            }
            list = list.child(el.on_click(
                cx.listener(move |me, _, window, cx| me.pick_inbox(pick.clone(), window, cx)),
            ));
        }
        match (&self.place.from, &self.filed) {
            (Some(r), _) => {
                list = list.child(
                    div()
                        .px(px(search::ROW_PAD_X))
                        .py(px(search::ROW_PAD_Y))
                        .text_size(px(search::SNIPPET))
                        .text_color(t.dim)
                        .child(format!(
                            "Which notes agents filed shows on {}'s own machine.",
                            r.name
                        )),
                )
            }
            (None, Some(Err(e))) => {
                list = list.child(
                    div()
                        .px(px(search::ROW_PAD_X))
                        .py(px(search::ROW_PAD_Y))
                        .text_size(px(search::SNIPPET))
                        .text_color(t.dim)
                        .child(format!(
                            "Which notes agents filed did not read: {}",
                            e.message
                        )),
                )
            }
            _ => {}
        }
        list
    }

    /// The actions at the top right, beside the search and never over it:
    /// the inbox, a new note, the day's note and the theme, as chips side
    /// by side; folded into one chip that opens them as a menu when the sky
    /// beside the page read leaves too little room for them and the search.
    pub(super) fn actions_row(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let right = self.size.0 - self.sky_width() + chrome::INSET;
        let chip = |id: &'static str| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .relative()
                .h(px(chrome::SWITCH_HEIGHT))
                .flex()
                .items_center()
                .gap(px(chrome::CHIP_GAP))
                .px(px(chrome::SWITCH_PAD_X))
                .rounded(px(chrome::RADIUS))
                .bg(veil(t, chrome::PANEL_ALPHA))
                .cursor_pointer()
                .text_size(px(chrome::TEXT))
                .text_color(t.text2)
                .hover(|s| s.text_color(t.text).bg(t.raised))
                .child(pointer::watch())
        };
        let mark = || img(t.art.mark.clone()).size(px(chrome::MARK));
        let mut row = div()
            .id("actions")
            .debug_selector(|| "actions".into())
            .absolute()
            .top(px(
                chrome::INSET + (search::FIELD_HEIGHT - chrome::SWITCH_HEIGHT) / 2.
            ))
            .right(px(right))
            .flex()
            .gap(px(chrome::CHIP_GAP))
            .occlude();
        if self.actions_folded() {
            return row.child(
                chip("more")
                    .tooltip(kit::tip("The inbox, notes and the theme"))
                    .on_click(cx.listener(|me, _, window, cx| me.actions_menu(window, cx)))
                    .child(mark())
                    .child(MORE),
            );
        }
        let waiting = self.data.as_ref().map_or(0, |d| d.waiting.len());
        for action in self.actions() {
            row =
                row.child(match action {
                    Action::Inbox => chip("inbox")
                        .when(self.inbox_popup.is_some(), |c| {
                            c.text_color(t.text).bg(t.raised)
                        })
                        .tooltip(kit::tip(
                            "The notes waiting in the inbox, and where agents filed them (Ctrl+I)",
                        ))
                        .on_click(cx.listener(|me, _, window, cx| {
                            me.toggle_inbox(Spot::here(window, cx), cx)
                        }))
                        .child(action.words())
                        .when(waiting > 0, |c| {
                            c.child(div().text_color(t.dim).child(waiting.to_string()))
                        }),
                    Action::Note => chip("note")
                        .tooltip(kit::tip(
                            "A new note, filed in the inbox when you close it (Ctrl+N)",
                        ))
                        .on_click(cx.listener(|me, _, window, cx| me.catch_note(window, cx)))
                        .child(action.words()),
                    Action::Today => chip("today")
                        .tooltip(kit::tip("Today's note (Ctrl+D)"))
                        .on_click(cx.listener(|me, _, window, cx| me.day_note(window, cx)))
                        .child(action.words()),
                    Action::Theme => chip("theme")
                        .tooltip(kit::tip("The theme (Ctrl+T)"))
                        .on_click(cx.listener(|me, _, window, cx| {
                            me.pick_theme(Spot::here(window, cx), window, cx)
                        }))
                        .child(mark())
                        .child(t.direction.label()),
                });
        }
        row
    }

    /// The folded actions, as a menu beside their chip.
    pub(super) fn actions_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        let waiting = self.data.as_ref().map_or(0, |d| d.waiting.len());
        let rows = self
            .actions()
            .into_iter()
            .map(|action| {
                let me = me.clone();
                let run = move |window: &mut Window, cx: &mut App| {
                    let _ = me.update(cx, |me, cx| match action {
                        Action::Inbox => me.toggle_inbox(me.actions_spot(), cx),
                        Action::Note => me.catch_note(window, cx),
                        Action::Today => me.day_note(window, cx),
                        Action::Theme => me.pick_theme(me.actions_spot(), window, cx),
                    });
                };
                match action {
                    Action::Inbox if waiting > 0 => {
                        MenuRow::new(format!("Inbox · {waiting}"), run).chord("Ctrl+I")
                    }
                    Action::Inbox => MenuRow::new(action.words(), run).chord("Ctrl+I"),
                    Action::Note => MenuRow::new(action.words(), run).chord("Ctrl+N"),
                    Action::Today => MenuRow::new(action.words(), run).chord("Ctrl+D"),
                    Action::Theme => {
                        MenuRow::new(format!("Theme: {}", self.styled.label()), run).chord("Ctrl+T")
                    }
                }
            })
            .collect();
        let at = window.mouse_position();
        self.frame
            .update(cx, |f, cx| f.menu(OWNER, rows, at, false, cx));
    }

    pub(super) fn sheet(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let w = reader::sheet_width(self.size.0, self.reader_k());
        let back = !self.back.is_empty();
        // Back and close: drawn marks a pointer finds, on targets it can
        // hit. The font's arrow and cross draw at a third of its size.
        let (ink, raised) = (t.text2, t.raised);
        let glyph = move |id: &'static str, back: bool, tip: &'static str| {
            div()
                .id(id)
                .size(px(size::CONTROL))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(size::RADIUS))
                .cursor_pointer()
                .hover(move |s| s.bg(raised))
                .tooltip(kit::tip(tip))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |b: Bounds<Pixels>, _, window, _| {
                            let o = b.origin;
                            let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
                            let g = chrome::GLYPH;
                            let mut path = PathBuilder::stroke(px(chrome::GLYPH_LINE));
                            if back {
                                path.move_to(at(g, g / 2.));
                                path.line_to(at(0., g / 2.));
                                path.move_to(at(g / 2.2, g * 0.08));
                                path.line_to(at(0., g / 2.));
                                path.line_to(at(g / 2.2, g * 0.92));
                            } else {
                                path.move_to(at(0., 0.));
                                path.line_to(at(g, g));
                                path.move_to(at(g, 0.));
                                path.line_to(at(0., g));
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, ink);
                            }
                        },
                    )
                    .size(px(chrome::GLYPH)),
                )
        };
        let bar = div()
            .absolute()
            .top(px(chrome::SHEET_BAR))
            .right(px(chrome::SHEET_BAR))
            .flex()
            .gap(px(4.))
            .child(
                kit::button(t, "send", "Send", kit::Kind::Quiet)
                    .tooltip(kit::tip("Send it to a session, in Ahjo Swarm"))
                    .on_click(cx.listener(|me, _, _, cx| me.send_page(cx))),
            )
            .when(self.editing.is_none() && self.place.from.is_none(), |d| {
                d.child(
                    kit::button(t, "edit", "Edit", kit::Kind::Quiet)
                        .tooltip(kit::tip("Write this page (E)"))
                        .on_click(cx.listener(|me, _, window, cx| me.edit(window, cx))),
                )
            })
            .when(back, |d| {
                d.child(
                    glyph("back", true, "Back (Alt+Left)")
                        .on_click(cx.listener(|me, _, _, cx| me.go_back(cx))),
                )
            })
            .child(
                glyph("close", false, "Close the page (Esc)")
                    .on_click(cx.listener(|me, _, _, cx| me.close(cx))),
            );
        let me = cx.entity().downgrade();
        div()
            .absolute()
            .top(px(0.))
            .right(px(0.))
            .w(px(w))
            .h_full()
            .occlude()
            .bg(veil(t, tokens::reader::SHEET_ALPHA))
            .border_l_1()
            .border_color(alpha(t.divider, 0.7))
            .shadow(vec![gpui::BoxShadow {
                color: t.shadow(),
                offset: gpui::point(px(-8.), px(0.)),
                blur_radius: px(tokens::reader::SHADOW),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            // A click on the page is the page's: the sky under it does not
            // take it as a click on a star.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                AnyView::from(self.reader.clone()).cached(StyleRefinement::default().size_full()),
            )
            .child(zoom::catch(self.reader_wheel.clone(), move |n, _, cx| {
                let _ = me.update(cx, |me, cx| {
                    let z = if n > 0 { Zoom::In } else { Zoom::Out };
                    me.zoom_reader(z, n.abs(), cx)
                });
            }))
            .child(bar)
    }
}

impl BrainApp {
    /// A note over the sky at a reading measure; a click beside it closes
    /// it, filing what was written.
    pub(super) fn note_pane(
        &self,
        t: &Theme,
        ed: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let w = (self.size.0 - 2. * chrome::INSET).min(tokens::editor::NOTE_W);
        let h = (self.size.1 - 2. * chrome::INSET).min(tokens::editor::NOTE_H);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(veil(t, chrome::PANEL_ALPHA))
            .occlude()
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|me, _, window, cx| {
                    cx.stop_propagation();
                    me.leave_note(window, cx);
                }),
            )
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(w))
                    .h(px(h))
                    .rounded(px(chrome::RADIUS))
                    .border_1()
                    .border_color(t.divider)
                    .overflow_hidden()
                    .shadow(vec![gpui::BoxShadow {
                        color: t.shadow(),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(40.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(ed.clone()),
            )
    }

    /// The page's source, left of the page it makes; the whole window when
    /// the page leaves too little beside it.
    pub(super) fn source_pane(&self, ed: &Entity<Editor>) -> impl IntoElement {
        let sheet = reader::sheet_width(self.size.0, self.reader_k());
        let room = self.size.0 - sheet;
        let w = if room >= tokens::editor::LEAST {
            room
        } else {
            self.size.0
        };
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .w(px(w))
            .h_full()
            .occlude()
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(ed.clone())
    }
}
