//! The brain area's part in Swarm: a page without a tab that owns the
//! Alt+L chord, and the Brain panel it opens over the terminals. The
//! panel's requests live here; its state and drawing stay apart.
use crate::state::{self, Brain, SETTLE_MS};
use ahjo_app_kit::widgets::select::Selectable;
use ahjo_app_kit::{
    Host, Page,
    chords::Chord,
    host::Request,
    link::client::LinkState,
    widgets::field::{self, FieldEvent, TextField},
};
use ahjo_core::NodeId;
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, IntoElement, Render, ScrollHandle, WeakEntity,
    Window, div,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// The page the Brain panel opens beside.
const PANEL_BESIDE: &str = "sessions";

/// Never shown: it lends its panel beside the terminals.
pub struct BrainPage {
    panel: Entity<Panel>,
}

impl Page for BrainPage {
    const ID: &'static str = "brain";
    const TITLE: &'static str = "Brain";
    const TAB: bool = false;
    const AREA: Option<&'static str> = Some("brain");

    fn chords(&self, _: &App) -> Vec<Chord> {
        let panel = self.panel.downgrade();
        vec![
            Chord::new("l", "Brain", move |window, cx| {
                let _ = panel.update(cx, |panel, cx| panel.toggle(window, cx));
            })
            .alt("l", Some(false))
            .everywhere(),
        ]
    }

    fn link(
        &mut self,
        _: &NodeId,
        state: Option<LinkState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The serving node came back: what the open panel lists is read
        // again from it.
        if state == Some(LinkState::Open) && self.panel.read(cx).is_open(cx) {
            self.panel.update(cx, |panel, cx| panel.read_arrivals(cx));
        }
    }
}

impl Render for BrainPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl BrainPage {
    pub fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel = cx.new(|cx| Panel::new(host, window, cx));
        Self { panel }
    }
}

/// The Brain panel: the search over what arrived lately or what it found,
/// and a page read in place, with its links followed and Send.
pub struct Panel {
    pub(crate) host: Host,
    pub(crate) state: Brain,
    pub(crate) field: Entity<TextField>,
    pub(crate) select: Rc<RefCell<Selectable>>,
    pub(crate) anchor: Rc<RefCell<Option<String>>>,
    pub(crate) scroll: ScrollHandle,
    /// The list's scroll, and that the keys moved its mark out of sight.
    pub(crate) list_scroll: ScrollHandle,
    pub(crate) reveal: bool,
    pub(crate) reader_focus: FocusHandle,
    /// Ctrl+wheel over the page read, a step not yet taken.
    pub(crate) zoom_wheel: Rc<std::cell::Cell<ahjo_app_kit::zoom::Wheel>>,
    /// Where the keyboard was before the panel opened.
    return_focus: Option<FocusHandle>,
    me: WeakEntity<Panel>,
}

