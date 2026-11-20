//! Mission create and rename dialog, adapted from the original navigator.
use super::*;
use ahjo_app_kit::theme::Theme;
use gpui::{FontWeight, MouseButton, SharedString};

#[derive(Clone)]
enum Doing {
    Create {
        node: String,
    },
    Rename {
        node: String,
        slug: String,
        title: String,
        rev: i64,
        etag: String,
    },
    Archive {
        node: String,
        slug: String,
        title: String,
        rev: i64,
        etag: String,
    },
}
struct Form {
    doing: Doing,
    name: Entity<TextField>,
    /// What a new mission is for, kept as its note.
    intent: Entity<TextField>,
    /// A new mission's rough steps, one per line.
    steps: Entity<TextField>,
    /// The example a new mission starts as a copy of; none starts empty.
    example: Option<&'static ahjo_missions_contract::Example>,
    problem: Option<String>,
    busy: bool,
}
#[derive(Default)]
pub(super) struct Manage {
    form: Option<Form>,
}
impl MissionsPage {
    pub(super) fn manage_unsaved(&self, cx: &gpui::App) -> Option<String> {
        let form = self.manage.form.as_ref()?;
        if form.busy {
            return Some("Saving mission".into());
        }
        let baseline = match &form.doing {
            Doing::Create { .. } => "",
            Doing::Rename { title, .. } | Doing::Archive { title, .. } => title.as_str(),
        };
        if form.name.read(cx).text != baseline {
            return Some("Mission name".into());
        }
        (matches!(form.doing, Doing::Create { .. })
            && !(form.intent.read(cx).text.trim().is_empty()
                && form.steps.read(cx).text.trim().is_empty()))
        .then(|| "New mission".into())
    }
    pub(super) fn open_mission_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unsaved(cx).is_some() {
            self.state.error = Some("Save or discard the current draft first.".into());
            cx.notify();
            return;
        }
        let Some(node) = self.host.nodes(cx).first().map(|node| node.id.to_string()) else {
            self.state.error = Some("No node is connected.".into());
            cx.notify();
            return;
        };
        self.open_manage(Doing::Create { node }, "", window, cx);
    }
    pub(super) fn open_mission_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unsaved(cx).is_some() {
            self.state.error = Some("Save or discard the current draft first.".into());
            cx.notify();
            return;
        }
        let (Some((node, slug)), Some(mission), Some(etag)) = (
            self.state.selected.clone(),
            self.state.mission.as_ref(),
            self.state.mission_etag.clone(),
        ) else {
            self.state.error = Some("The mission is still loading.".into());
            cx.notify();
            return;
        };
        let title = mission.title.clone();
        self.open_manage(
            Doing::Rename {
                node,
                slug,
                title: title.clone(),
                rev: mission.rev,
                etag,
            },
            &title,
            window,
            cx,
        );
    }
    pub(super) fn open_mission_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unsaved(cx).is_some() {
            self.state.error = Some("Save or discard the current draft first.".into());
            cx.notify();
            return;
        }
        let (Some((node, slug)), Some(mission), Some(etag)) = (
            self.state.selected.clone(),
            self.state.mission.as_ref(),
            self.state.mission_etag.clone(),
        ) else {
            self.state.error = Some("The mission is still loading.".into());
            cx.notify();
            return;
        };
        let title = mission.title.clone();
        self.open_manage(
            Doing::Archive {
                node,
                slug,
                title: title.clone(),
                rev: mission.rev,
                etag,
            },
            &title,
            window,
            cx,
        );
    }
    fn open_manage(
        &mut self,
        doing: Doing,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            let mut field = TextField::new(cx, "What it does, such as Write the changelog");
            field.set(title, true, cx);
            field
        });
        let intent = cx.new(|cx| {
            TextField::new(
                cx,
                "As rough as you like: what it makes, for whom, what good looks like",
            )
            .prose()
            .min_lines(4)
        });
        let steps = cx.new(|cx| {
            TextField::new(
                cx,
                "One per line, such as\nGather merged PRs\nDraft\nReview",
            )
            .prose()
            .min_lines(8)
        });
        cx.observe(&name, |_, _, cx| cx.notify()).detach();
        for field in [&name, &intent, &steps] {
            cx.subscribe_in(
                field,
                window,
                |page, field, event: &FieldEvent, window, cx| {
                    let Some(form) = page.manage.form.as_ref() else {
                        return;
                    };
                    // Enter in the name, and Ctrl+Enter anywhere, creates it;
                    // Tab goes to the next field shown.
                    let mut order = vec![form.name.clone()];
                    if matches!(form.doing, Doing::Create { .. }) {
                        order.push(form.intent.clone());
                        if form.example.is_none() {
                            order.push(form.steps.clone());
                        }
                    }
                    let at = order.iter().position(|f| f == field);
                    match event {
                        FieldEvent::SubmitAlt => page.submit_manage(cx),
                        FieldEvent::Submit if at == Some(0) => page.submit_manage(cx),
                        FieldEvent::Tab => {
                            let next = &order[at.map_or(0, |i| (i + 1) % order.len())];
                            ahjo_app_kit::widgets::field::focus(next, window, cx);
                        }
                        _ => {}
                    }
                },
            )
            .detach();
        }
        if !matches!(doing, Doing::Archive { .. }) {
            ahjo_app_kit::widgets::field::focus(&name, window, cx);
        }
        self.manage.form = Some(Form {
            doing,
            name,
            intent,
            steps,
            example: None,
            problem: None,
            busy: false,
        });
        self.state.error = None;
        cx.notify();
    }
    pub(super) fn manage_is_open(&self) -> bool {
        self.manage.form.is_some()
    }
    pub(super) fn close_manage(&mut self, cx: &mut Context<Self>) {
        if self.manage_unsaved(cx).is_some() {
            if let Some(form) = self.manage.form.as_mut() {
                form.problem = Some("Save or cancel the mission name draft first.".into());
            }
            cx.notify();
            return;
        }
        self.cancel_manage(cx);
    }
    fn cancel_manage(&mut self, cx: &mut Context<Self>) {
        if self.manage.form.as_ref().is_some_and(|form| form.busy) {
            return;
        }
        self.manage.form = None;
        self.resolve_wanted(cx);
        cx.notify();
    }
    fn submit_manage(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.manage.form.as_ref() else {
            return;
        };
        if form.busy {
            return;
        }
        let title = form.name.read(cx).text.trim().to_owned();
        let doing = form.doing.clone();
        let example = form.example;
        let intent = form.intent.read(cx).text.clone();
        let steps = if example.is_none() {
            form.steps.read(cx).text.clone()
        } else {
            String::new()
        };
        // An example names the mission after itself until it is renamed.
        let title = match example {
            Some(e) if title.is_empty() => e.title.to_owned(),
            _ => title,
        };
        if matches!(doing, Doing::Create { .. }) && title.is_empty() {
            self.manage.form.as_mut().unwrap().problem = Some("A mission needs a name.".into());
            cx.notify();
            return;
        }
        if let Doing::Rename {
            title: original, ..
        } = &doing
        {
            if title.is_empty() {
                self.manage.form.as_mut().unwrap().problem = Some("A mission needs a name.".into());
                cx.notify();
                return;
            }
            if &title == original {
                self.close_manage(cx);
                return;
            }
        }
        let request = match &doing {
            Doing::Create { node } => Ok(Request::new("POST", NodeId(node.clone()), "/api/missions")
                .json(&json!({"title":title, "example":example.map(|e| e.name).unwrap_or(""), "note":intent, "steps":steps, "updatedBy":"human:operator"}))),
            Doing::Rename { node, slug, rev, etag, .. } => edits::with_json_body(
                Request::new("PATCH", NodeId(node.clone()), mission_path(slug))
                    .header("If-Match", etag.clone()),
                &edits::board_body(&rename_patch(&title), *rev),
            ),
            Doing::Archive { node, slug, rev, etag, .. } => Ok(Request::new("DELETE", NodeId(node.clone()), mission_path(slug))
                .header("If-Match", etag.clone())
                .json(&json!({"expectedRev":rev}))),
        };
        let form = self.manage.form.as_mut().unwrap();
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                form.problem = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        form.busy = true;
        form.problem = None;
        let asked = self.host.request(request, cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation { return; }
                let Some(form) = page.manage.form.as_mut() else { return; };
                form.busy = false;
                match result {
                    Ok(answer) if answer.ok() => match &doing {
                        Doing::Create { node } => match serde_json::from_slice::<MissionResponse>(&answer.body) {
                            Ok(result) => { let slug = result.mission.mission.slug; page.manage.form = None; page.open(node.clone(), slug, cx); },
                            Err(error) => form.problem = Some(format!("The mission may have been created, but the answer could not be read: {error}. Check the list before retrying.")),
                        },
                        Doing::Archive { .. } => {
                            page.manage.form = None;
                            page.state.selected = None;
                            page.state.mission = None;
                            page.state.mission_etag = None;
                            page.state.layout = None;
                            page.state.layout_etag = None;
                            page.notes = None;
                            page.editor = None;
                            page.history = undo::History::default();
                            page.run = run::RunUi::new(cx);
                        }
                        Doing::Rename { title: old_title, .. } => match serde_json::from_slice::<EditResponse>(&answer.body) {
                            Ok(result) => {
                                page.history.wrote(undo::Version { rev:result.mission.mission.rev, layout:Some(result.layout.etag.clone()) });
                                page.history.record(undo::Entry { label:"mission name".into(), steps:vec![undo::Step::board(rename_patch(old_title))] });
                                page.state.mission_etag = result.mission.etag;
                                page.state.mission = Some(result.mission.mission);
                                page.state.layout_etag = Some(result.layout.etag);
                                page.state.layout = Some(result.layout.layout);
                                page.manage.form = None;
                            }
                            Err(error) => form.problem = Some(format!("The rename may have succeeded, but the answer could not be read: {error}. Check the mission before retrying.")),
                        },
                    },
                    Ok(answer) => form.problem = Some(answer.error_text()),
                    Err(error) if error.code == "unanswered" => form.problem = Some(format!("{}. The write may have succeeded; check the mission list before retrying.", error.message)),
                    Err(error) => form.problem = Some(error.message),
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    fn example_choice(
        &self,
        t: &Theme,
        chosen: Option<&'static ahjo_missions_contract::Example>,
        cx: &mut Context<Self>,
    ) -> Div {
        let row = |id: String,
                   title: &'static str,
                   about: &'static str,
                   on: bool,
                   pick: Option<&'static ahjo_missions_contract::Example>| {
            let selector = id.clone();
            div()
                .id(SharedString::from(id))
                .debug_selector(move || selector.clone())
                .px(px(PAD))
                .py(px(size::RADIUS))
                .rounded(px(size::RADIUS))
                .border_1()
                .border_color(if on { t.accent } else { t.divider })
                .cursor_pointer()
                .hover(|d| d.bg(t.raised))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(
                    div()
                        .text_size(px(size::SMALL))
                        .text_color(t.text2)
                        .child(about),
                )
                .on_click(cx.listener(move |page, _, _, cx| {
                    if let Some(form) = page.manage.form.as_mut() {
                        form.example = pick;
                    }
                    cx.notify();
                }))
        };
        let mut list = div().flex().flex_col().gap(px(size::RADIUS)).child(row(
            "mission-example-empty".into(),
            "Empty",
            "Just what you wrote, or nothing yet.",
            chosen.is_none(),
            None,
        ));
        for e in ahjo_missions_contract::EXAMPLES {
            list = list.child(row(
                format!("mission-example-{}", e.name),
                e.title,
                e.about,
                chosen.is_some_and(|c| c.name == e.name),
                Some(e),
            ));
        }
        list
    }
    pub(super) fn mission_form(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let form = self.manage.form.as_ref()?;
        let title = match &form.doing {
            Doing::Create { .. } => "New mission",
            Doing::Rename { .. } => "Rename mission",
            Doing::Archive { .. } => "Delete mission",
        };
        let action = match &form.doing {
            Doing::Create { .. } => "Create",
            Doing::Rename { .. } => "Rename",
            Doing::Archive { .. } => "Delete",
        };
        let mut go = kit::button(
            t,
            "mission-form-submit",
            if form.busy { "Saving…" } else { action },
            Kind::Primary,
        )
        .debug_selector(|| "mission-form-submit".into());
        if !form.busy {
            go = go.on_click(cx.listener(|page, _, _, cx| page.submit_manage(cx)));
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .bg(ahjo_app_kit::theme::alpha(t.bg, SCRIM))
                .flex()
                .justify_center()
                .items_start()
                .pt(px(size::TAB_BAR))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|page, _, _, cx| page.close_manage(cx)),
                )
                .child(
                    kit::panel(t)
                        .id("mission-form")
                        .debug_selector(|| "mission-form".into())
                        // Room to draft a mission in; a rename or a delete
                        // needs only a line.
                        .w(px(if matches!(form.doing, Doing::Create { .. }) {
                            size::MEASURE
                        } else {
                            size::NOTICE
                        }))
                        .p(px(PAD))
                        .flex()
                        .flex_col()
                        .gap(px(PAD))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                        .when(matches!(form.doing, Doing::Archive { .. }), |d| {
                            let Doing::Archive { title, .. } = &form.doing else {
                                return d;
                            };
                            d.child(
                                div()
                                    .debug_selector(|| "mission-archive-explanation".into())
                                    .child(format!(
                                        "“{title}” is deleted for good, with its layout and notes. Its runs stay."
                                    )),
                            )
                        })
                        .when(matches!(form.doing, Doing::Create { .. }), |d| {
                            d.child(pane::meta(t, "Ctrl+Enter creates it from any field.", Some(t.text2)))
                        })
                        .when(!matches!(form.doing, Doing::Archive { .. }), |d| {
                            d.child(kit::label(t, "Name")).child(
                                div()
                                    .debug_selector(|| "mission-form-name".into())
                                    .child(form.name.clone()),
                            )
                        })
                        // Start empty, or from an example that is staffed and
                        // wired already.
                        // What it is for and its rough steps: a sketch an
                        // agent turns into a mission that runs.
                        .when(matches!(form.doing, Doing::Create { .. }), |d| {
                            d.child(kit::label(t, "What are you making?")).child(
                                div()
                                    .debug_selector(|| "mission-form-intent".into())
                                    .child(form.intent.clone()),
                            )
                        })
                        .when(matches!(form.doing, Doing::Create { .. }) && form.example.is_none(), |d| {
                            d.child(kit::label(t, "Rough steps")).child(
                                div()
                                    .debug_selector(|| "mission-form-steps".into())
                                    .child(form.steps.clone()),
                            )
                        })
                        .when(matches!(form.doing, Doing::Create { .. }), |d| {
                            d.child(kit::label(t, "Or start from")).child(self.example_choice(t, form.example, cx))
                        })
                        .when_some(form.problem.clone(), |d, problem| {
                            d.child(
                                div()
                                    .debug_selector(|| "mission-form-error".into())
                                    .text_color(t.error)
                                    .child(problem),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(TAB_GAP))
                                .child(
                                    kit::button(
                                        t,
                                        SharedString::from("mission-form-cancel"),
                                        "Cancel",
                                        Kind::Plain,
                                    )
                                    .debug_selector(|| "mission-form-cancel".into())
                                    .on_click(cx.listener(|page, _, _, cx| page.cancel_manage(cx))),
                                )
                                .child(go),
                        ),
                ),
        )
    }
}

fn rename_patch(title: &str) -> MissionChange {
    MissionChange::Title(title.into())
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn rename_and_its_undo_decode() {
        assert_patch_decodes(&edits::board_body(&rename_patch("New mission title"), 7));
        assert_patch_decodes(&rename_patch("Previous title"));
    }
}
