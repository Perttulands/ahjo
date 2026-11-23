//! Send to Session, the one hand-off (CHROTE's `SendDrawer`): in the
//! window's panel, beside whatever page asked for it, which makes room for
//! it, so it never covers a session. Opened from a session (its window's Send, Peek's, its menu's,
//! Alt+S in its terminal) it is aimed at that session, listed with the
//! others; the line naming it goes only to another session picked instead.
//! Another page opens it through [`Terminals::handoff`] with a reference to
//! what it was showing; the reference is shown, not edited, and the note
//! under it is the operator's. It sends to one session or several, found
//! and marked in the shared finder ([`ahjo_app_kit::widgets::finder`]):
//! Enter takes the lit one, Tab marks another. Enter in the note sends
//! (paste and submit), Shift+Enter breaks the line, Ctrl+Enter or Paste
//! only pastes. Files dropped on the drawer (from the computer, or from
//! Files) or attached land on each target's node and their paths follow
//! the note; a file already on that node goes as its own path. "New
//! agent…" launches one and pastes the message into it without
//! submitting, once the node finds it ready for a paste.
//!
//! It never traps the keyboard: a click on a terminal gives that terminal
//! every key, Escape included, while Send stays open with its draft, and a
//! click back in it goes on with it.
//!
//! A send that arrived everywhere closes the drawer; where one failed the
//! drawer stays, keeps only the sessions it did not reach and says why for
//! each. Where the reason is a rule rather than a failure (a shell would
//! run the line naming a session; an outcome unknown might have arrived),
//! the operator can send anyway, by the button beside the line or Alt+Enter
//! from anywhere in the drawer. The rules are `store::send`'s.

use super::launcher::SEND;
use super::page::Terminals;
use super::surface;
use crate::http;
use crate::store::send;
use crate::store::sessions::{self, Key};
use crate::theme::{alpha, size, theme};
use ahjo_app_kit::drag::NodeFile;
use ahjo_app_kit::widgets::finder::{self, Choice, FinderEvent, Lead};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::pane;
use ahjo_core::NodeId;
use ahjo_terminals_contract::Outcome;
use gpui::{
    AnyElement, AsyncApp, ClickEvent, Context, ExternalPaths, FontWeight, KeyDownEvent,
    MouseButton, SharedString, WeakEntity, Window, div, prelude::*, px,
};
use std::path::PathBuf;

/// Where attachments land: a node, and the user there.
type Landing = (String, String);

/// The finder's row for a new agent, and its chips.
const NEW_AGENT: &str = "new-agent";
const AGENTS: &str = "agents";
const SHELLS: &str = "shells";
const WAITING: &str = "waiting";

/// The drawer's state besides its fields: what it hands over and how the
/// last send went.
#[derive(Default)]
pub struct Draft {
    /// The one line naming the object handed over.
    pub reference: Option<String>,
    /// The session Send was opened from: it is aimed at it, and the line
    /// naming it goes only to other sessions picked instead.
    pub source: Option<Key>,
    /// The reference is the line naming a session (`send::session_reference`).
    pub names_a_session: bool,
    /// Files from this computer, written on the target's node on send.
    pub files: Vec<Attachment>,
    /// Why the last send did not simply arrive, shown above the actions.
    pub said: Option<String>,
    /// Where the last send did not arrive, by session, and why.
    pub missed: Vec<(Key, String)>,
    /// Where the last send of several arrived, by name.
    pub reached: Vec<String>,
    /// A send's outcome is unknown: no sending again from here until the
    /// operator says to.
    pub stuck: bool,
    /// The operator chose to send the line naming a session to a shell
    /// too.
    pub anyway: bool,
    /// The picker's "New agent…" row is chosen.
    pub new_agent: bool,
    /// Explicitly chosen pane; absent means the target tile's pane.
    pub pane: Option<u32>,
    /// The drawer's launch, by node and request, awaiting its session.
    pub launch: Option<(String, u32)>,
    /// The drawer's launched session (node, id), awaiting its first pane.
    pub waiting: Option<(String, String)>,
    /// The sends in flight, by request, each with its session, whether it
    /// was a launch's paste, and whether it presses Enter.
    pub sending: Vec<(u32, Key, bool, bool)>,
    /// Files being written on the target's node.
    pub uploading: bool,
    /// The focused window's grid as the drawer opened, for its launch.
    pub launch_grid: Option<super::page::Grid>,
}

/// A local file, a clipboard image kept in memory until sent, or a file
/// on a node (dragged from Files), by its path there.
#[derive(Clone, Debug, PartialEq)]
pub enum Attachment {
    File(PathBuf),
    Image { name: String, bytes: Vec<u8> },
    OnNode(NodeFile),
}

