//! Canvas note pins and sticky threads, carried from the original Missions page.
use super::canvas::{CARD_SMALL, CARD_TEXT, View};
use super::*;
use ahjo_app_kit::theme::Theme;
use ahjo_missions_contract::{
    MISSION_THREAD, Mission,
    document::{Note, NotesDocument},
};
use gpui::{AnyElement, FontWeight, Hsla, MouseButton, SharedString};

/// Archon's stickies: a small paper beside its card, its byline and a few
/// lines; open, a wider thread with the field.
const STICKY_W: f32 = 210.;
const OPEN_W: f32 = 320.;
const GAP: f32 = 6.;
pub(super) const STICKY_ROOM: f32 = board::NOTE_ROOM;
/// A comment as a tag under its card: one line.
pub(super) const TAG_H: f32 = 26.;
pub(super) const TAG_ROOM: f32 = TAG_H + 2. * GAP;
/// A comment as a pin: a bubble astride its card's top-right corner.
const BUBBLE: f32 = 24.;
/// The least room a closed paper is drawn in: its byline and a line.
const STICKY_MIN: f32 = 44.;
/// A closed paper's words, and how many lines of the latest note it shows.
const STICKY_TEXT: f32 = 12.;
const STICKY_SMALL: f32 = 10.5;
const STICKY_LINES: usize = 3;
/// How light the paper is: Archon's yellow note paper and pale blue agent
/// paper, made from the theme's amber and accent.
const PAPER_LIGHT: f32 = 0.74;
const PAPER_LIGHT_ON_LIGHT: f32 = 0.86;
const PIN: f32 = 16.;
/// The room a gate's PASS and FAIL take at its right.
const GATE_WORDS: f32 = 48.;
const PIN_IN: f32 = 9.;
const PIN_MIN: f32 = 16.;
const AUTHOR: &str = "human:ui";

#[derive(Default)]
pub(super) struct NotesUi {
    pub(super) open: Option<String>,
    /// The open thread is being written in Flow, not on the board: only
    /// one place holds the field.
    pub(super) in_flow: bool,
    loading: bool,
    error: Option<String>,
}
/// How much of its author's colour an open note is tinted with: enough
/// to tell it from the page, never enough to read as a bar.
const TINT: f32 = 0.08;

