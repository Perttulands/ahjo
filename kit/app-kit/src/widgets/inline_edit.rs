//! A name renamed where it is shown (DESIGN.md, Act where it is), by one
//! rule on every page: Enter keeps what was typed, Escape puts the name
//! back, and the keyboard going anywhere else keeps what was typed, as
//! Enter does. Each edit ends once, so a page hears one of
//! [`InlineEditEvent::Committed`] or [`InlineEditEvent::Reverted`] however
//! the edit ended (an Escape that also moves the keyboard away is a
//! revert, not a revert and then a commit).
//!
//! The page owns where the field stands and what a name means: it starts
//! the edit with [`InlineEdit::start`], draws the view while
//! [`InlineEdit::is_editing`], and acts on the event. A rename the page
//! refuses (an empty or taken name) goes on with [`InlineEdit::resume`].
//! A rename standing on the app's stack as a layer gets Escape from the
//! shell, which closes the layer: its page reverts it there.

use crate::widgets::field::{self, FieldEvent, TextField};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Render,
    Subscription, Window, div, prelude::*,
};

/// How an edit ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineEditEvent {
    /// Enter, or the keyboard went elsewhere: the text as typed.
    Committed(String),
    /// Escape: the name stays as it was.
    Reverted,
}

pub struct InlineEdit {
    field: Entity<TextField>,
    is_editing: bool,
    _field: [Subscription; 2],
}

impl EventEmitter<InlineEditEvent> for InlineEdit {}

impl InlineEdit {
    pub fn new(placeholder: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new(cx, placeholder));
        let submit = cx.subscribe(&field, |edit, _, event: &FieldEvent, cx| {
            if matches!(event, FieldEvent::Submit) {
                edit.commit(cx);
            }
        });
        let focus = field.read(cx).focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |edit, _, cx| edit.commit(cx));
        Self {
            field,
            is_editing: false,
            _field: [submit, blur],
        }
    }

    /// Edits `name`: all of it selected, the keyboard in it.
    pub fn start(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set(name, true, cx));
        self.is_editing = true;
        field::focus(&self.field, window, cx);
        cx.notify();
    }

    /// An edit is open: the page draws it.
    pub fn is_editing(&self) -> bool {
        self.is_editing
    }

    /// What is typed now.
    pub fn text(&self, cx: &App) -> String {
        self.field.read(cx).text.clone()
    }

    /// Ends the edit keeping what was typed, as Enter does; nothing when
    /// none is open. For a page closing what the rename stands over.
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.is_editing) {
            cx.emit(InlineEditEvent::Committed(self.text(cx)));
            cx.notify();
        }
    }

    /// Ends the edit putting the name back, as Escape does; nothing when
    /// none is open.
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.is_editing) {
            cx.emit(InlineEditEvent::Reverted);
            cx.notify();
        }
    }

    /// The page refused what was committed (it says why): the edit goes
    /// on with the text as typed.
    pub fn resume(&mut self, cx: &mut Context<Self>) {
        self.is_editing = true;
        cx.notify();
    }
}

impl Focusable for InlineEdit {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.read(cx).focus_handle(cx)
    }
}

impl Render for InlineEdit {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .on_key_down(cx.listener(|edit, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape"
                    && !e.keystroke.modifiers.modified()
                    && edit.is_editing
                {
                    edit.revert(cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.field.clone())
    }
}
