//! The app's frame: what every Ahjo window has whatever it shows, with no
//! node behind it. The one dismissal stack of popups (menus and confirms
//! among them) and layers, the status line and the toast. Swarm's hub holds
//! one for the shell and its pages ([`crate::Hub::frame`]); Ahjo Desk and
//! Ahjo Brain each hold their own. Either way the same functions here draw
//! it ([`notices`], [`stack`]), work its keys ([`key`], [`menu_key`],
//! [`dismiss_top`]) and give the keyboard back as it changes ([`settle`]),
//! so a gesture behaves the same in every app.

use crate::chords::{KeyLike, Run};
use crate::layers::{self, Stack};
use crate::menu::{Menu, MenuRow, Pick};
use crate::popup::{self, Content, Outside, Popup, PopupId, Spot};
use crate::theme::{Theme, alpha, motion, size, theme};
use crate::widgets::{kit, pane};
use gpui::{
    Animation, AnimationExt, AnyElement, AnyView, App, Context, ElementId, Entity, EventEmitter,
    FocusHandle, KeyDownEvent, MouseButton, Pixels, Point, Task, Window, deferred, div,
    ease_out_quint, prelude::*, px,
};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

/// How long words that are not a failure stay on the status line.
pub const NOTICE_SHOWN: Duration = Duration::from_secs(8);
/// The order of what draws over the page: the notices, then everything on
/// the stack (each over what opened before it). An app's own overlays that
/// must stand over both take a priority above `STACK` plus the stack's
/// depth.
pub const NOTICES: usize = 25;
pub const STACK: usize = 40;

/// A line for the status line: what happened, whether it failed, and a
/// button that acts on it. A failure stays until it is closed or replaced;
/// other words go after [`NOTICE_SHOWN`].
#[derive(Clone)]
pub struct Status {
    pub at: SystemTime,
    pub text: String,
    pub failure: bool,
    pub action: Option<(String, Run)>,
}

impl Status {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            at: SystemTime::now(),
            text: text.into(),
            failure: false,
            action: None,
        }
    }

    pub fn failure(text: impl Into<String>) -> Self {
        Self {
            failure: true,
            ..Self::new(text)
        }
    }

    pub fn action(
        mut self,
        label: impl Into<String>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(run)));
        self
    }
}

/// A moment's word: it rises in and fades on its own.
pub struct Toast {
    pub text: String,
    pub at: Instant,
}

/// Where a layer draws: over the page (under a tab bar and above the
/// status line), or over the whole window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cover {
    Page,
    Window,
}

/// A layer on the dismissal stack: what it draws, what has the keyboard
/// while it is on top, and what closing it does.
#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub view: AnyView,
    pub cover: Cover,
    pub focus: Option<FocusHandle>,
    pub dismiss: Option<Run>,
    /// Escape is its content's (a terminal's program), so it does not
    /// close it; it says on screen how it closes.
    pub keeps_escape: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, view: impl Into<AnyView>) -> Self {
        Self {
            name: name.into(),
            view: view.into(),
            cover: Cover::Page,
            focus: None,
            dismiss: None,
            keeps_escape: false,
        }
    }

    /// The keyboard is in this layer's focus.
    pub fn has_keyboard(&self, window: &Window) -> bool {
        self.focus.as_ref().is_some_and(|f| f.is_focused(window))
    }

    pub fn keeps_escape(mut self) -> Self {
        self.keeps_escape = true;
        self
    }

    pub fn cover(mut self, cover: Cover) -> Self {
        self.cover = cover;
        self
    }

    pub fn focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    /// What closing it does, by Escape, a click outside or a close.
    pub fn on_dismiss(mut self, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.dismiss = Some(Rc::new(run));
        self
    }
}

/// What is on the dismissal stack: a popup (a menu among them), or a
/// layer over the page or the window.
#[derive(Clone)]
pub enum Surface {
    Popup(Popup),
    Layer(Layer),
}

