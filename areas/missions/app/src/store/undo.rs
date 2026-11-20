//! Undo and redo of mission edits, as Archon's cockpit keeps them
//! (stepsUndo.ts, stepsRedo.ts). An edit is one or more steps,
//! each a mission patch or a layout patch. Before a step is written, the
//! step that reverses it is worked out from the mission as it is then; an
//! entry keeps those reverses, newest first, so undo never needs the
//! mission as it is later. Undoing an entry writes its steps and keeps
//! their own reverses as the redo.

use super::staffing::Staffing;
use ahjo_missions_contract::{
    EndUpdate, GateUpdate, InputCardUpdate, LayoutNode as Placed, LimitUpdate, Mission,
    MissionChange, RestoredNode, Slot,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// A change to the mission board.
    Board(Box<MissionChange>),
    /// A layout patch: where cards sit.
    Layout(Vec<Placed>),
}

impl Step {
    pub fn board(change: MissionChange) -> Self {
        Self::Board(Box::new(change))
    }
}

/// One edit's undo: its steps run in order. `label` names the edit, as in
/// "Undid the move of “Plan”".
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub label: String,
    pub steps: Vec<Step>,
}

/// What a mission was as of a write: its revision and its layout's ETag
/// (none before it has a layout).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Version {
    pub rev: i64,
    pub layout: Option<String>,
}

/// Undo or redo refused: the mission is no longer as this history's last
/// write left it, so writing the entry would overwrite someone else's change.
#[derive(Debug, PartialEq, Eq)]
pub struct Stale;

/// One mission's history.
#[derive(Default, Debug)]
pub struct History {
    undos: Vec<Entry>,
    redos: Vec<Entry>,
    /// The version this history's last write left.
    at: Option<Version>,
}

impl History {
    /// This history's last write left the mission at `v`.
    pub fn wrote(&mut self, v: Version) {
        self.at = Some(v);
    }

    /// The entry to undo (or redo) if the mission is still `now`, as this
    /// history's last write left it. Otherwise another client has written
    /// since: the entries were worked out against what it overwrote, so the
    /// history is dropped and the step refused.
    pub fn take(&mut self, redo: bool, now: &Version) -> Result<Option<Entry>, Stale> {
        let stack = if redo { &self.redos } else { &self.undos };
        if stack.is_empty() {
            return Ok(None);
        }
        if self.at.as_ref() != Some(now) {
            *self = History::default();
            return Err(Stale);
        }
        Ok(if redo {
            self.redos.pop()
        } else {
            self.undos.pop()
        })
    }

    /// A new edit: it can be undone, and what was undone can no longer be
    /// redone.
    pub fn record(&mut self, e: Entry) {
        if !e.steps.is_empty() {
            self.undos.push(e);
            self.redos.clear();
        }
    }

    /// What undoing wrote, reversed: the redo.
    pub fn undone(&mut self, e: Entry) {
        if !e.steps.is_empty() {
            self.redos.push(e);
        }
    }

