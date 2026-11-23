//! Three-way editor save: only fields changed by the operator are written,
//! and a simultaneous edit of the same field is refused.
use super::*;
use crate::store::staffing::Staffing;
use ahjo_missions_contract::{
    EndUpdate, GateUpdate, InputCardUpdate, LimitUpdate, Mission, MissionInput, MissionOutput,
};

fn changed<T: PartialEq>(
    name: &str,
    base: &T,
    draft: &T,
    now: &T,
    problems: &mut Vec<String>,
) -> bool {
    if draft == base {
        return false;
    }
    if now != base {
        problems.push(format!("{name} was changed elsewhere while this window was open. Close it and open the card again to see the change."));
        false
    } else {
        true
    }
}
fn lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect()
}
fn fields(snapshot: &EditorSnapshot, index: usize) -> &String {
    &snapshot.fields[index]
}
fn number(value: &str, name: &str) -> Result<i64, String> {
    if value.trim().is_empty() {
        return Ok(0);
    }
    value
        .trim()
        .parse()
        .map_err(|_| format!("{name} must be a whole number"))
}
fn enum_value<T: std::str::FromStr<Err = String>>(
    raw: &str,
    problems: &mut Vec<String>,
) -> Option<T> {
    match raw.trim().parse() {
        Ok(value) => Some(value),
        Err(error) => {
            problems.push(error);
            None
        }
    }
}
fn input_values(inputs: &[MissionInput]) -> Vec<(String, String, String, bool)> {
    inputs
        .iter()
        .map(|i| {
            (
                i.name.clone(),
                i.description.clone(),
                i.kind.as_str().to_owned(),
                i.required,
            )
        })
        .collect()
}
fn output_values(outputs: &[MissionOutput]) -> Vec<(String, String, String, bool, String)> {
    outputs
        .iter()
        .map(|o| {
            (
                o.name.clone(),
                o.description.clone(),
                o.kind.as_str().to_owned(),
                o.required,
                o.from.clone(),
            )
        })
        .collect()
}

