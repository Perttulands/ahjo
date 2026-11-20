//! The panel beside the board where the operator works with the mission's
//! agents. Build: with the agent that crafts and edits the mission, who
//! answers with proposals the board previews until they are accepted.
//! Run: with the agents in the steps' seats, who leave notes on their
//! steps. Steps: the mission in words. Build is laid out as the look
//! chosen (`look::panel`): a conversation (`chat`) or the mission's page
//! (`notepad`).
use super::*;
use crate::look;
use ahjo_app_kit::theme::{Theme, alpha};
use ahjo_app_kit::widgets::pane;
use ahjo_missions_contract::{
    MISSION_THREAD, Mission,
    document::{Note, NotesDocument},
};
use gpui::{AnyElement, Hsla, SharedString};

const GAP: f32 = 8.;
const BUBBLE_R: f32 = 10.;
const SMALL: f32 = 11.5;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SideTab {
    #[default]
    Build,
    Run,
    Steps,
}

/// A change the building agent proposes: its note in the mission's thread
/// that starts "Proposed:", its first line the summary and each further
/// line a card it adds (+), changes (~) or removes (-), by title. Pending
/// while it is the thread's latest word.
pub(super) struct Proposal {
    pub(super) entry: String,
    pub(super) summary: String,
    pub(super) changes: Vec<(char, Option<String>, String)>,
}

pub(super) fn proposal(notes: &NotesDocument, m: &Mission) -> Option<Proposal> {
    let last = notes.mission.last()?;
    agent(&last.author)?;
    parse_proposal(last, m)
}

fn parse_proposal(n: &Note, m: &Mission) -> Option<Proposal> {
    let text = n.text.strip_prefix("Proposed:")?;
    let mut lines = text.lines();
    let summary = lines.next().unwrap_or_default().trim().to_owned();
    let id_of = |title: &str| {
        m.formations
            .iter()
            .map(|f| (&f.id, &f.title))
            .chain(m.gates.iter().map(|g| (&g.id, &g.title)))
            .chain(m.ends.iter().map(|e| (&e.id, &e.title)))
            .find(|(_, t)| t.eq_ignore_ascii_case(title))
            .map(|(id, _)| id.clone())
    };
    let changes = lines
        .filter_map(|l| {
            let l = l.trim();
            let sign = l.chars().next().filter(|c| "+~-".contains(*c))?;
            let title = l[1..].trim().to_owned();
            Some((sign, id_of(&title), title))
        })
        .collect();
    Some(Proposal {
        entry: n.id.clone(),
        summary,
        changes,
    })
}

fn agent(author: &str) -> Option<&str> {
    author.strip_prefix("agent:")
}
fn who(author: &str) -> String {
    match agent(author) {
        Some(name) => name.to_owned(),
        None => "You".to_owned(),
    }
}
fn when(note: &Note) -> String {
    crate::store::words::short_time(&note.created_at, time::OffsetDateTime::now_utc())
}

/// Every thread on the mission and its cards, in run order: the mission's
/// own first.
fn threads<'a>(notes: &'a NotesDocument, m: &Mission) -> Vec<(String, &'a [Note])> {
    let flow = ahjo_missions_contract::flow::build(m);
    let mut out: Vec<(String, &[Note])> = vec![(MISSION_THREAD.into(), &notes.mission)];
    let mut cards: Vec<_> = notes
        .elements
        .iter()
        .filter(|t| !t.entries.is_empty())
        .map(|t| (t.node_id.clone(), t.entries.as_slice()))
        .collect();
    cards.sort_by_key(|(id, _)| {
        let input = m.input_cards.iter().any(|c| c.id == *id);
        (
            if input { 0 } else { 1 },
            flow.numbers.get(id).copied().unwrap_or(usize::MAX),
        )
    });
    out.extend(cards);
    out
}

