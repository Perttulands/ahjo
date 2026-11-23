//! Drawing the page: the list (recent changes, the tree, or what a find
//! found) beside the opened file, in the shared page skeleton.
mod diffs;
mod reader;
mod tabs;
mod zoomed;

use crate::open::Show;
use crate::page::{FilesPage, Naming, Pick, Rename, RenameIn};
use crate::store::Side;
use crate::store::{format, paths, tree::Row};
use ahjo_app_kit::drag::NodeFile;
use ahjo_app_kit::theme::{Theme, size, theme};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::pane;
use ahjo_core::NodeId;
use ahjo_files_contract::{Changed, Match};
use gpui::{
    AnyElement, Context, Div, MouseButton, MouseMoveEvent, Render, ScrollDelta, SharedString,
    Window, div, img, prelude::*, px, uniform_list,
};

/// Under each of these widths a file's header leaves out one more of its
/// least needed words. The Files panel is often about 420 wide,
/// where the name needs the room the zoom steps would take.
const HEAD_NARROW: [f32; 3] = [640., 520., 440.];
/// JetBrains Mono's advance, as a share of its size: how many letters of a
/// name fit a width.
const ADVANCE: f32 = 0.6;
/// Recent changes shown before More.
const RECENT_FIRST: usize = 6;
/// A tree level's indent.
const INDENT: f32 = 12.;
/// The disclosure's column.
const DISCLOSURE: f32 = 12.;
/// Letters kept for a row's age.
const AGE: usize = 4;

/// Room left above a change or heading scrolled to, in pixels and lines.
pub const GO_ROOM: f32 = 24.;
pub const GO_LINES: usize = 3;

/// Whether the open file shows as rendered Markdown.
pub fn rendered(p: &FilesPage) -> bool {
    p.reading()
}

/// A width as last laid out.
pub type Width = std::rc::Rc<std::cell::Cell<Option<f32>>>;

