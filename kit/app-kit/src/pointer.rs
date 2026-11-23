//! The button the pointer last pressed, so what it opens is not covered
//! by its own tooltip and a popup it opens stands beside it, clear of its
//! neighbours (every [`crate::widgets::kit::button`] reports itself here).

use gpui::{
    App, Bounds, DispatchPhase, Global, HitboxBehavior, MouseButton, MouseDownEvent, Pixels,
    Window, canvas, div, prelude::*,
};

/// The button last pressed, while the pointer has not left it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pressed {
    pub bounds: Option<Bounds<Pixels>>,
}

impl Global for Pressed {}

/// The bounds of the button last pressed, while the pointer is still on it.
pub fn pressed(cx: &App) -> Option<Bounds<Pixels>> {
    cx.try_global::<Pressed>().and_then(|p| p.bounds)
}

/// The bounds of the button last pressed, if `at` (where a popup was
/// asked for) is on it.
pub fn pressed_at(at: gpui::Point<Pixels>, cx: &App) -> Option<Bounds<Pixels>> {
    pressed(cx).filter(|b| b.contains(&at))
}

/// Whether a tooltip shown now would cover what its button just opened:
/// the pointer pressed a button and has not left it since. A pressed
/// button the pointer has left is forgotten here.
pub fn quiet(window: &Window, cx: &mut App) -> bool {
    match pressed(cx) {
        Some(b) if b.contains(&window.mouse_position()) => true,
        Some(_) => {
            cx.set_global(Pressed::default());
            false
        }
        None => false,
    }
}

/// An empty element inside a button, as large as it, that notes a press on
/// it and forgets it once the pointer has left.
pub fn watch() -> impl IntoElement {
    div().absolute().inset_0().child(
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            |bounds, hitbox, window, cx| {
                if pressed(cx) == Some(bounds) && !bounds.contains(&window.mouse_position()) {
                    cx.set_global(Pressed::default());
                }
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture
                        && e.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        cx.set_global(Pressed {
                            bounds: Some(bounds),
                        });
                    }
                });
            },
        )
        .size_full(),
    )
}
