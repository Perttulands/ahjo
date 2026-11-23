//! A gate's panel (AH13 prototype): what the gate on the board stands for,
//! one click away. In a run, how its tries went; then what it lets
//! through, who decides, and what happens when it fails, with the loop's
//! number of tries set right here.
use super::*;
use ahjo_missions_contract::{GateKind, GateUpdate, LimitUpdate, Mission, WhenSpent};
use gpui::SharedString;

/// One way to decide a gate, as the panel offers it.
const DECIDERS: [(GateKind, &str); 3] = [
    (GateKind::Code, "A check"),
    (GateKind::Step, "A judge"),
    (GateKind::Human, "You"),
];

/// What happens once a loop's tries are spent, as the panel offers it.
const SPENT: [(WhenSpent, &str); 3] = [
    (WhenSpent::Fail, "The run fails"),
    (WhenSpent::Best, "The best goes on"),
    (WhenSpent::Stop, "It waits for you"),
];

impl MissionsPage {
    /// The gate's tries in the run shown: each verdict, newest last, and
    /// the answer it waits for from the operator.
    pub(super) fn gate_run_part(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        gate: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let run = self.selected_run()?;
        let mission = self.state.mission.as_ref()?;
        let most = canvas::back_loop(mission, gate).and_then(|(_, l)| l.and_then(|l| l.rounds));
        let mut part = div()
            .flex()
            .flex_col()
            .gap(px(TAB_GAP))
            .child(kit::label(t, "This run"));
        let verdicts: Vec<_> = run
            .events
            .iter()
            .filter(|e| e.gate_id == gate && e.kind == "gate_verdict" && !e.verdict.is_empty())
            .collect();
        if verdicts.is_empty() && !run.waiting_gates.iter().any(|w| w.gate_id == gate) {
            part = part.child(div().text_color(t.text2).child("Not reached yet."));
        }
        let problems: Vec<_> = self
            .run
            .evidence
            .evidence
            .as_ref()
            .into_iter()
            .flat_map(|e| &e.problems)
            .filter(|p| p.gate_id == gate || p.card_id == gate)
            .collect();
        let mut after = 0;
        for (i, e) in verdicts.iter().enumerate() {
            let n = if e.attempt > 0 {
                e.attempt
            } else {
                i as i64 + 1
            };
            // Why it was sent back: the problem recorded since the last
            // verdict, else the words the verdict came with.
            let why = problems
                .iter()
                .rfind(|p| p.seq > after && p.seq <= e.seq)
                .map(|p| ahjo_missions_contract::plain_reason(&p.reason.text))
                .filter(|_| e.verdict == "fail")
                .unwrap_or_else(|| e.detail.clone());
            after = e.seq;
            let (word, colour) = if e.verdict == "fail" {
                ("sent back", t.fail)
            } else {
                ("passed", t.pass)
            };
            let of = most.map(|r| format!(" of {r}")).unwrap_or_default();
            part = part.child(
                div()
                    .flex()
                    .flex_col()
                    .py(px(EDGE))
                    .child(
                        div()
                            .flex()
                            .gap(px(TAB_GAP))
                            .child(div().text_color(t.text2).child(format!("Try {n}{of}")))
                            .child(div().text_color(colour).child(word)),
                    )
                    .when(!why.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(size::SMALL))
                                .text_color(t.text2)
                                .child(why),
                        )
                    }),
            );
        }
        // The try under way, when the work is back at the step it loops to.
        if let Some((target, _)) = canvas::back_loop(mission, gate) {
            let state = crate::store::run_state::project(&run.events);
            let now = state.attempts.get(&target).copied().unwrap_or(0);
            let judged = verdicts.last().map(|e| e.attempt).unwrap_or(0);
            if now > judged
                && state.states.get(&target) == Some(&crate::store::run_state::CardState::Running)
            {
                let of = most.map(|r| format!(" of {r}")).unwrap_or_default();
                part = part.child(
                    div()
                        .flex()
                        .gap(px(TAB_GAP))
                        .child(div().text_color(t.text2).child(format!("Try {now}{of}")))
                        .child(
                            div()
                                .text_color(t.running)
                                .child(format!("{} is working on it", mission.card_name(&target))),
                        ),
                );
            }
        }
        if let Some(waiting) = run
            .waiting_gates
            .iter()
            .find(|w| w.gate_id == gate && w.pick.is_none())
        {
            let seq = waiting.requested_seq;
            // The same words as the run's own answer: what each answer does.
            let (mut approve, mut reject) = ("Approve".to_owned(), "Send back".to_owned());
            if let Some(flow::FlowStep {
                kind: flow::FlowStepKind::Gate { pass, fail, .. },
                ..
            }) = flow::build(mission)
                .sections
                .iter()
                .flat_map(|s| &s.steps)
                .find(|s| s.id == gate)
            {
                let alone = !crate::store::run_state::project(&run.events)
                    .states
                    .iter()
                    .any(|(id, state)| {
                        id != gate
                            && matches!(
                                state,
                                crate::store::run_state::CardState::Running
                                    | crate::store::run_state::CardState::Waiting
                            )
                    });
                approve = crate::store::gate::route(pass, "Approve", alone).1;
                reject = crate::store::gate::route(fail, "Send back", alone).1;
            }
            let mut actions = div().flex().flex_wrap().gap(px(TAB_GAP));
            for (word, verdict, kind) in [
                (approve, "pass", Kind::Primary),
                (reject, "fail", Kind::Plain),
            ] {
                let gate = gate.to_owned();
                actions = actions.child(
                    kit::button(
                        t,
                        SharedString::from(format!("panel-gate-{verdict}")),
                        word,
                        kind,
                    )
                    .debug_selector(move || format!("panel-gate-{verdict}"))
                    .on_click(cx.listener(move |p, _, _, cx| p.verdict(&gate, seq, verdict, cx))),
                );
            }
            part = part
                .child(div().text_color(t.needs).child("Waiting for you"))
                .children(self.run.responses.get(gate).map(|f| div().child(f.clone())))
                .child(actions);
        }
        Some(
            part.child(
                div()
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child("Changes below apply to the next run."),
            )
            .pb(px(TAB_GAP))
            .border_b_1()
            .border_color(t.divider),
        )
    }

    /// Who decides the gate, and what it takes from each kind of decider.
    pub(super) fn gate_decider_part(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        gate: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let mission = self.state.mission.as_ref()?;
        let g = mission.gates.iter().find(|g| g.id == gate)?;
        let mut chips = div().flex().gap(px(TAB_GAP));
        for (kind, word) in DECIDERS {
            let on = g.kinds.contains(&kind);
            let id = gate.to_owned();
            chips = chips.child(
                kit::button(
                    t,
                    SharedString::from(format!("decider-{}", kind.as_str())),
                    word,
                    if on { Kind::Plain } else { Kind::Quiet },
                )
                .debug_selector(move || format!("decider-{}", kind.as_str()))
                .on_click(cx.listener(move |p, _, _, cx| p.set_decider(&id, kind, cx))),
            );
        }
        let mut part = div()
            .flex()
            .flex_col()
            .gap(px(TAB_GAP))
            .child(kit::label(t, "Who decides"))
            .child(chips);
        if g.kinds.contains(&GateKind::Code) {
            part = part
                .child(kit::label(t, "Check"))
                .child(div().child(self.check.clone()))
                .child(kit::label(t, "Looks for"))
                .child(div().child(self.check_value.clone()))
                .when(self.editor_more, |d| {
                    d.child(kit::label(t, "Check version"))
                        .child(div().child(self.check_version.clone()))
                        .child(kit::label(t, "Files"))
                        .child(div().child(self.files.clone()))
                });
        }
        if g.kinds.contains(&GateKind::Step) {
            let judge = flow::build(mission)
                .judge_of
                .into_iter()
                .find(|(_, of)| of == gate)
                .and_then(|(judge, _)| mission.steps.iter().find(|f| f.id == judge));
            part = part.child(match judge {
                Some(f) => {
                    let who = f
                        .seats
                        .first()
                        .map(|s| {
                            crate::store::words::who(
                                s,
                                self.agents
                                    .as_ref()
                                    .and_then(|c| c.agents.iter().find(|a| a.id == s.agent)),
                            )
                        })
                        .unwrap_or_else(|| "nobody yet".into());
                    let id = f.id.clone();
                    left(
                        kit::button(t, "edit-judge", format!("{} · {who}", f.title), Kind::Quiet)
                            .tooltip(kit::tip("Edit the judge"))
                            .on_click(cx.listener(move |p, _, _, cx| p.edit_step(&id, cx))),
                    )
                }
                None => div()
                    .text_color(t.needs)
                    .child("No judge yet: wire a step into the gate's judge port."),
            });
        }
        if g.kinds.contains(&GateKind::Human) {
            part = part.child(
                div()
                    .text_color(t.text2)
                    .child("You answer it in Ahjo; the run waits for you."),
            );
        }
        Some(part)
    }

    /// Where a fail goes, and for a loop how many tries and what then.
    pub(super) fn gate_fail_part(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        gate: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let mission = self.state.mission.as_ref()?;
        let mut part = div()
            .flex()
            .flex_col()
            .gap(px(TAB_GAP))
            .child(kit::label(t, "When it fails"));
        let Some((target, limit)) = canvas::back_loop(mission, gate) else {
            let to = fail_target(mission, gate);
            return Some(part.child(div().text_color(t.text2).child(match to {
                Some(end) => format!("The run ends: {}", mission.card_name(&end)),
                None => "Nothing is wired to its fail exit yet.".into(),
            })));
        };
        part = part.child(
            div()
                .text_color(t.text2)
                .child(format!("Goes back to {}", mission.card_name(&target))),
        );
        let Some(limit) = limit else {
            let scope = target.clone();
            return Some(
                part.child(div().text_color(t.text2).child("Tries: until it passes"))
                    .child(left(
                        kit::button(t, "set-tries", "Set a number of tries", Kind::Quiet)
                            .debug_selector(|| "set-tries".into())
                            .on_click(cx.listener(move |p, _, _, cx| {
                                p.write_board(create_tries(&scope), WriteMode::Edit, cx)
                            })),
                    )),
            );
        };
        let rounds = limit.rounds.unwrap_or(1);
        let stepper = |id: &'static str, word: &'static str, to: i64| {
            let limit = limit.id.clone();
            kit::button(t, id, word, Kind::Plain)
                .debug_selector(move || id.into())
                .when(to < 1, |d| d.opacity(0.4))
                .on_click(cx.listener(move |p, _, _, cx| {
                    if to >= 1 {
                        p.write_board(set_tries(&limit, Some(to), None), WriteMode::Edit, cx)
                    }
                }))
        };
        part = part.child(
            div()
                .flex()
                .items_center()
                .gap(px(TAB_GAP))
                .child(div().w(px(56.)).text_color(t.text2).child("Tries"))
                .child(stepper("tries-less", "−", rounds - 1))
                .child(
                    div()
                        .w(px(28.))
                        .flex()
                        .justify_center()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(rounds.to_string()),
                )
                .child(stepper("tries-more", "+", rounds + 1)),
        );
        let mut then = div().flex().flex_wrap().gap(px(TAB_GAP));
        for (spent, word) in SPENT {
            let on = limit.when_spent == spent;
            let id = limit.id.clone();
            then = then.child(
                kit::button(
                    t,
                    SharedString::from(format!("spent-{}", spent.as_str())),
                    word,
                    if on { Kind::Plain } else { Kind::Quiet },
                )
                .debug_selector(move || format!("spent-{}", spent.as_str()))
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.write_board(set_tries(&id, None, Some(spent)), WriteMode::Edit, cx)
                })),
            );
        }
        part = part
            .child(div().text_color(t.text2).child("Then"))
            .child(then);
        // Another gate sending back to the same step spends the same tries.
        let shared: Vec<_> = mission
            .gates
            .iter()
            .filter(|o| {
                o.id != gate
                    && canvas::back_loop(mission, &o.id).is_some_and(|(to, _)| to == target)
            })
            .map(|o| mission.card_name(&o.id).to_owned())
            .collect();
        if !shared.is_empty() {
            part = part.child(
                div()
                    .text_size(px(size::SMALL))
                    .text_color(t.text2)
                    .child(format!(
                        "Shared with {}: both send back to {}.",
                        shared.join(", "),
                        mission.card_name(&target)
                    )),
            );
        }
        Some(part)
    }

    fn set_decider(&mut self, gate: &str, kind: GateKind, cx: &mut Context<Self>) {
        let patch = MissionChange::UpdateGate(GateUpdate {
            id: gate.into(),
            kinds: Some(vec![kind]),
            ..Default::default()
        });
        self.write_board(patch, WriteMode::Edit, cx);
    }
}

/// Where a gate's fail exit is wired, when that is not a step.
fn fail_target(mission: &Mission, gate: &str) -> Option<String> {
    let from = format!("{gate}:fail");
    let to = &mission.connections.iter().find(|c| c.from == from)?.to;
    to.split(':').next().map(str::to_owned)
}

fn set_tries(limit: &str, rounds: Option<i64>, when_spent: Option<WhenSpent>) -> MissionChange {
    MissionChange::UpdateLimit(LimitUpdate {
        id: limit.into(),
        rounds,
        when_spent,
        ..Default::default()
    })
}

fn create_tries(scope: &str) -> MissionChange {
    MissionChange::CreateLimit {
        title: "Tries".into(),
        scope: scope.into(),
        rounds: Some(3),
        seconds: None,
        warn_seconds: None,
        tokens: None,
        when_spent: WhenSpent::Fail,
        x: 0,
        y: 0,
    }
}
