//! What the layouts add to reading a file: the path over it as steps to
//! walk, the changes counted and stepped through, who works there, the
//! outline of a long document, the find opened over the file (Reader),
//! what changed grouped by where (Review) and a folder shown as a place.
use super::*;
use crate::store::{listing, who};
use ahjo_app_kit::theme::alpha;
use ahjo_app_kit::widgets::markdown::{self, Block};
use ahjo_app_kit::widgets::select;
use gpui::{Bounds, Pixels, Point, point};

/// The find opened over the file: its width, and its top from the page's.
const PALETTE_W: f32 = 640.;
const PALETTE_TOP: f32 = 56.;
/// The share of the page's height the find's list takes at most.
const PALETTE_H: f32 = 0.6;
/// The outline beside a long document: its width and the gap before it.
const OUTLINE_W: f32 = 220.;
/// The narrowest the text column gets to leave room for the outline.
const TEXT_MIN: f32 = 640.;
/// Headings deeper than this stay out of the outline.
const OUTLINE_DEPTH: u8 = 3;
/// An outline only for a document with this many headings.
const OUTLINE_MIN: usize = 3;
/// Files shown under a group in Review before the rest are counted.
const PER_GROUP: usize = 5;
/// A repository's uncommitted files listed at most.
const UNCOMMITTED: usize = 500;
/// Changed files a place shows.
const PLACE_RECENT: usize = 12;
/// The documents a place shows first when it has them.
const KEY_DOCS: [&str; 6] = [
    "README.md",
    "AGENTS.md",
    "VISION.md",
    "DESIGN.md",
    "ARCHITECTURE.md",
    "CLAUDE.md",
];
/// The live dot before a session's name.
const DOT: f32 = 6.;

/// Review's list: what changed, grouped under the folder a session works
/// in (or the project under the root), the group changed last first.
pub(super) fn review_lines(p: &FilesPage) -> Vec<Line> {
    let note = |text: &str, error: bool| Line::Note {
        text: text.into(),
        depth: 0,
        error,
    };
    let recent: &[Changed] = match &p.recent {
        Some(Ok(r)) => r,
        _ => &[],
    };
    let mut out = vec![];
    // What git says is not committed, a repository at a time: those a
    // session works in first.
    let mut repos: Vec<_> = p
        .repos
        .iter()
        .filter(|(_, f)| {
            f.iter()
                .any(|f| !listing::editor_scratch(paths::base_name(&f.path)))
        })
        .collect();
    repos.sort_by_key(|(repo, _)| (!p.worked_in(repo), repo.to_string()));
    for (repo, files) in &repos {
        out.push(Line::Group((*repo).clone(), 0));
        let mut files: Vec<_> = files
            .iter()
            .filter(|f| !listing::editor_scratch(paths::base_name(&f.path)))
            .collect();
        files.sort_by_key(|f| (f.state != 'C', &f.path));
        for f in files.iter().take(UNCOMMITTED) {
            out.push(Line::Recent(
                Changed {
                    path: f.path.clone(),
                    modified: 0,
                    files: None,
                },
                true,
            ));
        }
        if files.len() > UNCOMMITTED {
            out.push(Line::Note {
                text: format!("{} more not shown", files.len() - UNCOMMITTED),
                depth: 1,
                error: false,
            });
        }
    }
    // Then what changed lately anywhere git does not already account for.
    let mut groups: Vec<(String, u64, Vec<&Changed>)> = vec![];
    for c in recent {
        // Also a folder line that stands for files of a repository listed.
        if repos.iter().any(|(repo, _)| {
            paths::inside(repo, &c.path) || (c.files.is_some() && paths::inside(&c.path, repo))
        }) {
            continue;
        }
        let root = paths::root_of(&p.tree.roots, &c.path).map(|r| r.path.as_str());
        let folder = who::group_of(&p.live, &p.tree.node, root, &c.path);
        match groups.iter_mut().find(|g| g.0 == folder) {
            Some(g) => g.2.push(c),
            None => groups.push((folder, c.modified, vec![c])),
        }
    }
    for (folder, newest, files) in groups {
        let open = p.place.as_deref() == Some(folder.as_str()) || p.recent_more;
        out.push(Line::Group(folder, newest));
        let shown = if open { files.len() } else { PER_GROUP };
        for c in files.iter().take(shown) {
            out.push(Line::Recent((*c).clone(), true));
        }
        if files.len() > shown {
            out.push(Line::Note {
                text: format!("{} more here", files.len() - shown),
                depth: 1,
                error: false,
            });
        }
    }
    if out.is_empty() {
        return match &p.recent {
            None => vec![note("Reading\u{2026}", false)],
            Some(Err(why)) => vec![note(why, true)],
            Some(Ok(_)) => vec![note("Nothing changed lately", false)],
        };
    }
    out
}

