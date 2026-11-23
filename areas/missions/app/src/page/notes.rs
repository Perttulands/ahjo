//! Notes: a step's thread opens from a small mark on the step, as
//! Figma's comments do; the mission's own thread is in Activity.
use super::canvas::{CARD_SMALL, CARD_TEXT, View};
use super::*;
use ahjo_app_kit::theme::Theme;
use ahjo_missions_contract::{
    MISSION_THREAD, Mission,
    document::{Note, NotesDocument},
};
use gpui::{AnyElement, FontWeight, Hsla, MouseButton, SharedString};

/// An open thread, beside its card, with the field.
const OPEN_W: f32 = 320.;
const GAP: f32 = 6.;
/// The count on a card's note mark.
const STICKY_SMALL: f32 = 10.5;
const PIN: f32 = 16.;
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
            .find(|thread| thread.card_id == target)
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
                        .map(|t| (t.card_id.as_str(), &t.entries)),
                )
                .find(|(_, entries)| entries.iter().any(|n| n.id == id))
                .map(|(target, _)| target.to_owned())
        })
        .unwrap_or_else(|| MISSION_THREAD.into())
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
                "pass" | "fail" => format!("On the {route} wire to {}", mission.card_name(to)),
                _ => format!("On the wire to {}", mission.card_name(to)),
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
    pub(super) fn note_field(
        &self,
        t: &Theme,
        target: &str,
        first: bool,
        cx: &mut Context<Self>,
    ) -> Div {
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
                    // The keys are on the buttons' tips, so the row fits
                    // the narrowest panel.
                    .justify_end()
                    .child(
                        kit::button(t, "flow-note-discard", "Discard", Kind::Quiet)
                            .tooltip(kit::tip("Esc"))
                            .on_click(cx.listener(|page, _, _, cx| {
                                page.discard_note(cx);
                                page.close_canvas_note(cx);
                            })),
                    )
                    .child(
                        kit::button(t, SharedString::from(save.clone()), label, Kind::Primary)
                            .debug_selector(move || save.clone())
                            .tooltip(kit::tip("Ctrl+Enter"))
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
        let asked = self.host.call(
            NodeId(node.clone()),
            ahjo_missions_contract::commands::MissionNotes {
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
                page.note_paper.loading = false;
                match result {
                    Ok(notes) => {
                        page.notes = Some(notes.notes);
                        page.note_paper.error = None;
                    }
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
        // An open thread lies over everything else on the board, other
        // cards' marks included, so it goes last.
        let mut opened = Vec::new();
        for card in &board.cards {
            let entries = thread(notes, &card.id);
            // A gate carries its mark on the gate's own corner.
            let corner = mission
                .gates
                .iter()
                .find(|g| g.id == card.id)
                .map(|g| board::gate_frame(&g.criterion).pin)
                .map_or((card.x + card.w, card.y), |pin| {
                    (card.x + pin.0, card.y + pin.1)
                });
            out.push(self.canvas_note_pin(t, card, corner, entries, view, cx));
            if self.writing_in(&card.id, false) {
                opened.push(self.canvas_sticky(
                    t,
                    &card.id,
                    mission.card_name(&card.id),
                    entries,
                    (card.x + card.w + GAP, card.y),
                    view,
                    cx,
                ));
            }
        }
        // A thread on a wire is read in Activity; it opens here only when
        // asked for.
        for wire in &board.wires {
            if self.writing_in(&wire.id, false) {
                let middle = wire.middle();
                let name = format!(
                    "{} → {}",
                    mission.card_name(&wire.from_card),
                    mission.card_name(&wire.to_card)
                );
                opened.push(self.canvas_sticky(
                    t,
                    &wire.id,
                    &name,
                    thread(notes, &wire.id),
                    (middle.0, middle.1 + PIN),
                    view,
                    cx,
                ));
            }
        }
        out.extend(opened);
        out
    }
    /// A card's note mark astride its top-right corner: how many notes
    /// its thread holds, or, under the pointer, a plus to begin one.
    fn canvas_note_pin(
        &self,
        t: &Theme,
        card: &board::Card,
        corner: (f32, f32),
        entries: &[Note],
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let side = (PIN * z).max(PIN_MIN);
        let (right, top) = view.position(corner.0, corner.1);
        let has = !entries.is_empty();
        let done = resolved(entries);
        let target = card.id.clone();
        let chosen = self.editor.as_ref().is_some_and(|ed| ed.id == card.id);
        let selector = format!("canvas-note-pin-{target}");
        let tip = entries
            .last()
            .map(|n| format!("{}: {}", author(&n.author).1, n.text))
            .unwrap_or_else(|| "Add a note".into());
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(right - side / 2.))
            .top(px(top - side / 2.))
            .h(px(side))
            .min_w(px(side))
            .px(px(EDGE * z))
            .rounded(px(side / 2.))
            .border_1()
            .border_color(if has && !done { t.text2 } else { t.divider })
            .bg(t.raised)
            .text_size(px((STICKY_SMALL * z).max(size::SMALL - 2.)))
            .text_color(if done { t.dim } else { t.text })
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .opacity(if has || chosen { 1. } else { 0. })
            .hover(|d| d.opacity(1.))
            .tooltip(kit::tip(tip))
            .child(if has {
                entries.len().to_string()
            } else {
                "+".into()
            })
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
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let (x, y) = view.position(at.0, at.1);
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
            .w(px(OPEN_W * z))
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
        let mut list = pane::scroll(SharedString::from(format!("canvas-note-thread-{target}")))
            .gap(px(GAP * z))
            .max_h(px(size::NOTICE * z));
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
                    kit::close(t, close_selector, None)
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
                    self.note_id.is_some() || self.note.read(cx).text != self.note_base,
                    |d| {
                        d.child(
                            kit::button(t, "canvas-note-discard", "Discard", Kind::Quiet)
                                .debug_selector(|| "canvas-note-discard".into())
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
}