/// Who sits in a step's seats, by their agents' names.
fn seat_agents(f: &ahjo_missions_contract::Formation) -> String {
    let names: Vec<String> = f
        .slots
        .iter()
        .filter(|s| !s.harness.trim().is_empty())
        .map(|s| crate::store::words::slot_label(s).to_owned())
        .collect();
    match names.len() {
        0 => "no agent yet".to_owned(),
        1 => "its agent".to_owned(),
        n => format!("{n} agents"),
    }
}

fn resolved(entries: &[Note]) -> bool {
    entries.last().is_some_and(|n| n.resolves)
}

impl MissionsPage {
    /// The open asks on cards: threads whose latest word does not close
    /// them.
    pub(super) fn open_asks(&self, m: &Mission) -> usize {
        self.notes.as_ref().map_or(0, |n| {
            threads(n, m)
                .iter()
                .filter(|(id, e)| id != MISSION_THREAD && !e.is_empty() && !resolved(e))
                .count()
        })
    }

    /// Where the composer's words go: in Build, the mission's thread, read
    /// by the building agent; in Run, the step chosen.
    fn talk_target(&self) -> String {
        match self.side_tab {
            SideTab::Run => self.talk_to.clone().unwrap_or_default(),
            _ => MISSION_THREAD.into(),
        }
    }