    /// What redoing wrote, reversed: the undo again.
    pub fn redone(&mut self, e: Entry) {
        if !e.steps.is_empty() {
            self.undos.push(e);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undos.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redos.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardKind {
    Input,
    Step,
    Gate,
    End,
    Limit,
}

fn card(mission: &Mission, id: &str) -> Option<(RestoredNode, usize)> {
    macro_rules! find {
        ($cards:ident, $kind:ident) => {
            mission
                .$cards
                .iter()
                .enumerate()
                .find(|(_, card)| card.id == id)
                .map(|(index, card)| (RestoredNode::$kind(card.clone()), index))
        };
    }
    find!(input_cards, InputCard)
        .or_else(|| find!(steps, Step))
        .or_else(|| find!(gates, Gate))
        .or_else(|| find!(ends, End))
        .or_else(|| find!(limits, Limit))
}

/// Restore a deleted card, its wires, its position and its order among its kind.
fn restore_step(mission: &Mission, id: &str, (x, y): (i64, i64)) -> Option<Step> {
    let (node, index) = card(mission, id)?;
    let connections = mission
        .connections
        .iter()
        .filter(|wire| {
            let touches = |endpoint: &str| endpoint.split(':').next() == Some(id);
            touches(&wire.from) || touches(&wire.to)
        })
        .cloned()
        .collect();
    Some(Step::board(MissionChange::RestoreNode {
        node,
        connections,
        index: Some(index),
        x,
        y,
    }))
}

fn previous<T: Clone>(changed: &Option<T>, before: &T) -> Option<T> {
    changed.as_ref().map(|_| before.clone())
}

/// Work out the reverse of a step before it is sent. A create or a wire that
/// may join an occupied input needs its response to identify what it made.
pub fn inverse(mission: &Mission, layout: &[Placed], step: &Step) -> Result<Vec<Step>, String> {
    match step {
        Step::Board(change) => inverse_change(mission, layout, change),
        Step::Layout(nodes) => Ok(vec![Step::Layout(
            nodes
                .iter()
                .filter_map(|n| layout.iter().find(|p| p.id == n.id).cloned())
                .collect(),
        )]),
    }
}

pub fn inverse_change(
    mission: &Mission,
    layout: &[Placed],
    change: &MissionChange,
) -> Result<Vec<Step>, String> {
    use MissionChange::*;
    let missing = "the edited card no longer exists";
    let reverse = match change {
        Title(_) => Title(mission.title.clone()),
        CreateInputCard { .. }
        | CreateStep { .. }
        | CreateGate { .. }
        | CreateEnd { .. }
        | CreateLimit { .. }
        | RemovePort { .. } => return Ok(vec![]),
        DeleteInputCard { id }
        | DeleteStep { id }
        | DeleteGate { id }
        | DeleteEnd { id }
        | DeleteLimit { id } => {
            let at = layout
                .iter()
                .find(|p| p.id == *id)
                .map(|p| (p.x, p.y))
                .unwrap_or((200, 200));
            return restore_step(mission, id, at)
                .map(|step| vec![step])
                .ok_or_else(|| "the card is already gone".to_owned());
        }
        RestoreNode { node, .. } => match node {
            RestoredNode::InputCard(card) => delete(CardKind::Input, &card.id),
            RestoredNode::Step(card) => delete(CardKind::Step, &card.id),
            RestoredNode::Gate(card) => delete(CardKind::Gate, &card.id),
            RestoredNode::End(card) => delete(CardKind::End, &card.id),
            RestoredNode::Limit(card) => delete(CardKind::Limit, &card.id),
        },
        UpdatePort {
            step_id,
            port_id,
            label,
            kind,
        } => {
            let port = mission
                .step(step_id)
                .and_then(|f| f.inputs.iter().chain(&f.outputs).find(|p| p.id == *port_id))
                .ok_or("the edited port no longer exists")?;
            UpdatePort {
                step_id: step_id.clone(),
                port_id: port_id.clone(),
                label: previous(label, &port.label),
                kind: previous(kind, &port.kind),
            }
        }
        UpdateInputCard(update) => {
            let input = mission
                .input_cards
                .iter()
                .find(|n| n.id == update.id)
                .ok_or(missing)?;
            UpdateInputCard(InputCardUpdate {
                id: update.id.clone(),
                title: previous(&update.title, &input.title),
                goal: previous(&update.goal, &input.goal),
                files: previous(&update.files, &input.files),
                input_hint: previous(&update.input_hint, &input.input_hint),
                human_channel: previous(&update.human_channel, &input.human_channel),
                inputs: previous(&update.inputs, &input.inputs),
                outputs: previous(&update.outputs, &input.outputs),
            })
        }
        UpdateStep {
            id,
            title,
            device,
            fresh,
        } => {
            let step = mission.step(id).ok_or(missing)?;
            UpdateStep {
                id: id.clone(),
                title: previous(title, &step.title),
                device: previous(device, &step.device),
                fresh: previous(fresh, &step.fresh),
            }
        }
        EditStep { id, .. } => {
            let step = mission.step(id).ok_or(missing)?;
            EditStep {
                id: id.clone(),
                title: step.title.clone(),
                brief: Some(step.brief.clone().unwrap_or_default()),
            }
        }
        UpdateGate(update) => {
            let gate = mission.gate(&update.id).ok_or(missing)?;
            UpdateGate(GateUpdate {
                id: update.id.clone(),
                title: previous(&update.title, &gate.title),
                kinds: previous(&update.kinds, &gate.kinds),
                criterion: previous(&update.criterion, &gate.criterion),
                check: previous(&update.check, &gate.check),
                check_version: previous(&update.check_version, &gate.check_version),
                check_value: previous(&update.check_value, &gate.check_value),
                files: previous(&update.files, &gate.files),
            })
        }
        UpdateEnd(update) => {
            let end = mission.end(&update.id).ok_or(missing)?;
            UpdateEnd(EndUpdate {
                id: update.id.clone(),
                title: previous(&update.title, &end.title),
                outcome: previous(&update.outcome, &end.outcome),
            })
        }
        UpdateLimit(update) => {
            let limit = mission
                .limits
                .iter()
                .find(|n| n.id == update.id)
                .ok_or(missing)?;
            UpdateLimit(LimitUpdate {
                id: update.id.clone(),
                title: previous(&update.title, &limit.title),
                target: previous(&update.target, &limit.target),
                rounds: update.rounds.map(|_| limit.rounds.unwrap_or_default()),
                seconds: update.seconds.map(|_| limit.seconds.unwrap_or_default()),
                warn_seconds: update
                    .warn_seconds
                    .map(|_| limit.warn_seconds.unwrap_or_default()),
                tokens: update.tokens.map(|_| limit.tokens.unwrap_or_default()),
                when_spent: previous(&update.when_spent, &limit.when_spent),
            })
        }
        NameAgent {
            step_id, slot_id, ..
        } => {
            let slot =
                find_slot(mission, step_id, slot_id).ok_or("the named slot no longer exists")?;
            NameAgent {
                step_id: step_id.clone(),
                slot_id: slot_id.clone(),
                agent: slot.agent.clone(),
            }
        }
        AssignSlot {
            step_id, slot_id, ..
        } => {
            let slot =
                find_slot(mission, step_id, slot_id).ok_or("the staffed slot no longer exists")?;
            Staffing::from(slot).assign(step_id, slot_id)
        }
        WireConnection {
            join_if_occupied: true,
            ..
        }
        | RewireConnection {
            join_if_occupied: true,
            ..
        } => return Ok(vec![]),
        WireConnection { from, to, .. } => UnwireConnection {
            from: from.clone(),
            to: to.clone(),
        },
        UnwireConnection { from, to } => {
            let wire = mission
                .connections
                .iter()
                .find(|c| c.from == *from && c.to == *to)
                .ok_or("the wire no longer exists")?;
            WireConnection {
                id: wire.id.clone(),
                from: from.clone(),
                to: to.clone(),
                join_if_occupied: false,
            }
        }
        RewireConnection {
            from,
            previous_to,
            to,
            ..
        } => RewireConnection {
            from: from.clone(),
            previous_to: to.clone(),
            to: previous_to.clone(),
            join_if_occupied: false,
            remove_previous_input: false,
        },
        _ => return Err("This edit cannot be undone".into()),
    };
    Ok(vec![Step::board(reverse)])
}

fn find_slot<'a>(mission: &'a Mission, step: &str, slot: &str) -> Option<&'a Slot> {
    mission.step(step)?.slots.iter().find(|s| s.id == slot)
}

/// Reverse a change whose result identifies the node or wire it added.
pub fn answered(change: &MissionChange, before: &Mission, after: &Mission) -> Vec<Step> {
    use MissionChange::*;
    match change {
        WireConnection { from, to, .. } => {
            let Some(added) = after
                .connections
                .iter()
                .find(|c| c.from == *from && !before.connections.iter().any(|old| old.id == c.id))
            else {
                return vec![];
            };
            let reverse = if added.to == *to {
                UnwireConnection {
                    from: from.clone(),
                    to: to.clone(),
                }
            } else {
                // The joined wire goes with the extra port it created.
                let (step, port) = added.to.split_once(':').unwrap_or((&added.to, ""));
                RemovePort {
                    step_id: step.into(),
                    port_id: port.into(),
                }
            };
            vec![Step::board(reverse)]
        }
        RewireConnection {
            from,
            previous_to,
            to,
            ..
        } => {
            let Some(old) = before
                .connections
                .iter()
                .find(|c| c.from == *from && c.to == *previous_to)
            else {
                return vec![];
            };
            let Some(landed) = after.connections.iter().find(|c| c.id == old.id) else {
                return vec![];
            };
            vec![Step::board(RewireConnection {
                from: from.clone(),
                previous_to: landed.to.clone(),
                to: previous_to.clone(),
                join_if_occupied: false,
                remove_previous_input: landed.to != *to,
            })]
        }
        RemovePort { step_id, port_id } => {
            // An undone join is redone by wiring to an input of the same step.
            let removed = format!("{step_id}:{port_id}");
            let Some(to) = after
                .connections
                .iter()
                .find(|c| c.to.split_once(':').is_some_and(|(f, _)| f == step_id))
                .map(|c| c.to.clone())
                .or_else(|| {
                    let first = after.step(step_id)?.inputs.first()?;
                    Some(format!("{step_id}:{}", first.id))
                })
            else {
                return vec![];
            };
            before
                .connections
                .iter()
                .filter(|c| c.to == removed)
                .map(|c| {
                    Step::board(WireConnection {
                        id: String::new(),
                        from: c.from.clone(),
                        to: to.clone(),
                        join_if_occupied: true,
                    })
                })
                .collect()
        }
        _ => created(before, after)
            .map(|(id, kind)| vec![Step::board(delete(kind, id))])
            .unwrap_or_default(),
    }
}

fn created<'a>(before: &Mission, after: &'a Mission) -> Option<(&'a str, CardKind)> {
    fn added<'a, T>(before: &[T], after: &'a [T], id: impl Fn(&T) -> &str) -> Option<&'a str> {
        after
            .iter()
            .map(&id)
            .find(|new| !before.iter().any(|old| id(old) == *new))
    }
    added(&before.input_cards, &after.input_cards, |n| &n.id)
        .map(|id| (id, CardKind::Input))
        .or_else(|| added(&before.steps, &after.steps, |n| &n.id).map(|id| (id, CardKind::Step)))
        .or_else(|| added(&before.gates, &after.gates, |n| &n.id).map(|id| (id, CardKind::Gate)))
        .or_else(|| added(&before.ends, &after.ends, |n| &n.id).map(|id| (id, CardKind::End)))
        .or_else(|| added(&before.limits, &after.limits, |n| &n.id).map(|id| (id, CardKind::Limit)))
}

