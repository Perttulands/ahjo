//! Motion between pages and themes: the new page or theme comes up out of
//! a veil of the background that fades at once. Nothing waits on it: the
//! new content is drawn and clickable from the first frame, the veil never
//! takes the mouse, and once it has faded it is no longer drawn.

use gpui::{Animation, AnimationExt, AnyElement, Hsla, div, ease_out_quint, prelude::*};
use parvi_app_kit::theme::{Direction, Theme, motion};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Veils {
    seen: Option<(Option<usize>, Direction)>,
    n: usize,
    page: Option<Veil>,
    theme: Option<Veil>,
}

#[derive(Clone, Copy)]
struct Veil {
    n: usize,
    colour: Hsla,
    at: Instant,
}

impl Veils {
    /// Notes the page shown and the theme drawn; a change raises a veil.
    pub(crate) fn note(&mut self, shown: Option<usize>, t: &Theme) {
        if let Some((page, direction)) = self.seen {
            if direction != t.direction {
                self.n += 1;
                self.theme = Some(Veil {
                    n: self.n,
                    colour: Theme::new(direction).bg,
                    at: Instant::now(),
                });
            } else if page != shown {
                self.n += 1;
                self.page = Some(Veil {
                    n: self.n,
                    colour: t.bg,
                    at: Instant::now(),
                });
            }
        }
        self.seen = Some((shown, t.direction));
    }

    /// The veil over the page, while it fades.
    pub(crate) fn page(&mut self) -> Option<AnyElement> {
        draw(&mut self.page, motion::PAGE_IN, motion::PAGE_VEIL)
    }

    /// The veil over the whole window after a theme switch, while it fades.
    pub(crate) fn theme(&mut self) -> Option<AnyElement> {
        draw(&mut self.theme, motion::THEME_IN, 1.)
    }
}

fn draw(veil: &mut Option<Veil>, ms: u64, from: f32) -> Option<AnyElement> {
    let v = (*veil)?;
    let length = Duration::from_millis(ms);
    if v.at.elapsed() >= length {
        *veil = None;
        return None;
    }
    Some(
        div()
            .absolute()
            .inset_0()
            .bg(v.colour)
            .with_animation(
                ("veil", v.n),
                Animation::new(length).with_easing(ease_out_quint()),
                move |d, x| d.opacity(from * (1. - x)),
            )
            .into_any_element(),
    )
}