    /// The cards a pending proposal adds or changes, for the board to mark.
    pub(super) fn proposed_cards(&self, m: &Mission) -> Vec<(String, char)> {
        self.notes
            .as_ref()
            .and_then(|n| proposal(n, m))
            .map(|p| {
                p.changes
                    .into_iter()
                    .filter_map(|(sign, id, _)| id.map(|id| (id, sign)))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn talk_choose(&mut self, target: Option<String>, cx: &mut Context<Self>) {
        self.talk_to = target.clone();
        if let Some(id) = &target {
            self.flow_focus = Some(id.clone());
        }
        // The composer writes in this thread, here and not on the board.
        if !self.note_saving && self.note.read(cx).text == self.note_base {
            self.note_paper.open = Some(self.talk_target());
            self.note_paper.in_flow = true;
            self.note_id = None;
            self.note_etag = self.notes.as_ref().map(|n| n.etag.clone());
        }
        cx.notify();
    }

    fn talk_send(&mut self, cx: &mut Context<Self>) {
        if self.talk_target().is_empty() {
            self.state.error = Some("Choose the step whose agents it is for.".into());
            cx.notify();
            return;
        }
        self.note_paper.open = Some(self.talk_target());
        self.note_paper.in_flow = true;
        self.add_note(false, cx);
    }

    /// Close a card's thread: resolved by the operator, with what the
    /// composer holds or a plain word.
    pub(super) fn resolve_thread(&mut self, target: String, entry: String, cx: &mut Context<Self>) {
        self.close_thread(target, entry, "Resolved.", cx)
    }
    pub(super) fn close_thread(
        &mut self,
        target: String,
        entry: String,
        words: &str,
        cx: &mut Context<Self>,
    ) {
        let (Some((node, slug)), Some(notes)) = (self.state.selected.clone(), self.notes.as_ref())
        else {
            return;
        };
        let request = Request::new(
            "PATCH",
            NodeId(node),
            format!("{}/notes", mission_path(&slug)),
        )
        .header("If-Match", notes.etag.clone())
        .json(&json!({"target": target, "text": words, "author": "human:ui", "action": "resolve", "entryId": entry}));
        let asked = self.host.request(request, cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if generation != page.generation {
                    return;
                }
                match result {
                    Ok(a) if a.ok() => {
                        match serde_json::from_slice::<ahjo_missions_contract::NotesResponse>(
                            &a.body,
                        ) {
                            Ok(n) => page.notes = Some(n.notes),
                            Err(e) => page.state.error = Some(e.to_string()),
                        }
                    }
                    Ok(a) => page.state.error = Some(a.error_text()),
                    Err(e) => page.state.error = Some(format!("{}: {}", e.code, e.message)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The panel's two tabs, and what the chosen one holds.
    pub(super) fn side_panel(&self, t: &Theme, m: &Mission, cx: &mut Context<Self>) -> Div {
        let open = self.open_asks(m);
        // A tab's count is a small badge beside its word, so the three
        // tabs and the close button fit the panel's width.
        let tab = |id: &'static str, label: &'static str, count: usize, hot: Hsla, tab: SideTab| {
            let on = self.side_tab == tab;
            div()
                .id(id)
                .debug_selector(move || id.into())
                .flex()
                .items_center()
                .gap(px(GAP * 0.75))
                .px(px(GAP * 1.25))
                .py(px(GAP))
                .border_b_2()
                .border_color(if on {
                    t.accent
                } else {
                    gpui::transparent_black()
                })
                .text_color(if on { t.text } else { t.text2 })
                .font_weight(if on {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
                .cursor_pointer()
                .child(label)
                .when(count > 0, |d| {
                    d.child(
                        div()
                            .px(px(5.))
                            .rounded(px(8.))
                            .bg(hot)
                            .text_color(t.bg)
                            .text_size(px(size::SMALL))
                            .font_weight(FontWeight::BOLD)
                            .child(count.to_string()),
                    )
                })
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.side_tab = tab;
                    cx.notify();
                }))
        };
        let pending = self.notes.as_ref().and_then(|n| proposal(n, m)).is_some();
        let tabs = div()
            .flex_none()
            .flex()
            .items_end()
            .gap(px(GAP * 0.5))
            .px(px(size::PAD))
            .pt(px(GAP))
            .border_b_1()
            .border_color(t.divider)
            .child(tab(
                "side-build",
                "Build",
                usize::from(pending),
                t.accent,
                SideTab::Build,
            ))
            .child(tab("side-run", "Run", open, t.needs, SideTab::Run))
            .child(tab("side-steps", "Steps", 0, t.accent, SideTab::Steps))
            .child(div().flex_1())
            .child(
                pane::act(t, "side-close", "×")
                    .tooltip(kit::tip("Put the panel away"))
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.flow_open = Some(false);
                        p.canvas.room_changed();
                        cx.notify();
                    })),
            );
        let body = match self.side_tab {
            SideTab::Steps => div()
                .id("side-steps-body")
                .track_scroll(&self.flow_scroll)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    div()
                        .p(px(size::PAD))
                        .flex()
                        .flex_col()
                        .gap(px(size::SECTION_GAP))
                        .child(self.mission_flow(t, m, cx)),
                ),
            SideTab::Build | SideTab::Run => {
                let content = match (self.side_tab, look::panel()) {
                    (SideTab::Build, look::Panel::Notepad) => self.talk_notepad(t, m, cx),
                    (SideTab::Build, _) => self.talk_chat(t, m, false, cx),
                    _ => self.talk_chat(t, m, true, cx),
                };
                div()
                    .id("side-talk-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content)
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(tabs)
            .child(body)
            .when(self.side_tab != SideTab::Steps, |d| {
                d.child(self.talk_composer(t, m, cx))
            })
    }

    /// A conversation: every note on the mission and its cards in the order
    /// they were written, each card's beside a chip naming the card.
    fn talk_chat(&self, t: &Theme, m: &Mission, run: bool, cx: &mut Context<Self>) -> Div {
        let Some(notes) = self.notes.as_ref() else {
            return div()
                .p(px(size::PAD))
                .text_color(t.dim)
                .child("Reading notes…");
        };
        let mut all: Vec<(String, &Note, bool, bool)> = vec![];
        for (target, entries) in threads(notes, m) {
            // Build is the mission's own thread; Run the steps' threads.
            if (target == MISSION_THREAD) == run {
                continue;
            }
            let done = resolved(entries);
            for (i, n) in entries.iter().enumerate() {
                all.push((target.clone(), n, done, i + 1 == entries.len()));
            }
        }
        all.sort_by(|a, b| a.1.created_at.cmp(&b.1.created_at));
        let mut list = div().p(px(size::PAD)).flex().flex_col().gap(px(GAP * 1.5));
        if all.is_empty() {
            list = list.child(div().text_color(t.text2).child(if run {
                "Agents in the steps' seats leave notes here while a run works."
            } else {
                "Say what you are making; the building agent proposes the steps."
            }));
        }
        for (target, n, done, last) in all {
            let mine = agent(&n.author).is_none();
            let on = (target != MISSION_THREAD).then(|| m.node_name(&target).to_owned());
            let chip = on.map(|name| {
                let id = target.clone();
                div()
                    .id(SharedString::from(format!("talk-on-{}", n.id)))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(t.raised)
                    .border_1()
                    .border_color(t.divider)
                    .text_size(px(SMALL))
                    .text_color(t.text2)
                    .cursor_pointer()
                    .hover(|d| d.text_color(t.text))
                    .child(format!("on {name}"))
                    .tooltip(kit::tip("Show it on the board"))
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.talk_choose(Some(id.clone()), cx);
                        if let Some(m) = p.state.mission.clone() {
                            let board = p.shown_board(&m, false);
                            if let Some(c) = board.cards.iter().find(|c| c.id == id) {
                                p.canvas.show((c.x, c.y, c.w, c.h));
                            }
                        }
                    }))
            });
            let status = (last && target != MISSION_THREAD).then(|| {
                let (word, colour) = if done {
                    ("✓ resolved", look::ink(t).pass)
                } else {
                    ("open", t.needs)
                };
                div().text_size(px(SMALL)).text_color(colour).child(word)
            });
            let resolve = (last && !done && target != MISSION_THREAD).then(|| {
                let (target, entry) = (target.clone(), n.id.clone());
                pane::act(
                    t,
                    SharedString::from(format!("talk-resolve-{}", n.id)),
                    "Resolve",
                )
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.resolve_thread(target.clone(), entry.clone(), cx)
                }))
            });
            let head = div()
                .flex()
                .items_center()
                .gap(px(GAP))
                .text_size(px(SMALL))
                .when(mine, |d| d.flex_row_reverse())
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if mine { t.text } else { t.accent })
                        .child(who(&n.author)),
                )
                .child(div().text_color(t.dim).child(when(n)))
                .children(chip);
            let bubble = div()
                .max_w(px(300.))
                .px(px(GAP * 1.5))
                .py(px(GAP))
                .rounded(px(BUBBLE_R))
                .when(mine, |d| {
                    d.bg(alpha(t.accent, if t.light { 0.12 } else { 0.18 }))
                        .rounded_br(px(3.))
                })
                .when(!mine, |d| {
                    d.bg(t.raised)
                        .border_1()
                        .border_color(t.divider)
                        .rounded_bl(px(3.))
                })
                .when(n.resolves, |d| d.opacity(0.75))
                .text_color(t.text)
                .child(n.text.clone());
            let proposed = (!run && !mine).then(|| parse_proposal(n, m)).flatten();
            let bubble = match proposed {
                Some(p) => self.proposal_card(t, &p, last && !done, cx),
                None => bubble,
            };
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .when(mine, |d| d.items_end())
                    .when(!mine, |d| d.items_start())
                    .child(head)
                    .child(bubble)
                    .when(status.is_some() || resolve.is_some(), |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(GAP))
                                .children(status)
                                .children(resolve),
                        )
                    }),
            );
        }
        list
    }

    /// A proposal as the building agent puts it: what it changes, card by
    /// card, marked on the board too, and while pending, Accept or ask for
    /// something else.
    fn proposal_card(&self, t: &Theme, p: &Proposal, pending: bool, cx: &mut Context<Self>) -> Div {
        let ink = look::ink(t);
        let mut card = div()
            .w(px(312.))
            .flex()
            .flex_col()
            .rounded(px(BUBBLE_R))
            .rounded_bl(px(3.))
            .border_1()
            .border_color(if pending {
                alpha(t.accent, 0.7)
            } else {
                t.divider
            })
            .bg(t.raised)
            .overflow_hidden()
            .child(
                div()
                    .px(px(GAP * 1.5))
                    .py(px(GAP))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(SMALL))
                            .font_weight(FontWeight::BOLD)
                            .text_color(if pending { t.accent } else { t.dim })
                            .child(if pending {
                                "PROPOSED CHANGE · SHOWN ON THE BOARD"
                            } else {
                                "PROPOSED CHANGE"
                            }),
                    )
                    .child(div().text_color(t.text).child(p.summary.clone())),
            );
        let mut changes = div()
            .px(px(GAP * 1.5))
            .pb(px(GAP))
            .flex()
            .flex_col()
            .gap(px(2.));
        for (sign, _, title) in &p.changes {
            let (word, colour) = match sign {
                '+' => ("adds", ink.pass),
                '-' => ("removes", ink.fail),
                _ => ("changes", t.needs),
            };
            changes = changes.child(
                div()
                    .flex()
                    .gap(px(GAP))
                    .text_size(px(SMALL))
                    .child(
                        div()
                            .w(px(84.))
                            .flex_none()
                            .font_weight(FontWeight::BOLD)
                            .text_color(colour)
                            .child(format!("{sign} {word}")),
                    )
                    .child(div().text_color(t.text).child(title.clone())),
            );
        }
        card = card.child(changes);
        if pending {
            let entry = p.entry.clone();
            card = card.child(
                div()
                    .px(px(GAP * 1.5))
                    .py(px(GAP))
                    .flex()
                    .gap(px(GAP))
                    .border_t_1()
                    .border_color(t.divider)
                    .bg(t.surface)
                    .child(
                        kit::button(t, "proposal-accept", "Accept", Kind::Primary)
                            .debug_selector(|| "proposal-accept".into())
                            .on_click(cx.listener(move |p, _, _, cx| {
                                p.close_thread(
                                    MISSION_THREAD.into(),
                                    entry.clone(),
                                    "Accepted.",
                                    cx,
                                )
                            })),
                    )
                    .child(
                        kit::button(t, "proposal-change", "Ask for changes", Kind::Quiet).on_click(
                            cx.listener(|p, _, window, cx| {
                                p.talk_choose(None, cx);
                                ahjo_app_kit::widgets::field::focus(&p.note, window, cx);
                            }),
                        ),
                    ),
            );
        }
        card
    }

    /// The mission's page: what it is for, then the open asks by card in
    /// run order, each to resolve, and what is resolved under them.
    fn talk_notepad(&self, t: &Theme, m: &Mission, cx: &mut Context<Self>) -> Div {
        let Some(notes) = self.notes.as_ref() else {
            return div()
                .p(px(size::PAD))
                .text_color(t.dim)
                .child("Reading notes…");
        };
        let all = threads(notes, m);
        let heading = |text: &str| {
            div()
                .text_size(px(SMALL))
                .font_weight(FontWeight::BOLD)
                .text_color(t.dim)
                .child(text.to_uppercase())
        };
        let mut page = div()
            .p(px(size::PAD))
            .flex()
            .flex_col()
            .gap(px(size::SECTION_GAP));
        let intent = notes
            .mission
            .iter()
            .filter(|n| !n.resolves && agent(&n.author).is_none());
        let mut about = div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(heading("What this mission is for"));
        let mut any = false;
        for n in intent {
            any = true;
            about = about.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(22.))
                            .text_color(t.text)
                            .child(n.text.clone()),
                    )
                    .child(div().text_size(px(SMALL)).text_color(t.dim).child(format!(
                        "{} · {}",
                        who(&n.author),
                        when(n)
                    ))),
            );
        }
        if !any {
            about = about.child(
                div()
                    .text_color(t.text2)
                    .child("Write what you are making, as roughly as you like."),
            );
        }
        page = page.child(about);
        if let Some(p) = proposal(notes, m) {
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .child(heading("The building agent proposes"))
                    .child(self.proposal_card(t, &p, true, cx)),
            );
        }
        let open: Vec<_> = all
            .iter()
            .filter(|(id, e)| id != MISSION_THREAD && !resolved(e))
            .collect();
        let done: Vec<_> = all
            .iter()
            .filter(|(id, e)| id != MISSION_THREAD && resolved(e))
            .collect();
        let mut asks = div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(heading(&format!("Open asks on steps · {}", open.len())));
        for (target, entries) in &open {
            let first = &entries[0];
            let latest = entries.last().unwrap_or(first);
            let (target_c, entry) = (target.clone(), latest.id.clone());
            let focus = target.clone();
            asks = asks.child(
                div()
                    .id(SharedString::from(format!("notepad-ask-{target}")))
                    .flex()
                    .gap(px(GAP))
                    .p(px(GAP))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.divider)
                    .bg(t.bg)
                    .cursor_pointer()
                    .hover(|d| d.border_color(t.accent))
                    .on_click(
                        cx.listener(move |p, _, _, cx| p.talk_choose(Some(focus.clone()), cx)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("notepad-tick-{target}")))
                            .mt(px(2.))
                            .size(px(16.))
                            .flex_none()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(t.needs)
                            .cursor_pointer()
                            .hover(|d| d.bg(alpha(t.needs, 0.2)))
                            .tooltip(kit::tip("Resolve"))
                            .on_click(cx.listener(move |p, _, _, cx| {
                                cx.stop_propagation();
                                p.resolve_thread(target_c.clone(), entry.clone(), cx)
                            })),
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
                                    .text_size(px(SMALL))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(t.text2)
                                    .child(m.node_name(target).to_uppercase()),
                            )
                            .child(div().text_color(t.text).child(first.text.clone()))
                            .when(entries.len() > 1, |d| {
                                d.child(
                                    div()
                                        .text_size(px(SMALL))
                                        .text_color(t.accent)
                                        .child(format!("{}: {}", who(&latest.author), latest.text)),
                                )
                            }),
                    ),
            );
        }
        if open.is_empty() {
            asks = asks.child(div().text_color(t.text2).child("Nothing open."));
        }
        page = page.child(asks);
        if !done.is_empty() {
            let mut list = div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(heading(&format!("Resolved · {}", done.len())));
            for (target, entries) in done {
                let first = &entries[0];
                list = list.child(
                    div()
                        .flex()
                        .gap(px(GAP))
                        .text_color(t.dim)
                        .child(div().text_color(look::ink(t).pass).child("✓"))
                        .child(div().min_w_0().truncate().child(format!(
                            "{}: {}",
                            m.node_name(target),
                            first.text
                        ))),
                );
            }
            page = page.child(list);
        }
        page
    }

    /// Who the words go to, the field and Send.
    fn talk_composer(&self, t: &Theme, m: &Mission, cx: &mut Context<Self>) -> Div {
        let target = self.talk_target();
        let on_board = self.note_paper.open.is_some() && !self.note_paper.in_flow;
        let run = self.side_tab == SideTab::Run;
        let notepad = look::panel() == look::Panel::Notepad;
        let to = if run {
            match m.formations.iter().find(|f| f.id == target) {
                Some(f) => format!("{} · {}", f.title, seat_agents(f)),
                None => "choose a step".to_owned(),
            }
        } else {
            "the mission's building agent".to_owned()
        };
        // In Run the words go to a step's agents: any step.
        let flow = ahjo_missions_contract::flow::build(m);
        let mut steps: Vec<(usize, String)> = flow
            .numbers
            .iter()
            .filter(|(id, _)| m.formations.iter().any(|f| f.id == **id))
            .map(|(id, n)| (*n, id.clone()))
            .collect();
        steps.sort();
        let choices: Vec<(Option<String>, String)> = steps
            .into_iter()
            .filter_map(|(_, id)| {
                let f = m.formations.iter().find(|f| f.id == id)?;
                Some((Some(id), format!("{} · {}", f.title, seat_agents(f))))
            })
            .collect();
        let recipient = div()
            .id("talk-to")
            .debug_selector(|| "talk-to".into())
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(GAP))
            .py(px(2.))
            .rounded(px(12.))
            .bg(alpha(t.accent, 0.14))
            .text_color(t.accent)
            .text_size(px(SMALL))
            .font_weight(FontWeight::BOLD)
            .when(run, |d| d.cursor_pointer())
            .child(if run { format!("{to} ▾") } else { to })
            .when(run, |d| {
                d.on_click(cx.listener(move |p, e: &gpui::ClickEvent, _, cx| {
                    let page = cx.entity().downgrade();
                    let rows = choices
                        .iter()
                        .map(|(id, name)| {
                            let (page, id) = (page.clone(), id.clone());
                            ahjo_app_kit::menu::MenuRow::new(name.clone(), move |_, cx| {
                                let _ = page.update(cx, |p, cx| p.talk_choose(id.clone(), cx));
                            })
                        })
                        .collect();
                    p.host.menu(rows, e.position(), cx);
                }))
            });
        let reader = if run {
            "Lands on the step's thread, on the board too, for the agents in its seats."
        } else {
            "It answers here with a change; the board shows it until you accept."
        };
        div()
            .flex_none()
            .p(px(size::PAD))
            .border_t_1()
            .border_color(t.divider)
            .bg(t.bg)
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .text_size(px(SMALL))
                    .text_color(t.text2)
                    .child(match (run, notepad) {
                        (false, true) => "Ask",
                        _ => "To",
                    })
                    .child(recipient),
            )
            .child(if on_board {
                div()
                    .text_color(t.dim)
                    .child("Writing in a thread on the board…")
            } else {
                div()
                    .debug_selector(|| "talk-field".into())
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.divider)
                    .bg(t.surface)
                    .child(self.note.clone())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|p, _, _, cx| {
                            if p.note_paper.open.is_none() {
                                let target = p.talk_to.clone();
                                p.talk_choose(target, cx);
                            }
                        }),
                    )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(SMALL))
                            .text_color(t.dim)
                            .child(reader),
                    )
                    .child(
                        kit::button(
                            t,
                            "talk-send",
                            if self.note_saving {
                                "Sending…"
                            } else {
                                "Send"
                            },
                            Kind::Primary,
                        )
                        .flex_none()
                        .debug_selector(|| "talk-send".into())
                        .when(!self.note_saving && !on_board, |d| {
                            d.on_click(cx.listener(|p, _, _, cx| p.talk_send(cx)))
                        }),
                    ),
            )
    }

    /// The panel put away: an invitation at the board's edge to open it,
    /// with what is waiting there.
    pub(super) fn talk_pill(&self, t: &Theme, m: &Mission, cx: &mut Context<Self>) -> AnyElement {
        let open = self.open_asks(m);
        let pending = self.notes.as_ref().and_then(|n| proposal(n, m)).is_some();
        let words = match look::panel() {
            look::Panel::Chat => "Build with an agent",
            look::Panel::Notepad => "Mission notepad",
        };
        div()
            .id("talk-pill")
            .debug_selector(|| "talk-pill".into())
            .absolute()
            .top(px(size::PAD))
            .right(px(size::PAD))
            .flex()
            .items_center()
            .gap(px(GAP))
            .px(px(GAP * 1.5))
            .py(px(GAP))
            .rounded(px(18.))
            .bg(t.surface)
            .border_1()
            .border_color(alpha(t.accent, 0.6))
            .shadow_md()
            .cursor_pointer()
            .hover(|d| d.bg(t.raised))
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(if pending || open > 0 {
                        t.needs
                    } else {
                        t.accent
                    }),
            )
            .child(div().text_color(t.text).child(words))
            .when(pending || open > 0, |d| {
                d.child(
                    div()
                        .px(px(6.))
                        .rounded(px(8.))
                        .bg(alpha(t.needs, 0.2))
                        .text_size(px(SMALL))
                        .text_color(t.needs)
                        .child(if pending {
                            "1 proposal".to_owned()
                        } else {
                            format!("{open} open")
                        }),
                )
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|p, _, _, cx| {
                p.flow_open = Some(true);
                p.side_tab = SideTab::Build;
                p.canvas.room_changed();
                if p.unsaved(cx).is_none() {
                    p.editor = None;
                }
                cx.notify();
            }))
            .into_any_element()
    }
}