fn delete(kind: CardKind, id: &str) -> MissionChange {
    let id = id.to_owned();
    match kind {
        CardKind::Input => MissionChange::DeleteInputCard { id },
        CardKind::Step => MissionChange::DeleteStep { id },
        CardKind::Gate => MissionChange::DeleteGate { id },
        CardKind::End => MissionChange::DeleteEnd { id },
        CardKind::Limit => MissionChange::DeleteLimit { id },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MissionChange::*;
    use ahjo_missions_contract::{Brief, Connection, End, Gate, InputCard, Limit, Port};

    fn check_patches(steps: Vec<Step>) -> Vec<Step> {
        for step in &steps {
            if let Step::Board(change) = step {
                crate::page::assert_patch_decodes(change);
            }
        }
        steps
    }

    fn inverse(m: &Mission, layout: &[Placed], step: &Step) -> Result<Vec<Step>, String> {
        super::inverse(m, layout, step).map(check_patches)
    }
    fn answered(change: &MissionChange, before: &Mission, after: &Mission) -> Vec<Step> {
        check_patches(super::answered(change, before, after))
    }
    fn connection(id: &str, from: &str, to: &str) -> Connection {
        Connection {
            id: id.into(),
            from: from.into(),
            to: to.into(),
        }
    }
    fn wire(from: &str, to: &str, join_if_occupied: bool) -> MissionChange {
        WireConnection {
            id: String::new(),
            from: from.into(),
            to: to.into(),
            join_if_occupied,
        }
    }
    fn mission() -> Mission {
        Mission {
            title: "Delivery".into(),
            steps: vec![
                ahjo_missions_contract::Step {
                    id: "fmn_plan".into(),
                    title: "Plan".into(),
                    formation: ahjo_missions_contract::Formation::Solo,
                    slots: vec![Slot {
                        id: "s1".into(),
                        agent_id: "planner".into(),
                        harness: ahjo_missions_contract::Harness::ClaudeCode,
                        effort: ahjo_missions_contract::Effort::Xhigh,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ahjo_missions_contract::Step {
                    id: "fmn_beads".into(),
                    title: "Beads".into(),
                    formation: ahjo_missions_contract::Formation::Solo,
                    ..Default::default()
                },
            ],
            gates: vec![Gate {
                id: "gate_r".into(),
                title: "Review".into(),
                kinds: vec![ahjo_missions_contract::GateKind::Step],
                criterion: "ok".into(),
                ..Default::default()
            }],
            connections: vec![
                connection("c1", "fmn_plan:out", "fmn_beads:in"),
                connection("c2", "fmn_beads:out", "gate_r:in"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_delete_is_undone_by_restoring_the_card_with_its_wires_where_it_sat() {
        let m = mission();
        let at = [Placed {
            id: "fmn_beads".into(),
            x: 420,
            y: 80,
        }];
        let del = Step::board(DeleteStep {
            id: "fmn_beads".into(),
        });
        let inv = inverse(&m, &at, &del).unwrap();
        let Step::Board(change) = &inv[0] else {
            panic!("expected restore")
        };
        let RestoreNode {
            node: RestoredNode::Step(card),
            connections,
            x,
            y,
            index,
        } = change.as_ref()
        else {
            panic!("expected step restore");
        };
        assert_eq!(card.title, "Beads");
        assert_eq!(connections.len(), 2);
        assert_eq!((*x, *y, *index), (420, 80, Some(1)));
        assert_eq!(inverse(&m, &at, &inv[0]).unwrap(), vec![del]);
    }

    #[test]
    fn an_update_is_undone_by_writing_back_only_the_fields_it_changed() {
        let m = mission();
        let gate_patch = |criterion: &str| {
            Step::board(UpdateGate(GateUpdate {
                id: "gate_r".into(),
                criterion: Some(criterion.into()),
                ..Default::default()
            }))
        };
        assert_eq!(
            inverse(&m, &[], &gate_patch("better")).unwrap(),
            vec![gate_patch("ok")]
        );
        let staff = Staffing {
            harness: ahjo_missions_contract::Harness::OpenAiCodex,
            effort: ahjo_missions_contract::Effort::High,
            ..Default::default()
        };
        let undo_staff = inverse(&m, &[], &Step::board(staff.assign("fmn_plan", "s1"))).unwrap();
        assert_eq!(
            undo_staff,
            vec![Step::board(AssignSlot {
                step_id: "fmn_plan".into(),
                slot_id: "s1".into(),
                agent_id: "planner".into(),
                harness: ahjo_missions_contract::Harness::ClaudeCode,
                model: String::new(),
                effort: ahjo_missions_contract::Effort::Xhigh,
            })]
        );

        let name = |agent: &str| {
            Step::board(NameAgent {
                step_id: "fmn_plan".into(),
                slot_id: "s1".into(),
                agent: agent.into(),
            })
        };
        assert_eq!(inverse(&m, &[], &name("writer")).unwrap(), vec![name("")]);
        let fresh = |fresh| {
            Step::board(UpdateStep {
                id: "fmn_plan".into(),
                title: None,
                device: None,
                fresh: Some(fresh),
            })
        };
        assert_eq!(inverse(&m, &[], &fresh(true)).unwrap(), vec![fresh(false)]);
    }

    #[test]
    fn undo_of_a_staffing_change_restores_every_staffing_field() {
        let mut mission = mission();
        let seat = &mut mission.steps[0].slots[0];
        seat.model = "opus".into();
        let before = Staffing::from(&*seat);
        let edited = Staffing {
            agent_id: "reviewer".into(),
            harness: ahjo_missions_contract::Harness::OpenAiCodex,
            model: "gpt-6".into(),
            effort: ahjo_missions_contract::Effort::Low,
        };
        let forward = Step::board(edited.assign("fmn_plan", "s1"));
        assert_eq!(
            inverse(&mission, &[], &forward).unwrap(),
            vec![Step::board(before.assign("fmn_plan", "s1"))],
        );
    }

    #[test]
    fn wires_moves_and_creates_reverse() {
        let m = mission();
        let unwire = Step::board(UnwireConnection {
            from: "fmn_plan:out".into(),
            to: "fmn_beads:in".into(),
        });
        let Step::Board(change) = &inverse(&m, &[], &unwire).unwrap()[0] else {
            panic!("expected wire")
        };
        assert!(matches!(change.as_ref(), WireConnection { id, .. } if id == "c1"));
        let at = [Placed {
            id: "fmn_plan".into(),
            x: 40,
            y: 40,
        }];
        let moved = Step::Layout(vec![Placed {
            id: "fmn_plan".into(),
            x: 300,
            y: 90,
        }]);
        assert_eq!(
            inverse(&m, &at, &moved).unwrap(),
            vec![Step::Layout(at.to_vec())]
        );
        let mut after = m.clone();
        after.steps.push(ahjo_missions_contract::Step {
            id: "fmn_new".into(),
            title: "New".into(),
            ..Default::default()
        });
        assert_eq!(created(&m, &after), Some(("fmn_new", CardKind::Step)));
        assert_eq!(
            answered(
                &CreateStep {
                    title: "New".into(),
                    formation: ahjo_missions_contract::Formation::Solo,
                    x: 0,
                    y: 0
                },
                &m,
                &after
            ),
            vec![Step::board(DeleteStep {
                id: "fmn_new".into()
            })],
        );
    }

    #[test]
    fn undo_and_redo_patches_decode_for_each_card_kind() {
        let m = Mission {
            input_cards: vec![InputCard {
                id: "input".into(),
                ..Default::default()
            }],
            steps: vec![ahjo_missions_contract::Step {
                id: "step".into(),
                brief: Some(Brief {
                    work: "ahjo-2".into(),
                    ..Default::default()
                }),
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
        for (kind, id) in [
            (CardKind::Input, "input"),
            (CardKind::Step, "step"),
            (CardKind::Gate, "gate"),
            (CardKind::End, "end"),
            (CardKind::Limit, "limit"),
        ] {
            let delete = Step::board(delete(kind, id));
            let restore = inverse(&m, &[], &delete).unwrap();
            assert_eq!(restore.len(), 1);
            assert_eq!(inverse(&m, &[], &restore[0]).unwrap(), vec![delete]);
        }
    }

    #[test]
    fn title_and_wire_inverses_decode() {
        let before = mission();
        let rewire = |join_if_occupied| RewireConnection {
            from: "fmn_plan:out".into(),
            previous_to: "fmn_beads:in".into(),
            to: "gate_r:in".into(),
            join_if_occupied,
            remove_previous_input: false,
        };
        for change in [
            Title("Other".into()),
            wire("fmn_plan:out", "fmn_beads:in", false),
            rewire(false),
        ] {
            assert_eq!(
                inverse(&before, &[], &Step::board(change)).unwrap().len(),
                1
            );
        }
        let mut after = before.clone();
        after.connections[0].to = "gate_r:in_2".into();
        let steps = answered(&rewire(true), &before, &after);
        let Step::Board(reverse) = &steps[0] else {
            panic!("expected board edit")
        };
        assert!(matches!(
            reverse.as_ref(),
            RewireConnection {
                remove_previous_input: true,
                ..
            }
        ));
    }

    #[test]
    fn a_new_edit_clears_the_redo() {
        let mut h = History::default();
        let e = |l: &str| Entry {
            label: l.into(),
            steps: vec![Step::Layout(vec![])],
        };
        h.record(e("a"));
        h.wrote(Version::default());
        let a = h.take(false, &Version::default()).unwrap().unwrap();
        h.undone(a);
        assert!(h.can_redo());
        h.record(e("b"));
        assert!(!h.can_redo());
        assert!(h.can_undo());
    }

    #[test]
    fn undo_is_refused_once_another_client_has_written_the_mission() {
        // A renames a step at rev 5; B renames it again at rev 6.
        let mut h = History::default();
        let rename = Entry {
            label: "the title".into(),
            steps: vec![Step::board(UpdateStep {
                id: "fmn_plan".into(),
                title: Some("A".into()),
                device: None,
                fresh: None,
            })],
        };
        h.record(rename.clone());
        let mine = Version {
            rev: 5,
            layout: Some("l1".into()),
        };
        h.wrote(mine.clone());
        let theirs = Version {
            rev: 6,
            ..mine.clone()
        };
        assert_eq!(h.take(false, &theirs), Err(Stale));
        assert!(!h.can_undo() && !h.can_redo());
        h.record(rename.clone());
        h.wrote(mine.clone());
        let moved = Version {
            layout: Some("l2".into()),
            ..mine.clone()
        };
        assert_eq!(h.take(false, &moved), Err(Stale));
        h.record(rename.clone());
        h.wrote(mine.clone());
        let e = h.take(false, &mine).unwrap().unwrap();
        h.undone(e);
        let undone = Version { rev: 6, ..mine };
        h.wrote(undone.clone());
        assert_eq!(h.take(true, &undone).unwrap(), Some(rename));
    }

    #[test]
    fn a_wire_that_joined_an_occupied_input_is_undone_by_removing_the_port_it_added() {
        let before = mission();
        let mut after = before.clone();
        after
            .connections
            .push(connection("c3", "gate_r:pass", "fmn_beads:in_2"));
        let joining = wire("gate_r:pass", "fmn_beads:in", true);
        assert!(
            inverse(&before, &[], &Step::board(joining.clone()))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            answered(&joining, &before, &after),
            vec![Step::board(RemovePort {
                step_id: "fmn_beads".into(),
                port_id: "in_2".into()
            })],
        );
        after.connections.last_mut().unwrap().to = "fmn_plan:in".into();
        let plain = wire("gate_r:pass", "fmn_plan:in", true);
        let Step::Board(reverse) = &answered(&plain, &before, &after)[0] else {
            panic!("expected unwire")
        };
        assert!(matches!(reverse.as_ref(), UnwireConnection { to, .. } if to == "fmn_plan:in"));
    }

    #[test]
    fn an_undone_join_is_redone_by_joining_the_same_input_again() {
        let mut joined = mission();
        joined
            .connections
            .push(connection("c3", "gate_r:pass", "fmn_beads:in_2"));
        let remove = RemovePort {
            step_id: "fmn_beads".into(),
            port_id: "in_2".into(),
        };
        assert!(
            inverse(&joined, &[], &Step::board(remove.clone()))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            answered(&remove, &joined, &mission()),
            vec![Step::board(wire("gate_r:pass", "fmn_beads:in", true))],
        );
    }

    #[test]
    fn a_port_edit_reverses_only_its_changed_fields() {
        let mut m = mission();
        m.steps[0].inputs = vec![Port {
            id: "in".into(),
            label: "Original".into(),
            kind: ahjo_missions_contract::PortKind::Text,
        }];
        let update = |label: &str| {
            Step::board(UpdatePort {
                step_id: "fmn_plan".into(),
                port_id: "in".into(),
                label: Some(label.into()),
                kind: None,
            })
        };
        assert_eq!(
            inverse(&m, &[], &update("Renamed")).unwrap(),
            vec![update("Original")]
        );
    }
}