impl Surface {
    /// Escape closes it when it is on top; a layer that keeps Escape for
    /// its content does not close on it.
    pub fn escape_closes(&self) -> bool {
        match self {
            Surface::Popup(_) => true,
            Surface::Layer(l) => !l.keeps_escape,
        }
    }

    /// The keyboard is in it.
    pub fn has_keyboard(&self, window: &Window, cx: &App) -> bool {
        match self {
            Surface::Popup(p) => p.has_keyboard(window, cx),
            Surface::Layer(l) => l.has_keyboard(window),
        }
    }

    /// What has the keyboard while it is on top.
    pub fn focus(&self) -> Option<&FocusHandle> {
        match self {
            Surface::Popup(p) => p.focus.as_ref(),
            Surface::Layer(l) => l.focus.as_ref(),
        }
    }

    pub fn dismiss(&self) -> Option<&Run> {
        match self {
            Surface::Popup(p) => p.dismiss.as_ref(),
            Surface::Layer(l) => l.dismiss.as_ref(),
        }
    }
}

impl layers::Layer for Surface {
    fn kind(&self) -> &str {
        match self {
            Surface::Popup(p) => &p.name,
            Surface::Layer(l) => &l.name,
        }
    }
    fn passing(&self) -> bool {
        matches!(self, Surface::Popup(p) if p.is_menu())
    }
}

/// What the frame's owner hears, after the update that caused it.
pub enum FrameEvent {
    /// The stack changed: the keyboard goes to the top surface that takes
    /// it, or back to where it was, and what closed runs its dismiss
    /// ([`settle`]).
    Layers { closed: Vec<Surface> },
}

/// The stack, the status line and the toast of one window.
pub struct AppFrame {
    pub stack: Stack<Surface>,
    /// The number the next popup opened is known by.
    next_popup: u64,
    pub status: Option<Status>,
    pub toast: Option<Toast>,
    toast_task: Option<Task<()>>,
    /// Draws again when words that are not a failure go stale.
    status_task: Option<Task<()>>,
}

impl EventEmitter<FrameEvent> for AppFrame {}

impl Default for AppFrame {
    fn default() -> Self {
        Self {
            stack: Stack::default(),
            next_popup: 1,
            status: None,
            toast: None,
            toast_task: None,
            status_task: None,
        }
    }
}