impl FilesPage {
    /// A group's line in Review: the folder, who works there, its newest
    /// change.
    pub(super) fn group_row(
        &self,
        folder: String,
        newest: u64,
        picked: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = paths::shown(&self.tree.roots, &folder);
        let here = self.place.as_deref() == Some(folder.as_str());
        let working = self.sessions_at(&folder);
        let who = match working.as_slice() {
            [] => None,
            [one] => Some(one.name.clone()),
            [one, rest @ ..] => Some(format!("{} +{}", one.name, rest.len())),
        };
        let room = self.row_letters(0.).saturating_sub(AGE + 1);
        let selector = format!("group-{}", paths::base_name(&folder));
        let path = folder.clone();
        pane::row(t, SharedString::from(format!("group-{folder}")), picked)
            .h(px(size::ROW))
            .debug_selector(move || selector.clone())
            .tooltip(kit::tip(format!(
                "{folder}: who works here and what changed"
            )))
            .when(here && !picked, |r| r.text_color(t.text))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(size::HEAD_GAP))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(pane::middle(&name, room)),
                    )
                    .children(who.map(|w| {
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(DOT))
                            .text_size(px(size::SMALL))
                            .text_color(t.text2)
                            .child(live_dot(t))
                            .child(div().overflow_hidden().text_ellipsis().child(w))
                    })),
            )
            .child(pane::meta(
                t,
                match self.repos.get(&path) {
                    // A repository's line counts what is not committed.
                    Some(files) if newest == 0 => format!(
                        "{} uncommitted",
                        files
                            .iter()
                            .filter(|f| !listing::editor_scratch(paths::base_name(&f.path)))
                            .count()
                    ),
                    _ => format::ago(newest),
                },
                None,
            ))
            .on_mouse_down(
                MouseButton::Right,
                self.menu_at(Pick::Group(path.clone()), path.clone(), true, cx),
            )
            .on_click(cx.listener(move |p, _, w, cx| {
                p.set_pick(Pick::Group(path.clone()), cx);
                p.activate(w, cx);
            }))
            .into_any_element()
    }

    /// The path over the file (or place) as steps: each opens its folder's
    /// menu. A long path keeps its root and its nearest folders.
    pub(super) fn crumbs(&self, folder: &str, t: &Theme, cx: &mut Context<Self>) -> Div {
        let trail = match paths::root_of(&self.tree.roots, folder) {
            Some(root) => paths::crumbs(root, folder),
            None => vec![paths::Crumb {
                name: folder.to_owned(),
                path: folder.to_owned(),
            }],
        };
        // Letters for the folders: what the name and the actions leave.
        let room = self.name_room.get().map_or(usize::MAX, letters);
        let mut kept: Vec<usize> = (0..trail.len()).collect();
        let len = |kept: &[usize]| -> usize {
            kept.iter()
                .map(|&i| trail[i].name.chars().count() + 3)
                .sum()
        };
        while kept.len() > 2 && len(&kept) > room / 2 {
            kept.remove(1);
        }
        // The folders give way before the name: they clip, it stays.
        let mut row = div().flex().items_center().min_w_0().overflow_hidden();
        let mut last = 0;
        for (n, &i) in kept.iter().enumerate() {
            if i > last + 1 || (n > 0 && i != last + 1) {
                row = row.child(
                    div()
                        .flex_none()
                        .px(px(2.))
                        .text_color(t.dim)
                        .child("\u{2026}/"),
                );
            }
            last = i;
            let crumb = &trail[i];
            let path = crumb.path.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("crumb-{}", crumb.path)))
                    .debug_selector({
                        let id = format!("crumb-{}", crumb.name);
                        move || id.clone()
                    })
                    .px(px(2.))
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .rounded(px(size::RADIUS))
                    .text_color(t.dim)
                    .cursor_pointer()
                    .hover(|s| s.text_color(t.text).bg(t.raised))
                    .tooltip(kit::tip(format!("{}: what is in it", crumb.path)))
                    .child(crumb.name.clone())
                    .on_click(cx.listener(move |p, e: &gpui::ClickEvent, _, cx| {
                        p.crumb_menu(path.clone(), e.position(), cx);
                        cx.stop_propagation();
                    })),
            );
            row = row.child(div().flex_none().text_color(t.dim).child("/"));
        }
        row
    }

    /// A step's menu: the folder itself, then what is in it.
    fn crumb_menu(&mut self, folder: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        use ahjo_app_kit::menu::MenuRow;
        let me = self.me.clone();
        let here = folder.clone();
        let tree = self.side == Side::Folders;
        let mut rows = vec![
            MenuRow::new(
                if tree {
                    "Show in the list"
                } else {
                    "Show this folder"
                },
                move |_, cx| {
                    let _ = me.update(cx, |p, cx| {
                        if p.side == Side::Folders {
                            p.show_in_tree(&here, cx);
                        } else {
                            p.show_place(here.clone(), cx);
                        }
                    });
                },
            ),
            MenuRow::rule(),
        ];
        match self.tree.listings.get(&folder) {
            None => {
                self.list(folder.clone(), cx);
                rows.push(MenuRow::note("Reading\u{2026}"));
            }
            Some(entries) => {
                let shown = crate::store::listing::shown(entries);
                if shown.is_empty() {
                    rows.push(MenuRow::note("Empty"));
                }
                for e in shown.into_iter().take(40) {
                    let path = paths::join(&folder, &e.name);
                    let me = self.me.clone();
                    let is_folder = e.folder;
                    let label = if is_folder {
                        format!("{}/", e.name)
                    } else {
                        e.name.clone()
                    };
                    rows.push(MenuRow::new(label, move |w, cx| {
                        let _ = me.update(cx, |p, cx| {
                            if !is_folder {
                                p.open(path.clone(), w, cx);
                            } else if p.side == Side::Folders {
                                p.show_in_tree(&path, cx);
                            } else {
                                p.show_place(path.clone(), cx);
                            }
                        });
                    }));
                }
            }
        }
        self.host.menu(rows, at, cx);
    }

    /// What changed in the open file, said in the head: lines in and out
    /// (a click steps to the next change), or that it is new.
    pub(super) fn change_count(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let o = self.opened.as_ref()?;
        if o.editing {
            return None;
        }
        if let Some((prefix, other)) = self.rename_detail(&o.path) {
            let words = format!("{prefix}{other}");
            let selector = format!("files-{words}");
            return Some(
                pane::meta(t, words.clone(), Some(t.needs))
                    .id("files-renamed-file")
                    .max_w(gpui::relative(0.35))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .tooltip(kit::tip(words))
                    .debug_selector(move || selector.clone())
                    .into_any_element(),
            );
        }
        let c = o.changes.as_ref()?;
        let said = |words: &'static str, selector: &'static str| {
            pane::meta(t, words, Some(t.error))
                .debug_selector(move || selector.into())
                .into_any_element()
        };
        if matches!(o.show, Show::Gone) {
            return Some(said("Deleted, not committed", "files-gone-file"));
        }
        if self.states.get(&o.path) == Some(&'C') {
            return Some(said("Merge conflict", "files-conflict-file"));
        }
        if c.new_file {
            return Some(
                pane::meta(t, "New, not committed", Some(t.done))
                    .debug_selector(|| "files-new-file".into())
                    .into_any_element(),
            );
        }
        let n = c.count();
        let tip = format!(
            "{n} {} since the last commit. N steps to the next, Shift+N back",
            if n == 1 { "change" } else { "changes" }
        );
        Some(
            div()
                .id("files-change-count")
                .debug_selector(|| "files-change-count".into())
                .flex_none()
                .flex()
                .gap(px(size::HEAD_GAP / 2.))
                .px(px(size::HEAD_GAP / 2.))
                .rounded(px(size::RADIUS))
                .text_size(px(size::SMALL))
                .cursor_pointer()
                .hover(|s| s.bg(t.raised))
                .tooltip(kit::tip(tip))
                .child(div().text_color(t.done).child(format!("+{}", c.added)))
                .child(
                    div()
                        .text_color(t.error)
                        .child(format!("\u{2212}{}", c.removed)),
                )
                .on_click(cx.listener(|p, _, _, cx| p.step_change(true, cx)))
                .into_any_element(),
        )
    }

    /// Who works where the file is: the nearest live session, a click
    /// away.
    pub(super) fn provenance(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let working = self.sessions_at(path);
        let first = (*working.first()?).clone();
        let more = working.len() - 1;
        let tip = format!(
            "{} works in {}{}. Click to go to it",
            first.name,
            first.cwd.clone().unwrap_or_default(),
            if more > 0 {
                format!(", and {more} more here")
            } else {
                String::new()
            }
        );
        let name = first.name.clone();
        Some(
            div()
                .id("files-provenance")
                .debug_selector(|| "files-provenance".into())
                .flex_none()
                .flex()
                .items_center()
                .gap(px(DOT))
                .px(px(size::HEAD_GAP / 2.))
                .rounded(px(size::RADIUS))
                .text_size(px(size::SMALL))
                .text_color(t.text2)
                .cursor_pointer()
                .hover(|s| s.bg(t.raised).text_color(t.text))
                .tooltip(kit::tip(tip))
                .child(live_dot(t))
                .child(if more > 0 {
                    format!("{name} +{more}")
                } else {
                    name
                })
                .on_click(cx.listener(move |p, _, _, cx| p.reveal_session(&first, cx)))
                .into_any_element(),
        )
    }

    /// What leads the head: back and forth through the files opened, and
    /// in the Reader the find, which has no list to sit in.
    pub(super) fn head_lead(&self, t: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut lead = vec![];
        if self.side == Side::Hidden && !self.beside {
            lead.push(
                pane::act(t, "files-find", "Find")
                    .debug_selector(|| "files-find".into())
                    .tooltip(kit::tip(
                        "Find a file by name or by what is in it (Ctrl+P or /)",
                    ))
                    .on_click(cx.listener(|p, _, w, cx| p.find_anywhere(w, cx)))
                    .into_any_element(),
            );
        }
        lead
    }

    /// Rendered Markdown: one column centred, the changed blocks barred,
    /// its words selectable, and its outline beside it when there is room.
    pub(super) fn markdown_body(
        &self,
        t: &Theme,
        blocks: &[Block],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(o) = &self.opened else {
            return div().into_any_element();
        };
        let node = o.node.clone();
        let folder = paths::parent(&o.path);
        let me = cx.entity().downgrade();
        let on_link =
            std::rc::Rc::new(move |link: &str, window: &mut Window, cx: &mut gpui::App| {
                let _ = me.update(cx, |page, cx| {
                    page.follow_link(&node, &folder, link, window, cx)
                });
            });
        // Zoomed, the reading text and its measure grow together.
        let scale = self.code_text(cx) / size::TEXT;
        let mut zoomed = t.clone();
        zoomed.ty.read = (t.ty.read * scale).round();
        let look = markdown::Look {
            t: &zoomed,
            refs: false,
            on_link: Some(on_link),
            select: Some(self.md_select.clone()),
        };
        self.md_select.borrow_mut().begin();
        let marks = self.block_marks();
        let headings: Vec<(usize, u8, String)> = blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match b {
                Block::Heading(level, text) if *level <= OUTLINE_DEPTH => {
                    Some((i, *level, markdown::inline(text, false).text.to_string()))
                }
                _ => None,
            })
            .collect();
        let room = self.body_room.get().unwrap_or(0.);
        let outline =
            headings.len() >= OUTLINE_MIN && room >= TEXT_MIN * scale + OUTLINE_W + size::PAD;
        let selection = self.md_select.clone();
        let laid = self.md_blocks.clone();
        let go = self.md_go.clone();
        let scroll = self.md_scroll.clone();
        // Its pictures, each read the first time it is drawn; one still on
        // its way holds its place with its words.
        let md_path = o.path.clone();
        let md_node = o.node.clone();
        let mut pictures = std::collections::HashMap::new();
        for b in blocks {
            if let Block::Paragraph(p) = b
                && let Some((_, src)) = markdown::lone_picture(p)
                && let Some(state) = self.inline_picture(&md_node, &md_path, &src, cx)
            {
                pictures.insert(src, state);
            }
        }
        let most = size::READING * scale - 2. * size::PAD - size::HEAD_GAP;
        let draw = |alt: &str, src: &str| -> Option<AnyElement> {
            match pictures.get(src)? {
                crate::picture::Inline::Ready(image) => {
                    Some(FilesPage::inline_drawn(image.clone(), most, alt, t))
                }
                crate::picture::Inline::Reading => Some(
                    div()
                        .text_color(t.dim)
                        .child(format!("{alt} \u{2026}"))
                        .into_any_element(),
                ),
                crate::picture::Inline::Failed => None,
            }
        };
        let content =
            markdown::render_pictured(&look, "file-markdown", blocks, &marks, Some(&draw))
                .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, window, cx| {
                    selection.borrow_mut().laid_out();
                    *laid.borrow_mut() = bounds.clone();
                    let Some(i) = go.take() else {
                        return;
                    };
                    let Some(block) = bounds.get(i) else {
                        return;
                    };
                    let offset = scroll.offset();
                    let y = offset.y + scroll.bounds().top() - block.top() + px(GO_ROOM);
                    scroll.set_offset(point(offset.x, y.min(px(0.))));
                    window.defer(cx, |window, _| window.refresh());
                });
        let column = div()
            .flex_1()
            .min_w_0()
            .max_w(px(size::READING * scale))
            .p(px(size::PAD))
            // Room for the change bars left of the blocks.
            .pl(px(size::PAD + size::HEAD_GAP))
            .debug_selector(|| "files-markdown-content".into())
            .child(content);
        let scrolled = div()
            .id("files-markdown-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.md_scroll)
            .debug_selector(|| "files-markdown-rendered".into())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|p, _, w, cx| w.focus(&p.focus, cx)),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(column)
                    // The outline's room, so the text and it stand
                    // together in the middle.
                    .when(outline, |d| d.child(div().flex_none().w(px(OUTLINE_W)))),
            );
        let scrolled = select::track(scrolled, &self.md_select, cx.entity_id(), |_, _| {});
        div()
            .relative()
            .size_full()
            .child(measure(self.body_room.clone()))
            .child(scrolled)
            .when(outline, |d| {
                // What the text and the outline leave, either side of them;
                // zoomed, the text's measure is wider and the outline
                // stands further out.
                let text = (size::READING * scale).min(room - OUTLINE_W);
                let side = ((room - text - OUTLINE_W) / 2.).max(0.);
                d.child(
                    self.outline(t, &headings, &marks, blocks.len(), cx)
                        .right(px(side)),
                )
            })
            .into_any_element()
    }

    /// The headings of the document, the one being read lit, a change in a
    /// section marked beside its heading.
    fn outline(
        &self,
        t: &Theme,
        headings: &[(usize, u8, String)],
        marks: &[Option<ahjo_app_kit::widgets::code::Mark>],
        blocks: usize,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let top = self.md_scroll.bounds().top() + px(GO_ROOM + 1.);
        let laid = self.md_blocks.borrow();
        let reading = headings
            .iter()
            .rposition(|(i, _, _)| laid.get(*i).is_some_and(|b| b.top() <= top))
            .unwrap_or(0);
        let least = headings.iter().map(|h| h.1).min().unwrap_or(1);
        let mut list = div()
            .id("files-outline")
            .debug_selector(|| "files-outline".into())
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(OUTLINE_W))
            .pt(px(size::PAD))
            .pr(px(size::HEAD_PAD))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .text_size(px(size::SMALL));
        for (n, (i, level, text)) in headings.iter().enumerate() {
            let end = headings.get(n + 1).map_or(blocks, |h| h.0);
            let changed = marks
                .get(*i..end)
                .and_then(|m| m.iter().flatten().next().copied());
            let at = *i;
            let lit = n == reading;
            list = list.child(
                div()
                    .id(SharedString::from(format!("outline-{at}")))
                    .relative()
                    .flex_none()
                    .py(px(2.))
                    .pl(px(size::HEAD_GAP + (level - least) as f32 * size::HEAD_PAD))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .cursor_pointer()
                    .text_color(if lit { t.text } else { t.dim })
                    .hover(|s| s.text_color(t.text))
                    // The one being read is lit; a section with a change
                    // carries the change's colour on its edge.
                    .border_l_2()
                    .border_color(match (lit, changed) {
                        (true, _) => t.accent,
                        (false, Some(m)) => m.colour(t),
                        (false, None) => t.divider,
                    })
                    .tooltip(kit::tip(text.clone()))
                    .child(text.clone())
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.md_go.set(Some(at));
                        cx.notify();
                    })),
            );
        }
        list
    }

    /// The find opened over the file: the list's own head and lines, in a
    /// card under the top of the page.
    pub(super) fn palette(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .bg(alpha(t.bg, 0.4))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|p, _, w, cx| {
                    if p.opened.is_some() || p.place.is_some() {
                        p.close_palette(w, cx);
                    }
                }),
            )
            .child(
                div()
                    .id("files-palette")
                    .debug_selector(|| "files-palette".into())
                    .mt(px(PALETTE_TOP))
                    .w(px(PALETTE_W))
                    .max_w(gpui::relative(0.9))
                    .h(gpui::relative(PALETTE_H))
                    .flex()
                    .bg(t.surface)
                    .border_1()
                    .border_color(t.divider)
                    .rounded(px(size::RADIUS * 2.))
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        self.list_column(t, cx)
                            .flex_1()
                            .w_auto()
                            .min_w_0()
                            .border_r_0(),
                    ),
            )
    }

    /// The open files, or the place shown, and the second group of tabs
    /// beside them.
    fn main_view(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let main = if self.place.is_some() {
            div()
                .flex()
                .flex_col()
                .children((!self.beside).then(|| {
                    div()
                        .flex_none()
                        .flex()
                        .children(self.side_show(t, cx))
                        .child(div().flex_1().min_w_0().child(self.tab_strip(t, cx)))
                }))
                .child(self.place_view(t, cx).flex_1().min_h_0())
        } else {
            self.viewer(t, cx)
        };
        let Some(split) = self.split.clone() else {
            return main.flex_1().min_w_0();
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .child(main.flex_1().min_w_0().border_r_1().border_color(t.divider))
            .child(div().flex_1().min_w_0().h_full().flex().child(split))
    }

    /// Hidden side: the files across the page, the find over them when
    /// asked for (or when nothing is open).
    pub(super) fn reader_page(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let main = self.main_view(t, cx);
        let open = self.palette || (self.opened.is_none() && self.place.is_none());
        div()
            .relative()
            .size_full()
            .flex()
            .child(main.flex_1().min_w_0())
            .when(open, |d| d.child(self.palette(t, cx)))
    }

    /// The side (the folders, or what changed by where) beside the open
    /// files or the place shown.
    pub(super) fn side_page(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let list = self.list_column(t, cx);
        let detail = self.main_view(t, cx);
        pane::page(t, list, detail)
    }

    /// A folder as a place: who works there and what changed in it, its
    /// documents, and its README read.
    pub(super) fn place_view(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let Some(folder) = self.place.clone() else {
            return div();
        };
        let name = paths::base_name(&folder).to_owned();
        let parent = paths::parent(&folder);
        let head = pane::head(t)
            .h(px(size::CONTROL + size::HEAD_GAP))
            .relative()
            .child(measure(self.head_room.clone()))
            .children(self.head_lead(t, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(measure(self.name_room.clone()))
                    .when(!parent.is_empty() && parent != folder, |d| {
                        d.child(self.crumbs(&parent, t, cx))
                    })
                    .child(
                        div()
                            .flex_none()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(name.clone()),
                    ),
            )
            .child({
                let here = folder.clone();
                pane::act(t, "place-launch", "New session here")
                    .debug_selector(|| "place-launch".into())
                    .tooltip(kit::tip(format!("Start a session in {folder}")))
                    .on_click(cx.listener(move |p, _, _, cx| p.launch_in(&here, cx)))
            });
        let section = |title: &str| {
            div()
                .pt(px(size::SECTION_GAP))
                .pb(px(size::LABEL_BOTTOM))
                .text_size(px(size::SMALL))
                .text_color(t.dim)
                .child(title.to_owned())
        };
        let mut body = div()
            .w_full()
            .max_w(px(size::READING))
            .p(px(size::PAD))
            .flex()
            .flex_col();

        // Who works here.
        let working = self.sessions_at(&folder);
        body = body.child(section(if working.is_empty() {
            "No session works here"
        } else {
            "Working here"
        }));
        for s in working {
            let session = s.clone();
            let what = s.command.clone().unwrap_or_default();
            let when = s.last.map(|l| format::ago(l * 1000)).unwrap_or_default();
            body = body.child(
                pane::row(
                    t,
                    SharedString::from(format!("place-session-{}", s.name)),
                    false,
                )
                .h(px(size::ROW))
                .debug_selector({
                    let id = format!("place-session-{}", s.name);
                    move || id.clone()
                })
                .tooltip(kit::tip(format!(
                    "Go to {} in {}",
                    s.name,
                    s.cwd.clone().unwrap_or_default()
                )))
                .child(live_dot(t))
                .child(div().flex_none().text_color(t.text).child(s.name.clone()))
                .child(pane::cell(what).text_color(t.dim))
                .child(pane::meta(t, when, None))
                .on_click(cx.listener(move |p, _, _, cx| p.reveal_session(&session, cx))),
            );
        }

        // What changed in it.
        body = body.child(section("Changed here"));
        match &self.place_recent {
            None => body = body.child(pane::meta(t, "Reading\u{2026}", None)),
            Some(Err(why)) => body = body.child(pane::meta(t, why.clone(), Some(t.error))),
            Some(Ok(files)) if files.is_empty() => {
                body = body.child(pane::meta(t, "Nothing lately", None))
            }
            Some(Ok(files)) => {
                for c in files.iter().take(PLACE_RECENT) {
                    let path = c.path.clone();
                    let inner = paths::relative(&folder, &c.path).to_owned();
                    body = body.child(
                        pane::row(
                            t,
                            SharedString::from(format!("place-recent-{}", c.path)),
                            false,
                        )
                        .h(px(size::ROW))
                        .debug_selector({
                            let id = format!("place-recent-{}", paths::base_name(&c.path));
                            move || id.clone()
                        })
                        .child(pane::cell(inner))
                        .child(pane::meta(t, format::ago(c.modified), None))
                        .on_click(cx.listener(move |p, _, w, cx| p.open(path.clone(), w, cx))),
                    );
                }
            }
        }

        // Its documents.
        let docs: Vec<String> = self
            .tree
            .listings
            .get(&folder)
            .map(|entries| {
                KEY_DOCS
                    .iter()
                    .filter(|d| entries.iter().any(|e| !e.folder && e.name == **d))
                    .map(|d| d.to_string())
                    .collect()
            })
            .unwrap_or_default();
        if !docs.is_empty() {
            body = body.child(section("Documents")).child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(size::HEAD_GAP))
                    .children(docs.into_iter().map(|d| {
                        let path = paths::join(&folder, &d);
                        pane::act(t, SharedString::from(format!("place-doc-{d}")), d.clone())
                            .debug_selector(move || format!("place-doc-{d}"))
                            .on_click(cx.listener(move |p, _, w, cx| p.open(path.clone(), w, cx)))
                    })),
            );
        }

        // Its README, read.
        if let Some(blocks) = &self.place_readme {
            let look = markdown::Look {
                t,
                refs: false,
                on_link: None,
                select: None,
            };
            body = body.child(
                div()
                    .mt(px(size::SECTION_GAP))
                    .pt(px(size::SECTION_GAP))
                    .border_t_1()
                    .border_color(t.divider)
                    .child(markdown::render(&look, "place-readme", blocks)),
            );
        }
        let scrolled = div()
            .id("files-place")
            .debug_selector(|| "files-place".into())
            .size_full()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .child(body);
        pane::detail_frame(head, scrolled).debug_selector(|| "files-viewer".into())
    }
}

/// The small dot that says a session is live.
fn live_dot(t: &Theme) -> Div {
    div().flex_none().size(px(DOT)).rounded_full().bg(t.done)
}
