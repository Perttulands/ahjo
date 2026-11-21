//! Views on the number keys and the layout's own menus: keeping and
//! recalling an arrangement (Alt+Shift+1 to 9, as an RTS keeps control
//! groups), the views on the tab bar, and what a window's menu offers for
//! the layout: split, move, close.

use super::page::{Act, MenuItem, Terminals};
use super::surface;
use crate::store::layout::Side;
use crate::store::views::VIEW_KEYS;
use crate::store::workspace::{Arrange, MAX_WINDOWS, Workspace};
use crate::theme::{size, theme};
use ahjo_app_kit::widgets::kit;
use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, Pixels, Point, Window, anchored, deferred,
    div, point, prelude::*, px,
};

/// The chord a view is kept on, as menus show it.
pub fn view_chord(key: u8) -> String {
    format!("Alt+Shift+{key}")
}

/// The rows of a window's menu that act on the layout: split it, move it
/// across an edge, close it.
pub fn layout_rows(ws: &Workspace, i: usize) -> Vec<MenuItem> {
    let n = ws.windows().len();
    let full = n >= MAX_WINDOWS;
    let sides = [
        ("Right", Side::Right),
        ("Below", Side::Bottom),
        ("Left", Side::Left),
        ("Above", Side::Top),
    ];
    let split = if full {
        MenuItem::note("Split").refused("Eight windows is the most a tab holds.")
    } else {
        let sides = sides
            .iter()
            .map(|(label, side)| MenuItem::new(*label, Act::Split(i, *side)))
            .collect();
        MenuItem::sub("Split", None, sides)
    };
    let mut rows = vec![split];
    let moves: Vec<MenuItem> = [
        ("Left", Side::Left),
        ("Right", Side::Right),
        ("Up", Side::Top),
        ("Down", Side::Bottom),
    ]
    .into_iter()
    .filter_map(|(label, side)| {
        let to = ws.tab().layout.neighbor(i, side)?;
        Some(MenuItem::new(label, Act::Swap(i, to)))
    })
    .collect();
    if !moves.is_empty() {
        rows.push(MenuItem::sub("Move", None, moves).hint("Trades places with the window there"));
    }
    let close = MenuItem::new("Close window", Act::CloseWindow(i));
    let held = ws.windows().get(i).is_some_and(|w| !w.is_empty());
    rows.push(match (n > 1, held) {
        (false, _) => close.refused("A tab keeps one window."),
        (true, true) => close.hint("Its sessions keep running"),
        (true, false) => close,
    });
    rows
}

/// The tab menu's rows for its layout: how its windows are arranged.
pub fn arrange_rows() -> Vec<MenuItem> {
    vec![
        MenuItem::new("Side by side", Act::Arrange(Arrange::Row)),
        MenuItem::new("Stacked", Act::Arrange(Arrange::Column)),
        MenuItem::new("Grid", Act::Arrange(Arrange::Grid)),
        MenuItem::rule(),
        MenuItem::new("Even sizes", Act::EvenOut),
    ]
}

/// The views menu: each kept view to recall, then the numbers to keep the
/// arrangement on screen on, then renaming and deleting.
pub fn views_menu(ws: &Workspace) -> Vec<MenuItem> {
    let mut rows: Vec<MenuItem> = ws
        .views
        .iter()
        .map(|v| {
            MenuItem::new(format!("{}  {}", v.key, v.name), Act::RecallView(v.key))
                .chord(&view_chord(v.key))
        })
        .collect();
    if rows.is_empty() {
        rows.push(MenuItem::note(
            "No views yet. A view keeps this tab's windows and sessions on Alt+Shift+1 to 9.",
        ));
    }
    let keep = (1..=VIEW_KEYS)
        .map(|k| match ws.view(k) {
            Some(v) => MenuItem::new(format!("{k}  replace {}", v.name), Act::SaveView(k)),
            None => MenuItem::new(format!("{k}  free"), Act::SaveView(k)).chord(&view_chord(k)),
        })
        .collect();
    rows.push(MenuItem::rule());
    rows.push(MenuItem::sub("Keep this tab as view", None, keep));
    if !ws.views.is_empty() {
        let rename = (ws.views.iter())
            .map(|v| MenuItem::new(format!("{}  {}", v.key, v.name), Act::RenameView(v.key)))
            .collect();
        let delete = (ws.views.iter())
            .map(|v| {
                MenuItem::new(format!("{}  {}", v.key, v.name), Act::DeleteView(v.key)).danger()
            })
            .collect();
        rows.push(MenuItem::sub("Rename view", None, rename));
        rows.push(MenuItem::sub("Delete view", None, delete));
    }
    rows
}

/// One view's own menu, from its chip on the tab bar.
fn view_menu(key: u8) -> Vec<MenuItem> {
    vec![
        MenuItem::new("Show", Act::RecallView(key)).chord(&view_chord(key)),
        MenuItem::new("Replace with this tab", Act::SaveView(key)),
        MenuItem::new("Rename…", Act::RenameView(key)),
        MenuItem::rule(),
        MenuItem::new("Delete view", Act::DeleteView(key)).danger(),
    ]
}