impl AppFrame {
    /// Says something on the status line. A failure stays until it is
    /// closed or replaced; other words go after [`NOTICE_SHOWN`].
    pub fn report(&mut self, status: Status, cx: &mut Context<Self>) {
        self.status_task = (!status.failure).then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(NOTICE_SHOWN).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
        });
        self.status = Some(status);
        cx.notify();
    }

    /// Takes back the status said `at`, if nothing has replaced it.
    pub fn withdraw(&mut self, at: SystemTime, cx: &mut Context<Self>) {
        if self.status.as_ref().is_some_and(|s| s.at == at) {
            self.status = None;
            cx.notify();
        }
    }

    /// The status line as it stands now: a failure, or words not yet stale.
    pub fn status_shown(&self) -> Option<&Status> {
        self.status
            .as_ref()
            .filter(|s| s.failure || s.at.elapsed().is_ok_and(|age| age < NOTICE_SHOWN))
    }

    /// A moment's word, which fades on its own. Never a failure: that is
    /// [`AppFrame::report`]ed, and stays.
    pub fn toast(&mut self, text: &str, cx: &mut Context<Self>) {
        let at = Instant::now();
        self.toast = Some(Toast {
            text: text.to_string(),
            at,
        });
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            // Two wakes: one to start fading, one to go.
            cx.background_executor()
                .timer(Duration::from_millis(
                    motion::TOAST_IN + motion::TOAST_SHOWN,
                ))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
            cx.background_executor()
                .timer(Duration::from_millis(motion::TOAST_OUT))
                .await;
            let _ = this.update(cx, |f, cx| {
                if f.toast.as_ref().is_some_and(|t| t.at == at) {
                    f.toast = None;
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Opens a surface over the others.
    pub fn open(&mut self, s: Surface, cx: &mut Context<Self>) {
        let closed: Vec<Surface> = self
            .stack
            .iter()
            .filter(|x| layers::Layer::passing(*x))
            .cloned()
            .collect();
        self.stack.open(s);
        cx.emit(FrameEvent::Layers { closed });
        cx.notify();
    }

    /// Closes the surface of this kind and whatever opened above it.
    pub fn close(&mut self, kind: &str, cx: &mut Context<Self>) {
        let closed = self.stack.close(kind);
        if !closed.is_empty() {
            cx.emit(FrameEvent::Layers { closed });
            cx.notify();
        }
    }

    /// Opens `popup` over the others, named `<owner>/popup/<n>`.
    pub fn open_popup(&mut self, owner: &str, mut popup: Popup, cx: &mut Context<Self>) -> PopupId {
        let id = PopupId(self.next_popup);
        self.next_popup += 1;
        popup.id = id;
        popup.name = format!("{owner}/popup/{}", id.0);
        self.open(Surface::Popup(popup), cx);
        id
    }

    /// Closes popup `id` and whatever opened above it, if it is open.
    pub fn close_popup(&mut self, id: PopupId, cx: &mut Context<Self>) {
        if let Some(name) = self.popup(id).map(|p| p.name.clone()) {
            self.close(&name, cx);
        }
    }

    /// Popup `id`, while it is open.
    pub fn popup(&self, id: PopupId) -> Option<&Popup> {
        self.stack.iter().find_map(|s| match s {
            Surface::Popup(p) if p.id == id => Some(p),
            _ => None,
        })
    }

    /// Opens a menu of `rows` beside `at`: the button pressed there, else
    /// the point. `keyed` chooses its first row for the keyboard, as when
    /// a key opened it.
    pub fn menu(
        &mut self,
        owner: &str,
        rows: Vec<MenuRow>,
        at: Point<Pixels>,
        keyed: bool,
        cx: &mut Context<Self>,
    ) -> PopupId {
        let menu = if keyed {
            Menu::new(rows).keyed()
        } else {
            Menu::new(rows)
        };
        let spot = Spot::at(at, cx);
        self.open_popup(owner, Popup::menu(spot, menu), cx)
    }

    /// Asks before `row` runs, beside `spot`: its one row, a danger armed
    /// already, runs on a click or Enter; Escape or a press outside lets
    /// it go.
    pub fn confirm(
        &mut self,
        owner: &str,
        spot: Spot,
        row: MenuRow,
        cx: &mut Context<Self>,
    ) -> PopupId {
        let menu = Menu::new(vec![row.danger()]).armed(0);
        self.open_popup(owner, Popup::menu(spot, menu), cx)
    }

    /// Closes the open menu, if one is.
    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        let menu = self.stack.iter().find_map(|s| match s {
            Surface::Popup(p) if p.is_menu() => Some(p.name.clone()),
            _ => None,
        });
        if let Some(name) = menu {
            self.close(&name, cx);
        }
    }

    /// Escape: the top surface closes. False when nothing was open.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(top) = self.stack.dismiss() else {
            return false;
        };
        cx.emit(FrameEvent::Layers { closed: vec![top] });
        cx.notify();
        true
    }

    /// What has the keyboard while the stack stands as it does: the top
    /// surface that takes it.
    pub fn top_focus(&self) -> Option<FocusHandle> {
        self.stack.iter().rev().find_map(|s| s.focus().cloned())
    }

    /// Something whose name starts with `prefix`, other than a menu, is
    /// on the stack.
    pub fn has_open(&self, prefix: &str) -> bool {
        self.stack
            .iter()
            .any(|s| !layers::Layer::passing(s) && layers::Layer::kind(s).starts_with(prefix))
    }
}