impl Terminals {
    /// Opens Send from session `session` (a window's Send, Peek's, its
    /// menu's, Alt+S in its terminal), aimed at it. The line naming it
    /// goes along only to another session picked instead.
    pub fn open_send(&mut self, session: Key, window: &mut Window, cx: &mut Context<Self>) {
        let reference = Some(self.session_reference(&session));
        self.handoff_from(
            reference,
            Some(session.clone()),
            Some(session),
            true,
            window,
            cx,
        );
    }

    /// The line naming session `key` in a message: its name and where it
    /// runs.
    fn session_reference(&self, key: &Key) -> String {
        let place = self.where_is(&key.node);
        send::session_reference(
            &key.name,
            place
                .as_ref()
                .map(|p| (p.device.as_str(), p.environment.as_str())),
            &key.node,
        )
    }

    /// The sessions Send can go to, recent targets first, as the finder
    /// offers them.
    fn send_choices(&self) -> Vec<Choice> {
        let focused = self.focused_key();
        sessions::targets(&self.fleet.all, &self.settings.recent_targets)
            .into_iter()
            .map(|s| {
                let key = Key::of(s);
                let program = match sessions::running(s) {
                    sessions::Running::Harness(h) => Some(h),
                    sessions::Running::Command(c) => Some(c),
                    sessions::Running::Nothing => None,
                };
                let mut kinds = vec![];
                if sessions::is_agent(s) {
                    kinds.push(AGENTS);
                }
                if sessions::running(s) == sessions::Running::Nothing {
                    kinds.push(SHELLS);
                }
                if sessions::waiting(s).is_some() {
                    kinds.push(WAITING);
                }
                let device = self.where_is(&s.node).map(|p| p.device).unwrap_or_default();
                Choice {
                    id: key.text().into(),
                    label: s.name.clone(),
                    also: vec![
                        program.unwrap_or("").to_string(),
                        sessions::folder(s).to_string(),
                        device,
                    ],
                    detail: if focused.as_ref() == Some(&key) {
                        Some("focused window".into())
                    } else {
                        program.map(str::to_string)
                    },
                    lead: match (sessions::waiting(s), &s.user) {
                        (Some(_), _) => Lead::Waiting,
                        (None, Some(u)) => Lead::User(u.clone()),
                        (None, None) => Lead::None,
                    },
                    kinds,
                }
            })
            .collect()
    }

    /// The session a finder row names.
    fn send_key(&self, id: &str) -> Option<Key> {
        self.fleet.all.iter().map(Key::of).find(|k| k.text() == id)
    }

    /// Opens Send to Session with `reference`, the line naming what the
    /// operator was looking at, aimed at `target`. A fresh opening is a
    /// fresh message: the last note belonged to something else.
    pub fn handoff(
        &mut self,
        reference: Option<String>,
        target: Option<Key>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handoff_from(reference, target, None, false, window, cx);
    }

    fn busy(&self) -> bool {
        !self.send.sending.is_empty() || self.send.uploading || self.send.waiting.is_some()
    }

    fn handoff_from(
        &mut self,
        reference: Option<String>,
        target: Option<Key>,
        source: Option<Key>,
        names_a_session: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() {
            // The last one is still on its way: its draft stays until it lands.
            self.show_in_panel(surface::SEND, window, cx);
            return;
        }
        // Opened again while open, it keeps what the operator wrote and
        // attached.
        let open = self.in_panel(surface::SEND, cx);
        let files = match open {
            true => std::mem::take(&mut self.send.files),
            false => vec![],
        };
        let launch_grid = self.launch_grids.borrow()[self.settings.workspace.focus];
        self.send = Draft {
            reference,
            source,
            names_a_session,
            files,
            launch_grid,
            ..Default::default()
        };
        self.send_targets = target.into_iter().collect();
        self.send_finder.update(cx, |f, cx| f.clear(cx));
        if !open {
            self.send_note.update(cx, |f, cx| f.set("", false, cx));
        }
        self.show_in_panel(surface::SEND, window, cx);
    }

    /// The operator came back to the drawer: the keyboard goes to the
    /// message, or to the sessions while none is picked.
    pub(super) fn send_resume(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.in_panel(surface::SEND, cx) {
            return;
        }
        if self.send_targets.is_empty() && !self.send.new_agent {
            finder::focus(&self.send_finder, window, cx);
        } else {
            ahjo_app_kit::widgets::field::focus(&self.send_note, window, cx);
        }
    }