/// The full set of changed fields, with conflicts found before any write.
#[allow(clippy::too_many_arguments)]
fn operations(
    kind: EditKind,
    id: &str,
    seat: Option<&str>,
    port: Option<&str>,
    limit: Option<&str>,
    base: &EditorSnapshot,
    draft: &EditorSnapshot,
    now: &Mission,
) -> Result<Vec<undo::Step>, Vec<String>> {
    let mut ops = Vec::new();
    let mut problems = Vec::new();
    macro_rules! compare {
        ($index:expr, $name:expr, $current:expr) => {
            changed(
                $name,
                fields(base, $index),
                fields(draft, $index),
                &$current.to_string(),
                &mut problems,
            )
        };
    }
    macro_rules! drafted {
        ($index:expr, $name:expr, $current:expr) => {
            compare!($index, $name, $current).then(|| draft.fields[$index].clone())
        };
    }
    macro_rules! drafted_enum {
        ($index:expr, $name:expr, $current:expr) => {
            drafted!($index, $name, $current).and_then(|value| enum_value(&value, &mut problems))
        };
    }
    match kind {
        EditKind::Step => {
            let Some(f) = now.steps.iter().find(|f| f.id == id) else {
                return Err(vec!["The step is gone: it was deleted elsewhere.".into()]);
            };
            let original = f.brief.clone().unwrap_or_default();
            let mut brief = original.clone();
            let title_changed = compare!(0, "The title", &f.title);
            let mut brief_changed = false;
            if compare!(1, "The brief", &original.goal) {
                brief.goal = draft.fields[1].clone();
                brief_changed = true;
            }
            if compare!(3, "The repository", &original.repository) {
                brief.repository = draft.fields[3].clone();
                brief_changed = true;
            }
            if compare!(4, "The issue reference", &original.issue) {
                brief.issue = draft.fields[4].clone();
                brief_changed = true;
            }
            if compare!(5, "The files", &original.files.join("\n")) {
                brief.files = lines(&draft.fields[5]);
                brief_changed = true;
            }
            if compare!(6, "The links", &original.links.join("\n")) {
                brief.links = lines(&draft.fields[6]);
                brief_changed = true;
            }
            if title_changed || brief_changed {
                ops.push(undo::Step::board(MissionChange::EditStep {
                    id: id.into(),
                    title: if title_changed {
                        draft.fields[0].clone()
                    } else {
                        f.title.clone()
                    },
                    brief: Some(brief),
                }));
            }
            if compare!(2, "Where the step is placed", &f.environment) {
                ops.push(undo::Step::board(edits::environment_patch(
                    id,
                    &draft.fields[2],
                )));
            }
            if let Some(seat) = seat.filter(|_| base.fields[10..14] != draft.fields[10..14]) {
                let Some(current) = f.seats.iter().find(|s| s.id == seat) else {
                    return Err(vec![
                        "The selected seat is gone: it was deleted elsewhere.".into(),
                    ]);
                };
                let current = Staffing::from(current);
                let update = Staffing {
                    agent: drafted!(10, "The seat's agent", &current.agent)
                        .unwrap_or_else(|| current.agent.clone()),
                    harness: drafted_enum!(11, "The seat's harness", &current.harness)
                        .unwrap_or(current.harness),
                    model: drafted!(12, "The seat's model", &current.model)
                        .unwrap_or_else(|| current.model.clone()),
                    effort: drafted_enum!(13, "The seat's effort", &current.effort)
                        .unwrap_or(current.effort),
                };
                if update != current {
                    ops.push(undo::Step::board(update.assign(id, seat)));
                }
            }
            if let Some(port) = port.filter(|_| base.fields[14..16] != draft.fields[14..16]) {
                let Some(current) = f.inputs.iter().chain(&f.outputs).find(|p| p.id == port) else {
                    return Err(vec![
                        "The selected port is gone: it was deleted elsewhere.".into(),
                    ]);
                };
                let label = drafted!(14, "The port label", &current.label);
                let kind = drafted_enum!(15, "The port kind", &current.kind);
                if label.is_some() || kind.is_some() {
                    ops.push(undo::Step::board(MissionChange::UpdatePort {
                        step_id: id.into(),
                        port_id: port.into(),
                        label,
                        kind,
                    }));
                }
            }
        }
        EditKind::Input => {
            let Some(card) = now.input_cards.iter().find(|c| c.id == id) else {
                return Err(vec![
                    "The Input card is gone: it was deleted elsewhere.".into(),
                ]);
            };
            let mut update = InputCardUpdate {
                title: drafted!(0, "The title", &card.title),
                goal: drafted!(1, "The goal", &card.goal),
                ..Default::default()
            };
            if draft.inputs.iter().any(|i| i.0.trim().is_empty()) {
                problems.push("Every input needs a name.".into());
            }
            if changed(
                "What a run takes",
                &base.inputs,
                &draft.inputs,
                &input_values(&card.inputs),
                &mut problems,
            ) {
                let inputs: Vec<_> = (draft.inputs.iter())
                    .filter_map(|(name, description, kind, required)| {
                        Some(MissionInput {
                            name: name.trim().to_owned(),
                            description: description.clone(),
                            kind: enum_value(kind, &mut problems)?,
                            required: *required,
                        })
                    })
                    .collect();
                update.inputs = Some(inputs);
            }
            if draft.outputs.iter().any(|o| o.0.trim().is_empty()) {
                problems.push("Every output needs a name.".into());
            }
            if changed(
                "The outputs",
                &base.outputs,
                &draft.outputs,
                &output_values(&card.outputs),
                &mut problems,
            ) {
                let outputs: Vec<_> = draft
                    .outputs
                    .iter()
                    .filter_map(|(name, description, kind, required, from)| {
                        Some(MissionOutput {
                            name: name.clone(),
                            description: description.clone(),
                            kind: enum_value(kind, &mut problems)?,
                            required: *required,
                            from: from.clone(),
                        })
                    })
                    .collect();
                update.outputs = Some(outputs);
            }
            if update != InputCardUpdate::default() {
                update.id = id.into();
                ops.push(undo::Step::board(MissionChange::UpdateInputCard(update)));
            }
        }
        EditKind::Gate => {
            let Some(gate) = now.gate(id) else {
                return Err(vec!["The gate is gone: it was deleted elsewhere.".into()]);
            };
            let mut update = GateUpdate {
                title: drafted!(0, "The title", &gate.title),
                criterion: drafted!(1, "The criterion", &gate.criterion),
                files: drafted!(5, "The files", gate.files.join("\n")).map(|files| lines(&files)),
                check: drafted!(7, "The check", &gate.check),
                check_version: drafted!(8, "The check version", &gate.check_version),
                check_value: drafted!(9, "The check value", &gate.check_value),
                ..Default::default()
            };
            if update != GateUpdate::default() {
                update.id = id.into();
                ops.push(undo::Step::board(MissionChange::UpdateGate(update)));
            }
        }
        EditKind::End => {
            let Some(end) = now.end(id) else {
                return Err(vec!["The End is gone: it was deleted elsewhere.".into()]);
            };
            let mut update = EndUpdate {
                title: drafted!(0, "The title", &end.title),
                ..Default::default()
            };
            if update != EndUpdate::default() {
                update.id = id.into();
                ops.push(undo::Step::board(MissionChange::UpdateEnd(update)));
            }
        }
    }
    if let Some(id) = limit.filter(|_| base.fields[16..22] != draft.fields[16..22]) {
        let Some(limit) = now.limits.iter().find(|l| l.id == id) else {
            return Err(vec![
                "The selected limit is gone: it was deleted elsewhere.".into(),
            ]);
        };
        let title = drafted!(16, "The limit title", &limit.title);
        let mut knob = |index: usize, name: &str, current: Option<i64>| {
            let current = current.map(|value| value.to_string()).unwrap_or_default();
            if compare!(index, name, current) {
                match number(&draft.fields[index], name) {
                    Ok(value) => return Some(value),
                    Err(error) => problems.push(error),
                }
            }
            None
        };
        let mut update = LimitUpdate {
            title,
            rounds: knob(17, "The limit rounds", limit.rounds),
            seconds: knob(18, "The limit time", limit.seconds),
            warn_seconds: knob(19, "The warning time", limit.warn_seconds),
            tokens: knob(20, "The token limit", limit.tokens),
            when_spent: drafted_enum!(21, "What the limit does", &limit.when_spent),
            ..Default::default()
        };
        if update != LimitUpdate::default() {
            update.id = id.into();
            ops.push(undo::Step::board(MissionChange::UpdateLimit(update)));
        }
    }
    if problems.is_empty() {
        Ok(ops)
    } else {
        Err(problems)
    }
}
impl MissionsPage {
    pub(super) fn save_editor_merged(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        if editor.busy || self.writing {
            return;
        }
        let Some(base) = editor.baseline.clone() else {
            return;
        };
        let draft = self.editor_snapshot(editor, cx);
        let (Some((node, slug)), Some(_)) =
            (self.state.selected.clone(), self.state.mission.as_ref())
        else {
            return;
        };
        let (kind, id) = (editor.kind, editor.id.clone());
        let seat = self.seat.clone();
        let port = self.port.clone();
        let limit = self.limit.clone();
        if let Some(editor) = &mut self.editor {
            editor.busy = true;
            editor.error = None;
        }
        self.writing = true;
        let asked = read_definition(&self.host, NodeId(node.clone()), slug.clone(), cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let answer = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation {
                    return;
                }
                let response = match answer {
                    Ok(response) => response,
                    Err(error) => {
                        page.writing = false;
                        if let Some(editor) = &mut page.editor {
                            editor.busy = false;
                            editor.error = Some(format!("{}: {}", error.code, error.message));
                        }
                        cx.notify();
                        return;
                    }
                };
                let ops = operations(
                    kind,
                    &id,
                    seat.as_deref(),
                    port.as_deref(),
                    limit.as_deref(),
                    &base,
                    &draft,
                    &response.mission,
                );
                page.state.mission_etag = Some(response.etag);
                page.state.mission = Some(response.mission);
                match ops {
                    Err(problems) => {
                        page.writing = false;
                        if let Some(editor) = &mut page.editor {
                            editor.busy = false;
                            editor.error = Some(problems.join("; "));
                        }
                        cx.notify();
                    }
                    Ok(steps) if steps.is_empty() => {
                        page.writing = false;
                        let unchanged = page
                            .editor
                            .as_ref()
                            .is_none_or(|editor| page.editor_snapshot(editor, cx) == draft);
                        let keep = page.editor.as_ref().is_some_and(|e| e.keep_open);
                        if unchanged && !keep {
                            page.editor = None;
                            page.resolve_wanted(cx);
                        } else if let Some(editor) = &mut page.editor {
                            editor.busy = false;
                        }
                        cx.notify();
                    }
                    Ok(steps) => page.write_editor_steps(node, slug, steps, draft, cx),
                }
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_editor_save_patch_decodes() {
        use ahjo_missions_contract::{End, Gate, InputCard, Limit, Port, Seat, Step};
        let mission = Mission {
            input_cards: vec![InputCard {
                id: "input".into(),
                ..Default::default()
            }],
            steps: vec![Step {
                id: "step".into(),
                inputs: vec![Port {
                    id: "port".into(),
                    ..Default::default()
                }],
                seats: vec![Seat {
                    id: "seat".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            gates: vec![Gate {
                id: "gate".into(),
                ..Default::default()
            }],
            ends: vec![End {
                id: "end".into(),
                ..Default::default()
            }],
            limits: vec![Limit {
                id: "limit".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let base = EditorSnapshot {
            fields: vec![String::new(); 22],
            inputs: vec![],
            outputs: vec![],
        };
        for (kind, id, values, expected) in [
            (
                EditKind::Step,
                "step",
                &[
                    (0, "Build"),
                    (1, "Implement the task"),
                    (2, "node/environment"),
                    (3, "/srv/project"),
                    (4, "ahjo-2"),
                    (5, "README.md"),
                    (6, "https://example.com"),
                    (10, "builder"),
                    (11, "claude-code"),
                    (12, "opus"),
                    (13, "high"),
                    (14, "Input"),
                    (15, "text"),
                    (16, "Cap"),
                    (17, "3"),
                    (18, "90"),
                    (19, "60"),
                    (20, "200"),
                    (21, "stop"),
                ][..],
                &[
                    "edit_step",
                    "update_step",
                    "staff_seat",
                    "update_port",
                    "update_limit",
                ][..],
            ),
            (
                EditKind::Input,
                "input",
                &[(0, "Request"), (1, "Build")],
                &["update_input_card"],
            ),
            (
                EditKind::Gate,
                "gate",
                &[
                    (0, "Review"),
                    (1, "Tests pass"),
                    (5, "README.md"),
                    (7, "tests"),
                    (8, "1"),
                    (9, "passing"),
                ],
                &["update_gate"],
            ),
            (EditKind::End, "end", &[(0, "Done")], &["update_end"]),
        ] {
            let mut draft = base.clone();
            for &(index, value) in values {
                draft.fields[index] = value.into();
            }
            if matches!(kind, EditKind::Input) {
                draft
                    .inputs
                    .push(("report".into(), String::new(), "text".into(), true));
                draft.outputs.push((
                    "result".into(),
                    "The result".into(),
                    "text".into(),
                    true,
                    "step:out".into(),
                ));
            }
            let steps = operations(
                kind,
                id,
                Some("seat"),
                Some("port"),
                Some("limit"),
                &base,
                &draft,
                &mission,
            )
            .unwrap();
            assert_eq!(steps.len(), expected.len());
            for (step, name) in steps.iter().zip(expected) {
                let undo::Step::Board(patch) = step else {
                    panic!("expected board edit")
                };
                let is_operation = match patch.as_ref() {
                    MissionChange::EditStep { .. } => "edit_step",
                    MissionChange::UpdateStep { .. } => "update_step",
                    MissionChange::StaffSeat { .. } => "staff_seat",
                    MissionChange::UpdatePort { .. } => "update_port",
                    MissionChange::UpdateLimit(_) => "update_limit",
                    MissionChange::UpdateInputCard(_) => "update_input_card",
                    MissionChange::UpdateGate(_) => "update_gate",
                    MissionChange::UpdateEnd(_) => "update_end",
                    change => panic!("unexpected edit {change:?}"),
                };
                assert_eq!(is_operation, *name);
                assert_patch_decodes(patch);
                let reverse = undo::inverse(&mission, &[], step).unwrap();
                assert_eq!(reverse.len(), 1);
                let undo::Step::Board(patch) = &reverse[0] else {
                    panic!("expected reverse board edit")
                };
                assert_patch_decodes(patch);
            }
        }
    }

    #[test]
    fn edits_merge_untouched_fields_and_refuse_the_same_field() {
        let base = EditorSnapshot {
            fields: vec![String::new(); 22],
            inputs: vec![],
            outputs: vec![],
        };
        let mut base = base;
        base.fields[0] = "Original title".into();
        base.fields[1] = "Original goal".into();
        let mut draft = base.clone();
        draft.fields[1] = "My goal".into();
        let mut mission = Mission::default();
        mission.steps.push(ahjo_missions_contract::Step {
            id: "work".into(),
            title: "Concurrent title".into(),
            brief: Some(ahjo_missions_contract::Brief {
                goal: "Original goal".into(),
                repository: "kept elsewhere".into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        let ops = operations(
            EditKind::Step,
            "work",
            None,
            None,
            None,
            &base,
            &draft,
            &mission,
        )
        .unwrap();
        let undo::Step::Board(patch) = &ops[0] else {
            panic!("board edit")
        };
        let MissionChange::EditStep {
            title,
            brief: Some(brief),
            ..
        } = patch.as_ref()
        else {
            panic!("expected a step edit, got {patch:?}");
        };
        assert_eq!(title, "Concurrent title");
        assert_eq!(brief.repository, "kept elsewhere");
        assert_eq!(brief.goal, "My goal");
        mission.steps[0].brief.as_mut().unwrap().goal = "Other goal".into();
        assert!(
            operations(
                EditKind::Step,
                "work",
                None,
                None,
                None,
                &base,
                &draft,
                &mission
            )
            .unwrap_err()[0]
                .contains("The brief was changed elsewhere")
        );
    }
}