/// The stack changed (a [`FrameEvent::Layers`]): a popup just opened
/// remembers what had the keyboard, to give it back when it closes; what
/// closed runs its dismiss; then the keyboard goes to the top surface that
/// takes it. With none to take it, a closed popup that had the keyboard
/// gives it back to where it was as it opened; a dismiss that moved the
/// keyboard itself keeps it there; a surface closing with the keyboard
/// elsewhere leaves it there; else it goes to `page`. `window_focus` is
/// the window's own focus, never a place to give the keyboard back to.
pub fn settle(
    frame: &Entity<AppFrame>,
    closed: &[Surface],
    window_focus: &FocusHandle,
    page: impl FnOnce(&App) -> FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let before = window.focused(cx);
    frame.update(cx, |f, _| {
        for s in f.stack.iter_mut() {
            if let Surface::Popup(p) = s
                && p.back.is_none()
            {
                p.back = Some(
                    before
                        .as_ref()
                        .map_or_else(|| window_focus.downgrade(), |f| f.downgrade()),
                );
            }
        }
    });
    // The keyboard was in a surface that closed, or nowhere.
    let lost = before.is_none() || closed.iter().any(|s| s.has_keyboard(window, cx));
    // Where it was before the first of the closed popups opened. The
    // window itself (a press on a bar leaves it there) is no place to go
    // back to: the page takes it.
    let back = closed
        .iter()
        .rev()
        .find_map(|s| match s {
            Surface::Popup(p) => p.back.as_ref().and_then(|b| b.upgrade()),
            Surface::Layer(_) => None,
        })
        .filter(|f| f != window_focus);
    for s in closed {
        if let Some(run) = s.dismiss() {
            run(window, cx);
        }
    }
    let moved = window.focused(cx) != before;
    let to = match frame.read(cx).top_focus() {
        Some(f) => f,
        None if moved || !lost => return,
        None => back.unwrap_or_else(|| page(cx)),
    };
    window.focus(&to, cx);
}

/// A key for the frame, from an app that has no chords of its own to
/// weigh it against (Desk, Brain): the open menu's keys, and Escape
/// closing the top surface. True when it was the frame's; the app stops
/// it there.
pub fn key(frame: &Entity<AppFrame>, e: &KeyDownEvent, window: &mut Window, cx: &mut App) -> bool {
    let k = KeyLike::of(e);
    if menu_key(frame, &k, window, cx) {
        return true;
    }
    if k.key != "escape" || k.alt || k.ctrl || k.platform {
        return false;
    }
    // A layer that keeps Escape for its content closes on Shift+Escape.
    let closes = match frame.read(cx).stack.top() {
        Some(top) => top.escape_closes() != k.shift,
        None => false,
    };
    closes && dismiss_top(frame, window, cx)
}

/// Escape: the top surface closes, unless it is a popup that keeps itself
/// open now ([`Popup::may_close`]). False when nothing was open.
pub fn dismiss_top(frame: &Entity<AppFrame>, window: &mut Window, cx: &mut App) -> bool {
    let top = frame.read(cx).stack.top().cloned();
    match &top {
        None => return false,
        Some(Surface::Popup(p)) if !p.lets_close(window, cx) => return true,
        Some(_) => {}
    }
    frame.update(cx, |f, cx| f.dismiss(cx))
}

/// A press outside popup `id` that closes it, unless it keeps itself open
/// now.
pub fn press_outside(frame: &Entity<AppFrame>, id: PopupId, window: &mut Window, cx: &mut App) {
    let popup = frame.read(cx).popup(id).cloned();
    if popup.is_some_and(|p| p.lets_close(window, cx)) {
        frame.update(cx, |f, cx| f.close_popup(id, cx));
    }
}

/// The keys of a menu on top: Up and Down move its keyboard row, Enter,
/// Space or Right picks it. True if the key was the menu's.
pub fn menu_key(frame: &Entity<AppFrame>, k: &KeyLike, window: &mut Window, cx: &mut App) -> bool {
    if k.alt || k.ctrl || k.shift || k.platform {
        return false;
    }
    let cursor = match frame.read(cx).stack.top() {
        Some(Surface::Popup(Popup {
            content: Content::Menu(m),
            ..
        })) => m.cursor,
        _ => return false,
    };
    let by = match k.key.as_str() {
        "down" => 1,
        "up" => -1,
        "enter" | "space" | "right" => {
            let Some(i) = cursor else {
                return false;
            };
            menu_pick(frame, i, window, cx);
            return true;
        }
        _ => return false,
    };
    frame.update(cx, |f, cx| {
        if let Some(Surface::Popup(Popup {
            content: Content::Menu(m),
            ..
        })) = f.stack.top_mut()
        {
            m.step(by);
        }
        cx.notify();
    });
    true
}

