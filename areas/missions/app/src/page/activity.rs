//! Activity, the panel beside a mission's board: what the mission is for,
//! then who changed what and when and who said what, newest first, as
//! Linear's activity and a GitHub pull request's timeline read. A change
//! marks the cards it touched.
use super::*;
use crate::store::history::{self, Sign};
use ahjo_app_kit::theme::Theme;
use ahjo_missions_contract::{MISSION_THREAD, Mission, document::Note};
use gpui::{AnyElement, SharedString};

/// Where a change's "and N more" starts: past the +/− column.
const PAST_SIGN: f32 = 14.;
/// The lines a change shows; the rest are counted.
const LINES: usize = 6;

#[derive(Default)]
pub(super) struct ActivityUi {
    pub(super) revisions: Option<Vec<Mission>>,
    loading: bool,
    error: Option<String>,
    /// The change whose cards are marked on the board.
    pub(super) chosen: Option<i64>,
}

/// Who wrote or changed something, as the panel names them.
fn who(author: &str) -> String {
    match author.split_once(':') {
        Some(("human", "ui")) => "You".into(),
        Some((_, name)) if !name.is_empty() => name.replace(['-', '_'], " "),
        _ if author.is_empty() => "Someone".into(),
        _ => author.into(),
    }
}

enum Entry<'a> {
    /// A note, and for one on a card or a wire, its thread and what it
    /// is on.
    Note {
        note: &'a Note,
        on: Option<(String, String)>,
    },
    Change(history::Change),
}

impl Entry<'_> {
    fn at(&self) -> String {
        match self {
            Entry::Note { note, .. } => note.created_at.clone(),
            Entry::Change(c) => c.at.clone(),
        }
    }
}

