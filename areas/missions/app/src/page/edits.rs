//! Replays the ordered steps of one undo or redo. Each step is fenced by the
//! version left by the previous one, and a partial write keeps both the
//! completed reverse and the unwritten remainder in the history.
use super::*;
use ahjo_missions_contract::{
    LimitUpdate,
    commands::{LayoutShown, MissionEdit, MissionEdited},
    patch::{Changes, EditRequest},
};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Edit,
    Undo,
    Redo,
}

enum Receipt {
    Board(Box<MissionEdited>),
    Layout(LayoutShown),
}

struct Pending {
    node: String,
    slug: String,
    generation: u64,
    mode: Mode,
    submitted: Option<EditorSnapshot>,
    label: String,
    left: VecDeque<undo::Step>,
    reverses: Vec<undo::Step>,
    wrote: Option<undo::Version>,
}

impl MissionsPage {
    pub(super) fn step_history(&mut self, redo: bool, cx: &mut Context<Self>) {
        if self.writing || self.editor.as_ref().is_some_and(|editor| editor.busy) {
            return;
        }
        if self.unsaved(cx).is_some() {
            self.state.error =
                Some("Save or discard the current draft before undoing or redoing.".into());
            cx.notify();
            return;
        }
        let (Some((node, slug)), Some(mission)) =
            (self.state.selected.clone(), self.state.mission.as_ref())
        else {
            return;
        };
        let version = undo::Version {
            rev: mission.rev,
            layout: self.state.layout_etag.clone(),
        };
        match self.history.take(redo, &version) {
            Ok(Some(entry)) => {
                self.writing = true;
                self.history_next(
                    Pending {
                        node,
                        slug,
                        generation: self.generation,
                        mode: if redo { Mode::Redo } else { Mode::Undo },
                        submitted: None,
                        label: entry.label,
                        left: entry.steps.into(),
                        reverses: Vec::new(),
                        wrote: None,
                    },
                    cx,
                );
            }
            Ok(None) => (),
            Err(_) => {
                self.state.error =
                    Some("The mission changed elsewhere; undo history was cleared.".into());
                cx.notify();
            }
        }
    }

    /// Execute the editor's changed fields as one undoable, ordered edit.
    pub(super) fn write_editor_steps(
        &mut self,
        node: String,
        slug: String,
        steps: Vec<undo::Step>,
        submitted: EditorSnapshot,
        cx: &mut Context<Self>,
    ) {
        self.writing = true;
        self.history_next(
            Pending {
                node,
                slug,
                generation: self.generation,
                mode: Mode::Edit,
                submitted: Some(submitted),
                label: "edit card".into(),
                left: steps.into(),
                reverses: Vec::new(),
                wrote: None,
            },
            cx,
        );
    }

