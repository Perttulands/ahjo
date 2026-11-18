//! The one zoom rule on the open file (`ahjo_app_kit::zoom`): code, its
//! changes and rendered text are read bigger or smaller, kept for every
//! file; a picture steps through its own zoom.
use super::*;
use ahjo_app_kit::zoom::{self, Zoom};

/// The smallest and largest text a file is read at.
const MIN: f32 = 9.;
const MAX: f32 = 32.;

impl FilesPage {
    /// The size code and changes are drawn at.
    pub fn code_text(&self, cx: &gpui::App) -> f32 {
        self.host
            .setting::<crate::store::Kept>(cx)
            .unwrap_or_default()
            .text
            .map_or(size::TEXT, f32::from)
    }

    /// Zooms what the open file shows `n` steps: its picture, or its text.
    pub fn zoom_file(&mut self, z: Zoom, n: i32, cx: &mut Context<Self>) {
        let Some(o) = &self.opened else {
            return;
        };
        if matches!(o.show, Show::Picture { .. }) && !o.diff_open {
            match z {
                Zoom::Reset => self.set_zoom(None, cx),
                _ => {
                    for _ in 0..n.max(1) {
                        self.step_zoom(z == Zoom::In, cx);
                    }
                }
            }
            return;
        }
        let at = self.code_text(cx);
        let to = match z {
            Zoom::Reset => (at != size::TEXT).then_some(size::TEXT),
            Zoom::In => zoom::step(at, n, 1., MIN, MAX),
            Zoom::Out => zoom::step(at, -n, 1., MIN, MAX),
        };
        let Some(to) = to else {
            return;
        };
        let mut kept: crate::store::Kept = self.host.setting(cx).unwrap_or_default();
        kept.text = (to != size::TEXT).then_some(to as u8);
        let _ = self.host.set_setting(&kept, cx);
        for code in [self.code.clone(), self.pretty_code.clone()] {
            code.update(cx, |c, cx| c.set_text_size(to, cx));
        }
        let reset = if to == size::TEXT {
            ""
        } else {
            " · Ctrl+0 resets"
        };
        self.host.toast(&format!("Text {to} px{reset}"), cx);
        cx.notify();
    }

    /// Over the open file: Ctrl+wheel zooms it.
    pub(super) fn zoom_catch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity().downgrade();
        zoom::catch(self.zoom_wheel.clone(), move |n, _, cx| {
            let _ = me.update(cx, |p, cx| {
                let z = if n > 0 { Zoom::In } else { Zoom::Out };
                p.zoom_file(z, n.abs(), cx)
            });
        })
    }
}