impl Panel {
    fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new(cx, "Search the brain…"));
        cx.subscribe_in(
            &field,
            window,
            |panel, field, event, window, cx| match event {
                FieldEvent::Changed => {
                    let text = field.read(cx).text.clone();
                    panel.typed(&text, cx);
                }
                FieldEvent::Up => panel.step(-1, cx),
                FieldEvent::Down => panel.step(1, cx),
                FieldEvent::Submit => {
                    if let Some(path) = panel.state.marked() {
                        panel.open(&path, window, cx);
                    }
                }
                _ => {}
            },
        )
        .detach();
        Self {
            host,
            state: Brain::default(),
            field,
            select: Rc::default(),
            anchor: Rc::default(),
            scroll: ScrollHandle::new(),
            list_scroll: ScrollHandle::new(),
            reveal: false,
            reader_focus: cx.focus_handle(),
            zoom_wheel: Default::default(),
            return_focus: None,
            me: cx.weak_entity(),
        }
    }

    /// Whether the panel is the view beside the terminals.
    pub(crate) fn is_open(&self, cx: &App) -> bool {
        self.host
            .beside_of(PANEL_BESIDE, cx)
            .zip(self.me.upgrade())
            .is_some_and(|(beside, me)| beside.entity_id() == me.entity_id())
    }

    /// Alt+L: the panel opens over the terminals as anything beside them
    /// does, so no session changes size, with the search taking the keys;
    /// or it closes again.
    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_open(cx) {
            self.close(window, cx);
            return;
        }
        let Some(me) = self.me.upgrade() else {
            return;
        };
        self.return_focus = window.focused(cx);
        self.read_arrivals(cx);
        self.host.show_beside(PANEL_BESIDE, me.into(), cx);
        self.host.for_page(PANEL_BESIDE, &[]).show(cx);
        if self.state.page.is_some() {
            window.focus(&self.reader_focus, cx);
        } else {
            field::focus(&self.field, window, cx);
        }
    }

    /// Closes the panel, the keyboard going back where it was when the
    /// panel opened if it was in the panel.
    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let in_panel = self.field.read(cx).is_focused(window)
            || self.reader_focus.contains_focused(window, cx);
        self.host.close_beside(PANEL_BESIDE, cx);
        if let Some(focus) = self.return_focus.take()
            && in_panel
        {
            window.focus(&focus, cx);
        }
    }

    /// Reads what arrived lately, which the list shows before anything is
    /// typed: each time the panel opens.
    pub(crate) fn read_arrivals(&mut self, cx: &mut Context<Self>) {
        self.request(
            "/api/brain/changes",
            |panel, status, body, _| panel.state.changes_read(status, &body),
            cx,
        );
    }

    /// A GET on the serving node; `done` hears its status and body.
    fn request(
        &mut self,
        path: &str,
        done: impl FnOnce(&mut Self, u16, Vec<u8>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let task = self
            .host
            .request(Request::get(NodeId(String::new()), path), cx);
        cx.spawn(async move |this, cx| {
            let (status, body) = match task.await {
                Ok(answer) => (answer.status, answer.body),
                Err(error) => (0, error.message.into_bytes()),
            };
            let _ = this.update(cx, |panel, cx| {
                done(panel, status, body, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// What is typed searches once typing rests; the page read gives way
    /// to the results.
    pub(crate) fn typed(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.state.page.is_some() || self.state.page_error.is_some() {
            self.state.close();
        }
        let Some(asked) = self.state.typed(query) else {
            cx.notify();
            return;
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SETTLE_MS))
                .await;
            let _ = this.update(cx, |panel, cx| {
                if panel.state.asked != asked {
                    return;
                }
                let path = format!("/api/brain/search?q={}", state::encode(&panel.state.query));
                panel.request(
                    &path,
                    move |panel, status, body, _| {
                        if panel.state.found(asked, status, &body) {
                            panel.reveal = true;
                        }
                    },
                    cx,
                );
            });
        })
        .detach();
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        self.state.step(by);
        self.reveal = true;
        cx.notify();
    }

    /// Goes back one page along the trail; from its first page, back to
    /// the list, the search taking the keys again.
    pub(crate) fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let trail = &self.state.trail;
        if let Some((path, _)) = trail.len().checked_sub(2).and_then(|at| trail.get(at)) {
            let path = path.clone();
            self.follow(&path, None, cx);
        } else {
            self.state.close();
            self.reveal = true;
            field::focus(&self.field, window, cx);
            cx.notify();
        }
    }

    /// Opens a page chosen from the list: it starts a new trail, and the
    /// reader takes the keys.
    pub(crate) fn open(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(at) = self.state.listed().iter().position(|p| *p == path) {
            self.state.selected = at;
        }
        window.focus(&self.reader_focus, cx);
        self.read_page(path, true, None, cx);
    }

    /// Opens a page a link leads to, at `anchor`: it joins the trail.
    pub(crate) fn follow(&mut self, path: &str, anchor: Option<String>, cx: &mut Context<Self>) {
        self.read_page(path, false, anchor, cx);
    }

    fn read_page(
        &mut self,
        path: &str,
        fresh: bool,
        anchor: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let reading = self.state.open(fresh);
        let path = format!("/api/brain/page?path={}", state::encode(path));
        self.request(
            &path,
            move |panel, status, body, _| {
                if panel.state.read(reading, status, &body) && status == 200 {
                    panel
                        .scroll
                        .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
                    *panel.anchor.borrow_mut() = anchor;
                    panel.select.borrow_mut().selection.clear();
                }
            },
            cx,
        );
        cx.notify();
    }
}

/// The smallest and largest text a page is read at.
const READ_MIN: f32 = 11.;
const READ_MAX: f32 = 32.;

impl Panel {
    /// The size a page is read at: the theme's unless zoomed.
    pub(crate) fn read_size(&self, cx: &App) -> f32 {
        let kept: state::Kept = self.host.setting(cx).unwrap_or_default();
        kept.read
            .map_or(ahjo_app_kit::theme::theme(cx).ty.read, f32::from)
    }

    /// The one zoom rule (`ahjo_app_kit::zoom`) on the page read.
    pub(crate) fn zoom_read(
        &mut self,
        z: ahjo_app_kit::zoom::Zoom,
        n: i32,
        cx: &mut Context<Self>,
    ) {
        use ahjo_app_kit::zoom::{Zoom, step};
        let usual = ahjo_app_kit::theme::theme(cx).ty.read;
        let at = self.read_size(cx);
        let to = match z {
            Zoom::Reset => (at != usual).then_some(usual),
            Zoom::In => step(at, n, 1., READ_MIN, READ_MAX),
            Zoom::Out => step(at, -n, 1., READ_MIN, READ_MAX),
        };
        let Some(to) = to else {
            return;
        };
        let kept = state::Kept {
            read: (to != usual).then_some(to as u8),
        };
        let _ = self.host.set_setting(&kept, cx);
        let reset = if to == usual { "" } else { " · Ctrl+0 resets" };
        self.host.toast(&format!("Text {to} px{reset}"), cx);
        cx.notify();
    }
}