/// A click on row `i` of the menu on top, or Enter on its keyboard row.
pub fn menu_pick(frame: &Entity<AppFrame>, i: usize, window: &mut Window, cx: &mut App) {
    let pick = frame.update(cx, |f, cx| {
        let pick = match f.stack.top_mut() {
            Some(Surface::Popup(Popup {
                content: Content::Menu(m),
                ..
            })) => m.pick(i),
            _ => Pick::Nothing,
        };
        match pick {
            Pick::Run(_) => f.close_menu(cx),
            _ => cx.notify(),
        }
        pick
    });
    if let Pick::Run(run) = pick {
        window.defer(cx, move |window, cx| run(window, cx));
    }
}

/// What the app has to say, at the window's bottom right over the page:
/// the status line (a failure stays until it is closed or replaced, other
/// words go after a while) and a toast. Nothing when all is well.
pub fn notices(frame: &Entity<AppFrame>, cx: &mut App) -> Option<AnyElement> {
    let column = notice_column(frame, size::NOTICE, cx)?
        .absolute()
        .right(px(size::HEAD_PAD))
        .bottom(px(size::HEAD_PAD))
        .items_end();
    Some(deferred(column).with_priority(NOTICES).into_any_element())
}

/// The same notices at the top of the strip of the window from `left`,
/// `width` wide, `top` down: for an app whose corner something drawn
/// outside the window can cover (Desk's page, shown in a view of its own).
pub fn notices_over(
    frame: &Entity<AppFrame>,
    (left, width): (f32, f32),
    top: f32,
    cx: &mut App,
) -> Option<AnyElement> {
    // Within the strip, however narrow: its words wrap rather than run
    // over what is beside it.
    let most = size::NOTICE.min(width - 2. * size::HEAD_PAD).max(0.);
    let column = notice_column(frame, most, cx)?.items_center();
    let strip = div()
        .absolute()
        .top(px(top))
        .left(px(left))
        .w(px(width))
        .px(px(size::HEAD_PAD))
        .flex()
        .justify_center()
        .child(column);
    Some(deferred(strip).with_priority(NOTICES).into_any_element())
}

/// The notices, each at most `most` wide.
fn notice_column(frame: &Entity<AppFrame>, most: f32, cx: &mut App) -> Option<gpui::Div> {
    let t = theme(cx).clone();
    let f = frame.read(cx);
    let status = f.status_shown().cloned();
    let toast = f.toast.as_ref().map(|toast| render_toast(&t, toast, most));
    if status.is_none() && toast.is_none() {
        return None;
    }
    let mut column = div().flex().flex_col().gap(px(size::HEAD_GAP));
    if let Some(Status {
        at,
        text,
        failure,
        action,
    }) = status
    {
        let mut notice = kit::panel(&t)
            .max_w(px(most))
            .id("status")
            .px(px(size::HEAD_PAD))
            .py(px(size::HEAD_GAP))
            .flex()
            .items_center()
            .gap(px(size::HEAD_GAP))
            .child(
                div()
                    .id("status-text")
                    .debug_selector(|| "status-text".into())
                    .min_w_0()
                    .text_color(if failure { t.error } else { t.text })
                    .child(text),
            );
        if let Some((label, run)) = action {
            notice = notice.child(
                kit::button(&t, "status-action", label, kit::Kind::Plain)
                    .debug_selector(|| "status-action".into())
                    .on_click(move |_, window, cx| {
                        let run = run.clone();
                        window.defer(cx, move |window, cx| run(window, cx));
                    }),
            );
        }
        if failure {
            let frame = frame.downgrade();
            notice = notice.child(
                kit::button(&t, "status-close", "×", kit::Kind::Quiet)
                    .debug_selector(|| "status-close".into())
                    .tooltip(kit::tip("Close"))
                    .on_click(move |_, _, cx| {
                        let _ = frame.update(cx, |f, cx| f.withdraw(at, cx));
                    }),
            );
        }
        column = column.child(notice);
    }
    Some(column.children(toast))
}

