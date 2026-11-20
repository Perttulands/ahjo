//! A mission as defined, read top to bottom, and the head it wears while
//! its steps are changed on the canvas.
use super::*;
use ahjo_app_kit::menu::MenuRow;
use ahjo_app_kit::theme::Theme;
use ahjo_app_kit::widgets::pane::{self, Col};
use ahjo_missions_contract::{MISSION_THREAD, Mission};
use gpui::{AnyElement, SharedString};

/// Runs listed under a mission; the rest are in the list's filter.
const RUNS_SHOWN: usize = 20;
const RUN_NO_W: f32 = 48.;
const OUTCOME_W: f32 = 100.;
const WHEN_W: f32 = 110.;

impl MissionsPage {
    /// What an empty mission says in Flow and on the canvas: how to begin.
    pub(super) fn empty_mission(
        &self,
        t: &Theme,
        selector: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let button = format!("{selector}-input");
        card(t)
            .debug_selector(move || selector.into())
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("This mission is empty"),
            )
            .child(div().text_color(t.text2).child(
                "Start with an Input: what you hand the mission when you run it. \
                 Then add a step with the + on its card, and an End where the work is done.",
            ))
            .child(
                div().flex().gap(px(TAB_GAP)).child(
                    kit::button(
                        t,
                        SharedString::from(button.clone()),
                        "Add the Input",
                        Kind::Primary,
                    )
                    .debug_selector(move || button.clone())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|page, _, _, cx| {
                        page.mission_canvas_mode = true;
                        page.add_card(board::Kind::Input, cx);
                    })),
                ),
            )
    }

    /// A mission as defined, read top to bottom: what it asks for and
    /// gives back, its steps and who does each, then its runs.
    pub(super) fn mission_detail(
        &self,
        t: &Theme,
        mission: &Mission,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        let head = self.mission_head(t, mission, cx);
        if mission.input_cards.is_empty() && mission.formations.is_empty() {
            let button = "flow-empty-input";
            let body = pane::empty(
                t,
                "Nothing in this mission yet.",
                Some(
                    kit::button(t, button, "Define it", Kind::Primary)
                        .debug_selector(move || button.into())
                        .on_click(cx.listener(|page, _, _, cx| {
                            page.mission_canvas_mode = true;
                            page.add_card(board::Kind::Input, cx);
                        }))
                        .into_any_element(),
                ),
            )
            .debug_selector(|| "flow-empty".into());
            return (head, body);
        }
        let flow = flow::build(mission);
        let mut body = div().flex().flex_col().gap(px(size::SECTION_GAP));
        body = body.child(self.intent(t, mission, cx));
        let mut about = div().flex().flex_col();
        for card in &mission.input_cards {
            let selector = format!("flow-input-{}", card.id);
            let mut lines = div()
                .flex()
                .flex_col()
                .debug_selector(move || selector.clone());
            if !card.goal.trim().is_empty() {
                lines = lines.child(
                    div()
                        .pb(px(size::HEAD_GAP))
                        .text_color(t.text2)
                        .child(card.goal.trim().to_owned()),
                );
            }
            let asks = card
                .inputs
                .iter()
                .map(|value| {
                    let mut words = value.name.clone();
                    if !value.required {
                        words.push_str(" (optional)");
                    }
                    if value.kind != "text" && !value.kind.is_empty() {
                        words.push_str(&format!(", a {}", value.kind));
                    }
                    if !value.description.is_empty() {
                        words.push_str(&format!(": {}", value.description));
                    }
                    words
                })
                .collect::<Vec<_>>();
            lines = lines.child(pane::line(
                t,
                "Asks for",
                if asks.is_empty() {
                    // A run of a mission that declares no inputs is given a brief.
                    "a brief".to_owned()
                } else {
                    asks.join("; ")
                },
            ));
            let gives = card
                .outputs
                .iter()
                .map(|value| {
                    let from = value
                        .from
                        .split_once(':')
                        .map(|(step, _)| format!(" from {}", mission.node_name(step)))
                        .unwrap_or_else(|| " (not wired yet)".into());
                    let kind = if value.kind.is_empty() || value.kind == "text" {
                        String::new()
                    } else {
                        format!(", a {}", value.kind)
                    };
                    format!("{}{kind}{from}", value.name)
                })
                .collect::<Vec<_>>();
            lines = lines.child(pane::line(
                t,
                "Gives back",
                if gives.is_empty() {
                    "nothing declared".to_owned()
                } else {
                    gives.join("; ")
                },
            ));
            if super::canvas::asks_you(mission) {
                lines = lines.child(pane::line(t, "Asks you", "at its gates"));
            }
            lines = lines
                .children(self.limits_on(t, mission, &card.id, cx))
                .child(div().flex().child(self.note_act(t, &card.id, cx)))
                .children(self.flow_thread(t, &card.id, cx))
                .children(self.flow_wire_threads(t, mission, &card.id, cx));
            about = about.child(lines);
        }
        body = body.child(about);
        let mut steps = pane::section(t, "Steps");
        for step in flow.sections.iter().flat_map(|s| &s.steps) {
            steps = steps.child(self.step_block(t, mission, step, cx));
        }
        body = body.child(steps);
        if !mission.ends.is_empty() {
            let mut ends = pane::section(t, "Ends");
            for end in &mission.ends {
                ends = ends.child(
                    div()
                        .flex()
                        .flex_col()
                        .pb(px(size::HEAD_GAP))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(size::HEAD_GAP))
                                .child(pane::cell({
                                    let name = mission.node_name(&end.id);
                                    if name.eq_ignore_ascii_case(&end.outcome) {
                                        name.to_owned()
                                    } else {
                                        format!("{name} ({})", end.outcome)
                                    }
                                }))
                                .child(self.note_act(t, &end.id, cx)),
                        )
                        .children(self.flow_thread(t, &end.id, cx)),
                );
            }
            body = body.child(ends);
        }
        body = body.children(self.mission_runs(t, cx));
        (head, body)
    }

    /// The mission's head: its name, Start, Edit, and the rest in a menu.
    fn mission_head(&self, t: &Theme, mission: &Mission, cx: &mut Context<Self>) -> Div {
        let title = if mission.title.trim().is_empty() {
            mission.slug.clone()
        } else {
            mission.title.clone()
        };
        let ready = self.mission_run_ready();
        let complete = self.validation_complete();
        // An empty mission's body already says how to begin.
        let empty = mission.input_cards.is_empty() && mission.formations.is_empty();
        // Start only when it can run; until then, what it still lacks,
        // one click from the canvas where it is done.
        let start = if ready {
            kit::button(t, "prepare-run", "Start", Kind::Primary)
                .debug_selector(|| "prepare-run".into())
                .tooltip(kit::tip("Give it what it asks for and set it off"))
                .on_click(cx.listener(|p, _, _, cx| p.prepare_run(cx)))
                .into_any_element()
        } else {
            let todo = self.todo(mission).len();
            if complete && todo > 0 && !empty {
                pane::act(t, "mission-todo", format!("{todo} to do before it can run"))
                    .debug_selector(|| "mission-todo".into())
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.mission_canvas_mode = true;
                        cx.notify();
                    }))
                    .into_any_element()
            } else {
                div().into_any_element()
            }
        };
        let failed = self.validation_failed();
        pane::head(t)
            .child(pane::title(t, title))
            .child(
                div()
                    .debug_selector(move || {
                        if complete {
                            "mission-validation-complete".into()
                        } else {
                            "mission-validation-pending".into()
                        }
                    })
                    .child(
                        div()
                            .debug_selector(move || {
                                if ready {
                                    "mission-run-ready".into()
                                } else {
                                    "mission-run-quiet".into()
                                }
                            })
                            .child(start),
                    ),
            )
            .when(!empty, |d| {
                d.child(
                    pane::act(t, "mission-mode-canvas", "Edit")
                        .debug_selector(|| "mission-mode-canvas".into())
                        .tooltip(kit::tip("Change its steps and who does them"))
                        .on_click(cx.listener(|p, _, _, cx| {
                            p.mission_canvas_mode = true;
                            cx.notify();
                        })),
                )
            })
            .child(
                pane::act(t, "mission-more", "…")
                    .debug_selector(|| "mission-more".into())
                    .on_click(cx.listener(move |p, e: &gpui::ClickEvent, _, cx| {
                        let page = cx.entity().downgrade();
                        let mut rows = Vec::new();
                        if failed {
                            let page = page.clone();
                            rows.push(MenuRow::new("Check again", move |_, cx| {
                                let _ = page.update(cx, |p, cx| {
                                    p.validation.clear();
                                    p.sync_validation(cx);
                                    cx.notify();
                                });
                            }));
                        }
                        let rename = page.clone();
                        rows.push(MenuRow::new("Rename…", move |window, cx| {
                            let _ = rename.update(cx, |p, cx| p.open_mission_rename(window, cx));
                        }));
                        rows.push(MenuRow::new("Delete…", move |window, cx| {
                            let _ = page.update(cx, |p, cx| p.open_mission_archive(window, cx));
                        }));
                        p.host.menu(rows, e.position(), cx);
                    })),
            )
    }

    /// The head while the mission's steps are being changed on the canvas.
    pub(super) fn edit_head(&self, t: &Theme, mission: &Mission, cx: &mut Context<Self>) -> Div {
        let title = if mission.title.trim().is_empty() {
            mission.slug.clone()
        } else {
            mission.title.clone()
        };
        let busy = self.writing;
        let complete = self.validation_complete();
        let add = |id: &'static str, text: &'static str, kind: board::Kind| {
            pane::act(t, id, text)
                .debug_selector(move || id.into())
                .on_click(cx.listener(move |p, _, _, cx| p.add_card(kind, cx)))
        };
        pane::head(t)
            .child(
                pane::title(t, format!("Editing {title}"))
                    .child(div().debug_selector(move || {
                        if complete {
                            "mission-validation-complete".into()
                        } else {
                            "mission-validation-pending".into()
                        }
                    }))
                    .child(div().debug_selector(move || {
                        if busy {
                            "board-busy".into()
                        } else {
                            "board-ready".into()
                        }
                    })),
            )
            .when(mission.input_cards.is_empty(), |d| {
                d.child(add("new-input", "Add input", board::Kind::Input))
            })
            .child(add("new-step", "Add step", board::Kind::Formation))
            // Undo and Redo only when there is something to undo or redo.
            .when(self.history.can_undo(), |d| {
                d.child(
                    pane::act(t, "undo", "Undo")
                        .debug_selector(move || {
                            if busy {
                                "undo-busy".into()
                            } else {
                                "undo".into()
                            }
                        })
                        .on_click(cx.listener(|p, _, _, cx| p.undo(false, cx))),
                )
            })
            .when(self.history.can_redo(), |d| {
                d.child(
                    pane::act(t, "redo", "Redo")
                        .debug_selector(move || {
                            if busy {
                                "redo-busy".into()
                            } else {
                                "redo".into()
                            }
                        })
                        .on_click(cx.listener(|p, _, _, cx| p.undo(true, cx))),
                )
            })
            .child(
                // Gates, ends and the mission's note are rarer than steps.
                pane::act(t, "edit-more", "…")
                    .debug_selector(|| "edit-more".into())
                    .tooltip(kit::tip(
                        "Add a gate, a limit or an end, or note the mission",
                    ))
                    .on_click(cx.listener(|p, e: &gpui::ClickEvent, _, cx| {
                        let page = cx.entity().downgrade();
                        let add = |label: &'static str, kind: board::Kind| {
                            let page = page.clone();
                            MenuRow::new(label, move |_, cx| {
                                let _ = page.update(cx, |p, cx| p.add_card(kind, cx));
                            })
                        };
                        let mut rows = vec![
                            add("Add gate", board::Kind::Gate),
                            add("Add limit", board::Kind::Limit),
                            add("Add end", board::Kind::End),
                        ];
                        rows.push(MenuRow::new("Note on the mission", move |window, cx| {
                            let _ = page.update(cx, |p, cx| {
                                p.open_canvas_note(MISSION_THREAD.into(), window, cx)
                            });
                        }));
                        p.host.menu(rows, e.position(), cx);
                    })),
            )
            // One Done at a time: while a card is open, its own Done.
            .when(self.editor.is_none(), |d| {
                d.child(
                    kit::button(t, "mission-mode-flow", "Done", Kind::Plain)
                        .debug_selector(|| "mission-mode-flow".into())
                        .on_click(cx.listener(|p, _, _, cx| {
                            p.mission_canvas_mode = false;
                            cx.notify();
                        })),
                )
            })
    }

    /// One step as read: its number and name, who does it, its brief, and
    /// where it goes next.
    fn step_block(
        &self,
        t: &Theme,
        mission: &Mission,
        step: &flow::Step,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let id = step.id.clone();
        let selector = format!("flow-step-{id}");
        let edit_selector = format!("flow-edit-{id}");
        // A step asked for by name stands out, marked as on the canvas.
        let revealed = self.reveal_card.as_deref() == Some(step.id.as_str());
        // Chosen here or on the board beside: marked on both.
        let chosen = self.beside && self.flow_focus.as_deref() == Some(step.id.as_str());
        let marker = format!("mission-reveal-{id}");
        let chosen_marker = format!("flow-chosen-{id}");
        let places = self.flow_places.clone();
        let place = step.id.clone();
        let title_id = step.id.clone();
        let mut block = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .relative()
            .flex()
            .flex_col()
            .pb(px(size::PAD))
            // Where the step stands, for the board beside to scroll to it.
            .child(
                gpui::canvas(
                    move |bounds, _, _| {
                        places
                            .borrow_mut()
                            .insert(place.clone(), f32::from(bounds.origin.y));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .when(revealed, |d| {
                d.child(div().debug_selector(move || marker.clone()))
                    .border_l_2()
                    .border_color(t.accent)
                    .pl(px(size::ROW_PAD))
            })
            .when(chosen && !revealed, |d| {
                d.child(div().debug_selector(move || chosen_marker.clone()))
                    .border_l_2()
                    .border_color(t.accent)
                    .pl(px(size::ROW_PAD))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(size::HEAD_GAP))
                    .min_h(px(size::ROW))
                    .child(
                        pane::cell(format!("{}  {}", step.number, mission.node_name(&step.id)))
                            .font_weight(FontWeight::SEMIBOLD)
                            .id(SharedString::from(format!("flow-title-{title_id}")))
                            .debug_selector({
                                let selector = format!("flow-title-{title_id}");
                                move || selector
                            })
                            .when(self.beside, |d| {
                                d.cursor_pointer()
                                    .on_click(cx.listener(move |page, _, _, cx| {
                                        page.choose_in_flow(&title_id, false, cx)
                                    }))
                            }),
                    )
                    .child(
                        pane::act(t, SharedString::from(edit_selector.clone()), "Edit")
                            .debug_selector(move || edit_selector.clone())
                            .on_click(cx.listener(move |page, _, _, cx| page.edit_step(&id, cx))),
                    )
                    .child(self.note_act(t, &step.id, cx)),
            );
        // The lines under a step's title, at text size in the secondary
        // colour: who does it, where it leads, what decides. The brief is
        // what the step is, so it reads in the text colour.
        let sub = |text: String| div().text_color(t.text2).child(text);
        let quiet = |text: String| div().text_color(t.text).child(text);
        match &step.kind {
            flow::StepKind::Formation { next } => {
                if let Some(formation) = mission.formations.iter().find(|f| f.id == step.id) {
                    if formation.slots.is_empty() {
                        block = block.child(
                            div()
                                .text_color(t.error)
                                .child("Nobody does this step yet."),
                        );
                    } else if formation.slots.iter().all(|slot| slot.harness.is_empty()) {
                        // A sketched step: who does it is still open.
                        block = block.child(sub("Nobody chosen to do it yet".into()));
                    } else {
                        // Who, not the session they will work in: that is
                        // the step's own detail.
                        let who = formation
                            .slots
                            .iter()
                            .map(crate::store::words::who)
                            .collect::<Vec<_>>()
                            .join(", ");
                        block = block.child(sub(who));
                    }
                    if let Some(brief) = &formation.brief
                        && !brief.goal.trim().is_empty()
                    {
                        block = block.child(
                            quiet(brief.goal.trim().to_owned())
                                .pt(px(size::LABEL_BOTTOM))
                                .line_clamp(3),
                        );
                    }
                }
                block = block.child(sub(format!("then {}", run::target_words(next))));
                block = block.children(self.flow_wire_threads(t, mission, &step.id, cx));
            }
            flow::StepKind::Gate {
                deciders,
                judges,
                pass,
                fail,
            } => {
                let who = deciders
                    .iter()
                    .map(|d| match d {
                        flow::Decider::You => "you",
                        flow::Decider::Judge => "a judge",
                        flow::Decider::Code => "a check",
                    })
                    .collect::<Vec<_>>()
                    .join(" and ");
                block = block.child(sub(format!("decided by {who}")));
                if let Some(gate) = mission.gates.iter().find(|g| g.id == step.id)
                    && !gate.criterion.is_empty()
                {
                    block = block.child(quiet(gate.criterion.clone()).pt(px(size::LABEL_BOTTOM)));
                }
                for judge in judges {
                    if let Some(formation) = mission.formations.iter().find(|f| &f.id == judge) {
                        block = block.child(sub(format!(
                            "judged by {}: {}",
                            mission.node_name(judge),
                            formation
                                .slots
                                .iter()
                                .map(crate::store::words::who)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )));
                        block = block.children(self.flow_thread(t, judge, cx));
                    }
                }
                block = block.child(sub(format!(
                    "pass: {}; fail: {}",
                    run::target_words(pass),
                    run::target_words(fail)
                )));
                block = block.children(self.flow_wire_threads(t, mission, &step.id, cx));
            }
        }
        if step.parallel.len() > 1 {
            block = block.child(sub(format!(
                "alongside {}",
                step.parallel
                    .iter()
                    .filter(|other| **other != step.id)
                    .map(|other| mission.node_name(other))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        block = block.children(self.flow_thread(t, &step.id, cx));
        block.children(self.limits_on(t, mission, &step.id, cx))
    }

    /// The limits on a step, or on the whole mission when on its Input:
    /// what each caps and what happens then, in the colour of a stop.
    fn limits_on(
        &self,
        t: &Theme,
        mission: &Mission,
        target: &str,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        for limit in mission.limits.iter().filter(|limit| limit.target == target) {
            out.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(size::HEAD_GAP))
                    .child(
                        div()
                            .text_color(t.needs)
                            .child(format!("Limit: {}", crate::store::words::limit_line(limit))),
                    )
                    .child(self.note_act(t, &limit.id, cx))
                    .into_any_element(),
            );
            out.extend(self.flow_thread(t, &limit.id, cx));
        }
        out
    }

    /// What the mission is for, in the operator's words, first: the
    /// mission's own notes, with a field to write them in.
    fn intent(&self, t: &Theme, mission: &Mission, cx: &mut Context<Self>) -> Div {
        let mut section = pane::section(t, "What it is for");
        let changed = (!mission.updated_at.is_empty()).then(|| {
            let when = crate::store::words::short_time(
                &mission.updated_at,
                time::OffsetDateTime::now_utc(),
            );
            match mission.updated_by.split_once(':') {
                Some(("agent", name)) => format!("Changed {when} by {name}"),
                _ => format!("Changed {when}"),
            }
        });
        match self.flow_thread(t, MISSION_THREAD, cx) {
            Some(thread) => section = section.child(thread),
            None => {
                let button = "flow-intent";
                section = section.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(size::HEAD_GAP))
                        .child(div().flex_1().min_w_0().text_color(t.text2).child(
                            "Write what you are making, as roughly as you like: an agent \
                             reads it with ahjo mission show and builds from it.",
                        ))
                        .child(
                            kit::button(t, button, "Write it", Kind::Primary)
                                .flex_none()
                                .debug_selector(move || button.into())
                                .on_click(cx.listener(|page, _, window, cx| {
                                    page.open_canvas_note(MISSION_THREAD.into(), window, cx)
                                })),
                        ),
                );
            }
        }
        if self.note_paper.open.as_deref() != Some(MISSION_THREAD)
            && self.notes.as_ref().is_some_and(|n| !n.mission.is_empty())
        {
            section = section.child(div().flex().child(self.note_act(t, MISSION_THREAD, cx)));
        }
        section.children(changed.map(|words| pane::meta(t, words, None)))
    }

    /// The mission's runs, newest first; each opens.
    fn mission_runs(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        const COLS: [Col; 4] = [
            Col::fixed("Run", RUN_NO_W),
            Col::fill("Given"),
            Col::fixed("Outcome", OUTCOME_W),
            Col::fixed("When", WHEN_W),
        ];
        let (node, slug) = self.state.selected.clone()?;
        let group = crate::store::navigator::groups(&self.state)
            .into_iter()
            .find(|g| g.node == node && g.slug == slug)?;
        if group.runs.is_empty() {
            return None;
        }
        let now = time::OffsetDateTime::now_utc();
        let mut runs = group.runs;
        runs.sort_by_key(|row| std::cmp::Reverse(row.number));
        let rows = runs
            .into_iter()
            .take(RUNS_SHOWN)
            .map(|row| {
                let colour = match row.run.status.as_str() {
                    "failed" => t.error,
                    "waiting_human" | "blocked" => t.needs,
                    _ if !row.run.r#final => t.running,
                    _ => t.text2,
                };
                let word = match row.run.status.as_str() {
                    "succeeded" => "done",
                    "canceled" => "stopped",
                    other => crate::store::words::state_word(other),
                }
                .to_owned();
                let (node, slug, id) = (
                    row.node.clone(),
                    row.run.mission_slug.clone(),
                    row.run.run_id.clone(),
                );
                let selector = format!("mission-run-{id}");
                let hover = t.accent_soft();
                pane::table_row(
                    t,
                    &COLS,
                    vec![
                        pane::cell(row.number.to_string()).into_any_element(),
                        pane::cell(row.about.clone()).into_any_element(),
                        pane::cell(word).text_color(colour).into_any_element(),
                        pane::cell(crate::store::words::short_time(&row.run.started_at, now))
                            .into_any_element(),
                    ],
                )
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .cursor_pointer()
                .hover(move |d| d.bg(hover))
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.choose_run(node.clone(), slug.clone(), id.clone(), cx)
                }))
                .into_any_element()
            })
            .collect();
        Some(pane::section(t, "Runs").child(pane::table(t, &COLS, rows)))
    }
}

/// A card on the Flow view.
fn card(t: &Theme) -> Div {
    div()
        .p(px(PAD))
        .rounded(px(size::RADIUS))
        .border_1()
        .border_color(t.divider)
        .bg(t.surface)
        .flex()
        .flex_col()
        .gap(px(TAB_GAP))
}