impl MissionsPage {
    pub(super) fn load_history(&mut self, node: String, slug: String, cx: &mut Context<Self>) {
        if self.activity.loading {
            return;
        }
        self.activity.loading = true;
        let generation = self.generation;
        let asked = self.host.call(
            NodeId(node.clone()),
            ahjo_missions_contract::commands::MissionHistory {
                mission: slug.clone(),
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation
                    || page.state.selected.as_ref() != Some(&(node, slug))
                {
                    return;
                }
                page.activity.loading = false;
                match result {
                    Ok(v) => {
                        page.activity.revisions = Some(v.revisions);
                        page.activity.error = None;
                    }
                    Err(e) => page.activity.error = Some(format!("{}: {}", e.code, e.message)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The panel: what the mission is for, a field to write in, and the
    /// timeline.
    pub(super) fn mission_activity(
        &mut self,
        t: &Theme,
        mission: &Mission,
        cx: &mut Context<Self>,
    ) -> Div {
        // A save since the history was read: read it again.
        let behind = self
            .activity
            .revisions
            .as_ref()
            .is_some_and(|r| r.last().is_none_or(|m| m.rev < mission.rev));
        if behind
            && !self.activity.loading
            && let Some((node, slug)) = self.state.selected.clone()
        {
            self.load_history(node, slug, cx);
        }
        let purpose = mission
            .input_cards
            .iter()
            .map(|c| c.goal.trim())
            .find(|g| !g.is_empty());
        let mut body = div().flex().flex_col().gap(px(size::SECTION_GAP));
        if let Some(purpose) = purpose {
            body = body.child(
                div()
                    .debug_selector(|| "activity-purpose".into())
                    .text_color(t.text)
                    .child(purpose.to_owned()),
            );
        }
        body = body.child(self.activity_field(t, cx));
        // Its runs, newest first, ahead of the edits: what it did before who
        // changed it.
        body = body.children(self.mission_runs(t, cx));
        let mut entries: Vec<Entry> = vec![];
        if let Some(notes) = self.notes.as_ref() {
            entries.extend(
                notes
                    .mission
                    .iter()
                    .map(|note| Entry::Note { note, on: None }),
            );
            for thread in &notes.elements {
                let id = &thread.card_id;
                // A wire is named by the cards it joins.
                let on = match mission.connections.iter().find(|c| c.id == *id) {
                    Some(c) => {
                        let node = |end: &str| {
                            let card = end.split_once(':').map_or(end, |(card, _)| card);
                            mission.card_name(card).to_owned()
                        };
                        format!("{} → {}", node(&c.from), node(&c.to))
                    }
                    None => mission.card_name(id).to_owned(),
                };
                entries.extend(thread.entries.iter().map(|note| Entry::Note {
                    note,
                    on: Some((id.clone(), on.clone())),
                }));
            }
        }
        if let Some(revisions) = &self.activity.revisions {
            entries.extend(history::changes(revisions).into_iter().map(Entry::Change));
        }
        entries.sort_by_key(|e| std::cmp::Reverse(crate::store::words::parse_time(&e.at())));
        let now = time::OffsetDateTime::now_utc();
        let mut list = div().flex().flex_col().gap(px(size::PAD));
        for entry in &entries {
            list = list.child(self.activity_entry(t, entry, now, cx));
        }
        if let Some(error) = &self.activity.error {
            list = list.child(
                div()
                    .text_color(t.error)
                    .child(format!("Reading its history failed: {error}")),
            );
        }
        body.child(list)
    }

    /// Write in the mission's own thread; the field opens in place.
    fn activity_field(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if self.note_paper.open.as_deref() == Some(MISSION_THREAD) && self.note_paper.in_flow {
            let first = self.notes.as_ref().is_none_or(|n| n.mission.is_empty());
            return self
                .note_field(t, MISSION_THREAD, first, cx)
                .into_any_element();
        }
        div()
            .id("activity-write")
            .debug_selector(|| "activity-write".into())
            .px(px(size::ROW_PAD))
            .h(px(size::ROW))
            .flex()
            .items_center()
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(t.divider)
            .text_color(t.dim)
            .cursor_text()
            .hover(|d| d.border_color(t.text2))
            .child("Write a note…")
            .on_click(cx.listener(|page, _, window, cx| page.write_mission_note(window, cx)))
            .into_any_element()
    }

    /// Open the field for a note on the whole mission, at the head of
    /// Activity, showing it if it was put away.
    pub(super) fn write_mission_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_canvas_note(MISSION_THREAD.into(), window, cx);
        if self.note_paper.open.as_deref() == Some(MISSION_THREAD) {
            self.note_paper.in_flow = true;
            self.flow_words = false;
            self.flow_open = Some(true);
        }
    }

    fn activity_entry(
        &self,
        t: &Theme,
        entry: &Entry,
        now: time::OffsetDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (author, at) = match entry {
            Entry::Note { note, .. } => (note.author.as_str(), note.created_at.as_str()),
            Entry::Change(c) => (c.by.as_str(), c.at.as_str()),
        };
        let name = who(author);
        // The name leads; an initial beside it would only repeat it.
        let head = div()
            .flex()
            .items_center()
            .gap(px(TAB_GAP + 2.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap(px(TAB_GAP))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                    .when_some(
                        match entry {
                            Entry::Note {
                                on: Some((_, on)), ..
                            } => Some(format!("on {on}")),
                            _ => None,
                        },
                        |d, on| d.child(div().truncate().text_color(t.text2).child(on)),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(crate::store::words::short_time(at, now)),
            );
        match entry {
            Entry::Note { note, on } => {
                let body = div().flex().flex_col().gap(px(EDGE)).child(head).child(
                    div()
                        .when(note.resolves, |d| d.text_color(t.text2))
                        .child(note.text.clone()),
                );
                // A note on a card or a wire opens its thread there.
                match on {
                    Some((target, _)) => {
                        let selector = format!("activity-note-{target}-{}", note.id);
                        let target = target.clone();
                        body.id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .p(px(TAB_GAP))
                            .m(px(-TAB_GAP))
                            .rounded(px(size::RADIUS))
                            .cursor_pointer()
                            .hover(|d| d.bg(t.raised))
                            .on_click(cx.listener(move |page, _, window, cx| {
                                page.open_canvas_note(target.clone(), window, cx)
                            }))
                            .into_any_element()
                    }
                    None => body.into_any_element(),
                }
            }
            Entry::Change(change) => {
                let rev = change.rev;
                let chosen = self.activity.chosen == Some(rev);
                let cards: Vec<String> =
                    change.lines.iter().flat_map(|l| l.cards.clone()).collect();
                let mut lines = div().flex().flex_col().text_size(px(size::SMALL));
                for l in change.lines.iter().take(LINES) {
                    let (sign, colour) = match l.sign {
                        Sign::Added => ("+", t.pass),
                        Sign::Removed => ("−", t.fail),
                        Sign::Changed => ("~", t.text2),
                    };
                    lines = lines.child(
                        div()
                            .flex()
                            .gap(px(TAB_GAP))
                            .child(div().flex_none().text_color(colour).child(sign))
                            .child(div().min_w_0().text_color(t.text).child(l.text.clone())),
                    );
                }
                if change.lines.len() > LINES {
                    lines = lines.child(
                        div()
                            .pl(px(PAST_SIGN))
                            .text_color(t.dim)
                            .child(format!("and {} more", change.lines.len() - LINES)),
                    );
                }
                div()
                    .id(SharedString::from(format!("activity-change-{rev}")))
                    .debug_selector(move || format!("activity-change-{rev}"))
                    .flex()
                    .flex_col()
                    .gap(px(EDGE))
                    .p(px(TAB_GAP))
                    .m(px(-TAB_GAP))
                    .rounded(px(size::RADIUS))
                    .when(chosen, |d| d.bg(t.raised))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.raised))
                    .child(head)
                    .child(lines)
                    .on_click(cx.listener(move |page, _, _, cx| {
                        if page.activity.chosen == Some(rev) {
                            page.activity.chosen = None;
                            page.activity_marks.clear();
                        } else {
                            page.activity.chosen = Some(rev);
                            page.activity_marks = cards.clone();
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            }
        }
    }
}