    /// What the finder heard: a session taken goes alone unless others are
    /// marked, when it joins them; a mark adds or takes away one.
    pub(super) fn send_finder_event(
        &mut self,
        ev: &FinderEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            FinderEvent::Leave => {
                ahjo_app_kit::widgets::field::focus(&self.send_note, window, cx);
                return;
            }
            FinderEvent::Take(id) if id.as_ref() == NEW_AGENT => {
                self.send.new_agent = true;
                let l = self.launchers[SEND].clone();
                l.update(cx, |l, cx| l.focus_first(window, cx));
            }
            FinderEvent::Take(id) => {
                let Some(k) = self.send_key(id) else {
                    return;
                };
                self.send.new_agent = false;
                if self.send_targets.len() > 1 {
                    if !self.send_targets.contains(&k) {
                        self.send_targets.push(k);
                    }
                } else {
                    if self.send_targets.first() != Some(&k) {
                        self.send.pane = None;
                    }
                    self.send_targets = vec![k];
                }
                self.send_finder.update(cx, |f, cx| f.clear(cx));
                ahjo_app_kit::widgets::field::focus(&self.send_note, window, cx);
            }
            FinderEvent::Mark(_) | FinderEvent::MarkAll(_) => {
                self.send.new_agent = false;
                let mut marked: Vec<SharedString> =
                    self.send_targets.iter().map(|k| k.text().into()).collect();
                finder::marks_after(ev, &mut marked);
                self.send_targets = marked.iter().filter_map(|id| self.send_key(id)).collect();
                self.send.pane = None;
            }
        }
        self.send.said = None;
        self.send.missed.clear();
        self.send.stuck = false;
        cx.notify();
    }

    /// Takes `k` off the sessions Send goes to.
    fn send_drop(&mut self, k: &Key, cx: &mut Context<Self>) {
        self.send_targets.retain(|t| t != k);
        self.send.missed.retain(|(t, _)| t != k);
        self.send.pane = None;
        cx.notify();
    }

    /// Send (Enter) or Paste only (Ctrl+Enter) to the chosen targets.
    pub fn send_now(&mut self, enter: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.send.stuck {
            return;
        }
        if self.send.new_agent {
            self.send.said =
                Some("Launch the new agent below; the message is pasted into it.".into());
            cx.notify();
            return;
        }
        if self.draft_text(cx).trim().is_empty() && self.send.files.is_empty() {
            return;
        }
        if self.send_targets.is_empty() {
            finder::focus(&self.send_finder, window, cx);
            return;
        }
        // The session Send came from is not told its own name: with no
        // note or file, there is nothing for it.
        if !self.send_targets.iter().any(|k| self.has_words_for(k, cx)) {
            return;
        }
        self.deliver(self.send_targets.clone(), enter, false, cx);
    }

    /// Target `k` gets something: a note, a file or the reference.
    fn has_words_for(&self, k: &Key, cx: &Context<Self>) -> bool {
        let note = &self.send_note.read(cx).text;
        !self.send.files.is_empty()
            || !send::message(self.reference_to(k), note, &[])
                .trim()
                .is_empty()
    }

    fn draft_text(&self, cx: &Context<Self>) -> String {
        let note = self.send_note.read(cx).text.clone();
        let reference = self.send.reference.as_deref();
        send::message(reference, &note, &[])
    }

    /// The reference as target `k` gets it (`send::reference_for`); the
    /// session Send was opened from is not told its own name.
    fn reference_to(&self, k: &Key) -> Option<&str> {
        if self.send.source.as_ref() == Some(k) {
            return None;
        }
        send::reference_for(
            self.send.reference.as_deref(),
            self.send.names_a_session,
            self.fleet.session(k),
            self.send.anyway,
        )
    }

    /// Writes the attachments on each target's node, once a node, then
    /// pastes the message into each.
    fn deliver(&mut self, targets: Vec<Key>, enter: bool, launched: bool, cx: &mut Context<Self>) {
        self.send.said = None;
        self.send.missed.clear();
        self.send.reached.clear();
        let files = self.send.files.clone();
        if files.is_empty() {
            for k in targets {
                self.paste(k, enter, launched, vec![], cx);
            }
            self.settle(enter, cx);
            return;
        }
        self.send.uploading = true;
        cx.spawn(async move |this, cx| {
            // Each node and user written to once: where, and the paths
            // there or why not.
            let mut written: Vec<(Landing, Result<Vec<String>, String>)> = vec![];
            for k in &targets {
                let at = (k.node.clone(), k.user.clone());
                if written.iter().any(|(w, _)| *w == at) {
                    continue;
                }
                let node = NodeId(k.node.clone());
                let paths = write_drops(&this, &node, &k.user, &files, cx).await;
                written.push((at, paths));
            }
            let _ = this.update(cx, |s, cx| {
                s.send.uploading = false;
                for k in targets {
                    let at = (k.node.clone(), k.user.clone());
                    match written.iter().find(|(w, _)| *w == at).map(|(_, p)| p) {
                        Some(Ok(paths)) => s.paste(k, enter, launched, paths.clone(), cx),
                        Some(Err(why)) => s.send.missed.push((k, why.clone())),
                        None => {}
                    }
                }
                s.settle(enter, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Pastes the message into `k`'s pane, or notes why it could not be.
    fn paste(
        &mut self,
        k: Key,
        enter: bool,
        launched: bool,
        files: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let note = self.send_note.read(cx).text.clone();
        let Some(s) = self.fleet.session(&k) else {
            let why = format!("{} is not running.", k.name);
            self.send.missed.push((k, why));
            return;
        };
        let Some(pane) = self.send_pane(&k) else {
            let why = format!("{} has no pane to paste into.", k.name);
            self.send.missed.push((k, why));
            return;
        };
        if !s.panes.iter().any(|p| p.id == pane) {
            let why = format!("The selected pane of {} is gone; choose another.", k.name);
            self.send.missed.push((k, why));
            return;
        }
        // A launched agent may not report its program yet; it is an agent.
        let reference = match launched {
            true => self.send.reference.as_deref(),
            false => self.reference_to(&k),
        };
        let text = send::message(reference, &note, &files);
        if text.trim().is_empty() {
            // Nothing for it (`has_words_for`): only the others are sent to.
            return;
        }
        // A launched agent may still be starting: the node waits until it
        // is ready for the paste (#242).
        match self.fleet.paste_to(&k.node, pane, &text, enter, launched) {
            Ok(id) => {
                self.answered(&k, cx);
                self.send.sending.push((id, k, launched, enter));
            }
            Err(why) => self.send.missed.push((k, why)),
        }
        cx.notify();
    }

    /// Nothing is in flight any more: everywhere reached closes the
    /// drawer; otherwise it stays with the sessions not reached and why.
    fn settle(&mut self, enter: bool, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let reached = std::mem::take(&mut self.send.reached);
        for name in &reached {
            if let Some(k) = self.send_targets.iter().find(|k| &k.name == name).cloned() {
                sessions::remember(&mut self.settings.recent_targets, &k.text(), 5);
            }
        }
        if self.send.missed.is_empty() {
            if reached.is_empty() {
                return;
            }
            self.send.files.clear();
            self.send_note.update(cx, |f, cx| f.set("", false, cx));
            self.host.close_panel(surface::SEND, cx);
            self.report(&send::arrived_all(&reached, enter), false, None);
            self.save(cx);
        } else {
            // Only those not reached stay to be sent again.
            let missed: Vec<Key> = self.send.missed.iter().map(|(k, _)| k.clone()).collect();
            self.send_targets.retain(|k| missed.contains(k));
            if !reached.is_empty() {
                self.send.said = Some(format!("Arrived in {}.", send::names(&reached)));
            }
            let why: Vec<String> = self.send.missed.iter().map(|(_, w)| w.clone()).collect();
            self.report(&why.join(" "), true, None);
            self.save(cx);
        }
        cx.notify();
    }

    /// The chosen pane, otherwise the pane shown by the target's tile,
    /// otherwise the target's first pane. A pane is chosen only while
    /// Send goes to one session.
    fn send_pane(&self, key: &Key) -> Option<u32> {
        let session = self.fleet.session(key)?;
        self.send
            .pane
            .filter(|_| self.send_targets.len() == 1)
            .or_else(|| {
                let ws = &self.settings.workspace;
                ws.shown(ws.focus)
                    .filter(|binding| &binding.key == key)
                    .or_else(|| {
                        ws.windows()
                            .iter()
                            .filter_map(|w| w.shown())
                            .find(|binding| &binding.key == key)
                    })
                    .and_then(|binding| binding.pane)
                    .filter(|id| session.panes.iter().any(|pane| pane.id == *id))
                    .or_else(|| session.panes.first().map(|pane| pane.id))
            })
    }

    /// The node said what became of a send.
    pub(super) fn send_delivered(&mut self, id: u32, outcome: Outcome, cx: &mut Context<Self>) {
        let Some(at) = self.send.sending.iter().position(|(i, ..)| *i == id) else {
            return;
        };
        let (_, k, launched, enter) = self.send.sending.remove(at);
        match send::said(outcome, &k.name) {
            None if launched => {
                self.send.files.clear();
                self.send_note.update(cx, |f, cx| f.set("", false, cx));
                self.host.close_panel(surface::SEND, cx);
                self.report(&send::arrived(&k.name, launched, enter), false, None);
                self.save(cx);
                cx.notify();
                return;
            }
            None => self.send.reached.push(k.name.clone()),
            Some(why) => {
                self.send.stuck |= outcome == Outcome::Unknown;
                self.send.missed.push((k, why));
            }
        }
        self.settle(enter, cx);
        cx.notify();
    }

    /// The drawer's launcher asked `node` for a session (request `id`).
    pub fn send_launching(&mut self, node: &str, id: u32) {
        self.send.launch = Some((node.to_string(), id));
    }

    /// A launch answered: the drawer's own pastes its message, unsubmitted.
    pub(super) fn send_launched(
        &mut self,
        node: &str,
        id: u32,
        ok: bool,
        session: &str,
        cx: &mut Context<Self>,
    ) {
        match send::landed(self.send.launch.as_ref(), node, id, ok, session) {
            send::Landed::NotOurs => return,
            send::Landed::Failed => {}
            send::Landed::Wait(sid) => {
                self.send.new_agent = false;
                self.send.waiting = Some((node.to_string(), sid));
                self.send_waited(cx);
            }
        }
        self.send.launch = None;
        self.launchers[SEND].update(cx, |l, cx| l.done(cx));
        cx.notify();
    }

    /// The fleet changed: a launched session awaiting its message gets it
    /// once it is listed with a pane.
    pub(super) fn send_waited(&mut self, cx: &mut Context<Self>) {
        let Some((node, sid)) = &self.send.waiting else {
            return;
        };
        let Some((key, enter)) = send::launch_paste(&self.fleet.all, node, sid) else {
            return;
        };
        self.send.waiting = None;
        self.send_targets = vec![key.clone()];
        self.send.pane = None;
        self.deliver(vec![key], enter, true, cx);
    }

    /// Files from this computer, dropped on the drawer or picked.
    fn send_attach(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for p in paths {
            if p.is_dir() {
                self.send.said = Some(format!("{} is a folder; attach files.", p.display()));
            } else if !self.send.files.contains(&Attachment::File(p.clone())) {
                self.send.files.push(Attachment::File(p.clone()));
            }
        }
        cx.notify();
    }

    /// A file on a node, dragged from Files. An empty node is the serving
    /// node's, named here so it compares with each target's.
    fn send_attach_node(&mut self, file: &NodeFile, cx: &mut Context<Self>) {
        let mut file = file.clone();
        if file.node.as_str().is_empty()
            && let Some(here) = self.host.nodes(cx).into_iter().next()
        {
            file.node = here.id;
        }
        let file = Attachment::OnNode(file);
        if !self.send.files.contains(&file) {
            self.send.files.push(file);
        }
        cx.notify();
    }

    pub(super) fn send_clipboard(&mut self, item: gpui::ClipboardItem, cx: &mut Context<Self>) {
        // File managers may offer an image preview as well as the original file.
        let has_files = item.entries().iter().any(|e| matches!(e, gpui::ClipboardEntry::ExternalPaths(paths) if !paths.paths().is_empty()));
        for entry in item.into_entries() {
            match entry {
                gpui::ClipboardEntry::ExternalPaths(paths) => self.send_attach(paths.paths(), cx),
                gpui::ClipboardEntry::Image(image) if !has_files => {
                    self.send.files.push(Attachment::Image {
                        name: format!("clipboard.{}", image.format.extension()),
                        bytes: image.bytes,
                    });
                }
                gpui::ClipboardEntry::Image(_) | gpui::ClipboardEntry::String(_) => {}
            }
        }
        cx.notify();
    }

    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        ahjo_app_kit::widgets::path_picker::pick(
            self.host.clone(),
            "Attach",
            window,
            cx,
            |page, paths, cx| page.send_attach(&paths, cx),
        );
    }

    pub(super) fn render_send(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let busy = self.busy();
        self.offer_send_targets(cx);
        let mut body = pane::scroll("send-body")
            .debug_selector(|| "send-body".into())
            .flex_1()
            .p(px(12.))
            .gap(px(8.))
            .child(self.render_send_aimed(&t, cx))
            .child(self.send_finder.clone());
        if !self.send.new_agent
            && let [key] = self.send_targets.as_slice()
            && let Some(session) = self.fleet.session(key)
            && (session.panes.len() > 1
                || self
                    .send
                    .pane
                    .is_some_and(|id| !session.panes.iter().any(|p| p.id == id)))
        {
            let selected = self.send_pane(key);
            let mut choices = div().flex().flex_wrap().gap(px(6.));
            for (index, pane) in session.panes.iter().enumerate() {
                let id = pane.id;
                let label = format!("Pane {} · %{}", index + 1, id);
                choices = choices.child(
                    kit::button(
                        &t,
                        ("send-pane", index),
                        label,
                        if selected == Some(id) {
                            Kind::Primary
                        } else {
                            Kind::Quiet
                        },
                    )
                    .debug_selector(move || {
                        if selected == Some(id) {
                            format!("send-pane-selected-{index}")
                        } else {
                            format!("send-pane-{index}")
                        }
                    })
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.send.pane = Some(id);
                        s.send.said = None;
                        s.send.missed.clear();
                        cx.notify();
                    })),
                );
            }
            body = body.child(kit::label(&t, "Pane")).child(choices);
        }
        if self.send.new_agent {
            body = body
                .child(kit::note(
                    &t,
                    "The new agent gets the message pasted, not sent: press Enter there once it is ready.",
                    Some(t.dim),
                ))
                .child(self.launchers[SEND].clone());
        }
        body = body.child(div().mt(px(8.)).child(kit::label(&t, "Message")));
        body = body.children(self.render_send_reference(&t, cx));
        body = body
            .child(self.send_note.clone())
            .child(self.render_send_files(&t, cx));
        body = body.children(self.render_send_said(&t, cx));
        // Nothing goes until a target is chosen, and the button says which.
        let aimed: Vec<String> = match self.send.new_agent {
            true => vec![],
            false => self.send_targets.iter().map(|k| k.name.clone()).collect(),
        };
        let blocked = busy || self.send.stuck || aimed.is_empty();
        let go = if self.send.uploading {
            "Attaching…".to_string()
        } else if busy {
            "Sending…".to_string()
        } else {
            send::go_words(&aimed)
        };
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap(px(8.))
                .child(
                    kit::button(&t, "send-attach", "Attach…", Kind::Quiet)
                        .flex_none()
                        .tooltip(kit::tip("Or drop or paste files on the drawer"))
                        .on_click(
                            cx.listener(|s, _: &ClickEvent, window, cx| s.pick_files(window, cx)),
                        ),
                )
                .child(div().flex_1())
                .child(
                    kit::button(&t, "send-paste", "Paste only", Kind::Plain)
                        .flex_none()
                        .tooltip(kit::tip("Ctrl+Enter: paste without pressing Enter"))
                        .when(blocked, |b| b.opacity(0.5))
                        .on_click(cx.listener(|s, _: &ClickEvent, window, cx| {
                            s.send_now(false, window, cx)
                        })),
                )
                .child(
                    kit::button(&t, "send-go", go, Kind::Primary)
                        .flex_none()
                        .tooltip(kit::tip("Enter; Shift+Enter starts a new line"))
                        .debug_selector(|| "send-go".into())
                        .when(blocked, |b| b.opacity(0.5))
                        .on_click(cx.listener(|s, _: &ClickEvent, window, cx| {
                            s.send_now(true, window, cx)
                        })),
                ),
        );
        div()
            .id("send")
            .debug_selector(|| "send-drawer".into())
            .size_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            // A click back in Send goes on with the draft, where the click
            // itself gave the keyboard to nothing in it.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, _, window, cx| {
                    if !s.send_area.contains_focused(window, cx) {
                        s.send_resume(window, cx);
                        // Send's own focus would take the keyboard back.
                        window.prevent_default();
                    }
                }),
            )
            .track_focus(&self.send_area)
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| s.anyway_key(e, cx)))
            .drag_over::<ExternalPaths>(move |d, _, _, _| d.bg(alpha(t.accent, 0.08)))
            .drag_over::<NodeFile>(move |d, _, _, _| d.bg(alpha(t.accent, 0.08)))
            .on_drop(
                cx.listener(|s, paths: &ExternalPaths, _, cx| s.send_attach(paths.paths(), cx)),
            )
            .on_drop(cx.listener(|s, file: &NodeFile, _, cx| s.send_attach_node(file, cx)))
            .child(body)
            .into_any_element()
    }

    /// Hands the finder what it lists: a new agent first, then the
    /// sessions, recent ones first, with the chosen ones checked.
    fn offer_send_targets(&mut self, cx: &mut Context<Self>) {
        let first = Choice {
            id: NEW_AGENT.into(),
            label: "New agent…".into(),
            ..Default::default()
        };
        let choices = self.send_choices();
        let chosen: Vec<SharedString> = match self.send.new_agent {
            true => vec![NEW_AGENT.into()],
            false => self.send_targets.iter().map(|k| k.text().into()).collect(),
        };
        self.send_finder.update(cx, |f, _| {
            f.offer(
                Some(first),
                choices,
                vec![(AGENTS, "Agents"), (SHELLS, "Shells"), (WAITING, "Waiting")],
                chosen,
            )
        });
    }

    /// The line naming what was handed over: shown as it goes. Where a
    /// target is at a shell prompt it is left out for that one, struck
    /// through and saying why, with Send anyway beside it.
    fn render_send_reference(
        &self,
        t: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(r) = &self.send.reference else {
            return vec![];
        };
        // Aimed only at the session it names, it goes nowhere: not shown.
        let others = self.send_others().count();
        if others == 0 && !self.send_targets.is_empty() && !self.send.new_agent {
            return vec![];
        }
        let left_out = self.send_left_out();
        let dropped = !left_out.is_empty() && !self.send.anyway && left_out.len() == others;
        let mut out = vec![
            div()
                .id("send-reference")
                .debug_selector(|| "send-reference".into())
                .when(dropped, |d| d.line_through().opacity(0.6))
                .flex_none()
                .px(px(8.))
                .py(px(4.))
                .bg(t.raised)
                .rounded(px(size::RADIUS))
                .border_1()
                .border_color(t.divider)
                .text_size(px(size::SMALL))
                .text_color(t.text2)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(r.clone())
                .into_any_element(),
        ];
        if left_out.is_empty() {
            return out;
        }
        let who = send::names(&left_out);
        let one = left_out.len() == 1;
        let (words, act, selector) = if self.send.anyway {
            (
                format!("Sent to {who} too, where a shell would run it as a command."),
                "Leave it out",
                "send-leave-out",
            )
        } else {
            (
                format!(
                    "Not sent to {who}: {} at a shell prompt, which would run it as a command.",
                    if one { "it is" } else { "they are" }
                ),
                "Send anyway",
                "send-anyway",
            )
        };
        out.push(
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(kit::note(t, words, Some(t.dim))),
                )
                .child(
                    kit::button(t, selector, act, Kind::Quiet)
                        .flex_none()
                        .debug_selector(move || selector.into())
                        .tooltip(kit::tip("Alt+Enter, from anywhere in Send"))
                        .on_click(cx.listener(|s, _, _, cx| s.toggle_anyway(cx))),
                )
                // The key where the choice is: Tab goes from the message to
                // the sessions, so the keyboard reaches it by this alone.
                .child(
                    div()
                        .flex_none()
                        .pt(px(4.))
                        .child(pane::meta(t, "Alt+Enter", None)),
                )
                .into_any_element(),
        );
        out
    }

    /// The targets the line naming a session is left out for: those at a
    /// shell prompt, which would run it as a command.
    fn send_left_out(&self) -> Vec<String> {
        if self.send.new_agent || self.send.reference.is_none() {
            return vec![];
        }
        let others: Vec<&Key> = self.send_others().collect();
        let targets: Vec<_> = others.iter().map(|k| self.fleet.session(k)).collect();
        let names: Vec<&str> = others.iter().map(|k| k.name.as_str()).collect();
        send::left_out(self.send.names_a_session, &targets, &names)
    }

    /// The targets other than the session Send was opened from: those the
    /// reference may go to.
    fn send_others(&self) -> impl Iterator<Item = &Key> {
        (self.send_targets.iter()).filter(|k| self.send.source.as_ref() != Some(*k))
    }

    /// Send anyway, or Leave it out again.
    fn toggle_anyway(&mut self, cx: &mut Context<Self>) {
        self.send.anyway = !self.send.anyway;
        cx.notify();
    }

    /// Alt+Enter anywhere in the drawer is the button beside the
    /// reference: Send anyway, or Leave it out.
    fn anyway_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let alt_only = k.modifiers.alt && !k.modifiers.control && !k.modifiers.shift;
        if alt_only && k.key == "enter" && !self.send_left_out().is_empty() {
            cx.stop_propagation();
            self.toggle_anyway(cx);
        }
    }

    /// How the last send went where it did not simply arrive: each session
    /// not reached and why, and Send again where nobody knows whether it
    /// arrived.
    fn render_send_said(&self, t: &crate::theme::Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = vec![];
        if let Some(why) = &self.send.said {
            out.push(kit::note(t, why.clone(), Some(t.text2)).into_any_element());
        }
        for (k, why) in &self.send.missed {
            let name = k.name.clone();
            out.push(
                kit::note(t, why.clone(), Some(t.error))
                    .debug_selector(move || format!("send-missed-{name}"))
                    .into_any_element(),
            );
        }
        if self.send.stuck {
            out.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(kit::note(
                        t,
                        "Look at the session first: sending again may paste it twice.",
                        Some(t.dim),
                    )))
                    .child(
                        kit::button(t, "send-again", "Send again", Kind::Plain)
                            .flex_none()
                            .debug_selector(|| "send-again".into())
                            .on_click(cx.listener(|s, _, window, cx| {
                                s.send.stuck = false;
                                s.send_now(true, window, cx);
                            })),
                    )
                    .into_any_element(),
            );
        }
        out
    }

    /// Whom it goes to, named where the drawer starts: what Enter sends
    /// to, each with a way to take it off.
    fn render_send_aimed(&self, t: &crate::theme::Theme, cx: &mut Context<Self>) -> gpui::Div {
        let line = div()
            .flex_none()
            .min_h(px(size::ROW))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(div().text_color(t.text2).child("To"));
        if self.send.new_agent {
            return line.child(div().font_weight(FontWeight::SEMIBOLD).child("a new agent"));
        }
        if self.send_targets.is_empty() {
            return line
                .debug_selector(|| "send-aimed-none".into())
                .child(div().text_color(t.dim).child("pick sessions below"));
        }
        let focused = self.focused_key();
        let names: Vec<String> = self.send_targets.iter().map(|k| k.name.clone()).collect();
        let mut line = line.debug_selector(move || format!("send-aimed-{}", names.join(",")));
        for (i, k) in self.send_targets.iter().enumerate() {
            let gone = k.clone();
            line = line.child(
                div()
                    .h(px(22.))
                    .pl(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .rounded_full()
                    .bg(t.accent_light())
                    .border_1()
                    .border_color(t.divider)
                    .min_w_0()
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(k.name.clone()),
                    )
                    .when(focused.as_ref() == Some(k), |d| {
                        d.child(
                            div()
                                .pl(px(4.))
                                .text_size(px(size::SMALL))
                                .text_color(t.dim)
                                .child("focused"),
                        )
                    })
                    .child(
                        div()
                            .id(("send-drop", i))
                            .debug_selector({
                                let name = k.name.clone();
                                move || format!("send-drop-{name}")
                            })
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .text_color(t.text2)
                            .cursor_pointer()
                            .hover(|d| d.text_color(t.text).bg(t.accent_soft()))
                            .tooltip(kit::tip("Not to this one"))
                            .child("×")
                            .on_click(cx.listener(move |s, _, _, cx| s.send_drop(&gone, cx))),
                    ),
            );
        }
        line
    }

    /// The attached files, each with a way to take it off again.
    fn render_send_files(&self, t: &crate::theme::Theme, cx: &mut Context<Self>) -> gpui::Div {
        let mut list = div().flex().flex_col().gap(px(2.));
        for (i, p) in self.send.files.iter().enumerate() {
            let (name, size) = match p {
                Attachment::File(path) => (
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    std::fs::metadata(path)
                        .map(|m| send::bytes(m.len()))
                        .unwrap_or_default(),
                ),
                Attachment::Image { name, bytes } => {
                    (name.clone(), send::bytes(bytes.len() as u64))
                }
                // Where it is, for a file copied from another node.
                Attachment::OnNode(file) => (
                    file.name().to_string(),
                    (self.where_is(file.node.as_str()))
                        .map(|p| p.device)
                        .unwrap_or_default(),
                ),
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(size::SMALL))
                    .child(div().flex_1().min_w_0().overflow_hidden().child(name))
                    .child(div().text_color(t.dim).child(size))
                    .child(
                        kit::button(t, ("send-unattach", i), "Remove", Kind::Quiet).on_click(
                            cx.listener(move |s, _: &ClickEvent, _, cx| {
                                if i < s.send.files.len() {
                                    s.send.files.remove(i);
                                }
                                cx.notify();
                            }),
                        ),
                    ),
            );
        }
        list
    }
}