/// A toast: a word that something happened, fading on its own.
fn render_toast(t: &Theme, toast: &Toast, most: f32) -> AnyElement {
    let fade_at = Duration::from_millis(motion::TOAST_IN + motion::TOAST_SHOWN);
    let leaving = toast.at.elapsed() >= fade_at;
    let panel = kit::panel(t)
        .max_w(px(most))
        .id("toast")
        .debug_selector(|| "toast".into())
        .px(px(size::HEAD_PAD))
        .py(px(size::HEAD_GAP))
        .child(toast.text.clone());
    // Each toast rises in, and fades as it goes.
    let id = ElementId::Name(format!("toast-{:?}-{leaving}", toast.at).into());
    if leaving {
        panel
            .with_animation(
                id,
                Animation::new(Duration::from_millis(motion::TOAST_OUT)),
                |d, x| d.opacity(1. - x),
            )
            .into_any_element()
    } else {
        panel
            .with_animation(
                id,
                Animation::new(Duration::from_millis(motion::TOAST_IN))
                    .with_easing(ease_out_quint()),
                |d, x| d.opacity(x).mt(px(motion::PANEL_DROP * (1. - x))),
            )
            .into_any_element()
    }
}

/// What is on the stack over `cover`, bottom first, each over what opened
/// before it: the layers covering it, and with the window the popups.
/// Each goes where `cover` is drawn: a page's box (relative) or the
/// window's.
pub fn stack(
    frame: &Entity<AppFrame>,
    cover: Cover,
    window: &Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let surfaces: Vec<Surface> = frame.read(cx).stack.iter().cloned().collect();
    let mut out = vec![];
    for (depth, s) in surfaces.into_iter().enumerate() {
        match s {
            Surface::Layer(l) if l.cover == cover => {
                let id = l.view.entity_id().as_u64() as usize;
                out.push(
                    deferred(
                        div().absolute().inset_0().child(l.view).with_animation(
                            ("layer-in", id),
                            Animation::new(Duration::from_millis(motion::PANEL_IN))
                                .with_easing(ease_out_quint()),
                            |d, x| d.opacity(x),
                        ),
                    )
                    .with_priority(STACK + depth)
                    .into_any_element(),
                )
            }
            Surface::Popup(p) if cover == Cover::Window => {
                out.push(render_popup(frame, depth, p, window, cx))
            }
            _ => {}
        }
    }
    out
}

/// A popup beside what it was opened from ([`popup::beside`]), over a
/// cover that takes every press outside it: one that closes it, or one
/// that only keeps the press from what is under it.
fn render_popup(
    frame: &Entity<AppFrame>,
    depth: usize,
    popup: Popup,
    window: &Window,
    cx: &mut App,
) -> AnyElement {
    let id = popup.id();
    let content = match &popup.content {
        Content::Menu(menu) => render_menu(frame, depth, menu, cx),
        Content::View(view) => view.clone().into_any_element(),
    };
    let viewport = window.viewport_size();
    let mut cover = div().absolute().inset_0().occlude();
    if popup.outside == Outside::Close {
        for button in [MouseButton::Left, MouseButton::Right] {
            let frame = frame.clone();
            cover = cover.on_mouse_down(button, move |_, window, cx| {
                press_outside(&frame, id, window, cx)
            });
        }
    }
    deferred(
        cover.child(
            popup::beside(popup.spot, viewport).child(
                pane::scroll(id.element("popup"))
                    .debug_selector(|| "popup".into())
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    // Never larger than the window it is kept inside: a
                    // drawn popup gives way to it ([`popup::form`] keeps
                    // its actions in view), and anything else taller
                    // scrolls there.
                    .max_w(viewport.width - px(2. * size::POPUP_EDGE))
                    .max_h(viewport.height - px(2. * size::POPUP_EDGE))
                    .child(content)
                    .with_animation(
                        id.element("popup-in"),
                        Animation::new(Duration::from_millis(motion::PANEL_IN))
                            .with_easing(ease_out_quint()),
                        // A fade only: it stands where it will stay from
                        // its first frame.
                        |d, x| d.opacity(x),
                    ),
            ),
        ),
    )
    .with_priority(STACK + depth)
    .into_any_element()
}