/// Records the width of what it is put in, and draws again when it
/// changed.
fn measure(width: Width) -> impl IntoElement {
    gpui::canvas(
        move |bounds, window, cx| {
            let now = Some(f32::from(bounds.size.width));
            if width.get() != now {
                width.set(now);
                // A refresh asked for while drawing is dropped, so ask after it.
                window.defer(cx, |window, _| window.refresh());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// How many letters fit `width` pixels of text.
fn letters(width: f32) -> usize {
    (width / (size::TEXT * ADVANCE)).floor().max(4.) as usize
}

/// One line of the list.
#[derive(Clone, Debug)]
pub enum Line {
    Label(String),
    /// What changed; `true` for a file under the folder line opened above it.
    Recent(Changed, bool),
    More(usize),
    /// A word in place of rows: still reading, nothing, or why not.
    Note {
        text: String,
        depth: usize,
        error: bool,
    },
    Node(Row),
    Hit(usize, Match),
    InText(String),
    /// Review: a folder where things changed (as a group of the lines
    /// after it), its newest change.
    Group(String, u64),
    /// The find going to a line of the open file: the line, and what the
    /// row says.
    Go(usize, String),
    /// A symbol of the open file the find found.
    Symbol(crate::store::symbols::Symbol),
    /// A file lines were found in, and how many.
    HitFile(String, usize),
}

impl Line {
    /// The pick this line is, when it can be picked.
    pub fn pick(&self) -> Option<Pick> {
        match self {
            Line::Recent(c, _) => Some(Pick::Recent(c.path.clone())),
            Line::More(_) => Some(Pick::More),
            Line::Node(r) => Some(Pick::Node(r.path.clone())),
            Line::Hit(i, _) => Some(Pick::Hit(*i)),
            Line::InText(_) => Some(Pick::InText),
            Line::Group(folder, _) => Some(Pick::Group(folder.clone())),
            Line::Go(line, _) => Some(Pick::Go(*line)),
            Line::Symbol(s) => Some(Pick::Symbol(s.line)),
            Line::HitFile(path, _) => Some(Pick::HitFile(path.clone())),
            Line::Label(_) | Line::Note { .. } => None,
        }
    }
}

/// The lines the list shows now.
pub fn lines(p: &FilesPage) -> Vec<Line> {
    let mut out = Vec::new();
    if p.jump.is_some() {
        return p.jump_lines();
    }
    if let Some(find) = &p.find {
        // Names first, then the lines inside files, each under its label.
        let quoted = format!("\u{201c}{}\u{201d}", find.query.trim());
        out.push(Line::Label("Names".into()));
        let names = found_lines(
            &find.found,
            0,
            format!("No file name has {quoted}"),
            &mut out,
        );
        // What is beside you stands; the rest is still being looked for.
        if find.widening && find.found.is_some() {
            out.push(Line::Note {
                text: "Looking further\u{2026}".into(),
                depth: 0,
                error: false,
            });
        }
        let scope = find
            .inside
            .as_ref()
            .map(|i| i.folder.clone())
            .or_else(|| p.text_scope());
        if let Some(folder) = scope {
            // Said, so it is plain what was searched and what was not.
            let what = if p.repository_of(&folder).as_deref() == Some(folder.as_str()) {
                "repository"
            } else {
                "folder"
            };
            out.push(Line::Label(format!(
                "Inside {what} {}",
                paths::shown(&p.tree.roots, &folder)
            )));
            match &find.inside {
                Some(inside) => {
                    let first = out.len();
                    found_lines(
                        &inside.found,
                        names,
                        format!("No file here has {quoted}"),
                        &mut out,
                    );
                    group_by_file(&mut out, first);
                }
                None => out.push(Line::InText(quoted)),
            }
        }
        return out;
    }
    let main = !p.is_panel;
    if main && p.side == Side::Changes {
        return reader::review_lines(p);
    }
    // The Reader's find shows what changed; folders are reached by the
    // path over the file.
    let tree = !(main && p.side == Side::Hidden);
    if let Some(Ok(recent)) = &p.recent
        && !recent.is_empty()
    {
        out.push(Line::Label("Changed lately".into()));
        let shown = if p.recent_more || !tree {
            recent.len()
        } else {
            RECENT_FIRST.min(recent.len())
        };
        for changed in &recent[..shown] {
            out.push(Line::Recent(changed.clone(), false));
            let Some((_, inside)) = p
                .recent_inside
                .as_ref()
                .filter(|(folder, _)| changed.files.is_some() && *folder == changed.path)
            else {
                continue;
            };
            let note = |text: String, error: bool| Line::Note {
                text,
                depth: 0,
                error,
            };
            match inside {
                None => out.push(note("Reading\u{2026}".into(), false)),
                Some(Err(why)) => out.push(note(why.clone(), true)),
                Some(Ok(files)) => {
                    out.extend(files.iter().cloned().map(|c| Line::Recent(c, true)));
                }
            }
        }
        if shown < recent.len() {
            out.push(Line::More(recent.len() - shown));
        }
        if tree {
            out.push(Line::Label("Folders".into()));
        }
    }
    if !tree {
        return out;
    }
    out.extend(tree_lines(p, p.tree.rows()));
    out
}

/// The tree's `rows`, each folder that could not be read or holds nothing
/// saying so under it; while there are no roots yet, why.
fn tree_lines(p: &FilesPage, rows: Vec<Row>) -> Vec<Line> {
    let mut out = Vec::new();
    let none = rows.is_empty();
    for row in rows {
        let failed = p.tree.failed.get(&row.path).cloned();
        let empty = row.open
            && p.tree
                .listings
                .get(&row.path)
                .is_some_and(|l| crate::store::listing::shown(l).is_empty());
        let depth = row.depth + 1;
        out.push(Line::Node(row));
        if let Some(why) = failed {
            out.push(Line::Note {
                text: why,
                depth,
                error: true,
            });
        } else if empty {
            out.push(Line::Note {
                text: "Empty".into(),
                depth,
                error: false,
            });
        }
    }
    if none {
        out.push(Line::Note {
            text: if p.no_node {
                "Not connected to the node.".into()
            } else if p.tree.away {
                "The device does not answer".into()
            } else {
                "Reading…".into()
            },
            depth: 0,
            error: p.tree.away || p.no_node,
        });
    }
    out
}

/// The lines of the side panel's Files section: the tree from the focused
/// session's folder, or from the roots.
/// The lines found inside files from `first` on, under a line for each
/// file they are in, with how many.
fn group_by_file(out: &mut Vec<Line>, first: usize) {
    let hits = out.split_off(first);
    let mut i = 0;
    while i < hits.len() {
        let Line::Hit(_, hit) = &hits[i] else {
            out.push(hits[i].clone());
            i += 1;
            continue;
        };
        let path = hit.path.clone();
        let n = hits[i..]
            .iter()
            .take_while(|l| matches!(l, Line::Hit(_, h) if h.path == path))
            .count();
        out.push(Line::HitFile(path, n));
        out.extend(hits[i..i + n].iter().cloned());
        i += n;
    }
}

/// One group of hits, numbered from `first`: still looking, why not, none,
/// or the hits (with a word first when there were more). Answers how many.
fn found_lines(
    found: &Option<Result<ahjo_files_contract::Matches, String>>,
    first: usize,
    none: String,
    out: &mut Vec<Line>,
) -> usize {
    let note = |text: String, error: bool| Line::Note {
        text,
        depth: 0,
        error,
    };
    match found {
        None => {
            out.push(note("Looking\u{2026}".into(), false));
            0
        }
        Some(Err(e)) => {
            out.push(note(e.clone(), true));
            0
        }
        Some(Ok(m)) if m.matches.is_empty() => {
            out.push(note(none, false));
            0
        }
        Some(Ok(m)) => {
            // Said first: under a long list it would never be seen.
            if m.truncated {
                out.push(note(
                    format!("Only the first {}; type more of it", m.matches.len()),
                    false,
                ));
            }
            for (i, hit) in m.matches.iter().enumerate() {
                out.push(Line::Hit(first + i, hit.clone()));
            }
            m.matches.len()
        }
    }
}

/// Letters kept before what was found when a found line is cut short to
/// show it.
const LEAD: usize = 12;

/// A found line from a little before its first find: a long line's find
/// stays in view.
fn lead_to_found<'a>(text: &'a str, query: &str) -> std::borrow::Cow<'a, str> {
    let Some(at) = crate::store::find::in_text(text, query)
        .first()
        .map(|r| r.start)
    else {
        return text.into();
    };
    let before = text[..at].chars().count();
    if before <= LEAD {
        return text.into();
    }
    let from = text[..at]
        .char_indices()
        .nth(before - LEAD)
        .map_or(0, |(i, _)| i);
    format!("\u{2026}{}", &text[from..]).into()
}

/// The colour a file's state against HEAD shows in, by the theme's
/// meanings: new is done work, taken out an error's colour, changed and not
/// committed waits on the operator.
pub fn state_colour(t: &Theme, state: char) -> gpui::Hsla {
    match state {
        '?' | 'A' => t.done,
        'D' | 'C' => t.error,
        _ => t.needs,
    }
}

/// The letter a file's state shows as: U for a file git does not know yet.
pub fn state_word(state: char) -> &'static str {
    match state {
        '?' => "U",
        'A' => "A",
        'D' => "D",
        'R' => "R",
        'C' => "!",
        _ => "M",
    }
}

/// A state, said whole.
pub fn state_tip(state: char) -> &'static str {
    match state {
        '?' => "New, not yet known to git",
        'A' => "New, not committed",
        'D' => "Deleted, not committed",
        'R' => "Renamed, not committed",
        'C' => "Merge conflict: both sides changed it",
        _ => "Changed, not committed",
    }
}

impl FilesPage {
    fn rename_detail(&self, path: &str) -> Option<(&'static str, String)> {
        if self.states.get(path) != Some(&'R') {
            return None;
        }
        let (prefix, other) = match self.renames.get(path)? {
            Rename::From(source) => ("Renamed from ", source),
            Rename::To(destination) => ("Renamed to ", destination),
        };
        let repo = self.repository_of(path);
        let other = repo
            .as_deref()
            .map_or(other.as_str(), |repo| paths::relative(repo, other));
        Some((prefix, other.to_owned()))
    }

    /// Letters a row's words have in the list.
    fn row_letters(&self, indent: f32) -> usize {
        let width = self.list_room.get().unwrap_or(size::LIST) - 2. * size::ROW_PAD - indent;
        letters(width)
    }

