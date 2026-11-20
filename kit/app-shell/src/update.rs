//! Swarm's part in updating itself (`ahjo_app_kit::update`): it asks the
//! node when it starts, when the window comes forward and when the link
//! opens or is refused, shows Update at the end of the tab bar, and when
//! the node refused this app for speaking another protocol, offers the
//! update where the refusal is said. A restart waits for unsaved work as
//! closing the window does.

use crate::shell::Shell;
use ahjo_app_kit::host::Status;
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::theme::Theme;
use ahjo_app_kit::update::{Badge, State, Tone, Updates};
use ahjo_app_kit::widgets::kit;
use gpui::{AnyElement, AppContext, Context, Entity, Window, prelude::*, px};
use std::rc::Rc;

impl Shell {
    /// Offers the updates of this app, whose build is `own`, when it runs
    /// from an install (`install.json` beside it); otherwise nothing.
    pub fn offer_updates(
        &mut self,
        own: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let updates = cx.new(|_| Updates::new(own));
        if !updates.read(cx).installed() {
            return;
        }
        let me = cx.weak_entity();
        updates.update(cx, |u, _| {
            u.set_guard(Rc::new(move |window, cx| {
                me.update(cx, |s, cx| s.may_close(window, cx))
                    .unwrap_or(true)
            }))
        });
        let mut link = None;
        self.keep(
            cx.observe_in(&self.hub.clone(), window, move |s, hub, _, cx| {
                let here = hub.read(cx).fleet.here();
                let now = (here.state, here.hello.as_ref().map(|h| h.version.clone()));
                if link.as_ref() != Some(&now) {
                    let changed = matches!(now.0, LinkState::Open | LinkState::Refused);
                    link = Some(now);
                    if changed && let Some(u) = &s.updates {
                        u.update(cx, |u, cx| u.check(cx));
                    }
                }
            }),
        );
        self.keep(cx.observe_window_activation(window, |s, window, cx| {
            if window.is_window_active()
                && let Some(u) = &s.updates
            {
                u.update(cx, |u, cx| u.check(cx));
            }
        }));
        self.keep(cx.observe(&updates, |s, updates, cx| {
            s.refusal_offers(&updates, cx);
            cx.notify();
        }));
        updates.update(cx, |u, cx| u.check(cx));
        self.updates = Some(updates);
    }

    /// A node that refused this app is said in the status line with
    /// Reconnect; when the node hands out the build it speaks, Update
    /// takes its place.
    fn refusal_offers(&mut self, updates: &Entity<Updates>, cx: &mut Context<Self>) {
        if !matches!(updates.read(cx).state, State::Offered(_)) {
            return;
        }
        let refused = self.hub.read(cx).fleet.here();
        if refused.state != LinkState::Refused {
            return;
        }
        let why = refused.reason.clone().unwrap_or_default();
        let updates = updates.clone();
        self.hub.update(cx, |h, cx| {
            h.report(
                Status::failure(why).action("Update", move |window, cx| {
                    updates.update(cx, |u, cx| u.act(window, cx))
                }),
                cx,
            )
        });
    }

    /// The update's words at the end of the tab bar, and their width.
    pub(crate) fn update_badge(&self, cx: &Context<Self>) -> Option<Badge> {
        self.updates.as_ref()?.read(cx).badge()
    }

    pub(crate) fn render_update(
        &self,
        t: &Theme,
        badge: Badge,
        pad: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let updates = self.updates.clone();
        let colour = match badge.tone {
            Tone::Action => t.accent,
            Tone::Busy => t.dim,
            Tone::Failed => t.error,
        };
        kit::tab(t, "update", false, cx)
            .debug_selector(|| "update".into())
            .flex_none()
            .px(px(pad))
            .text_color(colour)
            .tooltip(kit::tip(badge.tip))
            .child(badge.words)
            .when(badge.acts, |d| {
                d.on_click(move |_, window, cx| {
                    if let Some(u) = &updates {
                        u.update(cx, |u, cx| u.act(window, cx));
                    }
                })
            })
            .into_any_element()
    }
}
