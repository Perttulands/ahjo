//! Reusable agents, their instructions and the missions that use them.
use std::collections::BTreeMap;

use ahjo_app_kit::ask::Ask;
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::theme::{size, theme};
use ahjo_app_kit::widgets::{
    field::TextField,
    kit::{self, Kind},
    pane,
};
use ahjo_app_kit::{Host, Mounted, Page, mount};
use ahjo_core::NodeId;
use ahjo_harness_contract::agents::{EditAgent, ListAgents, ListedAgents, ShowAgent, ShownAgent};
use gpui::{
    App, AppContext, Context, Entity, IntoElement, Render, SharedString, Window, div, prelude::*,
    px,
};

pub fn build(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, AgentsPage::new)
}

struct Fields {
    name: Entity<TextField>,
    instructions: Entity<TextField>,
    harness: Entity<TextField>,
    model: Entity<TextField>,
    effort: Entity<TextField>,
}

pub struct AgentsPage {
    host: Host,
    lists: BTreeMap<String, ListedAgents>,
    selected: Option<(NodeId, String)>,
    detail: Option<ShownAgent>,
    fields: Fields,
    shown: bool,
    loading: bool,
    editing: bool,
    saving: bool,
    generation: u64,
    error: Option<String>,
}

impl Page for AgentsPage {
    const ID: &'static str = "agents";
    const TITLE: &'static str = "Agents";
    const AREA: Option<&'static str> = Some("harness");
    const ANSWERS: &'static [ahjo_app_kit::ask::Answers] =
        &[ahjo_app_kit::ask::Answers::Reveal("agent")];
    fn answer(&mut self, ask: Ask, _: &'static str, _: &mut Window, cx: &mut Context<Self>) {
        if let Ask::Reveal { id, node, .. } = ask {
            let node = node.or_else(|| self.host.nodes(cx).first().map(|node| node.id.clone()));
            if let Some(node) = node {
                self.select(node, id, cx);
            } else {
                self.error = Some("No node is connected.".into());
                cx.notify();
            }
        }
    }

    fn shown(&mut self, shown: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.shown = shown;
        if shown {
            self.read_all(cx);
        }
    }
    fn link(
        &mut self,
        node: &NodeId,
        state: Option<LinkState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shown && state == Some(LinkState::Open) {
            self.read_list(node.clone(), cx);
        }
    }
}

