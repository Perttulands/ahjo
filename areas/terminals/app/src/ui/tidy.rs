//! The one quiet offer to tidy up, at the top of the Sessions rail: ended
//! sessions still in windows, and sessions idle a day or more. It names
//! what there is; its menu clears the ended tags, shows the idle sessions
//! picked in the rail, or kills them on a second click. Waved away, it
//! stays quiet until something new joins (`store::tidy`).

use super::page::{Act, MenuItem, Terminals, now_ms};
use crate::store::sessions::{Key, ago};
use crate::store::tidy::{self, Tidy};
use crate::store::workspace::{Binding, TileState};
use crate::theme::{size, theme};
use ahjo_app_kit::widgets::kit::{self, Kind};
use gpui::{AnyElement, Context, div, prelude::*, px};

impl Terminals {
    /// What could be cleared now.
    pub fn tidy(&self) -> Tidy {
        let ended = |k: &Key| {
            matches!(
                self.fleet.tile(Some(&Binding {
                    key: k.clone(),
                    pane: None,
                })),
                TileState::Ended(_)
            ) && !self.restarting.contains(k)
        };
        tidy::find(&self.settings.workspace, ended, &self.fleet.all, now_ms())
    }

    /// The offer's menu: each thing it can do, a live session's only on a
    /// second click.
    pub fn tidy_menu(&self, t: &Tidy) -> Vec<MenuItem> {
        let mut items = vec![];
        if !t.ended.is_empty() {
            items.push(
                MenuItem::new(
                    match t.ended.len() {
                        1 => "Clear the ended session from its window".to_string(),
                        n => format!("Clear {n} ended sessions from their windows"),
                    },
                    Act::ClearEnded(t.ended.clone()),
                )
                .hint(names(&t.ended)),
            );
        }
        if !t.idle.is_empty() {
            if !items.is_empty() {
                items.push(MenuItem::rule());
            }
            for (k, quiet) in &t.idle {
                items.push(
                    MenuItem::new(format!("Peek {}", k.name), Act::Peek(k.clone()))
                        .chord(&format!("idle {}", ago(*quiet))),
                );
            }
            let keys: Vec<Key> = t.idle.iter().map(|(k, _)| k.clone()).collect();
            items.push(MenuItem::new(
                "Pick them in the list",
                Act::PickKeys(keys.clone()),
            ));
            items.push(
                MenuItem::new(
                    format!("Kill {}", count(keys.len(), "idle session")),
                    Act::KillKeys(keys),
                )
                .danger(),
            );
        }
        items.push(MenuItem::rule());
        items.push(MenuItem::new("Not now", Act::TidyLater).hint("Asks again when there is more"));
        items
    }

    /// The offer, when there is something new to tidy.
    pub(super) fn render_tidy(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tidy = self.tidy();
        if tidy.is_empty() || !tidy.news(&self.settings.tidy_seen) {
            return None;
        }
        let t = theme(cx).clone();
        let menu = self.tidy_menu(&tidy);
        Some(
            div()
                .id("tidy")
                .debug_selector(|| "tidy".into())
                .mx(px(8.))
                .mb(px(6.))
                .px(px(8.))
                .py(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(size::RADIUS))
                .border_1()
                .border_color(t.divider)
                .text_size(px(size::SMALL))
                .text_color(t.text2)
                .child(div().flex_1().min_w_0().child(tidy.line()))
                .child(
                    kit::button(&t, "tidy-open", "Tidy…", Kind::Quiet)
                        .debug_selector(|| "tidy-open".into())
                        .on_click(cx.listener(move |s, e: &gpui::ClickEvent, window, cx| {
                            s.menu(menu.clone(), e.position(), window, cx);
                        })),
                )
                .child(
                    kit::button(&t, "tidy-later", "×", Kind::Quiet)
                        .debug_selector(|| "tidy-later".into())
                        .tooltip(kit::tip("Not now: asks again when there is more"))
                        .on_click(
                            cx.listener(|s, _, window, cx| s.act(Act::TidyLater, window, cx)),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

fn names(keys: &[Key]) -> String {
    let names: Vec<&str> = keys.iter().map(|k| k.name.as_str()).collect();
    names.join(", ")
}
