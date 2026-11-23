//! A quick report over whichever page the operator is using. What the
//! window showed as it opened goes with it ([`evidence`]): a picture of
//! the window taken before the report drew, the builds, the page, panel
//! and open surfaces, and the focused session's last lines, so whoever
//! takes the issue up needs to ask nothing. The picture is dropped on the
//! project's node, where an agent reads it by the path the issue names.
use crate::{Issue, tokens::*};
use ahjo_app_kit::Host;
use ahjo_app_kit::ask::Ask;
use ahjo_app_kit::capture::Picture;
use ahjo_app_kit::frame::{Layer, Status};
use ahjo_app_kit::host::{Request, WindowState};
use ahjo_app_kit::theme::{FAMILY, theme};
use ahjo_app_kit::widgets::field::{FieldEvent, TextField};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_core::NodeId;
use ahjo_issue_contract::commands::{CreateIssue, IssueKind, ListIssues};
use gpui::{
    App, AppContext, Context, Entity, Focusable, Render, Task, Window, div, prelude::*, px,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const LAYER: &str = "report";
/// How many of the focused session's newest lines a report carries.
const SESSION_LINES: usize = 40;
/// How long the picture may take to be written on the node.
const DROP_WITHIN: Duration = Duration::from_secs(60);

/// What a report attaches, gathered silently as it opened.
#[derive(Clone)]
struct Evidence {
    state: WindowState,
    /// The window before the report drew, or why there is none.
    picture: Result<Arc<Picture>, String>,
    focused: Option<ahjo_terminals_contract::FocusedSession>,
    /// The focused session's last lines; `None` while they are read.
    screen: Option<Result<String, String>>,
}

#[derive(Clone, PartialEq, Eq)]
struct Project {
    node: NodeId,
    name: String,
}

pub(crate) struct Report {
    host: Host,
    field: Entity<TextField>,
    sessions: Option<Entity<ahjo_terminals_contract::SessionList>>,
    projects: Vec<Project>,
    selected: Option<Project>,
    preferred: Option<Project>,
    pending_projects: usize,
    /// What goes with the report; `None` once the operator removed it.
    evidence: Option<Evidence>,
    error: Option<String>,
    filing: bool,
}

impl Report {
    fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new(cx, "Title, then description").prose());
        cx.subscribe_in(
            &field,
            window,
            |report: &mut Self, _, event: &FieldEvent, _, cx| {
                if *event == FieldEvent::SubmitAlt {
                    report.file(cx);
                }
            },
        )
        .detach();
        let sessions = host.model::<ahjo_terminals_contract::SessionList>(cx);
        Self {
            host,
            field,
            sessions,
            projects: Vec::new(),
            selected: None,
            preferred: None,
            pending_projects: 0,
            evidence: None,
            error: None,
            filing: false,
        }
    }

    fn open(
        &mut self,
        picture: Result<Picture, String>,
        state: WindowState,
        cx: &mut Context<Self>,
    ) {
        let focused = self
            .sessions
            .as_ref()
            .and_then(|s| s.read(cx).focused.clone());
        self.evidence = Some(Evidence {
            state,
            picture: picture.map(Arc::new),
            focused: focused.clone(),
            screen: None,
        });
        if let Some(focused) = focused {
            self.read_screen(focused, cx);
        }
        self.error = None;
        self.projects.clear();
        let remembered: Value = self.host.setting(cx).unwrap_or(Value::Null);
        self.preferred = remembered.get("report_project").and_then(|p| {
            Some(Project {
                node: NodeId(p["node"].as_str()?.into()),
                name: p["name"].as_str()?.into(),
            })
        });
        self.selected = None;
        let nodes = self.host.nodes(cx);
        self.pending_projects = nodes.len();
        for node in nodes {
            let task = self.host.call(
                node.id.clone(),
                ListIssues {
                    all: false,
                    project: None,
                },
                cx,
            );
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |report, cx| {
                    let parsed = result.ok();
                    if let Some(list) = parsed {
                        report
                            .projects
                            .extend(list.projects.into_iter().map(|p| Project {
                                node: node.id.clone(),
                                name: p.project,
                            }));
                        if report.selected.is_none()
                            && report
                                .preferred
                                .as_ref()
                                .is_some_and(|p| report.projects.contains(p))
                        {
                            report.selected = report.preferred.clone();
                        }
                    } else {
                        report.error = Some("Could not read projects from the node.".into());
                    }
                    report.pending_projects = report.pending_projects.saturating_sub(1);
                    if report.pending_projects == 0 && report.selected.is_none() {
                        report.selected = report.projects.first().cloned();
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Reads the focused session's last lines into the evidence.
    fn read_screen(
        &mut self,
        focused: ahjo_terminals_contract::FocusedSession,
        cx: &mut Context<Self>,
    ) {
        let task = self.screen(&focused, cx);
        cx.spawn(async move |this, cx| {
            let screen = task.await;
            let _ = this.update(cx, |report, cx| {
                if let Some(evidence) = report
                    .evidence
                    .as_mut()
                    .filter(|e| e.focused.as_ref() == Some(&focused))
                {
                    evidence.screen = Some(screen);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The last lines of `focused`'s screen, or why they were not read.
    fn screen(
        &self,
        focused: &ahjo_terminals_contract::FocusedSession,
        cx: &mut App,
    ) -> Task<Result<String, String>> {
        // The list's id names a session on another Unix user's server too.
        let session = self
            .sessions
            .as_ref()
            .and_then(|s| {
                s.read(cx)
                    .sessions
                    .iter()
                    .find(|s| {
                        s.node == focused.node
                            && s.name == focused.name
                            && s.user.as_deref().unwrap_or("") == focused.user
                    })
                    .map(|s| s.id.clone())
            })
            .unwrap_or_else(|| focused.name.clone());
        let task = self.host.call(
            NodeId(focused.node.clone()),
            ahjo_terminals_contract::commands::SessionRead {
                session,
                pane: None,
                lines: Some(SESSION_LINES),
                all: None,
            },
            cx,
        );
        cx.background_executor()
            .spawn(async move { task.await.map(|r| r.text).map_err(|e| e.message) })
    }

    fn file(&mut self, cx: &mut Context<Self>) {
        if self.filing {
            return;
        }
        let Some(project) = self.selected.clone() else {
            self.error = Some("Choose a project.".into());
            cx.notify();
            return;
        };
        let text = self.field.read(cx).text.clone();
        let (title, body) = text.split_once('\n').unwrap_or((&text, ""));
        let title = title.trim();
        if title.is_empty() {
            self.error = Some("Enter a title.".into());
            cx.notify();
            return;
        }
        let body = body.trim().to_owned();
        let title = title.to_owned();
        let evidence = self.evidence.clone();
        self.filing = true;
        self.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let description = match evidence {
                None => body,
                Some(mut evidence) => {
                    // Filed before the screen was read: it is read now.
                    if let Some(focused) = evidence
                        .focused
                        .clone()
                        .filter(|_| evidence.screen.is_none())
                    {
                        let Ok(task) = this.update(cx, |report, cx| report.screen(&focused, cx))
                        else {
                            return;
                        };
                        evidence.screen = Some(task.await);
                    }
                    let evidence = &evidence;
                    let kept = match &evidence.picture {
                        Ok(picture) => keep_picture(&this, &project.node, picture, cx).await,
                        Err(why) => Err(format!("not taken: {why}")),
                    };
                    let attached = evidence_text(evidence, &kept);
                    if body.is_empty() {
                        attached
                    } else {
                        format!("{body}\n\n{attached}")
                    }
                }
            };
            let Ok(task) = this.update(cx, |report, cx| {
                report.host.call(
                    project.node.clone(),
                    CreateIssue {
                        project: project.name.clone(),
                        title,
                        description,
                        kind: IssueKind::Bug,
                    },
                    cx,
                )
            }) else {
                return;
            };
            let result = task.await;
            let _ = this.update(cx, |report, cx| {
                report.filing = false;
                let reference = result
                    .map(|answer| answer.reference)
                    .map_err(|error| error.message);
                match reference {
                    Ok(reference) => {
                        let id = reference
                            .rsplit('#')
                            .next()
                            .unwrap_or(&reference)
                            .to_owned();
                        let mut settings: Value =
                            report.host.setting(cx).unwrap_or_else(|_| json!({}));
                        if !settings.is_object() {
                            settings = json!({});
                        }
                        settings["report_project"] =
                            json!({"node": project.node.as_str(), "name": project.name});
                        let _ = report.host.set_setting(&settings, cx);
                        report
                            .field
                            .update(cx, |field, cx| field.set("", false, cx));
                        report.host.close_layer(LAYER, cx);
                        let host = report.host.clone();
                        report.host.report(
                            Status::new(format!("Filed {id}")).action("Show", move |_, cx| {
                                let _ = host.ask(
                                    Ask::Reveal {
                                        what: "item".into(),
                                        id: reference.clone(),
                                        node: Some(project.node.clone()),
                                        part: None,
                                        beside: false,
                                    },
                                    cx,
                                );
                            }),
                            cx,
                        );
                    }
                    Err(error) => report.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Writes the picture as a PNG on `node`, where an agent on that machine
/// reads it: its path there, or why it is not kept.
async fn keep_picture(
    this: &gpui::WeakEntity<Report>,
    node: &NodeId,
    picture: &Arc<Picture>,
    cx: &mut gpui::AsyncApp,
) -> Result<String, String> {
    let picture = picture.clone();
    let png = cx
        .background_executor()
        .spawn(async move { picture.png() })
        .await
        .map_err(|e| format!("not written: {}", e.message))?;
    let path = format!("{}?name=report.png", ahjo_files_contract::DROPS_PATH);
    let mut request = Request::new("POST", node.clone(), path).within(DROP_WITHIN);
    request.body = Some(("application/octet-stream".into(), png));
    let answer = this
        .update(cx, |report, cx| report.host.request(request, cx))
        .map_err(|_| "not kept: the report closed".to_string())?
        .await;
    match answer {
        Ok(a) if a.ok() => serde_json::from_slice::<ahjo_files_contract::Dropped>(&a.body)
            .map(|d| d.path.display().to_string())
            .map_err(|e| format!("not kept: the node's answer did not read: {e}")),
        Ok(a) => Err(format!("not kept on the node: {}", a.error_text())),
        Err(e) => Err(format!("not kept on the node: {}", e.message)),
    }
}

/// The report's line of what goes with it.
fn summary(evidence: &Evidence) -> String {
    let state = &evidence.state;
    let mut parts = vec![match evidence.picture {
        Ok(_) => "screenshot".to_string(),
        Err(_) => "no screenshot".to_string(),
    }];
    parts.extend(state.page.map(|page| format!("page {page}")));
    if let Some(panel) = state.panel.as_ref().filter(|_| state.panel_is_open) {
        parts.push(format!("panel {panel}"));
    }
    if let Some(focused) = &evidence.focused {
        parts.push(format!("session {}", focused.name));
    }
    parts.push("build".into());
    parts.push(format!("{}×{}", state.width.round(), state.height.round()));
    format!("Attaches {}", parts.join(" · "))
}

/// What the filed issue says of what went with it: where the picture is
/// kept (`picture`, its path on the project's node or why there is none)
/// and what the window showed.
fn evidence_text(evidence: &Evidence, picture: &Result<String, String>) -> String {
    let state = &evidence.state;
    let or = |text: &str, none: &str| {
        if text.is_empty() {
            none.to_string()
        } else {
            text.to_string()
        }
    };
    let mut lines = vec!["Filed from Ahjo Swarm's report.".to_string()];
    lines.push(match (picture, &evidence.picture) {
        (Ok(path), Ok(p)) => format!(
            "Screenshot: {path} ({}×{} px, the window before the report opened)",
            p.width, p.height
        ),
        (Err(why), _) => format!("Screenshot: {why}"),
        (Ok(path), Err(_)) => format!("Screenshot: {path}"),
    });
    lines.push(format!(
        "App: ahjo-swarm {}",
        or(state.app_build, "(build unknown)")
    ));
    lines.push(format!(
        "Node: {} at {}, build {}",
        or(&state.node_id, "(not connected)"),
        state.node_url,
        or(&state.node_build, "unknown")
    ));
    lines.push(format!(
        "Window: {}×{} at scale {}",
        state.width.round(),
        state.height.round(),
        state.scale
    ));
    lines.push(format!("Page: {}", state.page.unwrap_or("(none)")));
    lines.push(match &state.panel {
        Some(panel) if state.panel_is_open => format!("Panel: {panel} (open)"),
        Some(panel) => format!("Panel: closed (last showed {panel})"),
        None => "Panel: closed".into(),
    });
    lines.push(format!(
        "Side panel: {}",
        if state.side_is_open {
            "open"
        } else {
            "put away"
        }
    ));
    lines.push(if state.surfaces.is_empty() {
        "Popups and layers: none".into()
    } else {
        format!("Popups and layers: {}", state.surfaces.join(", "))
    });
    match &evidence.focused {
        None => lines.push("Focused session: none".into()),
        Some(focused) => {
            let user = if focused.user.is_empty() {
                String::new()
            } else {
                format!(", user {}", focused.user)
            };
            lines.push(format!(
                "Focused session: {} on node {}{user}",
                focused.name, focused.node
            ));
            match &evidence.screen {
                Some(Ok(text)) => {
                    let text = text.trim_end();
                    // A fence longer than any run of backticks in the screen.
                    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                    let fence = "`".repeat(longest.max(2) + 1);
                    lines.push(format!(
                        "Its screen, up to {SESSION_LINES} newest lines:\n{fence}text\n{text}\n{fence}"
                    ));
                }
                Some(Err(why)) => lines.push(format!("Its screen was not read: {why}")),
                None => lines.push("Its screen was still being read.".into()),
            }
        }
    }
    lines.join("\n")
}

impl Render for Report {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let mut projects = div().flex().flex_wrap().gap(px(SPACE_XS));
        for (i, project) in self.projects.iter().cloned().enumerate() {
            let label = if self.selected.as_ref() == Some(&project) {
                format!("✓ {}", project.name)
            } else {
                project.name.clone()
            };
            projects = projects.child(
                kit::button(&t, format!("report-project-{i}"), "", Kind::Quiet)
                    .max_w(px(CARD_W - 2. * SPACE_4XL))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label),
                    )
                    .debug_selector(move || format!("report-project-{i}"))
                    .on_click(cx.listener(move |r, _, _, cx| {
                        r.selected = Some(project.clone());
                        cx.notify();
                    })),
            );
        }
        div()
            .size_full()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(SPACE_4XL))
            .child(
                div()
                    .id("report-box")
                    .debug_selector(|| "report-box".into())
                    .w(px(CARD_W))
                    .p(px(SPACE_4XL))
                    .bg(t.surface)
                    .border_1()
                    .border_color(t.divider)
                    .font_family(FAMILY)
                    .flex()
                    .flex_col()
                    .gap(px(SPACE_XL))
                    .child(kit::label(&t, "Report"))
                    .child(
                        div()
                            .debug_selector(|| "report-field".into())
                            .child(self.field.clone()),
                    )
                    .child(projects)
                    .when_some(self.evidence.as_ref().map(summary), |d, summary| {
                        d.child(
                            div()
                                .debug_selector(|| "report-evidence".into())
                                .flex()
                                .items_start()
                                .gap(px(SPACE_XS))
                                .child(kit::note(&t, summary, None).flex_1().min_w_0())
                                .child(
                                    kit::button(
                                        &t,
                                        "report-evidence-remove",
                                        "Remove",
                                        Kind::Quiet,
                                    )
                                    .debug_selector(|| "report-evidence-remove".into())
                                    .on_click(cx.listener(
                                        |report, _, _, cx| {
                                            report.evidence = None;
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                    })
                    .when_some(self.error.clone(), |d, error| {
                        d.child(kit::note(&t, error, Some(t.error)))
                    })
                    .child(
                        kit::button(
                            &t,
                            "report-file",
                            if self.filing {
                                "Filing…"
                            } else {
                                "File report"
                            },
                            Kind::Primary,
                        )
                        .on_click(cx.listener(|report, _, _, cx| report.file(cx))),
                    ),
            )
    }
}

impl Issue {
    pub(crate) fn open_report(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.host.is_open(LAYER, cx) {
            return;
        }
        let report = self
            .report
            .get_or_insert_with(|| cx.new(|cx| Report::new(self.host.clone(), window, cx)))
            .clone();
        // Before the report draws: the window as he saw it.
        let picture = self.host.capture(window, cx).map_err(|e| e.message);
        let state = self.host.window_state(window, cx);
        report.update(cx, |report, cx| report.open(picture, state, cx));
        let focus = report.read(cx).field.read(cx).focus_handle(cx);
        let opener = window.focused(cx);
        self.host.layer(
            Layer::new(LAYER, report)
                .focus(focus.clone())
                .on_dismiss(move |window, cx| {
                    if let Some(opener) = &opener {
                        window.focus(opener, cx);
                    }
                }),
            cx,
        );
        window.focus(&focus, cx);
    }
}