impl AgentsPage {
    fn new(host: Host, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let fields = Fields {
            name: cx.new(|cx| TextField::new(cx, "Name")),
            instructions: cx.new(|cx| TextField::new(cx, "Instructions").prose()),
            harness: cx.new(|cx| TextField::new(cx, "claude-code, openai-codex, or pi")),
            model: cx.new(|cx| TextField::new(cx, "Harness default")),
            effort: cx.new(|cx| TextField::new(cx, "low, medium, high, xhigh, max, or ultra")),
        };
        Self {
            host,
            lists: BTreeMap::new(),
            selected: None,
            detail: None,
            fields,
            shown: false,
            loading: false,
            editing: false,
            saving: false,
            generation: 0,
            error: None,
        }
    }
    fn read_all(&mut self, cx: &mut Context<Self>) {
        for node in self.host.nodes(cx) {
            self.read_list(node.id, cx);
        }
    }
    fn read_list(&mut self, node: NodeId, cx: &mut Context<Self>) {
        let task = self.host.call(node.clone(), ListAgents {}, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |page, cx| {
                match result {
                    Ok(list) => {
                        let first = list.agents.first().map(|a| a.id.clone());
                        page.lists.insert(node.to_string(), list);
                        if page.selected.is_none()
                            && let Some(id) = first
                        {
                            page.select(node, id, cx);
                        }
                    }
                    Err(error) => page.error = Some(error.message),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn select(&mut self, node: NodeId, id: String, cx: &mut Context<Self>) {
        if self.editing || self.saving {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.selected = Some((node.clone(), id.clone()));
        self.detail = None;
        self.loading = true;
        self.error = None;
        let task = self
            .host
            .call(node.clone(), ShowAgent { agent: id.clone() }, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation || page.selected.as_ref() != Some(&(node, id)) {
                    return;
                }
                page.loading = false;
                match result {
                    Ok(detail) => page.detail = Some(detail),
                    Err(error) => page.error = Some(error.message),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn edit(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = &self.detail else {
            return;
        };
        for (field, text) in [
            (&self.fields.name, detail.agent.name.clone()),
            (&self.fields.instructions, detail.agent.instructions.clone()),
            (&self.fields.harness, detail.agent.harness.to_string()),
            (&self.fields.model, detail.agent.model.clone()),
            (&self.fields.effort, detail.agent.effort.to_string()),
        ] {
            field.update(cx, |f, cx| f.set(&text, false, cx));
        }
        self.editing = true;
        self.error = None;
        cx.notify();
    }
    fn picker(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        harness: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let field = if harness {
            &self.fields.harness
        } else {
            &self.fields.effort
        };
        let current = field.read(cx).text.clone();
        let selected_harness = self.fields.harness.read(cx).text.clone();
        let list = self
            .selected
            .as_ref()
            .and_then(|(node, _)| self.lists.get(node.as_str()));
        let mut values: Vec<(String, String)> = match list {
            Some(list) if harness => list
                .harnesses
                .iter()
                .map(|h| (h.id.to_string(), String::new()))
                .collect(),
            Some(list) => list
                .harnesses
                .iter()
                .find(|h| h.id.as_str() == selected_harness)
                .map(|h| {
                    h.efforts
                        .iter()
                        .map(|e| {
                            (
                                e.to_string(),
                                list.effort_policy
                                    .iter()
                                    .find(|p| p.effort == *e)
                                    .map(|p| p.usage.clone())
                                    .unwrap_or_default(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        };
        if !current.is_empty() && !values.iter().any(|(id, _)| id == &current) {
            values.push((current.clone(), String::new()));
        }
        let mut chips = div().flex().flex_wrap().gap(px(size::RADIUS));
        for (id, hint) in values {
            let chosen = id == current;
            let selector = format!("agents-{}-{id}", if harness { "harness" } else { "effort" });
            let field = field.clone();
            chips = chips.child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .px(px(size::ROW_PAD))
                    .py(px(size::RADIUS))
                    .rounded(px(size::RADIUS))
                    .border_1()
                    .border_color(if chosen { t.accent } else { t.divider })
                    .when(chosen, |d| d.bg(t.raised))
                    .cursor_pointer()
                    .child(id.clone())
                    .when(!hint.is_empty(), |d| d.tooltip(kit::tip(hint)))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        field.update(cx, |f, cx| f.set(&id, false, cx));
                        cx.notify();
                    })),
            );
        }
        if list.is_none() {
            chips = chips.child(div().text_color(t.dim).child("Reading available settings…"));
        }
        chips
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let (Some((node, id)), Some(detail)) = (self.selected.clone(), self.detail.as_ref()) else {
            return;
        };
        let harness = match self.fields.harness.read(cx).text.trim().parse() {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let effort = match self.fields.effort.read(cx).text.trim().parse() {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let input = EditAgent {
            agent: id.clone(),
            etag: Some(detail.etag.clone()),
            name: Some(self.fields.name.read(cx).text.clone()),
            instructions: Some(self.fields.instructions.read(cx).text.clone()),
            harness: Some(harness),
            model: Some(self.fields.model.read(cx).text.clone()),
            effort: Some(effort),
            notes: None,
        };
        self.saving = true;
        self.error = None;
        let task = self.host.call(node.clone(), input, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |page, cx| {
                page.saving = false;
                match result {
                    Ok(_) => {
                        page.editing = false;
                        page.read_list(node.clone(), cx);
                        page.select(node, id, cx);
                    }
                    Err(error) => {
                        page.error = Some(format!(
                            "Could not save: {}. Your edits are still here.",
                            error.message
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for AgentsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let nodes = self.host.nodes(cx);
        let mut rows = div().flex().flex_col();
        for (node, list) in &self.lists {
            if self.lists.len() > 1 {
                let label = nodes
                    .iter()
                    .find(|n| n.id.as_str() == node)
                    .map(|n| format!("{} · {}", n.device, n.environment))
                    .unwrap_or_else(|| node.clone());
                rows = rows.child(pane::section(&t, label).px(px(size::ROW_PAD)));
            }
            for agent in &list.agents {
                let (node, id) = (NodeId(node.clone()), agent.id.clone());
                let selector = format!("agents-agent-{id}");
                let chosen = self.selected.as_ref() == Some(&(node.clone(), id.clone()));
                rows = rows.child(
                    pane::row(&t, SharedString::from(format!("{node}-{selector}")), chosen)
                        .debug_selector(move || selector.clone())
                        .child(pane::cell(agent.name.clone()))
                        .on_click(cx.listener(move |page, _, _, cx| {
                            page.select(node.clone(), id.clone(), cx)
                        })),
                );
            }
            for unreadable in &list.unreadable {
                rows = rows.child(
                    div()
                        .px(px(size::ROW_PAD))
                        .text_color(t.error)
                        .child(format!("{}: {}", unreadable.name, unreadable.reason)),
                );
            }
        }
        let head = pane::list_head(&t, None).child(
            pane::act(&t, "agents-refresh", "↻")
                .tooltip(kit::tip("Refresh agents"))
                .on_click(cx.listener(|p, _, _, cx| {
                    p.read_all(cx);
                    if let Some((node, id)) = p.selected.clone() {
                        p.select(node, id, cx);
                    }
                })),
        );
        let list = pane::list(&t, "agents-list", head, rows);
        let mut body = div().flex().flex_col().gap(px(size::HEAD_GAP));
        if let Some(error) = &self.error {
            body = body.child(div().text_color(t.error).child(error.clone()));
        }
        let mut head = pane::head(&t);
        if let Some(detail) = &self.detail {
            head = head.child(pane::title(&t, detail.agent.name.clone()));
            if self.editing {
                head = head
                    .child(
                        kit::button(
                            &t,
                            "agents-save",
                            if self.saving { "Saving…" } else { "Save" },
                            Kind::Quiet,
                        )
                        .debug_selector(|| "agents-save".into())
                        .on_click(cx.listener(|p, _, _, cx| p.save(cx))),
                    )
                    .child(
                        kit::button(&t, "agents-cancel", "Cancel", Kind::Quiet).on_click(
                            cx.listener(|p, _, _, cx| {
                                if !p.saving {
                                    p.editing = false;
                                    p.error = None;
                                    cx.notify();
                                }
                            }),
                        ),
                    );
                body = body
                    .child(pane::line(
                        &t,
                        "Name",
                        div()
                            .w_full()
                            .debug_selector(|| "agents-name".into())
                            .child(self.fields.name.clone()),
                    ))
                    .child(pane::line(&t, "Harness", self.picker(&t, true, cx)))
                    .child(pane::line(
                        &t,
                        "Model",
                        div()
                            .w_full()
                            .debug_selector(|| "agents-model".into())
                            .child(self.fields.model.clone()),
                    ))
                    .child(pane::line(&t, "Effort", self.picker(&t, false, cx)))
                    .child(pane::line(
                        &t,
                        "Instructions",
                        div()
                            .w_full()
                            .debug_selector(|| "agents-instructions".into())
                            .child(self.fields.instructions.clone()),
                    ));
            } else {
                head = head.child(
                    kit::button(&t, "agents-edit", "Edit", Kind::Quiet)
                        .debug_selector(|| "agents-edit".into())
                        .on_click(cx.listener(|p, _, _, cx| p.edit(cx))),
                );
                body = body
                    .child(pane::line(
                        &t,
                        "Name",
                        div()
                            .debug_selector(|| "agents-name".into())
                            .child(detail.agent.name.clone()),
                    ))
                    .child(pane::line(&t, "Harness", detail.agent.harness.to_string()))
                    .child(pane::line(
                        &t,
                        "Model",
                        if detail.agent.model.is_empty() {
                            "Harness default".to_string()
                        } else {
                            detail.agent.model.clone()
                        },
                    ))
                    .child(pane::line(&t, "Effort", detail.agent.effort.to_string()))
                    .child(pane::line(
                        &t,
                        "Instructions",
                        div()
                            .debug_selector(|| "agents-instructions".into())
                            .child(detail.agent.instructions.clone()),
                    ));
            }
            body = body.child(pane::section(&t, "Used in"));
            if detail.usage.usage.is_empty() {
                body = body.child(div().child("No missions use this agent."));
            }
            for usage in &detail.usage.usage {
                let (node, mission, step) = (
                    self.selected.as_ref().map(|s| s.0.clone()),
                    usage.mission_slug.clone(),
                    usage.step_id.clone(),
                );
                let label = format!(
                    "{} · {} · {}",
                    usage.mission_title, usage.step_title, usage.seat_label
                );
                let selector = format!("agents-usage-{mission}");
                body = body.child(
                    kit::button(
                        &t,
                        SharedString::from(format!(
                            "agents-use-{mission}-{step}-{}",
                            usage.seat_id
                        )),
                        label,
                        Kind::Quiet,
                    )
                    .self_start()
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |p, _, _, cx| {
                        let _ = p.host.ask(
                            Ask::Reveal {
                                what: "mission".into(),
                                id: mission.clone(),
                                node: node.clone(),
                                part: Some(step.clone()),
                                beside: false,
                            },
                            cx,
                        );
                    })),
                );
            }
            if !detail.usage.complete {
                body = body.child(
                    div()
                        .text_color(t.dim)
                        .child("Some missions could not be read; usage may be incomplete."),
                );
            }
            body = body.child(pane::section(&t, "Notes"));
            let mut notes: Vec<_> = detail.agent.notes.iter().collect();
            notes.sort_by(|a, b| b.at.cmp(&a.at));
            for (i, note) in notes.into_iter().enumerate() {
                let selector = format!("agents-note-{i}");
                let timestamp_selector = format!("agents-note-at-{}", note.at);
                body = body.child(
                    div().debug_selector(move || selector.clone()).child(
                        div()
                            .debug_selector(move || timestamp_selector.clone())
                            .child(format!("{} · {}\n{}", note.at, note.by, note.text)),
                    ),
                );
            }
        } else if self.loading {
            body = body.child(pane::empty(&t, "Reading agent…", None));
        } else if self.error.is_none() {
            body = body.child(pane::empty(&t, "Choose an agent.", None));
        }
        pane::page(&t, list, pane::detail(&t, "agents-detail", head, body))
    }
}
