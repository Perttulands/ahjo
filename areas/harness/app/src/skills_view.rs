//! The Skills view: every skill an agent can be given, with what it is
//! for, where it comes from and which harnesses see it; found by its
//! words, opened to read and changed in place. What is out of step with
//! its checkout, and its Sync or Link, is on the same list.

use crate::HarnessPage;
use crate::shape_state::{self as sh, Load, Pick, Shape};
use ahjo_app_kit::theme::{Theme, size};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::{markdown, pane};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

/// What the catalog read: each harness's view of one folder.
#[derive(Default)]
pub(crate) struct Catalog {
    pub dir: String,
    pub shapes: Vec<Shape>,
    pub error: Option<String>,
    /// The skill opened, by its file.
    pub chosen: Option<String>,
}

/// One skill, as every harness sees it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub name: String,
    pub description: String,
    pub path: String,
    pub origin: sh::Origin,
    /// The harnesses that see it, by name.
    pub seen: Vec<&'static str>,
    /// Why a harness that finds it does not use it.
    pub why: Vec<String>,
}

/// The skills of every harness's view, once each: two paths to the same
/// file (a link) are one skill.
pub(crate) fn entries(shapes: &[Shape]) -> Vec<Entry> {
    let mut out: Vec<(String, Entry)> = vec![];
    for shape in shapes {
        let harness = sh::HARNESSES
            .iter()
            .find(|(id, _)| *id == shape.harness)
            .map_or("?", |(_, n)| *n);
        for s in &shape.skills {
            let key = s.target.clone().unwrap_or_else(|| s.path.clone());
            let at = match out.iter().position(|(k, _)| *k == key) {
                Some(at) => at,
                None => {
                    out.push((
                        key,
                        Entry {
                            name: s.name.clone(),
                            description: s.description.clone(),
                            path: s.path.clone(),
                            origin: s.origin,
                            seen: vec![],
                            why: vec![],
                        },
                    ));
                    out.len() - 1
                }
            };
            let e = &mut out[at].1;
            if s.load == Load::Off {
                if let Some(w) = &s.why {
                    e.why.push(format!("{harness}: {w}"));
                }
            } else if !e.seen.contains(&harness) {
                e.seen.push(harness);
            }
        }
    }
    let mut out: Vec<Entry> = out.into_iter().map(|(_, e)| e).collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Whether a skill answers `find`: every word in its name or description.
pub(crate) fn matches(e: &Entry, find: &str) -> bool {
    let hay = format!("{} {}", e.name, e.description).to_lowercase();
    find.split_whitespace()
        .all(|w| hay.contains(&w.to_lowercase()))
}

impl HarnessPage {
    /// Reads every harness's skills for the Agent view's folder, or the
    /// home folder before one is picked.
    pub(crate) fn catalog_load(&mut self, cx: &mut Context<Self>) {
        let dir = if self.shape.dir.is_empty() {
            "~".to_owned()
        } else {
            self.shape.dir.clone()
        };
        if self.shape.node.is_empty() {
            match self.host.nodes(cx).first() {
                Some(n) => self.shape.node = n.id.to_string(),
                None => return,
            }
        }
        self.catalog.dir = dir.clone();
        self.catalog.shapes.clear();
        let node = self.shape.node.clone();
        for (harness, _) in sh::HARNESSES {
            let input = ahjo_harness_contract::commands::ShowShape {
                dir: dir.clone(),
                harness: Some((*harness).into()),
            };
            let asked = dir.clone();
            self.harness_call(&node, input, cx, move |page, answer, cx| {
                let c = &mut page.catalog;
                if c.dir != asked {
                    return;
                }
                match answer {
                    Ok(answer) => {
                        let shape = answer.result;
                        c.shapes.retain(|s| s.harness != shape.harness);
                        c.shapes.push(shape);
                        c.shapes.sort_by(|a, b| a.harness.cmp(&b.harness));
                        c.error = None;
                    }
                    Err(e) => c.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }
    }

    pub(crate) fn catalog_open(&mut self, path: String, cx: &mut Context<Self>) {
        if self.shape.editing {
            self.host.report(
                ahjo_app_kit::host::Status::failure(
                    "Save or discard the open file first (Ctrl+S, Escape)",
                ),
                cx,
            );
            return;
        }
        self.catalog.chosen = Some(path.clone());
        self.shape.pick = Pick::File(path.clone());
        self.file_read(path, cx);
        cx.notify();
    }

    /// The catalog's rows in the Skills list, under the finder.
    pub(crate) fn catalog_rows(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let find = self.inputs.find.read(cx).text.clone();
        let all = entries(&self.catalog.shapes);
        let shown: Vec<&Entry> = all.iter().filter(|e| matches(e, &find)).collect();
        let mut rows = div().flex().flex_col();
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pr(px(size::ROW_PAD))
                .child(pane::label(t, "Skills"))
                .child(kit::count(t, shown.len())),
        );
        if let Some(e) = &self.catalog.error {
            rows = rows.child(div().px(px(size::ROW_PAD)).child(kit::note(
                t,
                e.clone(),
                Some(t.error),
            )));
        }
        for e in shown {
            let selector = format!("skill-{}", e.name);
            let path = e.path.clone();
            let chosen = self.catalog.chosen.as_deref() == Some(&e.path);
            let marks = if e.seen.is_empty() {
                "unseen".to_string()
            } else {
                e.seen
                    .iter()
                    .map(|h| h.split_whitespace().next().unwrap_or(h))
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            rows = rows.child(
                pane::row(t, SharedString::from(selector.clone()), chosen)
                    .debug_selector(move || selector)
                    .tooltip(kit::tip(e.description.clone()))
                    .child(pane::cell(e.name.clone()))
                    .child(pane::meta(t, marks, e.seen.is_empty().then_some(t.needs)))
                    .on_click(cx.listener(move |s, _, _, cx| s.catalog_open(path.clone(), cx))),
            );
        }
        rows
    }

    /// One skill: what it is for, who sees it and from where, then its
    /// SKILL.md, read or edited in place.
    pub(crate) fn catalog_detail(&mut self, t: &Theme, path: &str, cx: &mut Context<Self>) -> Div {
        let all = entries(&self.catalog.shapes);
        let Some(e) = all.iter().find(|e| e.path == path).cloned() else {
            return pane::detail(
                t,
                "skill-detail",
                pane::head(t),
                pane::empty(t, "That skill is gone.", None),
            );
        };
        let home = self
            .catalog
            .shapes
            .first()
            .map(|s| s.home_dir.clone())
            .unwrap_or_default();
        let v = &self.shape;
        let mut head = pane::head(t)
            .px(px(size::PAD))
            .child(pane::title(t, e.name.clone()));
        if v.editing {
            head = head
                .child(
                    kit::button(
                        t,
                        "skill-save",
                        if v.saving { "Saving…" } else { "Save" },
                        Kind::Primary,
                    )
                    .debug_selector(|| "skill-save".into())
                    .on_click(cx.listener(|s, _, _, cx| s.file_save(cx))),
                )
                .child(
                    pane::act(t, "skill-discard", "Discard")
                        .on_click(cx.listener(|s, _, window, cx| s.file_discard(window, cx))),
                );
        } else {
            head = head.child(
                pane::act(t, "skill-edit", "Edit")
                    .debug_selector(|| "skill-edit".into())
                    .tooltip(kit::tip("Change its SKILL.md here; Ctrl+S saves"))
                    .on_click(cx.listener(|s, _, window, cx| s.file_edit(window, cx))),
            );
        }
        let mut facts = div()
            .px(px(size::PAD))
            .pt(px(size::PAD))
            .flex()
            .flex_col()
            .gap(px(size::HEAD_GAP))
            .child(
                div()
                    .font_family(t.ty.reading)
                    .text_size(px(t.ty.read))
                    .whitespace_normal()
                    .child(e.description.clone()),
            );
        let mut lines = div().flex().flex_col();
        let seen = if e.seen.is_empty() {
            "no harness".to_string()
        } else {
            e.seen.join(", ")
        };
        lines = lines.child(pane::line(t, "Seen by", div().child(seen)));
        lines = lines.child(pane::line(t, "From", div().child(e.origin.word())));
        lines = lines.child(pane::line(
            t,
            "File",
            div().child(sh::short(&e.path, None, &home)),
        ));
        for w in &e.why {
            lines = lines.child(pane::line(
                t,
                "Not used",
                div().text_color(t.needs).child(w.clone()),
            ));
        }
        facts = facts.child(lines);
        let mut body = div().size_full().flex().flex_col().child(facts);
        let opened = v.file.as_ref().filter(|f| f.path == e.path);
        match opened {
            None => body = body.child(pane::empty(t, "Reading…", None)),
            Some(_) if v.editing => {
                body = body.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .mt(px(size::HEAD_GAP))
                        .border_t_1()
                        .border_color(t.divider)
                        .child(self.inputs.reader.clone()),
                )
            }
            Some(f) => {
                let look = markdown::Look {
                    t,
                    refs: false,
                    on_link: None,
                    select: None,
                };
                let blocks = markdown::parse(sh::without_front_matter(&f.text));
                body = body.child(
                    div()
                        .id("skill-read")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p(px(size::PAD))
                        .font_family(t.ty.reading)
                        .text_size(px(t.ty.read))
                        .child(
                            markdown::render(&look, "skill-prose", &blocks)
                                .max_w(px(size::READING)),
                        ),
                );
            }
        }
        pane::detail_frame(head, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sh::SeenSkill;

    fn shape(harness: &str, skills: Vec<SeenSkill>) -> Shape {
        Shape {
            harness: harness.into(),
            skills,
            ..Shape::default()
        }
    }

    #[test]
    fn a_skill_two_harnesses_reach_through_links_is_one_skill() {
        let skill = |path: &str, load| SeenSkill {
            name: "handoff".into(),
            description: "Leave a handoff".into(),
            path: path.into(),
            target: Some("/home/u/skills/handoff/SKILL.md".into()),
            load,
            ..SeenSkill::default()
        };
        let all = entries(&[
            shape(
                "claude",
                vec![skill(
                    "/home/u/.claude/skills/handoff/SKILL.md",
                    Load::Start,
                )],
            ),
            shape(
                "codex",
                vec![skill(
                    "/home/u/.agents/skills/handoff/SKILL.md",
                    Load::Start,
                )],
            ),
        ]);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].seen, ["Claude Code", "Codex"]);
        assert!(matches(&all[0], "hand LEAVE"));
        assert!(!matches(&all[0], "deploy"));
    }
}
