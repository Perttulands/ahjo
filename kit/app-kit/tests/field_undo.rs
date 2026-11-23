//! Undo in a text field, as typed in a window: a word at a time, the
//! selection with it, redo by Ctrl+Shift+Z and Ctrl+Y, and the page's own
//! Ctrl+Z still the page's while the field does not have the keyboard.

use ahjo_app_kit::theme::Theme;
use ahjo_app_kit::widgets::field::{self, TextField};
use ahjo_testkit::app::click;
use gpui::{
    Entity, FocusHandle, KeyDownEvent, TestAppContext, VisualTestContext, Window, div, prelude::*,
};

/// A page with a field and an undo of its own, as Missions' board has.
struct Form {
    focus: FocusHandle,
    field: Entity<TextField>,
    page_undos: usize,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .size_full()
            .on_key_down(cx.listener(|form, e: &KeyDownEvent, _, _| {
                if e.keystroke.modifiers.control && e.keystroke.key == "z" {
                    form.page_undos += 1;
                }
            }))
            .child(
                div()
                    .debug_selector(|| "field".into())
                    .child(self.field.clone()),
            )
    }
}

fn shown(form: &Entity<Form>, cx: &mut VisualTestContext) -> String {
    form.read_with(cx, |form, cx| form.field.read(cx).text.clone())
}

#[gpui::test]
fn a_field_undoes_and_redoes_what_was_typed(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Theme::default());
        field::bind_keys(cx);
    });
    let (form, cx) = cx.add_window_view(|_, cx| Form {
        focus: cx.focus_handle(),
        field: cx.new(|cx| TextField::new(cx, "")),
        page_undos: 0,
    });
    cx.run_until_parked();
    click(cx, "field");
    cx.simulate_input("hello brave world");
    assert_eq!(shown(&form, cx), "hello brave world");

    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(shown(&form, cx), "hello brave ", "a word at a time");
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(shown(&form, cx), "hello ");
    cx.simulate_keystrokes("ctrl-shift-z");
    assert_eq!(shown(&form, cx), "hello brave ");
    cx.simulate_keystrokes("ctrl-y");
    assert_eq!(shown(&form, cx), "hello brave world");

    // A word deleted is a step of its own, and comes back with the caret
    // where it was.
    cx.simulate_keystrokes("ctrl-backspace");
    assert_eq!(shown(&form, cx), "hello brave ");
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(shown(&form, cx), "hello brave world");
    cx.simulate_input("!");
    assert_eq!(shown(&form, cx), "hello brave world!", "typed at the end");

    // Typing over a selection undoes to the selection.
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("x");
    assert_eq!(shown(&form, cx), "x");
    cx.simulate_keystrokes("ctrl-z");
    cx.simulate_input("y");
    assert_eq!(shown(&form, cx), "y", "the whole text was selected again");
    cx.simulate_keystrokes("ctrl-z ctrl-z");
    assert_eq!(shown(&form, cx), "hello brave world");
    assert_eq!(
        form.read_with(cx, |form, _| form.page_undos),
        0,
        "the field kept its Ctrl+Z"
    );

    // Text the page puts in is a new draft: undo does not go behind it.
    form.update(cx, |form, cx| {
        form.field
            .update(cx, |field, cx| field.set("saved", false, cx))
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(shown(&form, cx), "saved");

    // Away from the field, Ctrl+Z is the page's.
    cx.update(|window, cx| form.update(cx, |form, cx| window.focus(&form.focus, cx)));
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(form.read_with(cx, |form, _| form.page_undos), 1);
    assert_eq!(shown(&form, cx), "saved");
}
