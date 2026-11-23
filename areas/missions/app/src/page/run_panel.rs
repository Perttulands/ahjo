//! A card's panel on a run's board: what the card did in this run, and
//! what the plan says it does, to read. The plan is changed on the
//! mission's own board; the run only takes the operator's answers.
use super::*;
use crate::store::run_state::project;
use ahjo_missions_contract::{EndOutcome, GateKind};

impl MissionsPage {
    /// Opens card `id`'s read-only panel on the run's board.
    pub(super) fn open_run_card(&mut self, id: &str, cx: &mut Context<Self>) {
        self.editor = None;
        self.canvas.wire = None;
        self.run.inspect = Some(id.to_owned());
        cx.notify();
    }

    /// The open card's head in the run shown: its name (the panel's Close
    /// beside it).
    pub(super) fn run_card_head(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let t = theme(cx).clone();
        let mission = self.state.mission.as_ref()?;
        Some(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .child(ahjo_app_kit::widgets::pane::title(
                    &t,
                    mission.card_name(id).to_owned(),
                ))
                .into_any_element(),
        )
    }

    /// The open card's panel in the run shown: what it did and what it is.
    pub(super) fn run_card_column(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let t = theme(cx).clone();
        let mission = self.state.mission.as_ref()?;
        let run = self.selected_run()?;
        let fact = |label: &'static str, words: String| {
            div()
                .flex()
                .flex_col()
                .gap(px(EDGE))
                .child(kit::label(&t, label))
                .child({
                    let selector = format!("run-fact: {words}");
                    div().debug_selector(move || selector).child(words)
                })
        };
        let mut body = div()
            .id("run-card-panel")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(TAB_GAP));
        if let Some(gate) = mission.gate(id) {
            body = body.children(self.gate_run_part(&t, id, cx));
            body = body.child(fact("Lets through when", gate.criterion.clone()));
            let who = gate
                .kinds
                .iter()
                .map(|kind| match kind {
                    GateKind::Code => "A check",
                    GateKind::Step => "A judge",
                    GateKind::Human => "You",
                })
                .collect::<Vec<_>>()
                .join(", ");
            body = body.child(fact("Who decides", who));
            let fails = match canvas::back_loop(mission, id) {
                Some((target, limit)) => {
                    let tries = limit
                        .and_then(|l| l.rounds.map(|r| (r, l.when_spent)))
                        .map(|(r, spent)| {
                            let then = match spent {
                                ahjo_missions_contract::WhenSpent::Best => "the best goes on",
                                ahjo_missions_contract::WhenSpent::Stop => "it waits for you",
                                _ => "the run fails",
                            };
                            format!(", up to {r} tries; then {then}")
                        })
                        .unwrap_or_else(|| ", until it passes".into());
                    format!("Goes back to {}{tries}", mission.card_name(&target))
                }
                None => match mission
                    .connections
                    .iter()
                    .find(|c| c.from == format!("{id}:fail"))
                {
                    Some(c) => format!(
                        "The run ends: {}",
                        mission.card_name(c.to.split(':').next().unwrap_or(&c.to))
                    ),
                    None => "Nothing is wired to its fail exit.".into(),
                },
            };
            body = body.child(fact("When it fails", fails));
        } else if let Some(end) = mission.end(id) {
            let reached = project(&run.events).states.contains_key(id);
            body = body
                .child(fact(
                    "A run that ends here is",
                    match end.outcome {
                        EndOutcome::Rejected => "Rejected",
                        _ => "Done",
                    }
                    .into(),
                ))
                .child(fact(
                    "This run",
                    if reached {
                        "Ended here"
                    } else {
                        "Has not ended here"
                    }
                    .into(),
                ));
        } else if mission.input_cards.iter().any(|c| c.id == id) {
            if run.input_card_id == id {
                body = body.child(kit::label(&t, "This run was given"));
                for input in &run.inputs {
                    body = body.child(fact_line(&t, &input.name, &input.value));
                }
                if run.inputs.is_empty() {
                    body = body.child(div().text_color(t.text2).child("Nothing."));
                }
            } else {
                body = body.child(
                    div()
                        .text_color(t.text2)
                        .child("This run started elsewhere."),
                );
            }
        } else {
            return None;
        }
        Some(
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(body)
                .into_any_element(),
        )
    }
}

/// One value a run was given: its name over what it held.
fn fact_line(t: &ahjo_app_kit::theme::Theme, name: &str, value: &str) -> Div {
    let selector = format!("run-given-{name}: {value}");
    div()
        .flex()
        .flex_col()
        .debug_selector(move || selector)
        .child(div().text_color(t.text2).child(name.to_owned()))
        .child(div().child(value.to_owned()))
}