impl Terminals {
    /// A view's number key was pressed: a kept view comes back; a free
    /// number keeps the arrangement on screen, named after its sessions.
    pub fn view_key(&mut self, key: u8, cx: &mut Context<Self>) {
        if self.settings.workspace.view(key).is_some() {
            self.recall_view(key, cx);
        } else {
            self.keep_view(key, None, cx);
        }
    }

    pub fn keep_view(&mut self, key: u8, name: Option<&str>, cx: &mut Context<Self>) {
        let replaced = self.settings.workspace.view(key).is_some();
        if let Some(name) = self.settings.workspace.save_view(key, name) {
            let text = if replaced {
                format!("View {key} ({name}) now keeps this tab")
            } else {
                format!(
                    "Kept this tab as view {key} ({name}): {} brings it back",
                    view_chord(key)
                )
            };
            self.report(&text, false, None);
            self.save(cx);
        }
        cx.notify();
    }

    pub fn recall_view(&mut self, key: u8, cx: &mut Context<Self>) {
        if self.settings.workspace.view(key).is_none() {
            return;
        }
        for p in self.window_panes().into_iter().flatten() {
            self.fleet.release(&p.node, p.pane);
        }
        self.histories.clear();
        if let Some(name) = self.settings.workspace.recall_view(key) {
            self.report(&format!("View {key}: {name}"), false, None);
        }
        self.host.show(cx);
        self.sync(cx);
        self.save(cx);
        cx.notify();
    }

    /// The kept views on the tab bar, beside the Sessions tabs: a click
    /// shows one, a right click opens its menu. With none kept, one quiet
    /// word opens the views menu.
    pub(super) fn render_views(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let mut strip = div()
            .id("views")
            .debug_selector(|| "views".into())
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .pl(px(6.))
            .ml(px(4.))
            .border_l_1()
            .border_color(t.divider);
        for v in &self.settings.workspace.views {
            let key = v.key;
            strip = strip.child(
                div()
                    .id(("view", key as usize))
                    .debug_selector(move || format!("view-{key}"))
                    .flex_none()
                    .max_w(px(160.))
                    .h(px(24.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(size::RADIUS))
                    .cursor_pointer()
                    .text_size(px(size::SMALL))
                    .text_color(t.text2)
                    .hover(|d| d.bg(t.surface).text_color(t.text))
                    .child(
                        div()
                            .flex_none()
                            .px(px(4.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(t.divider)
                            .text_color(t.dim)
                            .child(key.to_string()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(v.name.clone()),
                    )
                    .tooltip(kit::tip(format!(
                        "{} · right-click to replace, rename or delete",
                        view_chord(key)
                    )))
                    .on_click(cx.listener(move |s, _, _, cx| s.recall_view(key, cx)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                            s.menu(view_menu(key), e.position, window, cx);
                        }),
                    ),
            );
        }
        // With no views saved there is nothing to recall; saving the
        // first is in the tab's menu, so no word stands here for it.
        if self.settings.workspace.views.is_empty() {
            return strip.into_any_element();
        }
        strip
            .child(
                div()
                    .id("views-menu")
                    .debug_selector(|| "views-menu".into())
                    .flex_none()
                    .px(px(6.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .hover(|d| d.text_color(t.text))
                    // No tooltip: it would land over the menu a quick
                    // click opens, and the menu names the keys itself.
                    .child("…")
                    .on_click(cx.listener(|s, e: &gpui::ClickEvent, window, cx| {
                        let rows = views_menu(&s.settings.workspace);
                        s.menu(rows, e.position(), window, cx);
                    })),
            )
            .into_any_element()
    }

    /// Naming a view, in a panel where the menu stood.
    pub(super) fn render_view_name(
        &mut self,
        at: Point<Pixels>,
        key: u8,
        rename: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx).clone();
        let note = if rename {
            "Enter renames · Esc cancels".to_string()
        } else {
            format!(
                "Enter keeps this tab's windows and sessions · {} brings them back · Esc cancels",
                view_chord(key)
            )
        };
        deferred(
            self.scrim(surface::VIEW_NAME, cx).child(
                anchored()
                    .position(point(at.x, at.y))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        kit::panel(&t)
                            .w(px(300.))
                            .p(px(10.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(kit::label(&t, format!("View {key}")))
                            .child(self.rename_field.clone())
                            .child(kit::note(&t, note, Some(t.dim))),
                    ),
            ),
        )
        .with_priority(10)
        .into_any_element()
    }

    /// The focused window's menu, from the keyboard (Alt+M): its session's
    /// with the layout's rows, or the layout's alone.
    pub fn focused_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ws = &self.settings.workspace;
        let i = ws.focus;
        let items = match ws.shown(i).map(|b| b.key.clone()) {
            Some(key) => self.session_menu(i, &key),
            None => layout_rows(ws, i),
        };
        let at = self.tile_bounds.borrow()[i]
            .map(|b| b.origin + point(px(24.), px(24.)))
            .unwrap_or_else(|| window.mouse_position());
        // The keyboard is in the menu from its first row.
        self.menu_at = at;
        let rows = items.into_iter().map(|i| i.row(&self.me)).collect();
        self.host.menu_keyed(rows, at, cx);
    }
}