/// A note's look, an annotation rather than a banner: a rule at its left
/// in its author's colour (the agent's the accent, the operator's amber),
/// a faint tint of it while the thread is open, and the plain page with a
/// grey rule once it is resolved.
struct Paper {
    rule: Hsla,
    fill: Hsla,
    text: Hsla,
}
fn paper(t: &Theme, agent: bool, done: bool) -> Paper {
    let own = if agent { t.accent } else { t.needs };
    if done {
        Paper {
            rule: t.divider,
            fill: own.opacity(0.),
            text: t.text2,
        }
    } else {
        Paper {
            rule: own,
            fill: own.opacity(TINT),
            text: t.text,
        }
    }
}
/// A closed sticky's paper and the dark words on it: the operator's in
/// the theme's amber, an agent's in its accent, a resolved one grey.
fn sticky_paper(t: &Theme, agent: bool, done: bool) -> (Hsla, Hsla) {
    let own = if done {
        t.text2
    } else if agent {
        t.accent
    } else {
        t.needs
    };
    let light = if t.light {
        PAPER_LIGHT_ON_LIGHT
    } else {
        PAPER_LIGHT
    };
    let paper = Hsla {
        l: light,
        s: own.s.min(if done { 0.05 } else { 0.75 }),
        ..own
    };
    (paper, if t.light { t.text } else { t.bg })
}
/// The small line over a note: who, in their colour, and when.
fn byline(t: &Theme, note: &Note, done: bool) -> Div {
    let (agent, name) = author(&note.author);
    div()
        .flex()
        .gap(px(GAP))
        .text_size(px(size::SMALL))
        .text_color(t.text2)
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .text_color(paper(t, agent, done).rule)
                .when(done, |d| d.text_color(t.text2))
                .child(name),
        )
        .child(when(note))
}
fn author(value: &str) -> (bool, String) {
    let (prefix, name) = value.split_once(':').unwrap_or((value, ""));
    if prefix == "agent" {
        (true, if name.is_empty() { "agent" } else { name }.into())
    } else {
        (
            false,
            if name.is_empty() || name == "ui" {
                "operator"
            } else {
                name
            }
            .into(),
        )
    }
}
fn when(note: &Note) -> String {
    let timestamp =
        crate::store::words::short_time(&note.created_at, time::OffsetDateTime::now_utc());
    let timestamp = if note.resolves {
        format!("{timestamp} · resolved it")
    } else {
        timestamp
    };
    if note.edited_at.is_some() {
        format!("{timestamp} · edited")
    } else {
        timestamp
    }
}
/// Whether a thread is answered for good: its latest entry resolves it.
fn resolved(entries: &[Note]) -> bool {
    entries.last().is_some_and(|note| note.resolves)
}
fn selector_of(target: &str) -> String {
    format!("canvas-note-{target}")
}
fn thread<'a>(notes: &'a NotesDocument, target: &str) -> &'a [Note] {
    if target == MISSION_THREAD {
        &notes.mission
    } else {
        notes
            .elements
            .iter()
            .find(|thread| thread.node_id == target)
            .map(|thread| thread.entries.as_slice())
            .unwrap_or(&[])
    }
}
/// The thread a note is in.
fn note_target(page: &MissionsPage, id: &str) -> String {
    page.notes
        .as_ref()
        .and_then(|notes| {
            std::iter::once((MISSION_THREAD, &notes.mission))
                .chain(
                    notes
                        .elements
                        .iter()
                        .map(|t| (t.node_id.as_str(), &t.entries)),
                )
                .find(|(_, entries)| entries.iter().any(|n| n.id == id))
                .map(|(target, _)| target.to_owned())
        })
        .unwrap_or_else(|| MISSION_THREAD.into())
}
/// Where a card's closed paper stands: under it, at its right edge. A
/// judged gate's stands right of its foot, clear of its judge's wires.
fn home(card: &board::Card, below: f32, judged: bool) -> (f32, f32) {
    let x = if judged {
        card.x + card.w / 2. + PIN
    } else {
        card.x.max(card.x + card.w - STICKY_W)
    };
    (x, card.y + card.h + GAP + below)
}
impl MissionsPage {
    /// Note: one word beside anything a note can go on, opening a field
    /// right there.
    pub(super) fn note_act(&self, t: &Theme, target: &str, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("flow-note-{target}");
        let target = target.to_owned();
        pane::act(t, SharedString::from(selector.clone()), "Note")
            .debug_selector(move || selector.clone())
            .tooltip(kit::tip("Write a note here"))
            .on_click(cx.listener(move |page, _, window, cx| {
                page.open_canvas_note(target.clone(), window, cx);
                page.note_paper.in_flow = page.note_paper.open.as_deref() == Some(&target);
            }))
            .into_any_element()
    }
    /// The cards that have a note thread: the board keeps room under each.
    pub(super) fn noted_cards(&self) -> std::collections::HashSet<String> {
        self.notes
            .as_ref()
            .map(|n| {
                n.elements
                    .iter()
                    .filter(|t| !t.entries.is_empty())
                    .map(|t| t.node_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Whether the field for `target` is open here (in Flow or on the board).
    fn writing_in(&self, target: &str, flow: bool) -> bool {
        self.note_paper.open.as_deref() == Some(target) && self.note_paper.in_flow == flow
    }
    /// A thread as Flow shows it, under what it is on: each note as paper
    /// in its author's colour, and the field when a note is being written
    /// here. None when there is nothing to show.
    pub(super) fn flow_thread(
        &self,
        t: &Theme,
        target: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let entries = self
            .notes
            .as_ref()
            .map(|n| thread(n, target))
            .unwrap_or(&[]);
        let open = self.writing_in(target, true);
        if entries.is_empty() && !open {
            return None;
        }
        let done = resolved(entries);
        let mut list = div()
            .debug_selector({
                let selector = format!("flow-thread-{target}");
                move || selector.clone()
            })
            .flex()
            .flex_col()
            .gap(px(GAP))
            .pt(px(GAP))
            .max_w(px(size::READING));
        for note in entries {
            let (agent, _) = author(&note.author);
            let look = paper(t, agent, done);
            let mut head = div()
                .flex()
                .items_center()
                .gap(px(GAP))
                .child(byline(t, note, done).flex_1());
            if note.author == AUTHOR && !open {
                let (id, text) = (note.id.clone(), note.text.clone());
                let selector = format!("flow-note-edit-{}", note.id);
                head = head.child(
                    kit::button(t, SharedString::from(selector.clone()), "Edit", Kind::Quiet)
                        .debug_selector(move || selector.clone())
                        .on_click(cx.listener(move |page, _, window, cx| {
                            page.open_canvas_note(note_target(page, &id), window, cx);
                            page.select_note(&id, &text, cx);
                            ahjo_app_kit::widgets::field::focus(&page.note, window, cx);
                        })),
                );
            }
            let entry = div()
                .flex()
                .flex_col()
                .gap(px(EDGE))
                .px(px(size::ROW_PAD))
                .py(px(GAP))
                .border_l(px(EDGE))
                .border_color(look.rule)
                .bg(look.fill)
                .text_color(look.text)
                .child(head)
                .child(note.text.clone());
            list = list.child(entry);
        }
        if open {
            list = list.child(self.note_field(t, target, entries.is_empty(), cx));
        }
        Some(list.into_any_element())
    }
    /// The notes on the wires leaving `node`, each under a line saying
    /// which wire, for Flow to put under the step's "then" line.
    pub(super) fn flow_wire_threads(
        &self,
        t: &Theme,
        mission: &Mission,
        node: &str,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let prefix = format!("{node}:");
        for wire in mission
            .connections
            .iter()
            .filter(|c| c.from.starts_with(&prefix) || c.from == node)
        {
            let Some(thread) = self.flow_thread(t, &wire.id, cx) else {
                continue;
            };
            let to = wire.to.split(':').next().unwrap_or_default();
            let route = wire.from.split_once(':').map_or("", |(_, port)| port);
            let on = match route {
                "pass" | "fail" => format!("On the {route} wire to {}", mission.node_name(to)),
                _ => format!("On the wire to {}", mission.node_name(to)),
            };
            out.push(
                div()
                    .pt(px(GAP))
                    .text_color(t.text2)
                    .child(on)
                    .into_any_element(),
            );
            out.push(thread);
        }
        out
    }
    /// The field a note is written in, with its Save and Discard.
    fn note_field(&self, t: &Theme, target: &str, first: bool, cx: &mut Context<Self>) -> Div {
        let save = format!("flow-note-save-{target}");
        let label = if self.note_saving {
            "Saving…"
        } else if self.note_id.is_some() {
            "Save edit"
        } else if first {
            "Add note"
        } else {
            "Reply"
        };
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(
                div()
                    .debug_selector(|| "flow-note-field".into())
                    .child(self.note.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(size::SMALL))
                            .text_color(t.dim)
                            .child("Ctrl+Enter keeps it, Esc puts it away"),
                    )
                    .child(
                        kit::button(t, "flow-note-discard", "Discard", Kind::Quiet).on_click(
                            cx.listener(|page, _, _, cx| {
                                page.discard_note(cx);
                                page.close_canvas_note(cx);
                            }),
                        ),
                    )
                    .child(
                        kit::button(t, SharedString::from(save.clone()), label, Kind::Primary)
                            .debug_selector(move || save.clone())
                            .when(!self.note_saving, |d| {
                                d.on_click(cx.listener(|page, _, _, cx| page.add_note(false, cx)))
                            }),
                    ),
            )
    }
    /// Keep a failed notes read visible and let the operator retry it.
    pub(super) fn load_canvas_notes(&mut self, node: String, slug: String, cx: &mut Context<Self>) {
        if self.note_paper.loading {
            return;
        }
        self.note_paper.loading = true;
        self.note_paper.error = None;
        let generation = self.generation;
        let asked = self.host.request(
            Request::get(
                NodeId(node.clone()),
                format!("{}/notes", mission_path(&slug)),
            ),
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
                page.note_paper.loading = false;
                match result {
                    Ok(answer) if answer.ok() => match serde_json::from_slice::<
                        ahjo_missions_contract::NotesResponse,
                    >(&answer.body)
                    {
                        Ok(notes) => {
                            page.notes = Some(notes.notes);
                            page.note_paper.error = None;
                        }
                        Err(error) => page.note_paper.error = Some(error.to_string()),
                    },
                    Ok(answer) => page.note_paper.error = Some(answer.error_text()),
                    Err(error) => {
                        page.note_paper.error = Some(format!("{}: {}", error.code, error.message))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn open_canvas_note(
        &mut self,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_saving
            || self.note.read(cx).text != self.note_base
            || self.editor.is_some() && self.unsaved(cx).is_some()
        {
            self.state.error =
                Some("Save or discard the current draft before opening another note.".into());
            cx.notify();
            return;
        }
        self.editor = None;
        self.note_id = None;
        self.note_base.clear();
        self.note.update(cx, |field, cx| field.set("", false, cx));
        self.note_paper.open = Some(target);
        self.note_paper.in_flow = false;
        self.note_etag = self.notes.as_ref().map(|notes| notes.etag.clone());
        ahjo_app_kit::widgets::field::focus(&self.note, window, cx);
        self.state.error = None;
        cx.notify();
    }
    pub(super) fn close_canvas_note(&mut self, cx: &mut Context<Self>) {
        if self.note_saving || self.note.read(cx).text != self.note_base {
            self.state.error =
                Some("Save or discard the current note draft before closing it.".into());
        } else {
            self.note_paper.open = None;
            self.note_etag = None;
            self.note_id = None;
            self.note_base.clear();
            self.note.update(cx, |field, cx| field.set("", false, cx));
            self.state.error = None;
        }
        cx.notify();
    }
    pub(super) fn canvas_notes(
        &self,
        t: &Theme,
        mission: &Mission,
        board: &board::Board,
        view: &View,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(notes) = self.notes.as_ref() else {
            let notice = if let Some(error) = &self.note_paper.error {
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .debug_selector(|| "canvas-notes-error".into())
                            .text_color(t.error)
                            .child(format!("Reading notes failed: {error}")),
                    )
                    .child(
                        kit::button(t, "canvas-notes-retry", "Try again", Kind::Quiet)
                            .debug_selector(|| "canvas-notes-retry".into())
                            .on_click(cx.listener(|page, _, _, cx| {
                                if let Some((node, slug)) = page.state.selected.clone() {
                                    page.load_canvas_notes(node, slug, cx);
                                }
                            })),
                    )
            } else {
                div().text_color(t.dim).child("Reading notes…")
            };
            return vec![
                div()
                    .absolute()
                    .left(px(PAD))
                    .bottom(px(PAD))
                    .child(notice)
                    .into_any_element(),
            ];
        };
        let mut out = Vec::new();
        // An open paper lies over everything else on the board, other
        // cards' pins included, so it goes last.
        let mut opened = Vec::new();
        let mut input_sticky = false;
        let comments = crate::look::comments();
        for card in board.cards.iter().filter(|c| c.kind != board::Kind::Frame) {
            let entries = thread(notes, &card.id);
            let open = self.writing_in(&card.id, false);
            match comments {
                crate::look::Comments::Pins if !entries.is_empty() => {
                    out.push(self.canvas_bubble(t, card, entries, view, cx))
                }
                _ => out.push(self.canvas_note_pin(t, card, false, view, cx)),
            }
            if entries.is_empty() && !open {
                continue;
            }
            if !open {
                // Closed, a pin says it all; a tag says its first line.
                if comments == crate::look::Comments::Tags {
                    out.push(self.canvas_tag(t, card, entries, view, cx));
                }
                continue;
            }
            // A closed paper never covers another card: it takes the room
            // down to the next card under it, and where there is too little
            // for a line, the card's pin alone says a thread is there.
            let judged = board
                .wires
                .iter()
                .any(|w| w.route == "judge" && w.from_node == card.id);
            let at = home(card, 0., judged);
            let room = board
                .cards
                .iter()
                .filter(|other| {
                    other.id != card.id
                        && other.y + other.h > at.1
                        && other.x < at.0 + STICKY_W
                        && other.x + other.w > at.0
                })
                .map(|other| other.y - GAP - at.1)
                .fold(f32::INFINITY, f32::min);
            // Where there is no room for its paper, the pin alone says a
            // thread is there.
            if !open && room < STICKY_MIN {
                continue;
            }
            if card.kind == board::Kind::Input {
                input_sticky = true;
            }
            let paper = self.canvas_sticky(
                t,
                &card.id,
                mission.node_name(&card.id),
                entries,
                at,
                (!open).then_some(room),
                view,
                cx,
            );
            if open { &mut opened } else { &mut out }.push(paper);
        }
        // A note on a wire: a pin at the wire's middle, its thread opening
        // there. Closed, the pin alone, so no paper lies across the wires.
        // Only connections: a limit's or a judge's line shares its card's id.
        for wire in board
            .wires
            .iter()
            .filter(|w| mission.connections.iter().any(|c| c.id == w.id))
        {
            let entries = thread(notes, &wire.id);
            let open = self.writing_in(&wire.id, false);
            if entries.is_empty() && !open {
                continue;
            }
            let middle = wire.middle();
            out.push(self.canvas_wire_pin(t, &wire.id, entries, middle, view, cx));
            if open {
                let name = format!(
                    "{} → {}",
                    mission.node_name(&wire.from_node),
                    mission.node_name(&wire.to_node)
                );
                opened.push(self.canvas_sticky(
                    t,
                    &wire.id,
                    &name,
                    entries,
                    (middle.0, middle.1 + PIN),
                    None,
                    view,
                    cx,
                ));
            }
        }
        let at = match board
            .cards
            .iter()
            .find(|card| card.kind == board::Kind::Input)
        {
            Some(card) => (
                card.x,
                home(card, if input_sticky { STICKY_ROOM } else { 0. }, false).1,
            ),
            None => board
                .cards
                .iter()
                .min_by(|a, b| (a.x + a.y).total_cmp(&(b.x + b.y)))
                .map_or((40., 40.), |card| (card.x, card.y + card.h + GAP)),
        };
        // Keep the mission paper out of the way of cards below its anchor. Its
        // height changes when the thread opens, so place it below all cards in
        // its horizontal span rather than estimating the paper height.
        let width = if self.writing_in(MISSION_THREAD, false) {
            OPEN_W
        } else {
            STICKY_W
        };
        let at = (
            at.0,
            board
                .cards
                .iter()
                .filter(|card| {
                    card.x < at.0 + width && card.x + card.w > at.0 && card.y + card.h > at.1
                })
                // Below a card's own paper too, where it has one.
                .fold(at.1, |y, card| {
                    let paper = if thread(notes, &card.id).is_empty() {
                        0.
                    } else {
                        STICKY_ROOM
                    };
                    y.max(card.y + card.h + GAP + paper)
                }),
        );
        // The mission's own paper appears once it holds something or is
        // being written; Note in the head opens it.
        // The mission's own thread is in the panel beside the board; here
        // only while it is being written from the board's menu.
        let writing = self.writing_in(MISSION_THREAD, false);
        if writing {
            let paper = self.canvas_sticky(
                t,
                MISSION_THREAD,
                "Mission notes",
                thread(notes, MISSION_THREAD),
                at,
                None,
                view,
                cx,
            );
            if writing { &mut opened } else { &mut out }.push(paper);
        }
        out.extend(opened);
        out
    }
    fn canvas_note_pin(
        &self,
        t: &Theme,
        card: &board::Card,
        has: bool,
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let size = (PIN * z).max(PIN_MIN);
        let inset = (PIN_IN * z).max(EDGE);
        let (right, top) = view.position(card.x + card.w, card.y);
        // Clear of the Input's play control, the End's round corner and a
        // gate's PASS and FAIL.
        let play_space = match card.kind {
            board::Kind::Input => (PAD + size::CONTROL + GAP) * z,
            board::Kind::End => board::END_H * z / 3.,
            board::Kind::Gate => GATE_WORDS * z,
            _ => 0.,
        };
        let x = right - inset - size - play_space;
        let y = if card.kind == board::Kind::End {
            top + (card.h * z - size) / 2.
        } else {
            top + inset
        };
        let look = paper(t, false, false);
        // A thread's pin is a small square of its paper, as on Archon's
        // cards; an empty one only an outline.
        let (fill, ink) = sticky_paper(t, false, false);
        let target = card.id.clone();
        let chosen = self.editor.as_ref().is_some_and(|ed| ed.id == card.id);
        let selector = format!("canvas-note-pin-{target}");
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(x))
            .top(px(y))
            .size(px(size))
            .rounded(px(EDGE))
            .when(!has, |d| d.border_1().border_color(look.rule))
            .when(has, |d| d.bg(fill))
            .text_size(px((STICKY_SMALL * z).max(size::SMALL - 2.)))
            .font_weight(FontWeight::BOLD)
            .text_color(if has { ink } else { look.rule })
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            // An empty pin shows on its own card while that card is open,
            // and under the pointer; otherwise the card stays plain.
            .opacity(if has || chosen { 1. } else { 0. })
            .hover(|d| d.opacity(1.))
            .tooltip(kit::tip(if has {
                "Open the note thread"
            } else {
                "Add a note"
            }))
            .when(has, |d| d.child("≡"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |page, _, window, cx| {
                page.open_canvas_note(target.clone(), window, cx)
            }))
            .into_any_element()
    }
    /// The pin of a wire's thread, centred on the wire, the latest note as
    /// its tooltip.
    fn canvas_wire_pin(
        &self,
        t: &Theme,
        target: &str,
        entries: &[Note],
        (x, y): (f32, f32),
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let side = (PIN * z).max(PIN_MIN);
        let (x, y) = view.position(x, y);
        let done = resolved(entries);
        let look = paper(t, entries.last().is_some_and(|n| author(&n.author).0), done);
        let tip = entries
            .last()
            .map(|n| format!("{}: {}", author(&n.author).1, n.text))
            .unwrap_or_else(|| "Add a note".into());
        let selector = format!("canvas-wire-note-{target}");
        let target = target.to_owned();
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(x - side / 2.))
            .top(px(y - side / 2.))
            .size(px(side))
            .rounded_full()
            .border_1()
            .border_color(look.rule)
            .bg(t.raised)
            .text_color(look.rule)
            .text_size(px((size::TEXT * z).max(size::SMALL)))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child("▰")
            .tooltip(kit::tip(tip))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |page, _, window, cx| {
                page.open_canvas_note(target.clone(), window, cx)
            }))
            .into_any_element()
    }
    #[allow(clippy::too_many_arguments)]
    fn canvas_sticky(
        &self,
        t: &Theme,
        target: &str,
        title: &str,
        entries: &[Note],
        at: (f32, f32),
        room: Option<f32>,
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let (x, y) = view.position(at.0, at.1);
        let open = self.writing_in(target, false);
        let latest = entries.last();
        let done = resolved(entries);
        let look = paper(t, latest.is_some_and(|note| author(&note.author).0), done);
        let selector = selector_of(target);
        let base = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px((if open { OPEN_W } else { STICKY_W }) * z))
            .p(px(GAP * z))
            .flex()
            .flex_col()
            .gap(px(GAP * z))
            // Opaque over the board, so no wire shows through its words.
            .border_1()
            .border_l(px(EDGE.max(EDGE * z)))
            .border_color(look.rule)
            .bg(t.raised)
            .text_color(look.text)
            .shadow_sm()
            .text_size(px(CARD_TEXT * z))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if !open {
            let target = target.to_owned();
            let summary = latest
                .map(|note| note.text.clone())
                .unwrap_or_else(|| "Write what this mission is for.".into());
            let agent = latest.is_some_and(|note| author(&note.author).0);
            let (fill, ink) = sticky_paper(t, agent, done);
            let who = latest.map(|note| author(&note.author).1);
            let mission = target == MISSION_THREAD;
            return div()
                .id(SharedString::from(selector_of(&target)))
                .debug_selector({
                    let selector = selector_of(&target);
                    move || selector.clone()
                })
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(STICKY_W * z))
                .px(px(GAP * 1.5 * z))
                .py(px(GAP * z))
                .flex()
                .flex_col()
                .gap(px(EDGE * z))
                .rounded(px(EDGE * z))
                .bg(fill)
                .text_color(ink)
                .shadow_md()
                .text_size(px(STICKY_TEXT * z))
                .when_some(room, |d, room| d.max_h(px(room * z)).overflow_hidden())
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(GAP * z))
                        .text_size(px(STICKY_SMALL * z))
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::BOLD)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(if mission {
                                    title.to_uppercase()
                                } else {
                                    who.unwrap_or_default().to_uppercase()
                                }),
                        )
                        .when(entries.len() > 1, |d| {
                            d.child(format!("+{}", entries.len() - 1))
                        })
                        .when(done, |d| d.child("resolved")),
                )
                .child(
                    div()
                        .line_clamp(STICKY_LINES)
                        .text_ellipsis()
                        .child(summary),
                )
                .on_click(cx.listener(move |page, _, window, cx| {
                    page.open_canvas_note(target.clone(), window, cx)
                }))
                .into_any_element();
        }
        let mut list = div()
            .id(SharedString::from(format!("canvas-note-thread-{target}")))
            .flex()
            .flex_col()
            .gap(px(GAP * z))
            .max_h(px(size::NOTICE * z))
            .overflow_y_scroll();
        for note in entries {
            let (_, name) = author(&note.author);
            let edit_id = note.id.clone();
            let delete_id = note.id.clone();
            let edit_text = note.text.clone();
            let delete_text = note.text.clone();
            let entry_selector = format!("canvas-note-entry-{}-{}", note.id, name);
            let entry_edge = paper(t, author(&note.author).0, done).rule;
            let mut entry = div()
                .debug_selector(move || entry_selector.clone())
                .flex()
                .flex_col()
                .p(px(GAP * z))
                .border_l(px(EDGE.max(EDGE * z)))
                .border_color(entry_edge)
                .child(byline(t, note, done).text_size(px(CARD_SMALL * z)))
                .child(note.text.clone());
            if note.author == AUTHOR {
                let edit_selector = format!("canvas-note-edit-{}", note.id);
                let delete_selector = format!("canvas-note-delete-{}", note.id);
                entry = entry.child(div().flex()
                    .child(kit::button(t, SharedString::from(edit_selector.clone()), "Edit", Kind::Quiet).debug_selector(move || edit_selector.clone()).on_click(cx.listener(move |page, _, window, cx| { page.select_note(&edit_id, &edit_text, cx); if page.note_id.as_deref() == Some(edit_id.as_str()) { ahjo_app_kit::widgets::field::focus(&page.note, window, cx); } })))
                    .child(kit::button(t, SharedString::from(delete_selector.clone()), "Delete", Kind::Quiet).debug_selector(move || delete_selector.clone()).on_click(cx.listener(move |page, _, _, cx| {
                        if page.note_saving || page.note.read(cx).text != page.note_base { page.state.error = Some("Save or discard the current note draft before deleting an entry.".into()); cx.notify(); return; }
                        page.select_note(&delete_id, &delete_text, cx);
                        page.add_note(true, cx);
                    }))));
            }
            list = list.child(entry);
        }
        let close_selector = format!("canvas-note-close-{target}");
        let save_selector = format!("canvas-note-save-{target}");
        let placeholder = if self.note_id.is_some() {
            "Save edit"
        } else if entries.is_empty() {
            "Add note"
        } else {
            "Reply"
        };
        base.child(
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::BOLD)
                        .child(title.to_owned()),
                )
                .child(
                    kit::button(
                        t,
                        SharedString::from(close_selector.clone()),
                        "×",
                        Kind::Quiet,
                    )
                    .debug_selector(move || close_selector.clone())
                    .on_click(cx.listener(|page, _, _, cx| page.close_canvas_note(cx))),
                ),
        )
        .child(list)
        .child(
            div()
                .debug_selector(|| "canvas-note-field".into())
                .child(self.note.clone()),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(GAP * z))
                .when(
                    target != MISSION_THREAD && !entries.is_empty() && !done,
                    |d| {
                        let (target, entry) = (
                            target.to_owned(),
                            entries.last().map(|n| n.id.clone()).unwrap_or_default(),
                        );
                        d.child(
                            kit::button(t, "canvas-note-resolve", "Resolve", Kind::Quiet)
                                .debug_selector(|| "canvas-note-resolve".into())
                                .on_click(cx.listener(move |page, _, _, cx| {
                                    page.resolve_thread(target.clone(), entry.clone(), cx)
                                })),
                        )
                    },
                )
                .when(
                    self.note_id.is_some() || self.note.read(cx).text != self.note_base,
                    |d| {
                        d.child(
                            kit::button(t, "canvas-note-discard", "Discard", Kind::Quiet)
                                .on_click(cx.listener(|page, _, _, cx| page.discard_note(cx))),
                        )
                    },
                )
                .child(
                    kit::button(
                        t,
                        SharedString::from(save_selector.clone()),
                        if self.note_saving {
                            "Saving…"
                        } else {
                            placeholder
                        },
                        Kind::Primary,
                    )
                    .debug_selector(move || save_selector.clone())
                    .when(!self.note_saving, |d| {
                        d.on_click(cx.listener(|page, _, _, cx| page.add_note(false, cx)))
                    }),
                ),
        )
        .when_some(self.state.error.clone(), |d, error| {
            d.child(
                div()
                    .debug_selector(|| "canvas-note-error".into())
                    .text_color(t.error)
                    .child(error),
            )
        })
        .into_any_element()
    }
    /// A comment as a pin: a bubble astride the card's top-right corner,
    /// amber with its count while open, a green tick once resolved.
    fn canvas_bubble(
        &self,
        t: &Theme,
        card: &board::Card,
        entries: &[Note],
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let side = (BUBBLE * z).max(PIN_MIN + 2.);
        let (right, top) = view.position(card.x + card.w, card.y);
        let done = resolved(entries);
        let colour = if done {
            crate::look::ink(t).pass
        } else {
            t.needs
        };
        let target = card.id.clone();
        let selector = format!("canvas-note-pin-{target}");
        let tip = entries
            .first()
            .map(|n| format!("{}: {}", author(&n.author).1, n.text))
            .unwrap_or_default();
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(right - side * 0.6))
            .top(px(top - side * 0.4))
            .h(px(side))
            .min_w(px(side))
            .px(px(5. * z))
            .rounded(px(side / 2.))
            .rounded_bl(px(2.))
            .when(!done, |d| {
                d.bg(colour).text_color(if t.light { t.text } else { t.bg })
            })
            .when(done, |d| {
                d.bg(t.surface)
                    .border_1()
                    .border_color(colour)
                    .text_color(colour)
            })
            .shadow_md()
            .text_size(px((CARD_SMALL * z).max(size::SMALL - 2.)))
            .font_weight(FontWeight::BOLD)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(if done {
                "✓".to_owned()
            } else {
                entries.len().to_string()
            })
            .tooltip(kit::tip(tip))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |page, _, window, cx| {
                page.open_canvas_note(target.clone(), window, cx)
            }))
            .into_any_element()
    }
    /// A comment as a tag: one line under the card, the ask's first words,
    /// amber while open, faint once resolved.
    fn canvas_tag(
        &self,
        t: &Theme,
        card: &board::Card,
        entries: &[Note],
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let (x, y) = view.position(card.x, card.y + card.h + GAP);
        let done = resolved(entries);
        let first = entries.first();
        let target = card.id.clone();
        let selector = selector_of(&target);
        let rule = if done { t.divider } else { t.needs };
        let words = first.map(|n| n.text.clone()).unwrap_or_default();
        let replies = entries.len().saturating_sub(1);
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(card.w * z))
            .h(px(TAG_H * z))
            .px(px(GAP * z))
            .flex()
            .items_center()
            .gap(px(GAP * z))
            .rounded(px(4. * z))
            .border_l(px(3. * z))
            .border_color(rule)
            .bg(if done {
                t.bg
            } else {
                ahjo_app_kit::theme::alpha(t.needs, if t.light { 0.12 } else { 0.10 })
            })
            .text_size(px(CARD_SMALL * z))
            .cursor_pointer()
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if done {
                        crate::look::ink(t).pass
                    } else {
                        t.needs
                    })
                    .child(if done { "✓" } else { "ASK" }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if done { t.dim } else { t.text })
                    .child(words),
            )
            .when(replies > 0, |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_color(t.text2)
                        .child(format!("+{replies}")),
                )
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |page, _, window, cx| {
                page.open_canvas_note(target.clone(), window, cx)
            }))
            .into_any_element()
    }
}