/// Writes each file on the target's node, in order: their paths there, or
/// why one could not be. A file already on that node goes as its own path;
/// one on another node is read there first.
async fn write_drops(
    page: &WeakEntity<Terminals>,
    node: &NodeId,
    user: &str,
    files: &[Attachment],
    cx: &mut AsyncApp,
) -> Result<Vec<String>, String> {
    let mut paths = vec![];
    for f in files {
        let (name, bytes) = match f {
            Attachment::OnNode(file) if &file.node == node => {
                paths.push(file.path.clone());
                continue;
            }
            Attachment::OnNode(file) => {
                let name = file.name().to_string();
                let task = page
                    .update(cx, |s, cx| {
                        s.host.request(http::file_bytes(&file.node, &file.path), cx)
                    })
                    .map_err(|_| "the page closed".to_string())?;
                let bytes = http::bytes_read(task.await)
                    .map_err(|e| format!("{name} could not be read: {e}"))?;
                (name, bytes)
            }
            Attachment::File(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let path = path.clone();
                let bytes = cx
                    .background_spawn(async move { std::fs::read(path) })
                    .await
                    .map_err(|e| format!("{name} could not be read: {e}"))?;
                (name, bytes)
            }
            Attachment::Image { name, bytes } => (name.clone(), bytes.clone()),
        };
        if bytes.len() > http::MAX_DROP {
            return Err(format!("{name} is over 256 MiB; copy it in Files instead."));
        }
        let task = page
            .update(cx, |s, cx| {
                s.host
                    .request(http::drop_file(node, user, &name, bytes), cx)
            })
            .map_err(|_| "the page closed".to_string())?;
        let path =
            http::dropped(task.await).map_err(|e| format!("{name} could not be attached: {e}"))?;
        paths.push(path);
    }
    Ok(paths)
}
