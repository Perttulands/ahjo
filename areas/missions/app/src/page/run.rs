//! Run controls and views; the channel projection remains in the store.
use super::*;
use crate::store::run_state;
use ahjo_app_kit::host::Request;
use ahjo_app_kit::{ask::Ask, theme::Theme};
use ahjo_missions_contract::{
    Mission, RunAttempts, RunProjection,
    commands::{MissionRun, MissionStarted},
    run_commands::{RunAbort, RunApprove, RunPick, RunReject, RunShow},
};
use gpui::{AnyElement, FocusHandle, Focusable, KeyDownEvent, SharedString, Subscription};
use std::collections::BTreeMap;

/// The attempts' popup at its largest.
const ATTEMPTS_W: f32 = 900.;
const ATTEMPTS_H: f32 = 650.;

pub(super) struct RunUi {
    pub(super) selected: Option<String>,
    /// The card whose read-only panel is open on the run's board.
    pub(super) inspect: Option<String>,
    /// The open step's live seats, their screens and what is written to
    /// them.
    pub(super) seats: super::run_step::SeatFollow,
    pub(super) reveal_step: Option<String>,
    pub(super) frozen: Option<Mission>,
    pub(super) starting: bool,
    /// What the start form is opened from; none (an ask from another
    /// page) opens it beside the mission's Start.
    pub(super) start_at: Option<Spot>,
    /// Where the mission's Start was last drawn.
    pub(super) start_button: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// The start form's popup, while it is open.
    pub(super) start_popup: Option<PopupId>,
    pub(super) start_keys_ready: bool,
    pub(super) start_subs: Vec<Subscription>,
    pub(super) target: Option<String>,
    pub(super) start_card: Option<String>,
    pub(super) start_definition: Option<(Mission, String)>,
    pub(super) request_id: String,
    pub(super) uncertain: Option<MissionRun>,
    pub(super) cwd: Entity<TextField>,
    pub(super) repository: Entity<TextField>,
    pub(super) base: Entity<TextField>,
    pub(super) issue: Entity<TextField>,
    pub(super) paths: Entity<TextField>,
    keep_sessions: bool,
    pub(super) busy: bool,
    /// Stop's confirmation, beside Stop, while it is open.
    pub(super) stop_popup: Option<PopupId>,
    pub(super) stop_reason: Entity<TextField>,
    /// The start form shows its rarely needed fields.
    pub(super) start_more: bool,
    /// A one-second redraw is pending so a shown running run's clock moves.
    pub(super) ticking: bool,
    /// The run shows as its graph, each card in its step's state, instead
    /// of the steps table.
    pub(super) graph: bool,
    pub(super) fields: BTreeMap<String, Entity<TextField>>,
    pub(super) responses: BTreeMap<String, Entity<TextField>>,
    pub(super) error: Option<String>,
    pub(super) findings: Vec<String>,
    pub(super) attempts: Option<RunAttempts>,
    pub(super) attempt_window: Option<crate::store::attempts::AttemptsWindow>,
    /// What the attempts were asked for from, and their popup once read.
    pub(super) attempts_from: Option<Spot>,
    pub(super) attempts_popup: Option<PopupId>,
    pub(super) attempts_focus: FocusHandle,
    pub(super) attempt_reason: Entity<TextField>,
    pub(super) attempts_at: Option<usize>,
    pub(super) pending_attempt: Option<(String, bool)>,
    pub(super) evidence: crate::store::evidence::Store,
    pub(super) gates: BTreeMap<
        String,
        (
            u64,
            Result<Option<ahjo_missions_contract::GateDecision>, String>,
        ),
    >,
}
impl RunUi {
    pub fn new(cx: &mut Context<MissionsPage>) -> Self {
        let cwd = cx.new(|cx| TextField::new(cx, "Working folder"));
        // The start form says where files will go as the folder is typed.
        cx.observe(&cwd, |_, _, cx| cx.notify()).detach();
        Self {
            selected: None,
            inspect: None,
            seats: Default::default(),
            reveal_step: None,
            frozen: None,
            starting: false,
            start_at: None,
            start_button: Default::default(),
            start_popup: None,
            start_keys_ready: false,
            start_subs: Vec::new(),
            target: None,
            start_card: None,
            start_definition: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            uncertain: None,
            cwd,
            repository: cx.new(|cx| TextField::new(cx, "Repository")),
            base: cx.new(|cx| TextField::new(cx, "Base")),
            issue: cx.new(|cx| TextField::new(cx, "Issue reference")),
            paths: cx.new(|cx| TextField::new(cx, "Context paths, one per line").prose()),
            keep_sessions: false,
            busy: false,
            stop_popup: None,
            stop_reason: cx.new(|cx| TextField::new(cx, "Why (optional)")),
            start_more: false,
            ticking: false,
            graph: false,
            fields: BTreeMap::new(),
            responses: BTreeMap::new(),
            error: None,
            findings: vec![],
            attempts: None,
            attempt_window: None,
            attempts_from: None,
            attempts_popup: None,
            attempts_focus: cx.focus_handle(),
            attempt_reason: cx.new(|cx| TextField::new(cx, "Why this attempt?")),
            attempts_at: None,
            pending_attempt: None,
            evidence: Default::default(),
            gates: BTreeMap::new(),
        }
    }
}
impl MissionsPage {
    pub(super) fn run_unsaved(&self, cx: &gpui::App) -> Option<String> {
        if self.run.uncertain.is_some() {
            return Some("Unconfirmed run start".into());
        }
        if self.run.starting
            && (self
                .run
                .fields
                .values()
                .any(|f| !f.read(cx).text.is_empty())
                || self.run.cwd.read(cx).text != self.last_folder().unwrap_or_default()
                || self.run.keep_sessions
                || [
                    &self.run.repository,
                    &self.run.base,
                    &self.run.issue,
                    &self.run.paths,
                ]
                .iter()
                .any(|f| !f.read(cx).text.is_empty()))
        {
            return Some("Run input".into());
        }
        if self
            .run
            .responses
            .values()
            .any(|f| !f.read(cx).text.is_empty())
        {
            return Some("Gate response".into());
        }
        None
    }
    pub(super) fn reveal_run(&mut self, id: String, part: Option<String>, cx: &mut Context<Self>) {
        self.show_run(id, cx);
        self.run.reveal_step = part;
        cx.notify();
    }
    pub(super) fn start_uncertain(&self) -> bool {
        self.run.uncertain.is_some()
    }
    pub(super) fn is_showing_run(&self) -> bool {
        self.run.selected.is_some() || self.run.starting
    }
    pub(super) fn selected_run(&self) -> Option<&RunProjection> {
        let (node, _) = self.state.selected.as_ref()?;
        self.state
            .nodes
            .get(node)?
            .runs
            .get(self.run.selected.as_ref()?)
    }
    pub(super) fn show_run(&mut self, run: String, cx: &mut Context<Self>) {
        if self.run_unsaved(cx).is_some() {
            self.run.error =
                Some("Save or send the current run draft before opening another run.".into());
            cx.notify();
            return;
        }
        let Some((node, _)) = self.state.selected.clone() else {
            return;
        };
        self.canvas.invalidate_fit();
        self.run.selected = Some(run.clone());
        self.run.reveal_step = None;
        self.end_stop(cx);
        self.end_start(cx);
        self.run.frozen = None;
        self.run.attempts = None;
        self.end_attempts(cx);
        self.run.attempts_at = None;
        self.run.pending_attempt = None;
        self.run.evidence = Default::default();
        self.run.gates.clear();
        self.run.responses.clear();
        self.run.error = None;
        let asked = self.host.call(
            NodeId(node),
            RunShow {
                run: run.clone(),
                mission: true,
                ..Default::default()
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.run.selected.as_deref() != Some(&run) {
                    return;
                }
                match result {
                    Ok(answer) => page.run.frozen = answer.mission,
                    Err(error) => {
                        page.run.error = Some(format!("{}: {}", error.code, error.message))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        self.sync_gate(cx);
        cx.notify();
    }
    pub(super) fn sync_gate(&mut self, cx: &mut Context<Self>) {
        let (Some((node, _)), Some(run_id), Some(run)) = (
            self.state.selected.clone(),
            self.run.selected.clone(),
            self.selected_run().cloned(),
        ) else {
            return;
        };
        self.run
            .responses
            .retain(|id, _| run.waiting_gates.iter().any(|g| g.gate_id == *id));
        for gate in &run.waiting_gates {
            if self
                .run
                .gates
                .get(&gate.gate_id)
                .is_none_or(|(seq, _)| *seq != gate.requested_seq)
            {
                self.run.responses.insert(
                    gate.gate_id.clone(),
                    cx.new(|cx| TextField::new(cx, "Your response").prose()),
                );
            }
        }
        self.run.gates.retain(|id, (seq, _)| {
            run.waiting_gates
                .iter()
                .any(|g| g.gate_id == *id && g.requested_seq == *seq)
        });
        if self
            .run
            .evidence
            .read_at
            .is_none_or(|at| at < run.event_count)
        {
            self.read_evidence(&node, &run_id, run.event_count, cx);
        }
        if self.run.attempts_at.is_some_and(|at| at < run.event_count) {
            self.attempts(cx);
        }
        self.follow_seats(true, cx);
        let mut pending = Vec::new();
        for gate in run.waiting_gates {
            if !self.run.gates.contains_key(&gate.gate_id) {
                self.run
                    .gates
                    .insert(gate.gate_id.clone(), (gate.requested_seq, Ok(None)));
                pending.push((gate.gate_id, gate.requested_seq));
            }
        }
        if pending.is_empty() {
            return;
        }
        let asked = self.host.call(
            NodeId(node.clone()),
            RunShow {
                run: run_id.clone(),
                ..Default::default()
            },
            cx,
        );
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation || page.run.selected.as_deref() != Some(&run_id) {
                    return;
                }
                for (id, seq) in pending {
                    if page
                        .run
                        .gates
                        .get(&id)
                        .is_none_or(|(current, _)| *current != seq)
                    {
                        continue;
                    }
                    let request = match &result {
                        Ok(answer) => answer
                            .asks
                            .iter()
                            .find(|ask| ask.gate.as_deref() == Some(&id) && ask.seq == Some(seq))
                            .and_then(|ask| ask.request.as_ref())
                            .filter(|request| request.requested_seq == seq)
                            .cloned()
                            .ok_or_else(|| "Gate request changed. Try again.".to_owned()),
                        Err(error) => Err(error.message.clone()),
                    };
                    page.run.gates.insert(id, (seq, request.map(Some)));
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn clear_start_context(&mut self, cx: &mut Context<Self>) {
        self.run.keep_sessions = false;
        for field in [
            &self.run.repository,
            &self.run.base,
            &self.run.issue,
            &self.run.paths,
        ] {
            field.update(cx, |field, cx| field.set("", false, cx));
        }
        let folder = self.last_folder().unwrap_or_default();
        self.run
            .cwd
            .update(cx, |field, cx| field.set(&folder, false, cx));
    }
    /// The open node's runs, newest first.
    fn node_runs(&self) -> Vec<&RunProjection> {
        let mut runs: Vec<&RunProjection> = self
            .state
            .selected
            .as_ref()
            .and_then(|(node, _)| self.state.nodes.get(node))
            .map(|feed| feed.runs.values().collect())
            .unwrap_or_default();
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs
    }
    /// The folder last chosen for a run of this mission, else for any run.
    fn last_folder(&self) -> Option<String> {
        let here = self.state.selected.as_ref().map(|(_, slug)| slug.as_str());
        let runs = self.node_runs();
        let chosen = |run: &&&RunProjection| !run.cwd.is_empty() && !made_for(run);
        runs.iter()
            .filter(chosen)
            .find(|run| Some(run.mission_slug.as_str()) == here)
            .or_else(|| runs.iter().find(chosen))
            .map(|run| run.cwd.clone())
    }
    /// Where a run's files go, said before it starts.
    fn files_go(&self, cx: &gpui::App) -> String {
        let repository = self.run.repository.read(cx).text.trim().to_owned();
        let folder = self.run.cwd.read(cx).text.trim().to_owned();
        if !repository.is_empty() {
            return format!(
                "Files go to a new branch of {repository}, in a checkout Ahjo makes for this run."
            );
        }
        if !folder.is_empty() {
            // The folder field already says where.
            return String::new();
        }
        // Ahjo makes a folder named after the run; where earlier ones are
        // shows where.
        match self
            .node_runs()
            .into_iter()
            .find(|run| made_for(run))
            .and_then(|run| run.cwd.rsplit_once('/').map(|(root, _)| root.to_owned()))
        {
            Some(root) => format!("Files go to a new folder for this run in {root}."),
            None => "Files go to a new folder Ahjo makes for this run.".into(),
        }
    }
    pub(super) fn prepare_run(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self
            .state
            .mission
            .as_ref()
            .and_then(|m| m.input_cards.first())
            .map(|c| c.id.clone())
        else {
            self.state.error = Some("This mission has no input card.".into());
            cx.notify();
            return;
        };
        self.prepare_card_run(&card, cx);
    }
    /// Start at the Input card whose play control was used.
    pub(super) fn prepare_card_run(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.mission_input_runnable(id) {
            // What it lacks, listed under the board and marked on it.
            self.run_tried = true;
            self.todo_open = true;
        }
        if self.unsaved(cx).is_some() {
            self.state.error = Some("Save or discard the current draft first.".into());
            cx.notify();
            return;
        }
        let Some(input) = self
            .state
            .mission
            .as_ref()
            .and_then(|m| m.input_cards.iter().find(|c| c.id == id))
        else {
            return;
        };
        let fields: Vec<(String, bool)> = if input.inputs.is_empty() {
            vec![("brief".into(), true)]
        } else {
            input
                .inputs
                .iter()
                .map(|i| {
                    (
                        i.name.clone(),
                        i.kind.effective() == ahjo_missions_contract::PortKind::Text,
                    )
                })
                .collect()
        };
        let Some(definition) = self
            .state
            .mission
            .clone()
            .zip(self.state.mission_etag.clone())
        else {
            return;
        };
        self.clear_start_context(cx);
        self.run.start_definition = Some(definition);
        self.run.target = None;
        self.run.start_card = Some(id.to_owned());
        self.run.request_id = uuid::Uuid::new_v4().to_string();
        self.run.uncertain = None;
        self.run.fields = fields
            .into_iter()
            .map(|(name, prose)| {
                let field = cx.new(|cx| {
                    let field = TextField::new(cx, &name);
                    if prose { field.prose() } else { field }
                });
                (name, field)
            })
            .collect();
        self.run.selected = None;
        self.run.findings.clear();
        self.run.error = None;
        self.run.start_keys_ready = false;
        self.run.starting = true;
        self.open_start(cx);
        cx.notify();
    }
    /// Open another mission with a run's output as its first input and
    /// the run's folder as where it works.
    pub(super) fn use_output(
        &mut self,
        node: String,
        slug: String,
        value: String,
        folder: String,
        cx: &mut Context<Self>,
    ) {
        self.carry = Some((value, folder));
        let target = (node.clone(), slug.clone());
        self.open(node, slug, cx);
        if self.state.selected.as_ref() != Some(&target) {
            self.carry = None;
        }
    }
    /// The start form of the open mission, its first input holding `value`.
    pub(super) fn start_with(&mut self, value: &str, folder: &str, cx: &mut Context<Self>) {
        // Asked for from another page: nothing here was clicked.
        self.run.start_at = None;
        self.prepare_run(cx);
        if !folder.is_empty() {
            self.run
                .cwd
                .update(cx, |field, cx| field.set(folder, false, cx));
        }
        let first = self
            .state
            .mission
            .as_ref()
            .and_then(|m| m.input_cards.first())
            .and_then(|card| card.inputs.first())
            .map(|input| input.name.clone())
            .unwrap_or_else(|| "brief".into());
        if let Some(field) = self.run.fields.get(&first) {
            field.update(cx, |field, cx| field.set(value, false, cx));
        }
    }
    /// The start form for a finished run's mission, holding its inputs.
    pub(super) fn run_again(
        &mut self,
        card: &str,
        inputs: &[(String, String)],
        cx: &mut Context<Self>,
    ) {
        let known = self
            .state
            .mission
            .as_ref()
            .is_some_and(|m| m.input_cards.iter().any(|c| c.id == card));
        if known {
            self.prepare_card_run(card, cx);
        } else {
            self.prepare_run(cx);
        }
        for (name, field) in &self.run.fields {
            if let Some((_, value)) = inputs.iter().find(|(given, _)| given == name) {
                field.update(cx, |field, cx| field.set(value, false, cx));
            }
        }
    }
    pub(super) fn prepare_step_run(&mut self, id: String, cx: &mut Context<Self>) {
        if self.start_uncertain() {
            return;
        }
        if self.unsaved(cx).is_some() {
            self.state.error = Some("Save or discard the current draft first.".into());
            cx.notify();
            return;
        }
        let Some(definition) = self
            .state
            .mission
            .clone()
            .zip(self.state.mission_etag.clone())
        else {
            return;
        };
        self.clear_start_context(cx);
        self.run.start_definition = Some(definition);
        self.run.target = Some(id);
        self.run.start_card = None;
        self.run.request_id = uuid::Uuid::new_v4().to_string();
        self.run.uncertain = None;
        self.run.fields = [(
            "brief".to_string(),
            cx.new(|cx| TextField::new(cx, "Brief").prose()),
        )]
        .into_iter()
        .collect();
        self.run.selected = None;
        self.run.findings.clear();
        self.run.error = None;
        self.run.start_keys_ready = false;
        self.run.starting = true;
        self.editor = None;
        self.open_start(cx);
        cx.notify();
    }
    /// The start form's fields in the order Tab goes through them.
    fn start_order(&self) -> Vec<Entity<TextField>> {
        let mut order: Vec<Entity<TextField>> = self
            .state
            .mission
            .as_ref()
            .and_then(|mission| {
                mission
                    .input_cards
                    .iter()
                    .find(|card| self.run.start_card.as_deref() == Some(card.id.as_str()))
            })
            .map(|card| {
                card.inputs
                    .iter()
                    .filter_map(|input| self.run.fields.get(&input.name).cloned())
                    .collect()
            })
            .unwrap_or_default();
        if order.is_empty() {
            order.extend(self.run.fields.values().cloned());
        }
        order.extend([
            self.run.issue.clone(),
            self.run.cwd.clone(),
            self.run.repository.clone(),
            self.run.base.clone(),
            self.run.paths.clone(),
        ]);
        order
    }
    pub(super) fn setup_start_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.run.starting || self.run.start_keys_ready {
            return;
        }
        self.run.start_keys_ready = true;
        self.run.start_subs.clear();
        let order = self.start_order();
        for (index, field) in order.iter().enumerate() {
            let order = order.clone();
            self.run.start_subs.push(cx.subscribe_in(
                field,
                window,
                move |page, _, event: &FieldEvent, window, cx| match event {
                    FieldEvent::Submit | FieldEvent::SubmitAlt => page.start_run(cx),
                    FieldEvent::Tab => ahjo_app_kit::widgets::field::focus(
                        &order[(index + 1) % order.len()],
                        window,
                        cx,
                    ),
                    _ => {}
                },
            ));
        }
    }
    /// Opens the start form beside what started it, or the mission's
    /// Start, the keyboard in its first field. A press outside leaves what
    /// was typed; Escape or Cancel closes it, but not while its start is
    /// unanswered.
    fn open_start(&mut self, cx: &mut Context<Self>) {
        if self.run.start_popup.is_some() {
            return;
        }
        let spot = self
            .run
            .start_at
            .take()
            .or_else(|| self.run.start_button.get().map(Spot))
            .unwrap_or_else(|| Spot::point(gpui::point(px(size::PAD), px(size::TAB_BAR))));
        let view = popup::drawn(&cx.entity(), |p: &mut Self, _, cx| p.run_start_form(cx), cx);
        let (asked, closed) = (cx.entity().downgrade(), cx.entity().downgrade());
        let mut start = Popup::beside(spot, view)
            .outside(Outside::Keep)
            .may_close(move |_, cx| {
                asked
                    .read_with(cx, |p, _| !p.run.busy && p.run.uncertain.is_none())
                    .unwrap_or(true)
            })
            .on_dismiss(move |_, cx| {
                let _ = closed.update(cx, |p, cx| p.start_closed(cx));
            });
        if let Some(first) = self.start_order().first() {
            start = start.focus(first.read(cx).focus_handle(cx));
        }
        self.run.start_popup = self.host.popup(start, cx);
    }
    /// The start form's popup closed: what was typed goes.
    fn start_closed(&mut self, cx: &mut Context<Self>) {
        if self.run.start_popup.take().is_none() {
            return;
        }
        self.run.starting = false;
        self.run.fields.clear();
        self.run.error = None;
        self.run.findings.clear();
        cx.notify();
    }
    /// The start form is done with: it closes, keeping nothing open.
    pub(super) fn end_start(&mut self, cx: &mut Context<Self>) {
        self.run.starting = false;
        if let Some(id) = self.run.start_popup.take() {
            self.host.close_popup(id, cx);
        }
    }
    /// Stop pressed: its confirmation beside it, the keyboard in its
    /// reason.
    pub(super) fn open_stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run
            .stop_reason
            .update(cx, |field, cx| field.set("", false, cx));
        let view = popup::drawn(&cx.entity(), |p: &mut Self, _, cx| p.stop_form(cx), cx);
        let page = cx.entity().downgrade();
        self.run.stop_popup = self.host.popup(
            Popup::beside(Spot::here(window, cx), view)
                .focus(self.run.stop_reason.read(cx).focus_handle(cx))
                .on_dismiss(move |_, cx| {
                    let _ = page.update(cx, |p, _| p.run.stop_popup = None);
                }),
            cx,
        );
    }
    fn end_stop(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.run.stop_popup.take() {
            self.host.close_popup(id, cx);
        }
    }
    /// The attempts, read, beside what asked for them; a press outside
    /// leaves the reason typed for a pick.
    fn open_attempts(&mut self, cx: &mut Context<Self>) {
        if self.run.attempts_popup.is_some() {
            return;
        }
        let spot = self
            .run
            .attempts_from
            .take()
            .unwrap_or_else(|| Spot::point(gpui::point(px(size::PAD), px(size::TAB_BAR))));
        let view = popup::drawn(
            &cx.entity(),
            |p: &mut Self, window, cx| p.attempts_form(window, cx),
            cx,
        );
        let page = cx.entity().downgrade();
        self.run.attempts_popup = self.host.popup(
            Popup::beside(spot, view)
                .focus(self.run.attempts_focus.clone())
                .outside(Outside::Keep)
                .on_dismiss(move |_, cx| {
                    let _ = page.update(cx, |p, cx| {
                        p.run.attempts_popup = None;
                        p.run.attempt_window = None;
                        cx.notify();
                    });
                }),
            cx,
        );
    }
    pub(super) fn end_attempts(&mut self, cx: &mut Context<Self>) {
        self.run.attempt_window = None;
        if let Some(id) = self.run.attempts_popup.take() {
            self.host.close_popup(id, cx);
        }
    }
    pub(super) fn start_run(&mut self, cx: &mut Context<Self>) {
        if self.run.busy {
            return;
        }
        let (Some((node, slug)), Some((m, etag))) = (
            self.state.selected.clone(),
            self.run.start_definition.as_ref(),
        ) else {
            return;
        };
        if self.run.uncertain.is_none()
            && self
                .state
                .mission
                .as_ref()
                .is_none_or(|current| current.rev != m.rev)
        {
            self.run.findings = vec!["The mission changed since this dialog opened. Cancel and start again to see what would run.".into()];
            cx.notify();
            return;
        }
        let etag = etag.clone();
        let target = self.run.target.clone();
        let input_card_id = if target.is_none() {
            self.run.start_card.clone().unwrap_or_default()
        } else {
            String::new()
        };
        // An uncertain start must retry its exact body. For a fresh start,
        // tell the operator about missing required inputs before sending it.
        let input = m.input_cards.iter().find(|card| card.id == input_card_id);
        if self.run.uncertain.is_none()
            && target.is_none()
            && let Some(card) = input
        {
            let required: Vec<&str> = if card.inputs.is_empty() {
                vec!["brief"]
            } else {
                card.inputs
                    .iter()
                    .filter(|i| i.required)
                    .map(|i| i.name.as_str())
                    .collect()
            };
            let missing: Vec<String> = required
                .into_iter()
                .filter(|name| {
                    self.run
                        .fields
                        .get(*name)
                        .is_none_or(|f| f.read(cx).text.trim().is_empty())
                })
                .map(|name| format!("{name} is required."))
                .collect();
            if !missing.is_empty() {
                self.run.findings = missing;
                self.run.error = None;
                cx.notify();
                return;
            }
        }
        let values = self
            .run
            .fields
            .iter()
            .filter_map(|(name, field)| {
                let value = &field.read(cx).text;
                if value.trim().is_empty() {
                    return None;
                }
                let kind = input
                    .and_then(|card| card.inputs.iter().find(|i| i.name == *name))
                    .map(|i| i.kind.as_str())
                    .unwrap_or("text");
                Some((
                    name.clone(),
                    if matches!(kind, "file" | "folder") {
                        value.trim().to_owned()
                    } else {
                        value.clone()
                    },
                ))
            })
            .collect();
        let start = MissionRun {
            mission: slug,
            input: (!input_card_id.is_empty()).then_some(input_card_id),
            from: target,
            expected_rev: Some(m.rev),
            expected_etag: Some(etag),
            inputs: values,
            actor: Some("human:operator".into()),
            request_id: Some(self.run.request_id.clone()),
            rerun_of: None,
            cwd: Some(self.run.cwd.read(cx).text.clone()),
            repository: Some(self.run.repository.read(cx).text.clone()),
            base: Some(self.run.base.read(cx).text.clone()),
            keep_sessions: self.run.keep_sessions,
            issue: Some(self.run.issue.read(cx).text.clone()),
            context_paths: self
                .run
                .paths
                .read(cx)
                .text
                .lines()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
        };
        let start = self.run.uncertain.clone().unwrap_or(start);
        self.run.uncertain = Some(start.clone());
        self.run.busy = true;
        self.run.error = None;
        self.run.findings.clear();
        let asked = self.host.call(NodeId(node), start, cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if generation != page.generation {
                    return;
                }
                page.run.busy = false;
                match start_answer(result) {
                    StartAnswer::Started(id) => {
                        page.run.uncertain = None;
                        page.end_start(cx);
                        page.show_run(id, cx);
                    }
                    StartAnswer::Refused(error) => {
                        page.run.uncertain = None;
                        if let Some(findings) = ahjo_missions_contract::admission_findings(&error) {
                            page.run.error = Some(if findings.is_empty() {
                                format!("The run could not start: {}", error.message)
                            } else {
                                "The run could not start:".into()
                            });
                            page.run.findings = findings
                                .iter()
                                .map(|finding| {
                                    if finding.card_id.is_empty() {
                                        finding.message.clone()
                                    } else {
                                        let name = page
                                            .state
                                            .mission
                                            .as_ref()
                                            .map(|m| m.card_name(&finding.card_id))
                                            .unwrap_or(&finding.card_id);
                                        format!("{name}: {}", finding.message)
                                    }
                                })
                                .collect();
                        } else {
                            page.run.error = Some(error.message);
                        }
                    }
                    StartAnswer::Unknown(reason) => {
                        page.run.error = Some(format!(
                            "Start outcome unknown: {reason}. Retry the same request."
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn run_command<C: ahjo_core::TypedCommand>(
        &mut self,
        command: impl FnOnce(String) -> C,
        cx: &mut Context<Self>,
    ) {
        if self.run.busy {
            return;
        }
        let (Some((node, _)), Some(run)) = (self.state.selected.clone(), self.run.selected.clone())
        else {
            return;
        };
        let node = NodeId(node);
        self.run.busy = true;
        self.run.error = None;
        let asked = self.host.call(node.clone(), command(run.clone()), cx);
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.run.selected.as_deref() != Some(&run)
                    || page
                        .state
                        .selected
                        .as_ref()
                        .is_none_or(|(n, _)| n != node.as_str())
                {
                    return;
                }
                page.run.busy = false;
                if let Err(error) = result {
                    page.run.error = Some(format!("{}: {}", error.code, error.message));
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn verdict(&mut self, gate: &str, seq: u64, verdict: &str, cx: &mut Context<Self>) {
        let reason = self
            .run
            .responses
            .get(gate)
            .map(|f| f.read(cx).text.clone());
        if verdict == "pass" {
            self.run_command(
                |run| RunApprove {
                    run,
                    gate: gate.into(),
                    seq,
                    reason,
                    actor: Some("human:operator".into()),
                    relayed_by: None,
                },
                cx,
            );
        } else {
            self.run_command(
                |run| RunReject {
                    run,
                    gate: gate.into(),
                    seq,
                    reason,
                    actor: Some("human:operator".into()),
                    relayed_by: None,
                },
                cx,
            );
        }
    }
    /// Pick a candidate: an attempt, and its step where several steps feed
    /// the gate.
    pub(super) fn pick(
        &mut self,
        gate: &str,
        seq: u64,
        (step, attempt): (Option<String>, i64),
        cx: &mut Context<Self>,
    ) {
        let reason = self
            .run
            .responses
            .get(gate)
            .map(|f| f.read(cx).text.clone());
        self.pick_with_reason(gate, seq, (step, attempt), reason, cx);
    }
    fn pick_with_reason(
        &mut self,
        gate: &str,
        seq: u64,
        (step, attempt): (Option<String>, i64),
        reason: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.run_command(
            |run| RunPick {
                run,
                gate: gate.into(),
                seq,
                attempt,
                step,
                reason,
                actor: Some("human:operator".into()),
                relayed_by: None,
            },
            cx,
        );
    }
    pub(super) fn read_evidence(
        &mut self,
        node: &str,
        run: &str,
        at: usize,
        cx: &mut Context<Self>,
    ) {
        self.run.evidence.read_at = Some(at);
        let node = node.to_owned();
        let run = run.to_owned();
        let asked = self.host.call(
            NodeId(node.clone()),
            RunShow {
                run: run.clone(),
                evidence: true,
                ..Default::default()
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.run.selected.as_deref() != Some(&run)
                    || page.state.selected.as_ref().is_none_or(|(n, _)| n != &node)
                    || page.run.evidence.read_at != Some(at)
                {
                    return;
                }
                match result {
                    Ok(answer) => match answer.evidence {
                        Some(evidence) => {
                            page.run.evidence.evidence = Some(evidence);
                            page.run.evidence.error = None;
                        }
                        None => page.run.evidence.error = Some("The node sent no evidence.".into()),
                    },
                    Err(error) => page.run.evidence.error = Some(error.message),
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// Read image bytes through Files on the node that owns the run.
    fn load_attempt_images(&mut self, cx: &mut Context<Self>) {
        let Some(window) = self.run.attempt_window.as_ref() else {
            return;
        };
        let node = window.node.clone();
        let run = window.run.clone();
        let paths: Vec<_> = window
            .attempts()
            .iter()
            .flat_map(|a| &a.outputs)
            .filter_map(|o| {
                crate::store::evidence::image_format(&o.path).map(|format| (o.path.clone(), format))
            })
            .collect();
        for (path, format) in paths {
            if self.run.evidence.images.contains_key(&path) {
                continue;
            }
            self.run
                .evidence
                .images
                .insert(path.clone(), crate::store::evidence::Image::Loading);
            let asked = self.host.request(
                Request::get(
                    NodeId(node.clone()),
                    format!(
                        "{}?path={}",
                        ahjo_files_contract::FILE_BYTES_PATH,
                        segment(&path)
                    ),
                ),
                cx,
            );
            let svg = cx.svg_renderer();
            let selected_node = node.clone();
            let selected_run = run.clone();
            cx.spawn(async move |this, cx| {
                let result = asked.await;
                let image = match result {
                    Ok(answer) if answer.ok() => {
                        let task = cx.background_executor().spawn(async move {
                            gpui::Image::from_bytes(format, answer.body).to_image_data(svg)
                        });
                        match task.await {
                            Ok(image) => crate::store::evidence::Image::Ready(image),
                            Err(error) => crate::store::evidence::Image::Failed(error.to_string()),
                        }
                    }
                    Ok(answer) => crate::store::evidence::Image::Failed(answer.error_text()),
                    Err(error) => crate::store::evidence::Image::Failed(error.message),
                };
                let _ = this.update(cx, |page, cx| {
                    if page.run.selected.as_deref() != Some(&selected_run)
                        || page
                            .state
                            .selected
                            .as_ref()
                            .is_none_or(|(n, _)| n != &selected_node)
                    {
                        return;
                    }
                    page.run.evidence.images.insert(path, image);
                    cx.notify();
                });
            })
            .detach();
        }
    }
    pub(super) fn attempts(&mut self, cx: &mut Context<Self>) {
        let (Some((node, _)), Some(run)) = (self.state.selected.clone(), self.run.selected.clone())
        else {
            return;
        };
        let at = self
            .selected_run()
            .map(|r| r.event_count)
            .unwrap_or_default();
        self.run.attempts_at = Some(at);
        let asked = self.host.call(
            NodeId(node.clone()),
            RunShow {
                run: run.clone(),
                attempts: true,
                ..Default::default()
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.run.selected.as_deref() != Some(&run)
                    || page.state.selected.as_ref().is_none_or(|(n, _)| n != &node)
                    || page.run.attempts_at != Some(at)
                {
                    return;
                }
                match result {
                    Ok(answer) => match answer.attempts {
                        Some(attempts) => {
                            if let Some(window) = &mut page.run.attempt_window
                                && let Some(step) =
                                    attempts.steps.iter().find(|s| s.step_id == window.step)
                            {
                                window.read(step.clone(), None);
                                window.read_at = at;
                            }
                            page.run.attempts = Some(attempts);
                            if let Some((step_id, compare)) = page.run.pending_attempt.take()
                                && let (Some(data), Some((node, _))) =
                                    (&page.run.attempts, &page.state.selected)
                                && let Some(step) =
                                    data.steps.iter().find(|s| s.step_id == step_id).cloned()
                            {
                                let mut window = crate::store::attempts::AttemptsWindow::new(
                                    node,
                                    &run,
                                    &step_id,
                                    &step.title,
                                    at,
                                );
                                window.read(step, None);
                                if compare {
                                    window.toggle_compare();
                                }
                                page.run.attempt_window = Some(window);
                                page.open_attempts(cx);
                            }
                            page.load_attempt_images(cx);
                        }
                        None => page.run.error = Some("The node sent no attempts.".into()),
                    },
                    Err(e) => page.run.error = Some(e.message),
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// The steps table's tries open the step's attempts window, as a
    /// gate's Compare does; retain the run/node identity across the read.
    pub(super) fn show_step_attempts(
        &mut self,
        step: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run.pending_attempt = Some((step.to_owned(), false));
        self.run.attempts_from = Some(Spot::here(window, cx));
        self.attempts(cx);
    }
    /// The old attempts window's shortcuts move the focused attempt or the
    /// other comparison side; text-entry and modified shortcuts stay theirs.
    fn attempts_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.run.attempt_window.as_mut() else {
            return;
        };
        let key = &event.keystroke;
        if key.modifiers.control || key.modifiers.alt || key.modifiers.platform {
            return;
        }
        let numbers = open.numbers();
        match key.key.as_str() {
            "left" | "right" => {
                let delta = if key.key == "left" { -1 } else { 1 };
                if key.modifiers.shift && open.compare.is_none() {
                    open.toggle_compare();
                }
                if key.modifiers.shift && open.compare.is_some() {
                    open.side = 1 - open.side;
                }
                open.step(delta);
                if key.modifiers.shift && open.compare.is_some() {
                    open.side = 1 - open.side;
                }
            }
            "home" => {
                if let Some(n) = numbers.first() {
                    open.choose(*n);
                }
            }
            "end" => {
                if let Some(n) = numbers.last() {
                    open.choose(*n);
                }
            }
            "c" if numbers.len() > 1 => open.toggle_compare(),
            digit if digit.len() == 1 && digit.bytes().all(|c| c.is_ascii_digit()) => {
                let n = digit.parse::<i64>().unwrap_or(0);
                if !numbers.contains(&n) {
                    return;
                }
                open.choose(n);
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
    /// Where an input's value can come from besides typing: a file picked
    /// on this device, or what an earlier run produced.
    fn input_sources(
        &self,
        t: &Theme,
        name: &str,
        kind: &str,
        field: &Entity<TextField>,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let mut row = div().flex().flex_wrap().items_center().gap(px(TAB_GAP));
        let mut any = kind == "file";
        if kind == "file" {
            let field = field.clone();
            let selector = format!("run-input-{name}-choose");
            row = row.child(
                kit::button(
                    t,
                    SharedString::from(selector.clone()),
                    "Choose file…",
                    Kind::Quiet,
                )
                .debug_selector(move || selector.clone())
                .on_click(cx.listener(move |p, _, window, cx| {
                    let field = field.clone();
                    ahjo_app_kit::widgets::path_picker::pick(
                        p.host.clone(),
                        "Choose a file",
                        window,
                        cx,
                        move |_, paths, cx| {
                            if let Some(path) = paths.first() {
                                let path = path.to_string_lossy().into_owned();
                                field.update(cx, |f, cx| f.set(&path, false, cx));
                            }
                            cx.notify();
                        },
                    );
                })),
            );
        }
        let runs = self
            .state
            .selected
            .as_ref()
            .and_then(|(node, _)| self.state.nodes.get(node))
            .map(|feed| {
                let here = self
                    .state
                    .selected
                    .as_ref()
                    .map_or("", |(_, slug)| slug.as_str());
                crate::store::run_state::earlier_outputs(feed.runs.values(), kind, here, 6)
            })
            .unwrap_or_default();
        if !runs.is_empty() {
            any = true;
            row = row.child(kit::note(t, "From earlier runs:", None));
        }
        for (index, (label, value)) in runs.into_iter().enumerate() {
            let field = field.clone();
            let selector = format!("run-input-{name}-earlier-{index}");
            let tip = value.clone();
            row = row.child(
                kit::button(t, SharedString::from(selector.clone()), label, Kind::Quiet)
                    .debug_selector(move || selector.clone())
                    // Held inside the form; the whole value is in its tip.
                    .max_w_full()
                    .justify_start()
                    .truncate()
                    .tooltip(kit::tip(tip))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        field.update(cx, |f, cx| f.set(&value, false, cx));
                        cx.notify();
                    })),
            );
        }
        any.then_some(row)
    }
    /// The start form, in its popup.
    fn run_start_form(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = &theme(cx).clone();
        if self.run.starting {
            let title = self
                .run
                .start_definition
                .as_ref()
                .map(|(mission, _)| mission)
                .or(self.state.mission.as_ref())
                .map(|m| {
                    if m.title.trim().is_empty() {
                        m.slug.clone()
                    } else {
                        m.title.clone()
                    }
                })
                .unwrap_or_default();
            let mut form = div().p(px(PAD)).flex().flex_col().gap(px(TAB_GAP)).child(
                div()
                    .pb(px(TAB_GAP))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("Start {title}")),
            );
            let declared = self.run.start_definition.as_ref().and_then(|(mission, _)| {
                mission
                    .input_cards
                    .iter()
                    .find(|card| self.run.start_card.as_deref() == Some(card.id.as_str()))
            });
            let names: Vec<&str> = match declared {
                Some(card) if self.run.target.is_none() && !card.inputs.is_empty() => card
                    .inputs
                    .iter()
                    .map(|input| input.name.as_str())
                    .collect(),
                _ => self.run.fields.keys().map(String::as_str).collect(),
            };
            for name in names {
                let Some(field) = self.run.fields.get(name) else {
                    continue;
                };
                let input =
                    declared.and_then(|card| card.inputs.iter().find(|input| input.name == name));
                let required = input.is_some_and(|input| input.required)
                    || (name == "brief" && input.is_none());
                let description = input
                    .map(|input| input.description.as_str())
                    .or_else(|| declared.map(|card| card.input_hint.as_str()))
                    .unwrap_or("");
                let kind = input
                    .map(|input| input.kind.as_str())
                    .filter(|kind| !kind.is_empty())
                    .unwrap_or("text");
                let selector = format!("run-input-{name}");
                form = form
                    .child(kit::label(
                        t,
                        format!("{name}{}", if required { "" } else { " (optional)" }),
                    ))
                    .child(
                        div()
                            .debug_selector(move || selector.clone())
                            .child(field.clone()),
                    );
                if !description.is_empty() {
                    form = form.child(div().text_color(t.dim).child(description.to_owned()));
                }
                form = form.children(self.input_sources(t, name, kind, field, cx));
            }
            let cwd = self.run.cwd.clone();
            form = form
                .child(kit::label(t, "Folder"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(TAB_GAP))
                        .child(div().flex_1().min_w_0().child(self.run.cwd.clone()))
                        .child(
                            kit::button(t, "run-cwd-choose", "Choose…", Kind::Quiet)
                                .debug_selector(|| "run-cwd-choose".into())
                                .on_click(cx.listener(move |p, _, window, cx| {
                                    let cwd = cwd.clone();
                                    ahjo_app_kit::widgets::path_picker::pick_folder(
                                        p.host.clone(),
                                        "Choose the working folder",
                                        window,
                                        cx,
                                        move |_, paths, cx| {
                                            if let Some(path) = paths.first() {
                                                let path = path.to_string_lossy().into_owned();
                                                cwd.update(cx, |f, cx| f.set(&path, false, cx));
                                            }
                                            cx.notify();
                                        },
                                    );
                                })),
                        ),
                )
                .child(
                    div()
                        .when(self.files_go(cx).is_empty(), |d| d.hidden())
                        .debug_selector(|| "run-files-go".into())
                        .text_size(px(size::SMALL))
                        .text_color(t.dim)
                        .child(self.files_go(cx)),
                );
            // What few runs need waits behind one line, open once any of it
            // holds something.
            let more = self.run.start_more
                || self.run.keep_sessions
                || !self.run.repository.read(cx).text.trim().is_empty()
                || !self.run.issue.read(cx).text.trim().is_empty()
                || !self.run.paths.read(cx).text.trim().is_empty();
            if !more {
                form = form.child(
                    div().flex().pt(px(TAB_GAP)).child(
                        kit::button(t, "run-start-more", "More…", Kind::Quiet)
                            .debug_selector(|| "run-start-more".into())
                            .on_click(cx.listener(|p, _, _, cx| {
                                p.run.start_more = true;
                                cx.notify();
                            })),
                    ),
                );
            } else {
                if self.repositories.as_ref().is_some_and(|r| !r.is_empty())
                    || !self.run.repository.read(cx).text.trim().is_empty()
                {
                    form = form
                        .child(kit::label(t, "Repository"))
                        .child(self.repository_picker(
                            t,
                            &self.run.repository,
                            "run-repository",
                            cx,
                        ));
                }
                // A base names where a repository's branch starts; without
                // one it means nothing.
                if !self.run.repository.read(cx).text.trim().is_empty() {
                    form = form
                        .child(kit::label(t, "Base"))
                        .child(div().child(self.run.base.clone()));
                }
                form = form
                    .child(kit::label(t, "Issue"))
                    .child(div().child(self.run.issue.clone()))
                    .child(kit::label(t, "Context paths"))
                    .child(
                        div()
                            .debug_selector(|| "run-context-paths".into())
                            .child(self.run.paths.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(TAB_GAP))
                            .child(
                                kit::toggle(t, "run-keep-sessions", self.run.keep_sessions)
                                    .debug_selector(|| "run-keep-sessions".into())
                                    .on_click(cx.listener(|p, _, _, cx| {
                                        if !p.run.busy && p.run.uncertain.is_none() {
                                            p.run.keep_sessions = !p.run.keep_sessions;
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(kit::label(t, "Keep seat sessions after the run")),
                    );
            }
            if self.run.uncertain.is_some() && !self.run.busy {
                form = form.child("Retry sends the same start request and its original values.");
            }
            form = form.when_some(self.run.error.clone(), |d, e| {
                let message = self
                    .run
                    .start_definition
                    .as_ref()
                    .map(|(mission, _)| crate::store::validation::readable_message(mission, &e))
                    .unwrap_or(e);
                d.child(div().text_color(t.error).child(message))
            });
            for (i, finding) in self.run.findings.iter().enumerate() {
                let selector = if finding.ends_with(" is required.") {
                    format!("run-required-{}", finding.trim_end_matches(" is required."))
                } else {
                    format!("run-finding-{i}-{finding}")
                };
                let display = self
                    .run
                    .start_definition
                    .as_ref()
                    .map(|(mission, _)| {
                        crate::store::validation::readable_message(mission, finding)
                    })
                    .unwrap_or_else(|| finding.clone());
                form = form.child(
                    div()
                        .debug_selector(move || selector.clone())
                        .text_color(t.error)
                        .child(display),
                );
            }
            let mut actions = div()
                .p(px(PAD))
                .flex_none()
                .flex()
                .justify_end()
                .gap(px(PAD))
                .debug_selector(|| "run-start-actions".into());
            if !self.run.busy && self.run.uncertain.is_none() {
                actions = actions.child(
                    kit::button(t, "run-cancel", "Cancel", Kind::Quiet)
                        .debug_selector(|| "run-cancel".into())
                        .on_click(cx.listener(|page, _, _, cx| {
                            if let Some(id) = page.run.start_popup {
                                page.host.close_popup(id, cx);
                            }
                        })),
                );
            }
            actions = actions.child(
                kit::button(t, "run-start", "Start", Kind::Primary)
                    .debug_selector(|| "run-start".into())
                    .on_click(cx.listener(|p, _, _, cx| p.start_run(cx))),
            );
            let dialog = popup::form("start-fields", form, actions)
                .debug_selector(|| "start-dialog".into())
                .w(px(size::NOTICE))
                .max_h(px(START_FORM_MAX));
            return Some(dialog.into_any_element());
        }
        None
    }
    /// Stop's confirmation, in its popup.
    fn stop_form(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = &theme(cx).clone();
        let run = self.selected_run()?;
        if run.r#final {
            return None;
        }
        let mission = self.run.frozen.as_ref().or(self.state.mission.as_ref());
        let mut body = div()
            .id("stop-run")
            .debug_selector(|| "stop-run".into())
            .w(px(size::NOTICE))
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(PAD))
            .child(format!(
                "Stop {}?",
                mission
                    .map(|m| m.title.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&run.mission_slug)
            ));
        for point in run_state::points(run) {
            if point.kind == run_state::PointKind::Running && !point.card.is_empty() {
                let title = mission
                    .map(|mission| mission.card_name(&point.card))
                    .unwrap_or(&point.card);
                body = body.child(format!("The run stops working on {title}."));
            }
        }
        for gate in &run.waiting_gates {
            let title = mission
                .map(|mission| mission.card_name(&gate.gate_id))
                .unwrap_or(&gate.gate_id);
            body = body.child(format!("{title} stops waiting for you."));
        }
        body = body
            .child(
                "The run ends as canceled and cannot be resumed. Its workspace and outputs stay.",
            )
            .child("Why (optional)")
            .child(
                div()
                    .debug_selector(|| "stop-reason".into())
                    .child(self.run.stop_reason.clone()),
            )
            .child(
                kit::button(t, "stop-keep", "Keep running", Kind::Plain)
                    .debug_selector(|| "stop-keep".into())
                    .on_click(cx.listener(|page, _, _, cx| page.end_stop(cx))),
            )
            .child(
                kit::button(t, "stop-confirm", "Stop run", Kind::Primary)
                    .debug_selector(|| "stop-confirm".into())
                    .on_click(cx.listener(|page, _, _, cx| {
                        let reason = page.run.stop_reason.read(cx).text.trim().to_owned();
                        page.end_stop(cx);
                        page.run_command(
                            |run| RunAbort {
                                run,
                                reason: Some(reason),
                                actor: Some("human:operator".into()),
                            },
                            cx,
                        );
                    })),
            );
        Some(body.into_any_element())
    }
    /// The attempts of a step, side by side, in their popup.
    fn attempts_form(&mut self, room: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = &theme(cx).clone();
        let room = room.viewport_size();
        let run = self.selected_run()?.clone();
        let (node, _) = self.state.selected.clone()?;
        if let Some(window) = &self.run.attempt_window {
            let mut bar = div()
                .flex()
                .items_center()
                .gap(px(TAB_GAP))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Attempts: {}", window.title)),
                )
                .child(
                    kit::button(t, "attempt-prev", "Previous", Kind::Quiet).on_click(cx.listener(
                        |p, _, _, cx| {
                            if let Some(w) = &mut p.run.attempt_window {
                                w.step(-1);
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(
                    kit::button(t, "attempt-next", "Next", Kind::Quiet).on_click(cx.listener(
                        |p, _, _, cx| {
                            if let Some(w) = &mut p.run.attempt_window {
                                w.step(1);
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(
                    kit::button(t, "attempt-compare", "Compare", Kind::Quiet)
                        .debug_selector(|| "attempt-compare".into())
                        .on_click(cx.listener(|p, _, _, cx| {
                            if let Some(w) = &mut p.run.attempt_window {
                                w.toggle_compare();
                            }
                            cx.notify();
                        })),
                );
            if window.compare.is_some() {
                bar = bar.child(
                    kit::button(
                        t,
                        "attempt-side",
                        if window.side == 0 {
                            "Focus right"
                        } else {
                            "Focus left"
                        },
                        Kind::Quiet,
                    )
                    .on_click(cx.listener(|p, _, _, cx| {
                        if let Some(w) = &mut p.run.attempt_window {
                            w.side = 1 - w.side;
                        }
                        cx.notify();
                    })),
                );
            }
            let mut panel = div()
                .id("attempts-window")
                .track_focus(&self.run.attempts_focus)
                .on_key_down(cx.listener(Self::attempts_key))
                .flex()
                .flex_col()
                .gap(px(PAD))
                .debug_selector(|| "attempts-window".into())
                .child(bar);
            let mut pips = div().flex().flex_wrap().gap(px(PAD));
            for attempt in window.attempts() {
                let number = attempt.attempt;
                let label = format!(
                    "Attempt {number} · {}",
                    crate::store::attempts::outcome_word(crate::store::attempts::outcome(
                        attempt,
                        &run.waiting_gates
                            .iter()
                            .map(|g| g.gate_id.clone())
                            .collect()
                    ))
                );
                let selector = format!("attempt-pip-{number}");
                pips = pips.child(
                    kit::button(t, SharedString::from(selector.clone()), label, Kind::Quiet)
                        .debug_selector(move || selector.clone())
                        .on_click(cx.listener(move |p, _, _, cx| {
                            if let Some(w) = &mut p.run.attempt_window {
                                w.choose(number);
                            }
                            cx.notify();
                        })),
                );
            }
            panel = panel.child(pips);
            let selected: Vec<i64> = window
                .compare
                .map(|(a, b)| vec![a, b])
                .unwrap_or_else(|| window.chosen().into_iter().collect());
            let mut columns = div().flex().gap(px(PAD));
            for number in selected {
                if let Some(attempt) = window.attempts().iter().find(|a| a.attempt == number) {
                    let selector = format!("attempt-card-{}", attempt.attempt);
                    let mut card = div()
                        .debug_selector(move || selector.clone())
                        .flex_1()
                        .min_w_0()
                        .p(px(PAD))
                        .border_1()
                        .border_color(t.divider)
                        .child(format!(
                            "Attempt {} · {}",
                            attempt.attempt,
                            crate::store::attempts::outcome_word(crate::store::attempts::outcome(
                                attempt,
                                &run.waiting_gates
                                    .iter()
                                    .map(|g| g.gate_id.clone())
                                    .collect()
                            ))
                        ));
                    let timing =
                        crate::store::attempts::timing(attempt, time::OffsetDateTime::now_utc());
                    if !timing.is_empty() {
                        card = card.child(timing);
                    }
                    if let Some(reason) = &attempt.stop_reason {
                        card = card.child(reason.text.clone());
                    }
                    for verdict in &attempt.verdicts {
                        let waiting = run
                            .waiting_gates
                            .iter()
                            .map(|g| g.gate_id.clone())
                            .collect();
                        card = card.child(format!(
                            "{} · {}",
                            crate::store::attempts::verdict_headline(verdict, &waiting),
                            verdict.reason.text
                        ));
                        for evidence in &verdict.evidence {
                            card = card.child(format!("• {}", evidence.text.text));
                        }
                    }
                    if let Some(pick) = &attempt.picked {
                        let by = crate::store::attempts::picker_words(&pick.decided_by, |id| {
                            self.run
                                .frozen
                                .as_ref()
                                .map(|m| m.card_name(id).to_owned())
                                .unwrap_or_else(|| id.to_owned())
                        });
                        card =
                            card.child(format!("Picked as the best · {by}: {}", pick.reason.text));
                    }
                    let recommendation = run
                        .waiting_gates
                        .iter()
                        .filter_map(|gate| gate.pick.as_ref())
                        .filter(|pick| {
                            pick.get("step_id").and_then(|v| v.as_str())
                                == Some(window.step.as_str())
                        })
                        .filter_map(|pick| pick.get("recommendation"))
                        .find(|rec| {
                            rec.get("attempt").and_then(|v| v.as_i64()) == Some(attempt.attempt)
                        });
                    if let Some(rec) = recommendation {
                        card = card.child(format!(
                            "The judge picks this one: {}",
                            rec.get("reason")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                        ));
                    }
                    if attempt.outputs.is_empty() && attempt.stop_reason.is_none() {
                        card = card.child(match attempt.status.as_str() {
                            "running" => "No output yet.",
                            "stopped" => "Stopped before its output.",
                            _ => "No output recorded.",
                        });
                    }
                    for output in &attempt.outputs {
                        card = card.child(format!("{}: {}", output.label, output.text.text));
                        match self.run.evidence.images.get(&output.path) {
                            Some(crate::store::evidence::Image::Ready(image)) => {
                                let selector = format!("attempt-image-{}", attempt.attempt);
                                card = card.child(
                                    div().debug_selector(move || selector.clone()).child(
                                        gpui::img(image.clone())
                                            .w_full()
                                            .max_h(px(size::NOTICE))
                                            .object_fit(gpui::ObjectFit::Contain),
                                    ),
                                );
                            }
                            Some(crate::store::evidence::Image::Loading) => {
                                card = card.child("Loading image…");
                            }
                            Some(crate::store::evidence::Image::Failed(error)) => {
                                card = card.child(format!("Image unavailable: {error}"));
                            }
                            None => (),
                        }
                        if output.text.truncated {
                            card = card.child("Showing the start of a long output.");
                        }
                        if !output.path.is_empty() {
                            let path = output.path.clone();
                            let node = node.clone();
                            card = card.child(
                                kit::button(
                                    t,
                                    SharedString::from(format!(
                                        "attempt-path-{}-{}",
                                        window.step, attempt.attempt
                                    )),
                                    "Open file",
                                    Kind::Quiet,
                                )
                                .on_click(cx.listener(
                                    move |p, _, _, cx| {
                                        if let Err(e) = p.host.ask(
                                            Ask::OpenPath {
                                                node: NodeId(node.clone()),
                                                path: path.clone(),
                                                beside: false,
                                            },
                                            cx,
                                        ) {
                                            p.run.error = Some(e.message);
                                            cx.notify();
                                        }
                                    },
                                )),
                            );
                        }
                    }
                    columns = columns.child(card);
                }
            }
            panel = panel.child(columns);
            if let Some((gate, seq, candidates)) = run.waiting_gates.iter().find_map(|gate| {
                let pick = gate.pick.as_ref()?;
                (pick.get("step_id")?.as_str()? == window.step).then(|| {
                    (
                        gate.gate_id.clone(),
                        gate.requested_seq,
                        pick.get("candidates")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default(),
                    )
                })
            }) && let Some(chosen) = window
                .chosen()
                .filter(|number| candidates.iter().any(|c| c.as_i64() == Some(*number)))
            {
                panel = panel
                    .child(
                        div()
                            .debug_selector(|| "attempt-pick-reason".into())
                            .child(self.run.attempt_reason.clone()),
                    )
                    .child(
                        kit::button(
                            t,
                            "attempts-pick-go",
                            format!("Pick attempt {chosen}"),
                            Kind::Primary,
                        )
                        .debug_selector(|| "attempts-pick-go".into())
                        .on_click(cx.listener(move |page, _, _, cx| {
                            let reason = page.run.attempt_reason.read(cx).text.trim().to_owned();
                            page.pick_with_reason(&gate, seq, (None, chosen), Some(reason), cx);
                        })),
                    );
            }
            // Wide enough to read two attempts side by side, and never
            // wider or taller than the window keeps room for.
            return Some(
                div()
                    .w(px(ATTEMPTS_W).min(room.width - px(2. * size::POPUP_EDGE)))
                    .max_h(px(ATTEMPTS_H).min(room.height - px(2. * size::POPUP_EDGE)))
                    .flex()
                    .flex_col()
                    .child(pane::scroll("attempts-body").p(px(PAD)).child(panel))
                    .into_any_element(),
            );
        }
        None
    }
    pub(super) fn open_beside(&mut self, node: &str, path: &str, cx: &mut Context<Self>) {
        if let Err(error) = self.host.ask(
            Ask::OpenPath {
                node: NodeId(node.to_owned()),
                path: path.to_owned(),
                beside: true,
            },
            cx,
        ) {
            self.run.error = Some(error.message);
            cx.notify();
        }
    }
}
/// A status phrase as a sentence.
pub(super) fn sentence(phrase: &str) -> String {
    let mut chars = phrase.chars();
    match chars.next() {
        Some(first) => {
            let text = first.to_uppercase().chain(chars).collect::<String>();
            if text.ends_with(['.', '!', '?']) {
                text
            } else {
                text + "."
            }
        }
        None => String::new(),
    }
}
pub(super) fn segment(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub(super) fn target_words(targets: &[flow::Target]) -> String {
    targets
        .iter()
        .map(|target| match target {
            flow::Target::Step {
                card_id,
                number,
                title,
                back,
            } => format!(
                "{}{}{}",
                number.map(|n| format!("{n}. ")).unwrap_or_default(),
                if title.is_empty() { card_id } else { title },
                if *back { " ↩" } else { "" }
            ),
            flow::Target::End {
                card_id,
                title,
                outcome,
            } => format!(
                "{} ({})",
                if title.is_empty() { card_id } else { title },
                outcome
            ),
            flow::Target::Nowhere => "not connected".into(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

enum StartAnswer {
    Started(String),
    Refused(ahjo_core::Error),
    Unknown(String),
}
fn start_answer(result: Result<MissionStarted, ahjo_core::Error>) -> StartAnswer {
    match result {
        Ok(answer) if !answer.run_id.is_empty() => StartAnswer::Started(answer.run_id),
        Ok(_) => StartAnswer::Unknown("the node did not name the run".into()),
        Err(error)
            if matches!(
                error.code.as_str(),
                "unreachable" | "unanswered" | "invalid_response"
            ) =>
        {
            StartAnswer::Unknown(error.message)
        }
        Err(error) => StartAnswer::Refused(error),
    }
}
/// A run in the folder Ahjo made for it, named after the run.
fn made_for(run: &RunProjection) -> bool {
    run.repository.is_empty() && run.cwd.ends_with(&format!("/{}", run.run_id))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_start_without_an_answer_stays_unknown_until_the_node_answers() {
        assert!(matches!(
            start_answer(Err(ahjo_core::Error::new("unanswered", "lost"))),
            StartAnswer::Unknown(_)
        ));
        assert!(matches!(
            start_answer(Err(ahjo_core::Error::new(
                "invalid_response",
                "bad response"
            ))),
            StartAnswer::Unknown(_)
        ));
        assert!(
            matches!(start_answer(Ok(MissionStarted { node: "n".into(), run_id: "run_1".into() })), StartAnswer::Started(id) if id == "run_1")
        );
        assert!(matches!(
            start_answer(Err(ahjo_core::Error::new("conflict", "changed"))),
            StartAnswer::Refused(_)
        ));
    }
}