    fn line(&self, line: Line, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let picked = line.pick().is_some_and(|l| self.pick.as_ref() == Some(&l));
        match line {
            Line::Label(text) => {
                let fit = (self.row_letters(0.) as f32 * size::TEXT / size::SMALL) as usize;
                pane::label(t, pane::middle(&text, fit))
                    .h(px(size::ROW))
                    .whitespace_nowrap()
                    .into_any_element()
            }
            Line::Note { text, depth, error } => div()
                .debug_selector({
                    let id = format!("files-note-{text}");
                    move || id
                })
                .h(px(size::ROW))
                .flex()
                .items_center()
                .pl(px(size::ROW_PAD + depth as f32 * INDENT + DISCLOSURE))
                .pr(px(size::ROW_PAD))
                .text_size(px(size::SMALL))
                .text_color(if error { t.error } else { t.dim })
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text)
                .into_any_element(),
            Line::More(n) => pane::row(t, "recent-more", picked)
                .h(px(size::ROW))
                .debug_selector(|| "recent-more".into())
                .child(pane::meta(t, format!("{n} more"), None))
                .on_click(cx.listener(|p, _, w, cx| {
                    p.set_pick(Pick::More, cx);
                    p.activate(w, cx);
                }))
                .into_any_element(),
            Line::Recent(c, inside) => {
                let mut name = paths::base_name(&c.path).to_owned();
                let mut folder = paths::base_name(&paths::parent(&c.path)).to_owned();
                // A folder in which many files changed together.
                if let Some(n) = c.files {
                    name.push('/');
                    folder = format!("{n} files, in {folder}");
                }
                let indent = if inside { INDENT + DISCLOSURE } else { 0. };
                // An uncommitted file says how it stands, and where in its
                // repository it is.
                let state = self.states.get(&c.path).copied();
                let rename = self.rename_detail(&c.path);
                if c.modified == 0
                    && let Some(repo) = self.repository_of(&c.path)
                {
                    folder = paths::parent(&c.path)
                        .get(repo.len() + 1..)
                        .unwrap_or_default()
                        .to_owned();
                }
                let taken = if c.modified == 0 { 0 } else { AGE + 1 } + state.map_or(0, |_| 2);
                let room = self.row_letters(indent).saturating_sub(taken);
                // A folder line that cannot say where it is says how many.
                if let Some(n) = c.files {
                    let left = room.saturating_sub(name.chars().count() + 1);
                    if folder.chars().count() > (left as f32 * size::TEXT / size::SMALL) as usize {
                        folder = format!("{n} files");
                    }
                }
                let detail = rename
                    .as_ref()
                    .filter(|_| c.modified == 0)
                    .map(|(prefix, other)| (*prefix, other.as_str()))
                    .or((!folder.is_empty()).then_some(("", folder.as_str())));
                let tip = match &rename {
                    Some((prefix, other)) => format!("{}\n{prefix}{other}", c.path),
                    None => state.map_or_else(
                        || c.path.clone(),
                        |s| format!("{}\n{}", c.path, state_tip(s)),
                    ),
                };
                let path = c.path.clone();
                let selector = format!("recent-{name}");
                self.file_row(
                    t,
                    SharedString::from(format!("recent-{}", c.path)),
                    Pick::Recent(c.path.clone()),
                    picked,
                    &name,
                    detail,
                    room,
                    (c.modified != 0).then(|| format::ago(c.modified)),
                    cx,
                )
                .when_some(state.filter(|_| !picked), |row, s| {
                    row.text_color(state_colour(t, s))
                })
                .children(state.map(|s| {
                    let selector = format!("recent-state-{name}-{s}");
                    pane::meta(t, state_word(s), Some(state_colour(t, s)))
                        .debug_selector(move || selector.clone())
                }))
                .pl(px(size::ROW_PAD - size::ROW_INSET + indent))
                .debug_selector(move || selector.clone())
                .tooltip(kit::tip(tip))
                .on_mouse_down(
                    MouseButton::Right,
                    self.menu_at(
                        Pick::Recent(c.path.clone()),
                        c.path.clone(),
                        c.files.is_some(),
                        cx,
                    ),
                )
                .on_click(cx.listener(move |p, e: &gpui::ClickEvent, w, cx| {
                    p.set_pick(Pick::Recent(path.clone()), cx);
                    p.activate_as(e.click_count() > 1, w, cx);
                }))
                .into_any_element()
            }
            Line::Hit(i, hit) => {
                let folder = paths::parent(&hit.path);
                let place = paths::root_of(&self.tree.roots, &folder)
                    .map(|r| {
                        let inner = paths::relative(&r.path, &folder);
                        if inner == folder {
                            r.name.clone()
                        } else {
                            paths::join(&r.name, inner)
                        }
                    })
                    .unwrap_or(folder);
                let name = match hit.line {
                    Some(line) => format!("{}:{line}", hit.name),
                    None => hit.name.clone(),
                };
                let room = self.row_letters(0.);
                let selector = format!("hit-{name}");
                let row = match hit.text.as_deref().map(str::trim) {
                    // A hit in the text, under its file: its line's number
                    // and the line, what was found in it marked.
                    Some(text) => {
                        let query = self.find.as_ref().map_or("", |f| f.query.trim());
                        // A line that fits is shown whole.
                        let fits = text.chars().count() + 6 <= room;
                        let text = &*if fits {
                            text.into()
                        } else {
                            lead_to_found(text, query)
                        };
                        let marked: Vec<_> = crate::store::find::in_text(text, query)
                            .into_iter()
                            .map(|r| {
                                (
                                    r,
                                    gpui::HighlightStyle {
                                        color: Some(t.text),
                                        background_color: Some(t.accent_light()),
                                        ..Default::default()
                                    },
                                )
                            })
                            .collect();
                        pane::row(t, SharedString::from(format!("hit-{i}")), picked)
                            .h(px(size::ROW))
                            .pl(px(size::ROW_PAD - size::ROW_INSET + DISCLOSURE))
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(size::COUNT * 2.))
                                    .text_size(px(size::SMALL))
                                    .text_color(t.dim)
                                    .child(hit.line.map(|l| l.to_string()).unwrap_or_default()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_color(t.text2)
                                    .font_weight(gpui::FontWeight::NORMAL)
                                    .child(
                                        gpui::StyledText::new(SharedString::from(text.to_owned()))
                                            .with_highlights(marked),
                                    ),
                            )
                    }
                    None => self.file_row(
                        t,
                        SharedString::from(format!("hit-{i}")),
                        Pick::Hit(i),
                        picked,
                        &name,
                        Some(("", &place)),
                        room,
                        None,
                        cx,
                    ),
                };
                row.debug_selector(move || selector.clone())
                    .tooltip(kit::tip(hit.path.clone()))
                    .on_mouse_down(
                        MouseButton::Right,
                        // A line inside a file is renamed in its file's
                        // heading, where the name is.
                        self.menu_at(
                            match hit.line {
                                Some(_) => Pick::HitFile(hit.path.clone()),
                                None => Pick::Hit(i),
                            },
                            hit.path.clone(),
                            false,
                            cx,
                        ),
                    )
                    .on_click(cx.listener(move |p, _, w, cx| {
                        p.set_pick(Pick::Hit(i), cx);
                        p.activate(w, cx);
                    }))
                    .into_any_element()
            }
            Line::InText(quoted) => pane::row(t, "files-in-text", picked)
                .h(px(size::ROW))
                .debug_selector(|| "files-in-text".into())
                .tooltip(kit::tip("Enter looks inside every file here"))
                .child(pane::cell(format!("Look for {quoted} inside them")).text_color(t.text2))
                .on_click(cx.listener(|p, _, w, cx| {
                    p.set_pick(Pick::InText, cx);
                    p.activate(w, cx);
                }))
                .into_any_element(),
            Line::Node(row) => self.tree_row(row, picked, t, cx),
            Line::Group(folder, newest) => self.group_row(folder, newest, picked, t, cx),
            Line::HitFile(path, n) => {
                let name = paths::base_name(&path).to_owned();
                let folder = paths::shown(&self.tree.roots, &paths::parent(&path));
                let room = self.row_letters(0.).saturating_sub(4);
                let selector = format!("hit-file-{name}");
                let open = path.clone();
                self.file_row(
                    t,
                    SharedString::from(format!("hit-file-{path}")),
                    Pick::HitFile(path.clone()),
                    picked,
                    &name,
                    Some(("", &folder)),
                    room,
                    Some(n.to_string()),
                    cx,
                )
                .debug_selector(move || selector.clone())
                .tooltip(kit::tip(path.clone()))
                .on_mouse_down(
                    MouseButton::Right,
                    self.menu_at(Pick::HitFile(path.clone()), path.clone(), false, cx),
                )
                .on_click(cx.listener(move |p, e: &gpui::ClickEvent, w, cx| {
                    p.set_pick(Pick::HitFile(open.clone()), cx);
                    p.activate_as(e.click_count() > 1, w, cx);
                }))
                .into_any_element()
            }
            Line::Go(line, words) => pane::row(t, "files-go-line", picked)
                .h(px(size::ROW))
                .debug_selector(|| "files-go-line".into())
                .child(pane::cell(words))
                .on_click(cx.listener(move |p, _, w, cx| {
                    p.set_pick(Pick::Go(line), cx);
                    p.activate(w, cx);
                }))
                .into_any_element(),
            Line::Symbol(sym) => {
                let line = sym.line;
                let selector = format!("files-symbol-{}", sym.name);
                pane::row(t, SharedString::from(format!("symbol-{line}")), picked)
                    .h(px(size::ROW))
                    .debug_selector(move || selector.clone())
                    .child(div().flex_none().w(px(sym.depth as f32 * INDENT)))
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(size::COUNT * 3.))
                            .text_size(px(size::SMALL))
                            .text_color(t.dim)
                            .child(sym.kind),
                    )
                    .child(pane::cell(sym.name.clone()))
                    .child(pane::meta(t, (line + 1).to_string(), None))
                    .on_click(cx.listener(move |p, _, w, cx| {
                        p.set_pick(Pick::Symbol(line), cx);
                        p.activate(w, cx);
                    }))
                    .into_any_element()
            }
        }
    }

    /// A file's row: its name shortened in the middle to `room` letters,
    /// its location or rename in dim after it while there is room, and `age`.
    #[allow(clippy::too_many_arguments)]
    fn file_row(
        &self,
        t: &Theme,
        id: SharedString,
        pick: Pick,
        picked: bool,
        name: &str,
        detail: Option<(&str, &str)>,
        room: usize,
        age: Option<String>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        if self.rename_place() == Some(RenameIn::Line(pick)) {
            return pane::row(t, id, picked)
                .h(px(size::ROW))
                .child(self.name_field_in_row(cx));
        }
        let shown = pane::middle(name, room);
        let left = room.saturating_sub(shown.chars().count() + 1);
        pane::row(t, id, picked)
            .h(px(size::ROW))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap(px(size::HEAD_GAP))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(div().flex_none().child(shown))
                    .children(detail.filter(|_| left >= 4).map(|(prefix, path)| {
                        // Small letters: more of them fit. Shortened in its
                        // middle, a folder keeps its root and its nearest
                        // folders, the parts that tell two hits apart.
                        let fit = (left as f32 * size::TEXT / size::SMALL) as usize;
                        let path = pane::middle(path, fit.saturating_sub(prefix.len()));
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(size::SMALL))
                            .text_color(t.dim)
                            .font_weight(gpui::FontWeight::NORMAL)
                            .child(format!("{prefix}{path}"))
                    })),
            )
            .children(age.map(|a| pane::meta(t, a, None)))
    }

    fn tree_row(&self, row: Row, picked: bool, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let indent = row.depth as f32 * INDENT;
        let room = self.row_letters(indent + DISCLOSURE);
        let reading = self
            .opened
            .as_ref()
            .is_some_and(|o| o.path == row.path && o.node == self.tree.node);
        let path = row.path.clone();
        let folder = row.folder;
        let selector = format!("file-row-{}", row.name);
        let id = format!("node-{}", row.path);
        let state = self.states.get(&row.path).copied();
        let holds = row.folder && self.holding.contains(&row.path);
        let state_colour = state.map(|s| state_colour(t, s));
        let disclosure = match (row.folder, row.open) {
            (true, true) => "▾",
            (true, false) => "▸",
            _ => "",
        };
        let line = pane::row(t, SharedString::from(id), picked)
            .h(px(size::ROW))
            .gap_0()
            .debug_selector(move || selector.clone())
            .when(reading && !picked, |r| r.text_color(t.text))
            .when(picked, |r| {
                let id = format!("picked-{}", row.name);
                r.child(div().absolute().debug_selector(move || id))
            })
            .child(div().flex_none().w(px(indent)))
            .child(
                div()
                    .flex_none()
                    .w(px(DISCLOSURE))
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(disclosure),
            )
            .child(
                if self.rename_place() == Some(RenameIn::Line(Pick::Node(row.path.clone()))) {
                    self.name_field_in_row(cx).into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .when_some(state_colour, |d, c| d.text_color(c))
                        .when(state == Some('D'), |d| d.line_through())
                        .child(pane::middle(&row.name, room.saturating_sub(2)))
                        .into_any_element()
                },
            )
            .when(row.reading, |r| r.child(pane::meta(t, "…", None)))
            .when_some(state, |r, s| {
                let selector = format!("state-{}-{}", row.name, s);
                r.child(
                    pane::meta(t, state_word(s), state_colour)
                        .debug_selector(move || selector.clone()),
                )
            })
            .when(holds, |r| {
                r.child(
                    div()
                        .flex_none()
                        .size(px(size::DOT - 2.))
                        .rounded_full()
                        .bg(t.needs),
                )
            })
            .when(folder, |r| {
                let destination = path.clone();
                r.on_drop(cx.listener(move |p, dropped: &gpui::ExternalPaths, _, cx| {
                    p.upload_paths(destination.clone(), dropped.paths(), cx);
                    cx.stop_propagation();
                }))
            })
            // A file goes where it is dragged as itself: Send attaches it.
            .when(!folder, |r| {
                let file = NodeFile {
                    node: NodeId(self.tree.node.clone()),
                    path: path.clone(),
                };
                r.on_drag(file, |file, _, _, cx| cx.new(|_| file.clone()))
            });
        line.on_mouse_down(
            MouseButton::Right,
            self.menu_at(Pick::Node(path.clone()), path.clone(), folder, cx),
        )
        .on_click(cx.listener(move |p, e: &gpui::ClickEvent, w, cx| {
            w.focus(&p.focus, cx);
            p.set_pick(Pick::Node(path.clone()), cx);
            // A click on a folder opens or closes it; a file
            // previews, and a second click keeps it open.
            if folder && e.click_count() > 1 {
                return;
            }
            p.activate_as(e.click_count() > 1, w, cx);
        }))
        .into_any_element()
    }

    /// The menu of `path`, from its line `from`, where a rename edits it.
    fn menu_at(
        &self,
        from: Pick,
        path: String,
        folder: bool,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static {
        cx.listener(move |p, e: &gpui::MouseDownEvent, window, cx| {
            window.focus(&p.focus, cx);
            p.row_menu(path.clone(), folder, e.position, Some(from.clone()), cx);
            cx.stop_propagation();
        })
    }

    /// The field a name is given in: Enter keeps it, Esc lets it go.
    fn name_field(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        div()
            .id("files-name")
            .debug_selector(|| "files-name".into())
            .on_key_down(cx.listener(|page, e: &gpui::KeyDownEvent, w, cx| {
                if e.keystroke.key == "escape" {
                    page.stop_naming(w, cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.name_field.clone())
    }

    /// The name field in a line of the list: a press in it selects text
    /// instead of picking or opening the line.
    fn name_field_in_row(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        self.name_field(cx)
            .flex_1()
            .min_w_0()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
    }

    /// Where the name being renamed is edited: the line or the viewer it
    /// was asked from while it shows the name, else the list's head.
    fn rename_place(&self) -> Option<RenameIn> {
        let Some(Naming::Rename(path)) = &self.naming else {
            return None;
        };
        match &self.naming_in {
            Some(RenameIn::Line(
                Pick::Node(_) | Pick::Recent(_) | Pick::HitFile(_) | Pick::Hit(_),
            )) => self.naming_in.clone(),
            Some(RenameIn::Viewer) if self.opened.as_ref().is_some_and(|o| &o.path == path) => {
                Some(RenameIn::Viewer)
            }
            _ => None,
        }
    }

    /// The list column: its head holds the find (or a new file's or
    /// folder's name) and New; under it the lines, where a rename is made.
    fn list_column(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let in_head = self.naming.is_some() && self.rename_place().is_none();
        let field = match in_head {
            true => self.name_field(cx),
            false => div()
                .id("files-search")
                .debug_selector(|| "files-search".into())
                .on_key_down(
                    cx.listener(|page, e: &gpui::KeyDownEvent, w, cx| page.search_key(e, w, cx)),
                )
                .child(self.query.clone()),
        };
        let mut head = pane::list_head(t, Some(field.into_any_element()))
            .child(measure(self.list_room.clone()));
        if let Some(naming) = self.naming.as_ref().filter(|_| in_head) {
            head = head.child(
                pane::act(t, "files-name-apply", naming.verb())
                    .debug_selector(|| "files-name-apply".into())
                    .on_click(cx.listener(|p, _, w, cx| p.apply_naming(w, cx))),
            );
        } else if self.tree.writable() && !self.palette {
            head = head.child(
                pane::act(t, "files-new", "New")
                    .debug_selector(|| "files-new".into())
                    .tooltip(kit::tip(
                        "A file, a folder or an upload, in the folder picked",
                    ))
                    .on_click(cx.listener(|p, e: &gpui::ClickEvent, _, cx| {
                        if let Some(folder) = p.tree.folder_here() {
                            p.new_menu(folder, e.position(), cx);
                        }
                    })),
            );
        }
        head = head.children(self.panel_acts(t, cx));
        let n = lines(self).len();
        let body = uniform_list(
            "files-list",
            n,
            cx.processor(|p, range: std::ops::Range<usize>, _, cx| {
                let t = theme(cx).clone();
                let all = lines(p);
                range
                    .filter_map(|at| all.get(at).cloned())
                    // Each row takes the list's whole width.
                    .map(|line| {
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .child(p.line(line, &t, cx))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .debug_selector(|| "files-list".into());
        let drop_into = self.tree.folder_here();
        let side = (!self.is_panel && !self.palette).then(|| self.side_switch(t, cx));
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .children(side)
            .child(body);
        let frame = if !self.is_panel && (self.side == Side::Changes || self.finding.get()) {
            pane::list_wide_frame(t, head, body)
        } else {
            pane::list_frame(t, head, body)
        };
        frame
            .when(self.is_panel, |l| l.flex_1().min_w_0().border_r_0())
            .on_drop(cx.listener(move |p, dropped: &gpui::ExternalPaths, _, cx| {
                if let Some(folder) = drop_into.clone() {
                    p.upload_paths(folder, dropped.paths(), cx);
                    cx.stop_propagation();
                }
            }))
    }

    /// The viewer's head: the file's folder dim and its name, Send, then
    /// what can be done with it now.
    fn viewer_head(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let mut head = pane::head(t)
            .h(px(size::CONTROL + size::HEAD_GAP))
            .relative()
            .child(measure(self.head_room.clone()));
        if self.is_panel {
            head = head.child(
                pane::act(t, "files-back", "Back")
                    .debug_selector(|| "files-back".into())
                    .tooltip(kit::tip("To the list, the file picked in it"))
                    .on_click(cx.listener(|p, _, _, cx| p.back_to_list(cx))),
            );
        }
        if !self.is_panel {
            head = head.children(self.head_lead(t, cx));
        }
        let Some(o) = &self.opened else {
            return head.child(div().flex_1());
        };
        // A narrow header leaves out the picture's pixel size and 1:1, then
        // the percentage, then the zoom steps; the name gives way in the
        // middle, keeping its extension.
        let narrow = self.head_room.get().map_or(0, |w| {
            HEAD_NARROW.iter().filter(|&&at| w < at).count() as u8
        });
        let room = self.name_room.get().map(letters);
        let name = room.map_or(o.name.clone(), |n| paths::elide_name(&o.name, n));
        let folder = paths::parent(&o.path);
        let folder_fits = room.is_none_or(|n| n >= o.name.chars().count() + 12);
        head = head.child(
            div()
                .id("opened-name")
                .flex_1()
                .min_w_0()
                .relative()
                .flex()
                .overflow_hidden()
                .whitespace_nowrap()
                .tooltip(kit::tip(o.path.clone()))
                .debug_selector({
                    let id = format!("file-view-{}", o.name);
                    move || id.clone()
                })
                .child(measure(self.name_room.clone()))
                .items_center()
                .when(!folder.is_empty() && folder_fits, |d| {
                    d.child(self.crumbs(&folder, t, cx))
                })
                .child(if self.rename_place() == Some(RenameIn::Viewer) {
                    self.name_field(cx).flex_1().min_w_0().into_any_element()
                } else {
                    div()
                        .flex_none()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(name)
                        .into_any_element()
                })
                .children(self.symbol_crumb(t, cx)),
        );
        let text = matches!(o.show, Show::Text { .. });
        let dirty = self.drafts.contains_key(&o.key());
        if o.editing {
            if o.changed_on_disk {
                head = head.child(
                    pane::meta(t, "Changed on disk", Some(t.error))
                        .debug_selector(|| "files-changed-on-disk".into()),
                );
            } else if dirty || o.saving {
                head = head.child(
                    pane::meta(t, if o.saving { "Saving…" } else { "Unsaved" }, None)
                        .debug_selector(|| "files-unsaved".into()),
                );
            }
            // Save stands out only while there is something to save.
            head = head
                .child(
                    kit::button(
                        t,
                        "files-save",
                        "Save",
                        if dirty { Kind::Primary } else { Kind::Quiet },
                    )
                    .debug_selector(|| "files-save".into())
                    .tooltip(kit::tip("Save (Ctrl+S)"))
                    .when(!dirty, |b| b.opacity(ahjo_app_kit::theme::fade::IDLE))
                    .on_click(cx.listener(|p, _, _, cx| p.save(cx))),
                )
                .child(
                    pane::act(t, "files-discard", if dirty { "Discard" } else { "Done" })
                        .debug_selector(|| "files-discard".into())
                        .tooltip(kit::tip("Esc"))
                        .on_click(cx.listener(|p, _, w, cx| p.discard(w, cx))),
                );
            return head;
        }
        head = head.children(self.change_count(t, cx));
        if narrow < 1 {
            let path = o.path.clone();
            head = head.children(self.provenance(&path, t, cx));
        }
        if narrow < 3 || !self.is_panel {
            head = head.child(
                pane::act(t, "files-send", "Send")
                    .debug_selector(|| "files-send".into())
                    .tooltip(kit::tip(if text {
                        "Send it, or the lines selected, to a session"
                    } else {
                        "Send it to a session"
                    }))
                    .on_click(cx.listener(|p, _, _, cx| p.send(cx))),
            );
        }
        if let Some(zoom) = self.zoom_bar(t, narrow, cx) {
            head = head.child(zoom);
        }
        if o.diff.is_some() && !matches!(o.show, Show::Gone) {
            head = head.child(
                pane::act(
                    t,
                    "files-changes",
                    if o.diff_open { "File" } else { "Changes" },
                )
                .debug_selector(|| "files-changes".into())
                .tooltip(kit::tip(if o.diff_open {
                    "Back to the file"
                } else {
                    "Every change since the last commit, line by line"
                }))
                .on_click(cx.listener(|p, _, _, cx| p.toggle_diff(cx))),
            );
        }
        if o.editable() {
            head = head.child(
                pane::act(t, "files-edit", "Edit")
                    .debug_selector(|| "files-edit".into())
                    .on_click(cx.listener(|p, _, w, cx| p.edit(w, cx))),
            );
        } else if text && !matches!(o.show, Show::Reading) {
            head = head.child(pane::meta(t, "Too large to edit here", None));
        }
        head = head.child(
            pane::act(t, "files-more", "\u{2026}")
                .debug_selector(|| "files-more".into())
                .tooltip(kit::tip("Long lines, and the file's menu"))
                .on_click(
                    cx.listener(|p, e: &gpui::ClickEvent, _, cx| p.file_menu(e.position(), cx)),
                ),
        );
        head.children(self.panel_acts(t, cx))
    }

    /// In the panel: Close, which puts it away.
    fn panel_acts(&self, t: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        self.is_panel.then(|| {
            pane::act(t, "files-close-panel", "Close")
                .debug_selector(|| "files-close-panel".into())
                .tooltip(kit::tip("Esc, Alt+E"))
                .on_click(cx.listener(|p, _, _, cx| p.close_panel(cx)))
                .into_any_element()
        })
    }

    /// A decision the page waits on, on a line under the viewer's head.
    fn decision(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let line = |words: String| {
            div()
                .flex_none()
                .min_h(px(size::TAB_BAR))
                .px(px(size::HEAD_PAD))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(size::HEAD_GAP))
                .border_b_1()
                .border_color(t.divider)
                .child(div().flex_1().min_w_0().text_color(t.text).child(words))
        };
        if self.conflict {
            return Some(
                line("It changed on disk since you began. Your edit is kept.".into())
                    .child(
                        kit::button(t, "conflict-copy", "Save as a copy", Kind::Primary)
                            .debug_selector(|| "conflict-copy".into())
                            .on_click(
                                cx.listener(|p, _, _, cx| p.write(crate::open::Write::Copy, cx)),
                            ),
                    )
                    .child(
                        pane::act(t, "conflict-overwrite", "Overwrite theirs")
                            .debug_selector(|| "conflict-overwrite".into())
                            .on_click(cx.listener(|p, _, _, cx| {
                                p.write(crate::open::Write::Overwrite, cx)
                            })),
                    )
                    .child(
                        pane::act(t, "conflict-cancel", "Keep editing")
                            .on_click(cx.listener(|p, _, w, cx| p.cancel_conflict(w, cx))),
                    ),
            );
        }
        if self.confirm_discard {
            return Some(
                line(match &self.opened {
                    Some(o) => format!("Discard your changes to {}?", o.name),
                    None => "Discard your changes?".into(),
                })
                .child(
                    kit::button(t, "files-confirm-discard", "Discard", Kind::Primary)
                        .debug_selector(|| "files-confirm-discard".into())
                        .on_click(cx.listener(|p, _, w, cx| p.discard(w, cx))),
                )
                .child(
                    pane::act(t, "files-keep-editing", "Keep editing")
                        .debug_selector(|| "files-keep-editing".into())
                        .on_click(cx.listener(|p, _, w, cx| p.keep_editing(w, cx))),
                ),
            );
        }
        if self.confirm_close_panel {
            return Some(
                line("Closing loses your unsaved changes.".into())
                    .child(
                        kit::button(t, "files-close-discard", "Discard and close", Kind::Primary)
                            .debug_selector(|| "files-close-discard".into())
                            .on_click(cx.listener(|p, _, _, cx| p.discard_and_close_panel(cx))),
                    )
                    .child(
                        pane::act(t, "files-close-cancel", "Keep editing")
                            .on_click(cx.listener(|p, _, _, cx| p.cancel_close_panel(cx))),
                    ),
            );
        }
        if let Some(name) = self.upload_conflict_name() {
            return Some(
                line(format!("{name} is already there."))
                    .child(
                        kit::button(t, "upload-replace", "Replace", Kind::Primary)
                            .debug_selector(|| "upload-replace".into())
                            .on_click(cx.listener(|p, _, _, cx| p.resolve_upload(Some(true), cx))),
                    )
                    .child(
                        pane::act(t, "upload-keep", "Keep both")
                            .debug_selector(|| "upload-keep".into())
                            .on_click(cx.listener(|p, _, _, cx| p.resolve_upload(Some(false), cx))),
                    )
                    .child(
                        pane::act(t, "upload-cancel", "Skip")
                            .on_click(cx.listener(|p, _, _, cx| p.resolve_upload(None, cx))),
                    ),
            );
        }
        None
    }

    fn viewer_body(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(o) = &self.opened else {
            return pane::vacant(
                t,
                "Choose a file to read it, or find one by name with Ctrl+P.",
                None,
            )
            .into_any_element();
        };
        if o.diff_open
            && let Some(sides) = o.sides.clone()
        {
            let truncated = o.diff.as_ref().is_some_and(|d| d.truncated);
            return self.diff_view(t, sides, truncated, cx);
        }
        match &o.show {
            Show::Reading => pane::empty(t, "Reading…", None).into_any_element(),
            Show::Failed(why) => div()
                .p(px(size::PAD))
                .debug_selector(|| "files-failed".into())
                .child(kit::note(t, why.clone(), Some(t.error)))
                .into_any_element(),
            Show::Gone => pane::empty(t, "Deleted since the last commit", None).into_any_element(),
            Show::Not(why) => {
                let node = ahjo_core::NodeId(o.node.clone());
                let path = o.path.clone();
                pane::empty(
                    t,
                    why.clone(),
                    Some(
                        pane::act(t, "viewer-download", "Download")
                            .debug_selector(|| "viewer-download".into())
                            .on_click(cx.listener(move |p, _, _, cx| {
                                p.download_path(node.clone(), path.clone(), cx)
                            }))
                            .into_any_element(),
                    ),
                )
                .into_any_element()
            }
            Show::Picture { image, natural } => self.picture(image, *natural, cx),
            Show::Model(m) => self.model(t, m, cx),
            Show::Media(_) => pane::empty(
                t,
                "It plays in this computer's own player",
                Some(
                    kit::button(t, "files-play", "Open", Kind::Primary)
                        .debug_selector(|| "files-play".into())
                        .on_click(cx.listener(|p, _, _, cx| p.play(cx)))
                        .into_any_element(),
                ),
            )
            .into_any_element(),
            Show::Text {
                markdown: Some(blocks),
            } if !o.editing && !o.source && !self.drafts.contains_key(&o.key()) => {
                self.markdown_body(t, blocks, cx)
            }
            Show::Text { .. }
                if o.pretty.is_some()
                    && !o.editing
                    && !o.source
                    && !self.drafts.contains_key(&o.key()) =>
            {
                div()
                    .size_full()
                    .debug_selector(|| "files-json-formatted".into())
                    .child(self.pretty_code.clone())
                    .into_any_element()
            }
            Show::Text { .. } => {
                let selector = format!("files-code-top-{}", self.code.read(cx).top_line() + 1);
                let code = div()
                    .size_full()
                    .debug_selector(move || selector.clone())
                    .child(self.code.clone())
                    .into_any_element();
                self.with_outline(t, code, cx)
            }
        }
    }

    fn model(&self, t: &Theme, m: &crate::model::Preview, cx: &mut Context<Self>) -> AnyElement {
        fn with(p: &mut FilesPage, f: impl FnOnce(&mut crate::model::Preview)) {
            if let Some(Show::Model(model)) = p.opened.as_mut().map(|o| &mut o.show) {
                f(model)
            }
        }
        div()
            .id("model-stage")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .child(
                pane::act(t, "model-reset", "Reset view").on_click(cx.listener(|p, _, _, cx| {
                    with(p, |m| m.reset());
                    cx.notify()
                })),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|p, e: &gpui::MouseDownEvent, _, _| {
                    with(p, |m| {
                        m.last = Some((f32::from(e.position.x), f32::from(e.position.y)))
                    })
                }),
            )
            .on_mouse_move(cx.listener(|p, e: &MouseMoveEvent, _, cx| {
                with(p, |model| {
                    if e.dragging() {
                        let now = (f32::from(e.position.x), f32::from(e.position.y));
                        if let Some(last) = model.last {
                            model.yaw += (now.0 - last.0) * 0.01;
                            model.pitch = (model.pitch + (now.1 - last.1) * 0.01).clamp(-1.5, 1.5);
                            model.redraw();
                        }
                        model.last = Some(now);
                    } else {
                        model.last = None;
                    }
                });
                cx.notify();
            }))
            .on_scroll(cx.listener(|p, e: &gpui::ScrollEvent, _, cx| {
                with(p, |model| {
                    let y = match e.delta {
                        ScrollDelta::Lines(v) => v.y,
                        ScrollDelta::Pixels(v) => f32::from(v.y) / 20.,
                    };
                    model.zoom = (model.zoom * (y * 0.1).exp()).clamp(0.1, 10.);
                    model.redraw();
                });
                cx.notify();
                cx.stop_propagation();
            }))
            .child(img(m.image.clone()).max_w_full().max_h_full())
            .into_any_element()
    }

    /// Whether the file shows as text set to a reading width: Markdown,
    /// rendered.
    pub(crate) fn reading(&self) -> bool {
        self.opened.as_ref().is_some_and(|o| {
            matches!(o.show, Show::Text { markdown: Some(_) })
                && !o.editing
                && !o.source
                && !self.drafts.contains_key(&o.key())
        })
    }

    fn viewer(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let mut head = self.viewer_head(t, cx);
        if self.reading() {
            // Over the centred text the head keeps its words in line with it.
            head = head.px(px(size::PAD));
        }
        let body = div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.closing_line(t, cx))
            .children(self.decision(t, cx))
            .children(self.infile_bar(t, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(self.viewer_body(t, cx))
                    .child(self.zoom_catch(cx)),
            );
        let tabs = (!self.is_panel).then(|| {
            div()
                .flex_none()
                .flex()
                .children(self.side_show(t, cx))
                .child(div().flex_1().min_w_0().child(self.tab_strip(t, cx)))
        });
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .children(tabs)
            .child(
                pane::detail_frame(head, body)
                    .flex_1()
                    .min_h_0()
                    .debug_selector(|| "files-viewer".into()),
            )
    }
}

impl Render for FilesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.release_pictures(window);
        self.no_node = self.tree.node.is_empty() && self.host.offline(cx);
        if self.newest_due {
            cx.defer_in(window, |page, window, cx| page.open_newest(window, cx));
        }
        if self.focus_file_due
            && !self
                .opened
                .as_ref()
                .is_some_and(|o| matches!(o.show, Show::Reading))
        {
            cx.defer_in(window, |page, window, cx| {
                // Unless the keyboard went elsewhere meanwhile.
                if page.focus_file_due && page.focus.is_focused(window) {
                    page.focus_file(window, cx);
                }
                page.focus_file_due = false;
            });
        }
        if self.find.as_ref().is_some_and(|f| f.open_first) {
            cx.defer_in(window, |page, window, cx| page.open_first_found(window, cx));
        }
        if self.leave_after_save && !self.opened.as_ref().is_some_and(|o| o.saving) {
            self.leave_after_save = false;
            cx.defer_in(window, |page, window, cx| page.finish_edit(window, cx));
        }
        self.finding.set(
            !self.is_panel
                && (self.find.is_some()
                    || self.jump.is_some()
                    || self.query.read(cx).is_focused(window)),
        );
        let t = theme(cx).clone();
        let root = div()
            .debug_selector(|| "files-page".into())
            .size_full()
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .on_modifiers_changed(cx.listener(Self::modifiers))
            .bg(t.bg)
            .text_color(t.text);
        if self.split_of.is_some() {
            return root
                .flex()
                .child(self.viewer(&t, cx))
                .children(self.switcher(&t));
        }
        if self.is_panel {
            // In the panel it is one column: the list, or the file
            // opened from it.
            return root.flex().child(if self.opened.is_some() {
                self.viewer(&t, cx)
            } else {
                self.list_column(&t, cx)
            });
        }
        let page = match self.side {
            Side::Hidden => self.reader_page(&t, cx),
            Side::Folders | Side::Changes => self.side_page(&t, cx),
        };
        root.child(page).children(self.switcher(&t))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_long_found_line_starts_a_little_before_its_find() {
        let line = "matches!(self.show, Show::Text { .. }) && self.saved.len() <= MAX_EDIT";
        let shown = super::lead_to_found(line, "MAX_EDIT");
        assert_eq!(shown, "\u{2026}ed.len() <= MAX_EDIT");
        assert_eq!(
            super::lead_to_found("short MAX_EDIT", "max_edit"),
            "short MAX_EDIT"
        );
        assert_eq!(super::lead_to_found("nothing", "zz"), "nothing");
    }
}
