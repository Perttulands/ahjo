//! Creating, renaming and deleting a mission: New opens a popup beside
//! the button it came from, Delete asks beside the button its menu came
//! from, and a rename edits the name in the mission's head.
use super::*;
use ahjo_app_kit::menu::MenuRow;
use ahjo_app_kit::popup::{self, Outside, Popup, PopupId};
use ahjo_app_kit::theme::Theme;
use ahjo_app_kit::theme::theme;
use ahjo_app_kit::widgets::inline_edit::{InlineEdit, InlineEditEvent};
use ahjo_missions_contract::commands::{MissionCreate, MissionDelete, MissionEdited};
use gpui::{AnyElement, Focusable, FontWeight, SharedString};

/// New's form, in its popup.
struct Form {
    /// The node the mission is made on.
    node: String,
    name: Entity<TextField>,
    /// What a new mission is for, kept as its note.
    intent: Entity<TextField>,
    /// A new mission's rough steps, one per line.
    steps: Entity<TextField>,
    /// The example a new mission starts as a copy of; none starts empty.
    example: Option<&'static ahjo_missions_contract::Example>,
    problem: Option<String>,
    busy: bool,
    popup: Option<PopupId>,
}
/// The mission's name, edited in its head.
struct Rename {
    node: String,
    slug: String,
    title: String,
    rev: i64,
    etag: String,
    edit: Entity<InlineEdit>,
    problem: Option<String>,
    busy: bool,
    _edit: gpui::Subscription,
}
#[derive(Default)]
pub(super) struct Manage {
    form: Option<Form>,
    rename: Option<Rename>,
}
impl MissionsPage {
    pub(super) fn manage_unsaved(&self, cx: &gpui::App) -> Option<String> {
        if let Some(rename) = &self.manage.rename {
            if rename.busy {
                return Some("Saving mission".into());
            }
            if rename.edit.read(cx).text(cx) != rename.title {
                return Some("Mission name".into());
            }
        }
        let form = self.manage.form.as_ref()?;
        if form.busy {
            return Some("Saving mission".into());
        }
        if !form.name.read(cx).text.is_empty() {
            return Some("Mission name".into());
        }
        (!(form.intent.read(cx).text.trim().is_empty()
            && form.steps.read(cx).text.trim().is_empty()))
        .then(|| "New mission".into())
    }
    pub(super) fn open_mission_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = Spot::here(window, cx);
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
        self.open_manage(node, at, window, cx);
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
        let (title, rev) = (mission.title.clone(), mission.rev);
        let edit =
            cx.new(|cx| InlineEdit::new("What it does, such as Write the changelog", window, cx));
        // Enter or a click elsewhere renames it; Esc keeps the name.
        let subscription = cx.subscribe(&edit, |page, _, event, cx| match event {
            InlineEditEvent::Committed(name) => page.submit_rename(name.trim(), cx),
            InlineEditEvent::Reverted => page.end_rename(cx),
        });
        edit.update(cx, |edit, cx| edit.start(&title, window, cx));
        self.manage.rename = Some(Rename {
            node,
            slug,
            title,
            rev,
            etag,
            edit,
            problem: None,
            busy: false,
            _edit: subscription,
        });
        self.state.error = None;
        cx.notify();
    }
    /// Esc, or the rename asked of the node done: the head shows the name.
    fn end_rename(&mut self, cx: &mut Context<Self>) {
        if self.manage.rename.take().is_some() {
            self.resolve_wanted(cx);
            cx.notify();
        }
    }
    /// Escape on the page while the rename stands in the head, its field
    /// not holding the keyboard: the name stays as it was.
    pub(super) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if let Some(rename) = self.manage.rename.as_ref().filter(|r| !r.busy) {
            let edit = rename.edit.clone();
            edit.update(cx, |edit, cx| edit.revert(cx));
            self.end_rename(cx);
        }
    }
    /// Renames the mission `title`. Its own name changes nothing; none,
    /// or one the node refuses, keeps the rename open, saying why.
    fn submit_rename(&mut self, title: &str, cx: &mut Context<Self>) {
        let Some(rename) = self.manage.rename.as_mut() else {
            return;
        };
        if title == rename.title {
            self.end_rename(cx);
            return;
        }
        if title.is_empty() {
            rename.problem = Some("A mission needs a name.".into());
            rename.edit.update(cx, |edit, cx| edit.resume(cx));
            cx.notify();
            return;
        }
        rename.busy = true;
        rename.problem = None;
        let previous_title = rename.title.clone();
        let asked = self.host.call(
            NodeId(rename.node.clone()),
            edits::board_edit(
                rename.slug.clone(),
                rename_patch(title),
                rename.rev,
                rename.etag.clone(),
            ),
            cx,
        );
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result: Result<MissionEdited, _> = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation {
                    return;
                }
                let Some(rename) = page.manage.rename.as_mut() else {
                    return;
                };
                rename.busy = false;
                match result {
                    Ok(edit) => {
                        page.history.wrote(undo::Version {
                            rev: edit.definition.mission.rev,
                            layout: Some(edit.layout_etag.clone()),
                        });
                        page.history.record(undo::Entry {
                            label: "mission name".into(),
                            steps: vec![undo::Step::board(rename_patch(&previous_title))],
                        });
                        page.state.mission_etag = Some(edit.definition.etag);
                        page.state.mission = Some(edit.definition.mission);
                        page.state.set_layout(edit.layout, edit.layout_etag, true);
                        page.end_rename(cx);
                    }
                    Err(error) => {
                        rename.problem = Some(match error.code.as_str() {
                            "unanswered" | "invalid_response" => format!(
                                "{}. The write may have succeeded; check the mission list before retrying.",
                                error.message
                            ),
                            _ => error.message,
                        });
                        rename.edit.update(cx, |edit, cx| edit.resume(cx));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    /// Asks to delete the mission, beside `at`, the button its menu came
    /// from.
    pub(super) fn open_mission_archive(
        &mut self,
        at: Spot,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let (title, rev) = (mission.title.clone(), mission.rev);
        let page = cx.entity().downgrade();
        let row = MenuRow::new("Delete mission", move |_, cx| {
            let (node, slug, etag) = (node.clone(), slug.clone(), etag.clone());
            let _ = page.update(cx, |p, cx| p.delete_mission(node, slug, rev, etag, cx));
        })
        .hint(format!(
            "“{title}” is deleted for good, with its layout and notes. Its runs stay."
        ));
        self.host.confirm(at, row, cx);
    }
    fn delete_mission(
        &mut self,
        node: String,
        slug: String,
        rev: i64,
        etag: String,
        cx: &mut Context<Self>,
    ) {
        let asked = self.host.call(
            NodeId(node),
            MissionDelete {
                mission: slug.clone(),
                rev: Some(rev),
                etag: Some(etag),
            },
            cx,
        );
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation
                    || page.state.selected.as_ref().is_none_or(|(_, s)| *s != slug)
                {
                    return;
                }
                match result {
                    Ok(_) => {
                        page.state.selected = None;
                        page.state.mission = None;
                        page.state.mission_etag = None;
                        page.state.clear_layout();
                        page.notes = None;
                        page.editor = None;
                        page.history = undo::History::default();
                        page.run = run::RunUi::new(cx);
                    }
                    Err(error) => {
                        page.state.error =
                            Some(format!("The mission was not deleted: {}", error.message));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn open_manage(&mut self, node: String, at: Spot, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| TextField::new(cx, "What it does, such as Write the changelog"));
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
                    let mut order = vec![form.name.clone(), form.intent.clone()];
                    if form.example.is_none() {
                        order.push(form.steps.clone());
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
        let view = popup::drawn(&cx.entity(), |p: &mut Self, _, cx| p.mission_form(cx), cx);
        let (asked, closed) = (cx.entity().downgrade(), cx.entity().downgrade());
        let popup = self.host.popup(
            Popup::beside(at, view)
                .focus(name.read(cx).focus_handle(cx))
                .outside(Outside::Keep)
                .may_close(move |_, cx| {
                    asked
                        .update(cx, |p, cx| p.manage_may_close(cx))
                        .unwrap_or(true)
                })
                .on_dismiss(move |_, cx| {
                    let _ = closed.update(cx, |p, cx| p.manage_closed(cx));
                }),
            cx,
        );
        self.manage.form = Some(Form {
            node,
            name,
            intent,
            steps,
            example: None,
            problem: None,
            busy: false,
            popup,
        });
        self.state.error = None;
        cx.notify();
    }
    pub(super) fn manage_is_open(&self) -> bool {
        self.manage.form.is_some() || self.manage.rename.is_some()
    }
    /// The rename in the head is open: Escape lets it go.
    pub(super) fn is_renaming(&self) -> bool {
        self.manage.rename.is_some()
    }
    /// Escape or a press outside New's popup: a draft in it stays and
    /// says so, as does a mission being created.
    fn manage_may_close(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(form) = self.manage.form.as_ref() else {
            return true;
        };
        if form.busy {
            return false;
        }
        if self.manage_unsaved(cx).is_some() {
            if let Some(form) = self.manage.form.as_mut() {
                form.problem = Some("Create it, or Cancel to let the draft go.".into());
            }
            cx.notify();
            return false;
        }
        true
    }
    /// New's popup closed.
    fn manage_closed(&mut self, cx: &mut Context<Self>) {
        if self.manage.form.as_ref().is_some_and(|f| f.popup.is_some()) {
            self.manage.form = None;
            self.resolve_wanted(cx);
            cx.notify();
        }
    }
    /// Cancel: the form goes, its popup with it.
    pub(super) fn cancel_manage(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.manage.form.as_ref() else {
            return;
        };
        if form.busy {
            return;
        }
        match form.popup {
            Some(id) => self.host.close_popup(id, cx),
            None => {
                self.manage.form = None;
                self.resolve_wanted(cx);
                cx.notify();
            }
        }
    }
    /// The form did what it was for: it goes, its popup with it.
    fn end_manage(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.manage.form.take().and_then(|f| f.popup) {
            self.host.close_popup(id, cx);
        }
    }
    fn submit_manage(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.manage.form.as_ref() else {
            return;
        };
        if form.busy {
            return;
        }
        let title = form.name.read(cx).text.trim().to_owned();
        let node = form.node.clone();
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
        let form = self.manage.form.as_mut().unwrap();
        if title.is_empty() {
            form.problem = Some("A mission needs a name.".into());
            cx.notify();
            return;
        }
        form.busy = true;
        form.problem = None;
        let asked = self.host.call(
            NodeId(node.clone()),
            MissionCreate {
                slug: String::new(),
                title: Some(title),
                example: example.map(|e| e.name.to_owned()),
                note: Some(intent),
                steps: Some(steps),
                updated_by: Some("human:operator".into()),
            },
            cx,
        );
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| {
                if page.generation != generation { return; }
                let Some(form) = page.manage.form.as_mut() else { return; };
                form.busy = false;
                match result {
                    Ok(created) => {
                        page.end_manage(cx);
                        page.open(node, created.created, cx);
                    }
                    Err(error) if matches!(error.code.as_str(), "unanswered" | "invalid_response") => {
                        form.problem = Some(format!("{}. The write may have succeeded; check the mission list before retrying.", error.message));
                    }
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
    /// New's form, in its popup.
    fn mission_form(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = &theme(cx).clone();
        let form = self.manage.form.as_ref()?;
        let mut go = kit::button(
            t,
            "mission-form-submit",
            if form.busy { "Saving…" } else { "Create" },
            Kind::Primary,
        )
        .debug_selector(|| "mission-form-submit".into());
        if !form.busy {
            go = go.on_click(cx.listener(|page, _, _, cx| page.submit_manage(cx)));
        }
        let fields = div()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(PAD))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("New mission"))
            .child(pane::meta(
                t,
                "Ctrl+Enter creates it from any field.",
                Some(t.text2),
            ))
            .child(kit::label(t, "Name"))
            .child(
                div()
                    .debug_selector(|| "mission-form-name".into())
                    .child(form.name.clone()),
            )
            // Start empty, or from an example that is staffed and
            // wired already.
            // What it is for and its rough steps: a sketch an
            // agent turns into a mission that runs.
            .child(kit::label(t, "What are you making?"))
            .child(
                div()
                    .debug_selector(|| "mission-form-intent".into())
                    .child(form.intent.clone()),
            )
            .when(form.example.is_none(), |d| {
                d.child(kit::label(t, "Rough steps")).child(
                    div()
                        .debug_selector(|| "mission-form-steps".into())
                        .child(form.steps.clone()),
                )
            })
            .child(kit::label(t, "Or start from"))
            .child(self.example_choice(t, form.example, cx))
            .when_some(form.problem.clone(), |d, problem| {
                d.child(
                    div()
                        .debug_selector(|| "mission-form-error".into())
                        .text_color(t.error)
                        .child(problem),
                )
            });
        let actions = div()
            .debug_selector(|| "mission-form-actions".into())
            .p(px(PAD))
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
            .child(go);
        Some(
            popup::form("mission-form-fields", fields, actions)
                .debug_selector(|| "mission-form".into())
                // Room to draft a mission in.
                .w(px(size::MEASURE))
                .into_any_element(),
        )
    }

    /// The mission's name in its head, or the field it is renamed in there.
    pub(super) fn mission_title(&self, t: &Theme, title: String) -> Div {
        let Some(rename) = self.manage.rename.as_ref() else {
            return pane::title(t, title);
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(TAB_GAP))
            .child(
                div()
                    .debug_selector(|| "mission-form-name".into())
                    .w(px(size::NOTICE))
                    .flex_shrink_1()
                    .min_w_0()
                    .child(rename.edit.clone()),
            )
            .when_some(rename.problem.clone(), |d, problem| {
                d.child(
                    div()
                        .debug_selector(|| "mission-form-error".into())
                        .min_w_0()
                        .truncate()
                        .text_color(t.error)
                        .child(problem),
                )
            })
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
        edits::assert_board_edit_decodes(rename_patch("New mission title"));
        assert_patch_decodes(&rename_patch("Previous title"));
    }
}
