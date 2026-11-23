//! The selected seat's agent, harness, model and effort pickers.
use super::*;
use crate::store::staffing::{self, Choice, Staffing};
use ahjo_app_kit::theme::Theme;
use gpui::{SharedString, Stateful};

#[derive(Clone, Copy)]
pub(super) enum Part {
    Agent,
    Harness,
    Model,
    Effort,
}

/// The staffing parts picked for one seat while another write was in flight,
/// in the order they were picked.
pub(super) struct StaffPick {
    step_id: String,
    seat_id: String,
    picks: Vec<(Part, String)>,
}

/// Put one picked value on a staffing; false when it does not parse.
fn apply(staff: &mut Staffing, part: Part, id: &str) -> bool {
    match part {
        Part::Agent => staff.agent = id.to_owned(),
        Part::Harness => match id.parse() {
            Ok(harness) => staff.harness = harness,
            Err(_) => return false,
        },
        Part::Model => staff.model = id.to_owned(),
        Part::Effort => match id.parse() {
            Ok(effort) => staff.effort = effort,
            Err(_) => return false,
        },
    }
    true
}

/// A choice from the old staffing menu, with the current choice highlighted.
fn chip(t: &Theme, id: SharedString, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(TAB_GAP))
        .py(px(size::RADIUS))
        .rounded(px(size::RADIUS))
        .border_1()
        .border_color(if selected { t.accent } else { t.divider })
        .when(selected, |d| d.bg(t.raised))
        .hover(|d| d.border_color(t.text2))
        .cursor_pointer()
}
impl MissionsPage {
    pub(super) fn read_agents(&mut self, node: String, cx: &mut Context<Self>) {
        let asked = self.host.call(
            NodeId(node),
            ahjo_harness_contract::agents::ListAgents {},
            cx,
        );
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let answer = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation == generation {
                    page.agents = answer.ok();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn draft_staffing(&self, cx: &Context<Self>) -> Staffing {
        Staffing {
            agent: self.agent.read(cx).text.clone(),
            harness: self.harness.read(cx).text.parse().unwrap_or_default(),
            model: self.model.read(cx).text.clone(),
            effort: self.effort.read(cx).text.parse().unwrap_or_default(),
        }
    }

    /// Draw all choices for the selected seat, preserving unknown current
    /// values while the catalog is loading or after it changes.
    pub(super) fn seat_pickers(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let staff = self.draft_staffing(cx);
        let choices = staffing::choices(self.agents.as_ref(), &staff);
        let row = |label: &'static str,
                   part: Part,
                   values: Vec<Choice>,
                   selected: &str,
                   cx: &mut Context<Self>| {
            let mut chips = div().flex().flex_wrap().gap(px(TAB_GAP));
            for choice in values {
                let on = choice.id == selected;
                let selector = format!(
                    "staffing-{}-{}{}",
                    label.to_lowercase(),
                    if choice.id.is_empty() {
                        "default"
                    } else if matches!(part, Part::Model)
                        && choice.id == ahjo_missions_contract::HARNESS_DEFAULT_MODEL
                    {
                        "harness-default"
                    } else {
                        &choice.id
                    },
                    if on { "-selected" } else { "" }
                );
                let id = choice.id;
                chips = chips.child(
                    chip(t, SharedString::from(selector.clone()), on)
                        .debug_selector(move || selector.clone())
                        .child(choice.label)
                        .on_click(cx.listener(move |page, _, _, cx| {
                            page.pick_staffing(part, &id, cx);
                        })),
                );
            }
            div()
                .flex()
                .flex_col()
                .gap(px(size::RADIUS))
                .child(kit::label(t, label))
                .child(chips)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(TAB_GAP))
            .child(row("Agent", Part::Agent, choices.agents, &staff.agent, cx))
            .child(row(
                "Harness",
                Part::Harness,
                choices.harnesses,
                staff.harness.as_str(),
                cx,
            ))
            .child(row("Model", Part::Model, choices.models, &staff.model, cx))
            .child(row(
                "Effort",
                Part::Effort,
                choices.efforts,
                staff.effort.as_str(),
                cx,
            ))
            .when(self.agents.is_none(), |d| {
                d.child(
                    div()
                        .text_color(t.dim)
                        .child("Reading what this node offers…"),
                )
            })
    }
    fn pick_staffing(&mut self, part: Part, id: &str, cx: &mut Context<Self>) {
        let mut staff = self.draft_staffing(cx);
        if !apply(&mut staff, part, id) {
            return;
        }
        self.show_staffing(&staff, cx);
        cx.notify();
        let (Some(editor), Some(seat_id)) = (&self.editor, &self.seat) else {
            return;
        };
        let (step_id, seat_id) = (editor.id.clone(), seat_id.clone());
        // A pick staffs the seat at once. A pick during a write keeps only
        // what was picked, to put on the seat as that write leaves it.
        if !self.writing {
            self.write_board(staff.assign(&step_id, &seat_id), WriteMode::Edit, cx);
        } else if let Some(pending) = self
            .staff_pending
            .iter_mut()
            .find(|pending| pending.step_id == step_id && pending.seat_id == seat_id)
        {
            pending.picks.push((part, id.to_owned()));
        } else {
            self.staff_pending.push(StaffPick {
                step_id,
                seat_id,
                picks: vec![(part, id.to_owned())],
            });
        }
    }

    fn show_staffing(&self, staff: &Staffing, cx: &mut Context<Self>) {
        for (field, value) in [
            (&self.agent, staff.agent.as_str()),
            (&self.harness, staff.harness.as_str()),
            (&self.model, staff.model.as_str()),
            (&self.effort, staff.effort.as_str()),
        ] {
            if field.read(cx).text != *value {
                field.update(cx, |f, cx| f.set(value, false, cx));
            }
        }
    }

    /// Every write's end calls this once `writing` is false: the picks made
    /// for a seat while it was in flight go on that seat as the write left
    /// it. With no mission to write them on, the operator is told they were
    /// not saved.
    pub(super) fn staff_picked_meanwhile(&mut self, cx: &mut Context<Self>) {
        if self.staff_pending.is_empty() {
            return;
        }
        let pending = self.staff_pending.remove(0);
        let seat = self
            .state
            .mission
            .as_ref()
            .and_then(|mission| mission.steps.iter().find(|step| step.id == pending.step_id))
            .and_then(|step| step.seats.iter().find(|seat| seat.id == pending.seat_id));
        let (Some(seat), Some(_)) = (seat, &self.state.mission_etag) else {
            self.staff_pending.clear();
            let lost = "The staffing picked meanwhile was not saved; pick it again.";
            self.state.error = Some(match self.state.error.take() {
                Some(error) => format!("{error} {lost}"),
                None => lost.to_owned(),
            });
            cx.notify();
            return;
        };
        let mut staff = Staffing::from(seat);
        for (part, id) in &pending.picks {
            apply(&mut staff, *part, id);
        }
        let shown = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.id == pending.step_id)
            && self.seat.as_deref() == Some(pending.seat_id.as_str());
        if shown {
            self.show_staffing(&staff, cx);
        }
        let patch = staff.assign(&pending.step_id, &pending.seat_id);
        self.write_board(patch, WriteMode::Queued, cx);
    }
}

impl MissionsPage {
    /// A step's repository, picked from those the node's config names; a
    /// step with none works in the run's folder.
    pub(super) fn repository_picker(
        &self,
        t: &Theme,
        field: &Entity<TextField>,
        prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let current = field.read(cx).text.clone();
        let names = self.repositories.clone().unwrap_or_default();
        let mut chips = div().flex().flex_wrap().gap(px(TAB_GAP));
        let known = current.is_empty() || names.contains(&current);
        for (name, label) in std::iter::once((String::new(), "None".to_owned()))
            .chain(names.iter().map(|n| (n.clone(), n.clone())))
            .chain((!known).then(|| (current.clone(), format!("{current} (not set up here)"))))
        {
            let on = name == current;
            let selector = format!(
                "{prefix}-{}{}",
                if name.is_empty() { "none" } else { &name },
                if on { "-selected" } else { "" }
            );
            let field = field.clone();
            chips = chips.child(
                chip(t, SharedString::from(selector.clone()), on)
                    .debug_selector(move || selector.clone())
                    .child(label)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        field.update(cx, |f, cx| f.set(&name, false, cx));
                        cx.notify();
                    })),
            );
        }
        let hint = match &self.repositories {
            None => "Reading what this node offers…",
            Some(names) if names.is_empty() => {
                "None set up on this node; add one under [missions] repositories in its config."
            }
            Some(_) => "Works on its own branch. Add more under [missions] repositories.",
        };
        div()
            .flex()
            .flex_col()
            .gap(px(size::RADIUS))
            .when(!names.is_empty() || !current.is_empty(), |d| d.child(chips))
            .child(div().text_color(t.dim).child(hint))
    }

    /// Read the repositories the node's config names.
    pub(super) fn read_repositories(&mut self, node: String, cx: &mut Context<Self>) {
        let listed = self.host.call(
            NodeId(node),
            ahjo_missions_contract::commands::MissionRepositories {},
            cx,
        );
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let listed = listed.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation == generation {
                    page.repositories = listed.ok().map(|listed| listed.repositories);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
