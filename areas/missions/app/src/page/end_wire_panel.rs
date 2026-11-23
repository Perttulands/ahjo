//! An end's and a wire's panels. An end says how a run that reaches it
//! ends; a wire says the two cards it joins, each one click away, and can
//! be taken off the board.
use super::*;
use ahjo_missions_contract::{EndOutcome, EndUpdate, HumanChannel, InputCardUpdate};
use gpui::SharedString;

/// How a run that reaches an end ends, as the panel offers it.
const OUTCOMES: [(EndOutcome, &str); 2] = [
    (EndOutcome::Done, "Done"),
    (EndOutcome::Rejected, "Rejected"),
];

/// How a run asks the operator, as an input's panel offers it.
const CHANNELS: [(HumanChannel, &str); 2] = [
    (HumanChannel::Notify, "A notification"),
    (HumanChannel::Session, "Its session"),
];

impl MissionsPage {
    /// How a run started here asks the operator. A notification is the
    /// only way that serves a gate he decides, and the default, so the
    /// choice shows only on a card set to its session, to set it back.
    pub(super) fn input_channel_part(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        input: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let card = (self.state.mission.as_ref()?.input_cards.iter()).find(|c| c.id == input)?;
        let chosen = card.human_channel;
        if chosen != HumanChannel::Session {
            return None;
        }
        let mut chips = div().flex().gap(px(TAB_GAP));
        for (channel, word) in CHANNELS {
            let id = input.to_owned();
            chips = chips.child(
                kit::button(
                    t,
                    SharedString::from(format!("channel-{}", channel.as_str())),
                    word,
                    if chosen == channel {
                        Kind::Plain
                    } else {
                        Kind::Quiet
                    },
                )
                .debug_selector(move || format!("channel-{}", channel.as_str()))
                .on_click(cx.listener(move |p, _, _, cx| {
                    let patch = MissionChange::UpdateInputCard(InputCardUpdate {
                        id: id.clone(),
                        human_channel: Some(channel),
                        ..Default::default()
                    });
                    p.write_board(patch, WriteMode::Edit, cx);
                })),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(TAB_GAP))
                .child(kit::label(t, "A run asks you through"))
                .child(chips),
        )
    }

    /// Done or rejected, chosen here and saved as chosen.
    pub(super) fn end_outcome_part(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        end: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let e = self.state.mission.as_ref()?.end(end)?;
        let mut chips = div().flex().gap(px(TAB_GAP));
        for (outcome, word) in OUTCOMES {
            let id = end.to_owned();
            chips = chips.child(
                kit::button(
                    t,
                    SharedString::from(format!("outcome-{}", outcome.as_str())),
                    word,
                    if e.outcome == outcome {
                        Kind::Plain
                    } else {
                        Kind::Quiet
                    },
                )
                .debug_selector(move || format!("outcome-{}", outcome.as_str()))
                .on_click(cx.listener(move |p, _, _, cx| {
                    let patch = MissionChange::UpdateEnd(EndUpdate {
                        id: id.clone(),
                        outcome: Some(outcome),
                        ..Default::default()
                    });
                    p.write_board(patch, WriteMode::Edit, cx);
                })),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(TAB_GAP))
                .child(kit::label(t, "A run that ends here is"))
                .child(chips),
        )
    }

    /// The open wire's head: what it is, and Delete while the mission is
    /// not running (the panel's Close beside them).
    pub(super) fn wire_head(&self, id: &str, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let t = theme(cx).clone();
        let mission = self.state.mission.as_ref()?;
        let wire = mission.connections.iter().find(|c| c.id == id)?;
        let running = self.selected_run().is_some();
        let unwire = MissionChange::UnwireConnection {
            from: wire.from.clone(),
            to: wire.to.clone(),
        };
        Some(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .gap(px(TAB_GAP))
                .child(ahjo_app_kit::widgets::pane::title(&t, "Wire"))
                .when(!running, |d| {
                    d.child(
                        kit::button(&t, "wire-delete", "Delete", Kind::Quiet)
                            .debug_selector(|| "wire-delete".into())
                            .on_click(cx.listener(move |p, _, window, cx| {
                                p.write_board(unwire.clone(), WriteMode::Edit, cx);
                                p.close_editor(window, cx);
                            })),
                    )
                })
                .into_any_element(),
        )
    }

    /// The open wire's panel: its two ends, each opening its card.
    pub(super) fn wire_column(&self, id: &str, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let t = theme(cx).clone();
        let mission = self.state.mission.as_ref()?;
        let wire = mission.connections.iter().find(|c| c.id == id)?;
        let (from, route) = wire.from.split_once(':').unwrap_or((&wire.from, ""));
        let to = wire.to.split(':').next().unwrap_or(&wire.to);
        let from_words = match route {
            "pass" => format!("{}, when it passes", mission.card_name(from)),
            "fail" => format!("{}, when it fails", mission.card_name(from)),
            _ => mission.card_name(from).to_owned(),
        };
        let end = |selector: &'static str, words: String, card: &str| {
            let card = card.to_owned();
            left(
                kit::button(&t, selector, words, Kind::Quiet)
                    .debug_selector(move || selector.into())
                    .on_click(cx.listener(move |p, _, _, cx| p.edit_step(&card, cx))),
            )
        };
        let body = div()
            .id("wire-panel")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(TAB_GAP))
            .child(kit::label(&t, "From"))
            .child(end("wire-from", from_words, from))
            .child(kit::label(&t, "To"))
            .child(end("wire-to", mission.card_name(to).to_owned(), to));
        Some(
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(body)
                .into_any_element(),
        )
    }

    /// Opens wire `id`'s panel in place of a card's, unless a card's
    /// draft is unsaved.
    pub(super) fn open_wire(&mut self, id: String, cx: &mut Context<Self>) {
        if self.unsaved(cx).is_some() {
            self.state.error = Some("Save or discard the current draft first.".into());
        } else {
            self.editor = None;
            self.run.inspect = None;
            self.canvas.wire = Some(id);
        }
        cx.notify();
    }
}