    fn history_next(&mut self, mut pending: Pending, cx: &mut Context<Self>) {
        let Some(step) = pending.left.front().cloned() else {
            self.history_finished(pending, None, false, cx);
            return;
        };
        let Some(mission) = self.state.mission.clone() else {
            self.history_finished(
                pending,
                Some("The mission is being read again.".into()),
                false,
                cx,
            );
            return;
        };
        let layout = self
            .state
            .layout
            .as_ref()
            .map(|layout| layout.cards.as_slice())
            .unwrap_or_default();
        let reverse = match undo::inverse(&mission, layout, &step) {
            Ok(reverse) => reverse,
            Err(error) => {
                self.history_finished(pending, Some(error), false, cx);
                return;
            }
        };
        let node = NodeId(pending.node.clone());
        let mut patches = Vec::new();
        let asked = match &step {
            undo::Step::Board(patch) => {
                let Some(etag) = self.state.mission_etag.clone() else {
                    self.history_finished(
                        pending,
                        Some("The mission is being read again.".into()),
                        false,
                        cx,
                    );
                    return;
                };
                patches.push((patch.clone(), reverse.clone()));
                if pending.submitted.is_some() {
                    // One drawer save produces independent field edits. Their
                    // reverses all read the same definition before the batch.
                    for step in pending.left.iter().skip(1) {
                        let undo::Step::Board(patch) = step else {
                            break;
                        };
                        match undo::inverse(&mission, layout, step) {
                            Ok(reverse) => patches.push((patch.clone(), reverse)),
                            Err(error) => {
                                self.history_finished(pending, Some(error), false, cx);
                                return;
                            }
                        }
                    }
                }
                let edit = if patches.len() == 1 {
                    board_edit(pending.slug.clone(), *patch.clone(), mission.rev, etag)
                } else {
                    MissionEdit {
                        mission: pending.slug.clone(),
                        change: EditRequest {
                            rev: Some(mission.rev),
                            updated_by: "human:operator".into(),
                            changes: Changes::Batch(
                                patches.iter().map(|(patch, _)| (**patch).clone()).collect(),
                            ),
                        },
                        rev: None,
                        etag: Some(etag),
                    }
                };
                let asked = self.host.call(node, edit, cx);
                cx.background_executor().spawn(async move {
                    asked.await.map(|receipt| Receipt::Board(Box::new(receipt)))
                })
            }
            undo::Step::Layout(nodes) => {
                let Some(etag) = self.state.layout_etag.clone() else {
                    self.history_finished(
                        pending,
                        Some("The layout is being read again.".into()),
                        false,
                        cx,
                    );
                    return;
                };
                let asked = self.host.call(
                    node,
                    LayoutEdit {
                        mission: pending.slug.clone(),
                        etag: Some(etag),
                        change: EditLayout {
                            cards: Some(nodes.clone()),
                            ..Default::default()
                        },
                    },
                    cx,
                );
                cx.background_executor()
                    .spawn(async move { asked.await.map(Receipt::Layout) })
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = asked.await;
            let _ = this.update(cx, |page, cx| {
                if pending.generation != page.generation {
                    return;
                }
                match answer {
                    Ok(Receipt::Board(response)) => {
                        for (patch, reverse) in patches {
                            let reverse = if reverse.is_empty() {
                                undo::answered(&patch, &mission, &response.definition.mission)
                            } else {
                                reverse
                            };
                            pending.reverses.extend(reverse);
                            if let (Some(submitted), Some(editor)) =
                                (&pending.submitted, &mut page.editor)
                                && let Some(base) = &mut editor.baseline
                            {
                                accept_editor_operation(base, submitted, &patch);
                            }
                            pending.left.pop_front();
                        }
                        pending.wrote = Some(undo::Version {
                            rev: response.definition.mission.rev,
                            layout: Some(response.layout_etag.clone()),
                        });
                        page.state.mission_etag = Some(response.definition.etag);
                        page.state.mission = Some(response.definition.mission);
                        page.state
                            .set_layout(response.layout, response.layout_etag, true);
                    }
                    Ok(Receipt::Layout(response)) => {
                        pending.reverses.extend(reverse);
                        pending.left.pop_front();
                        pending.wrote = Some(undo::Version {
                            rev: mission.rev,
                            layout: Some(response.etag.clone()),
                        });
                        page.state.set_layout(response.layout, response.etag, true);
                    }
                    Err(error) => {
                        page.history_finished(
                            pending,
                            Some(format!("{}: {}", error.code, error.message)),
                            true,
                            cx,
                        );
                        return;
                    }
                }
                page.history_next(pending, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn history_finished(
        &mut self,
        pending: Pending,
        error: Option<String>,
        refresh: bool,
        cx: &mut Context<Self>,
    ) {
        let mode = pending.mode;
        keep_written(
            &mut self.history,
            mode,
            pending.label,
            pending.reverses,
            pending.left,
            pending.wrote,
        );
        if mode == Mode::Edit {
            if let Some(error) = &error {
                if let Some(editor) = &mut self.editor {
                    editor.busy = false;
                    editor.error = Some(error.clone());
                }
            } else if let Some(submitted) = pending.submitted {
                let changed_since = self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| self.editor_snapshot(editor, cx) != submitted);
                let keep = self.editor.as_ref().is_some_and(|editor| editor.keep_open);
                if changed_since || keep {
                    if let Some(editor) = &mut self.editor {
                        editor.busy = false;
                        editor.baseline = Some(submitted);
                        // A write that lands answers what the last one lacked.
                        editor.error = None;
                        // Edits typed meanwhile are applied in turn.
                        if changed_since && !keep {
                            editor.error = Some(
                                "Saved the submitted changes. Your newer edits are still here."
                                    .into(),
                            );
                        }
                    }
                    if changed_since && keep {
                        self.apply_soon(cx);
                    }
                } else {
                    self.editor = None;
                }
            }
        }
        // An edit's error is told once, beside the edit that caused it.
        self.state.error = error.filter(|_| !(mode == Mode::Edit && self.editor.is_some()));
        if refresh {
            self.history_refresh(pending.node, pending.slug, pending.generation, cx);
        } else {
            self.writing = false;
            if mode == Mode::Edit && self.editor.is_none() {
                self.resolve_wanted(cx);
            }
            cx.notify();
        }
    }

    /// A rejected version or an unknown response cannot be trusted. Re-read
    /// both resources without discarding drafts or the partial history; its
    /// version fence refuses the next replay if another writer moved either.
    fn history_refresh(
        &mut self,
        node: String,
        slug: String,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let mission = read_definition(&self.host, NodeId(node.clone()), slug.clone(), cx);
        let layout = self
            .host
            .call(NodeId(node), LayoutShow { mission: slug }, cx);
        cx.spawn(async move |this, cx| {
            let (mission, layout) = (mission.await, layout.await);
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation {
                    return;
                }
                match mission {
                    Ok(response) => { page.state.mission_etag = Some(response.etag); page.state.mission = Some(response.mission); }
                    Err(_) => { page.state.mission_etag = None; page.state.mission = None; }
                }
                match layout {
                    Ok(response) => page.state.set_layout(response.layout, response.etag, true),
                    Err(_) => page.state.clear_layout(),
                }
                if page.state.mission.is_none() || page.state.layout.is_none() {
                    page.state.error = Some("Could not reload the mission and layout after the failed edit. Reopen the mission before editing.".into());
                }
                page.writing = false;
                cx.notify();
            });
        })
        .detach();
    }
}

/// Keep the reverse of what landed and the remaining work independently.
fn keep_written(
    history: &mut undo::History,
    mode: Mode,
    label: String,
    mut reverses: Vec<undo::Step>,
    left: VecDeque<undo::Step>,
    wrote: Option<undo::Version>,
) {
    reverses.reverse();
    let completed = undo::Entry {
        label: label.clone(),
        steps: reverses,
    };
    let unwritten = undo::Entry {
        label,
        steps: left.into(),
    };
    if let Some(version) = wrote {
        history.wrote(version);
    }
    match mode {
        Mode::Edit => history.record(completed),
        Mode::Undo => {
            history.undone(completed);
            history.redone(unwritten);
        }
        Mode::Redo => {
            history.redone(completed);
            history.undone(unwritten);
        }
    }
}

// A multi-operation save may fail after a prefix has landed. Advance only the
// saved portions so the remaining draft can be retried without a false conflict.
fn accept_editor_operation(
    base: &mut EditorSnapshot,
    submitted: &EditorSnapshot,
    patch: &MissionChange,
) {
    let mut accept = |range| base.accept_fields(submitted, range);
    match patch {
        MissionChange::EditStep { .. } => {
            accept(0..2);
            accept(3..7);
        }
        MissionChange::UpdateStep {
            environment: Some(_),
            ..
        } => accept(2..3),
        MissionChange::StaffSeat { .. } => accept(10..14),
        MissionChange::UpdatePort { .. } => accept(14..16),
        MissionChange::UpdateLimit(_) => accept(16..22),
        MissionChange::UpdateInputCard(_) => {
            accept(0..3);
            base.outputs = submitted.outputs.clone();
        }
        MissionChange::UpdateGate(_) => {
            accept(0..3);
            accept(5..6);
            accept(7..10);
        }
        MissionChange::UpdateEnd(_) => {
            accept(0..1);
            accept(2..3);
        }
        _ => (),
    }
}

pub(super) fn board_edit(
    mission: String,
    patch: MissionChange,
    rev: i64,
    etag: String,
) -> MissionEdit {
    MissionEdit {
        mission,
        change: EditRequest {
            rev: None,
            updated_by: "human:operator".into(),
            changes: Changes::Single(Box::new(patch)),
        },
        rev: Some(rev),
        etag: Some(etag),
    }
}

#[cfg(test)]
pub(super) fn assert_board_edit_decodes(patch: MissionChange) {
    let expected = serde_json::to_value(&patch).unwrap();
    let encoded =
        serde_json::to_vec(&board_edit("draft".into(), patch, 7, "version".into())).unwrap();
    let decoded: MissionEdit = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.rev, Some(7));
    assert_eq!(decoded.etag.as_deref(), Some("version"));
    assert_eq!(decoded.change.updated_by, "human:operator");
    let Changes::Single(received) = decoded.change.changes else {
        panic!("expected one edit");
    };
    assert_eq!(serde_json::to_value(received).unwrap(), expected);
}

pub(super) fn port_patch(
    op: &str,
    id: &str,
    port: Option<&str>,
    label: &str,
    kind: &str,
) -> Result<Option<MissionChange>, String> {
    let step_id = id.into();
    Ok(Some(match op {
        "input" | "output" => MissionChange::AddPort {
            step_id,
            direction: op.into(),
            label: label.into(),
            kind: kind.trim().parse()?,
        },
        "update" => MissionChange::UpdatePort {
            step_id,
            port_id: port.unwrap_or_default().into(),
            label: Some(label.into()),
            kind: Some(kind.trim().parse()?),
        },
        "remove" => MissionChange::RemovePort {
            step_id,
            port_id: port.unwrap_or_default().into(),
        },
        _ => return Ok(None),
    }))
}

pub(super) fn add_limit_patch(target: &str) -> MissionChange {
    MissionChange::CreateLimit {
        title: "Limit".into(),
        scope: target.into(),
        rounds: Some(3),
        seconds: None,
        warn_seconds: None,
        tokens: None,
        when_spent: ahjo_missions_contract::WhenSpent::Stop,
        x: 0,
        y: 0,
    }
}

pub(super) fn delete_limit_patch(id: &str) -> MissionChange {
    MissionChange::DeleteLimit { id: id.into() }
}

pub(super) fn update_limit_patch(
    id: &str,
    title: &str,
    when_spent: &str,
    [rounds, seconds, warn_seconds, tokens]: [&str; 4],
) -> Result<MissionChange, String> {
    let parse = |name: &str, text: &str| {
        let text = text.trim();
        if text.is_empty() {
            Ok(0)
        } else {
            text.parse::<i64>()
                .map_err(|_| format!("{name} must be a whole number"))
        }
    };
    Ok(MissionChange::UpdateLimit(LimitUpdate {
        id: id.into(),
        title: Some(title.into()),
        scope: None,
        rounds: Some(parse("rounds", rounds)?),
        seconds: Some(parse("seconds", seconds)?),
        warn_seconds: Some(parse("warn_seconds", warn_seconds)?),
        tokens: Some(parse("tokens", tokens)?),
        when_spent: Some(when_spent.trim().parse()?),
    }))
}

pub(super) fn step_formation_patch(id: &str, kind: Formation, seat: &str) -> MissionChange {
    MissionChange::SetStepFormation {
        id: id.into(),
        formation: kind,
        keep_seat_id: seat.into(),
        seats: None,
    }
}

pub(super) fn environment_patch(id: &str, environment: &str) -> MissionChange {
    MissionChange::UpdateStep {
        id: id.into(),
        title: None,
        environment: Some(environment.into()),
    }
}

pub(super) fn delete_card_patch(kind: EditKind, id: &str) -> MissionChange {
    let id = id.to_owned();
    match kind {
        EditKind::Step => MissionChange::DeleteStep { id },
        EditKind::Input => MissionChange::DeleteInputCard { id },
        EditKind::Gate => MissionChange::DeleteGate { id },
        EditKind::End => MissionChange::DeleteEnd { id },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_board_edits_decode() {
        for patch in [
            step_formation_patch("step", Formation::Peer, "seat"),
            environment_patch("step", "node/environment"),
            crate::store::staffing::Staffing {
                agent: "planner".into(),
                harness: ahjo_missions_contract::Harness::ClaudeCode,
                model: "opus".into(),
                effort: ahjo_missions_contract::Effort::High,
            }
            .assign("step", "seat"),
            add_limit_patch("step"),
            delete_limit_patch("limit"),
            update_limit_patch("limit", "Cap", "stop", ["3", "90", "", "200"]).unwrap(),
        ] {
            assert_board_edit_decodes(patch);
        }
        for op in ["input", "output", "update", "remove"] {
            assert_patch_decodes(
                &port_patch(op, "step", Some("port"), "Result", "text")
                    .unwrap()
                    .unwrap(),
            );
        }
        for kind in [
            EditKind::Step,
            EditKind::Input,
            EditKind::Gate,
            EditKind::End,
        ] {
            assert_patch_decodes(&delete_card_patch(kind, "card"));
        }
    }

    #[test]
    fn partial_editor_save_accepts_only_the_operation_that_landed() {
        let mut base = EditorSnapshot {
            fields: vec!["before".into(); 22],
            outputs: vec![],
        };
        let submitted = EditorSnapshot {
            fields: vec!["after".into(); 22],
            outputs: vec![],
        };
        accept_editor_operation(
            &mut base,
            &submitted,
            &MissionChange::EditStep {
                id: "step".into(),
                title: String::new(),
                brief: None,
            },
        );
        assert_eq!(base.fields[1], "after");
        assert_eq!(base.fields[2], "before");
        assert_eq!(base.fields[10], "before");
    }
    #[test]
    fn a_partial_undo_keeps_its_completed_reverse_and_unwritten_work() {
        let first = undo::Step::board(MissionChange::Title("first".into()));
        let second = undo::Step::board(MissionChange::Title("second".into()));
        let opposite = undo::Step::board(MissionChange::Title("opposite".into()));
        let mut history = undo::History::default();
        let before = undo::Version {
            rev: 1,
            layout: Some("a".into()),
        };
        let after = undo::Version {
            rev: 2,
            layout: Some("a".into()),
        };
        history.record(undo::Entry {
            label: "change".into(),
            steps: vec![first, second.clone()],
        });
        history.wrote(before.clone());
        history.take(false, &before).unwrap().unwrap();
        keep_written(
            &mut history,
            Mode::Undo,
            "change".into(),
            vec![opposite.clone()],
            VecDeque::from([second.clone()]),
            Some(after.clone()),
        );
        assert_eq!(
            history.take(false, &after).unwrap().unwrap().steps,
            vec![second]
        );
        assert_eq!(
            history.take(true, &after).unwrap().unwrap().steps,
            vec![opposite]
        );
    }
}