/// A menu's rows.
fn render_menu(frame: &Entity<AppFrame>, depth: usize, menu: &Menu, cx: &mut App) -> AnyElement {
    let t = theme(cx).clone();
    let mut list = kit::panel(&t)
        .min_w(px(size::MENU))
        .py(px(4.))
        .flex()
        .flex_col();
    for (i, row) in menu.rows.iter().enumerate() {
        if row.is_rule() {
            list = list.child(div().my(px(4.)).child(kit::rule(&t)));
            continue;
        }
        let armed = menu.armed == Some(i);
        let keyed = menu.cursor == Some(i);
        let label = if armed {
            format!("{}? Click again", row.label)
        } else {
            row.label.clone()
        };
        let inert = row.inert();
        let colour = if inert {
            t.dim
        } else if row.danger {
            t.error
        } else {
            t.text
        };
        let line = div()
            .h(px(size::ROW))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().w(px(12.)).text_color(t.accent).child(if row.checked {
                "✓"
            } else {
                ""
            }))
            .child({
                let name = format!("menu-label-{}", row.label);
                div().flex_1().debug_selector(move || name).child(label)
            })
            .children(
                row.chord
                    .clone()
                    .map(|c| div().text_size(px(size::SMALL)).text_color(t.dim).child(c)),
            );
        let frame = frame.clone();
        list = list.child(
            div()
                .id(("menu", depth * 100 + i))
                .debug_selector(|| format!("menu-row-{i}"))
                .px(px(12.))
                .flex()
                .flex_col()
                .text_color(colour)
                .when(!inert, |d| {
                    d.cursor_pointer().hover(|d| d.bg(t.accent_light()))
                })
                .when(keyed && !armed, |d| d.bg(t.accent_light()))
                .when(armed, |d| d.bg(alpha(t.error, 0.15)))
                .child(line)
                .children(row.hint.clone().map(|h| {
                    div()
                        .max_w(px(280.))
                        .pl(px(24.))
                        .pb(px(6.))
                        .text_size(px(size::SMALL))
                        .text_color(t.dim)
                        .child(h)
                }))
                .on_click(move |_, window, cx| menu_pick(&frame, i, window, cx)),
        );
    }
    list.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AppContext;

    #[gpui::test]
    fn a_failure_stays_and_other_words_go_stale(cx: &mut gpui::TestAppContext) {
        let frame = cx.new(|_| AppFrame::default());
        frame.update(cx, |f, cx| f.report(Status::new("Saved"), cx));
        assert!(frame.read_with(cx, |f, _| f.status_shown().is_some()));
        frame.update(cx, |f, _| {
            let s = f.status.as_mut().unwrap();
            s.at -= NOTICE_SHOWN;
        });
        assert!(
            frame.read_with(cx, |f, _| f.status_shown().is_none()),
            "words go once stale"
        );
        frame.update(cx, |f, cx| f.report(Status::failure("Not saved"), cx));
        frame.update(cx, |f, _| {
            let s = f.status.as_mut().unwrap();
            s.at -= NOTICE_SHOWN * 10;
        });
        assert!(
            frame.read_with(cx, |f, _| f.status_shown().is_some()),
            "a failure stays until closed or replaced"
        );
        let at = frame.read_with(cx, |f, _| f.status.as_ref().unwrap().at);
        frame.update(cx, |f, cx| f.withdraw(at, cx));
        assert!(frame.read_with(cx, |f, _| f.status.is_none()));
    }
}
