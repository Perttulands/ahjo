//! What changed between two saved revisions of a mission, said as a person
//! reads a diff: what was added, removed or changed, and on which cards
//! (prototype, proto/board-v3).
use super::words::{harness_name, role_name, span};
use ahjo_missions_contract::{Formation, Mission, Slot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Added,
    Removed,
    Changed,
}

/// One line of a change, and the cards it is about.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub sign: Sign,
    pub text: String,
    pub nodes: Vec<String>,
}

/// One saved revision: who saved it, when, and what it changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub rev: i64,
    pub by: String,
    pub at: String,
    pub lines: Vec<Line>,
}

/// Saves by one person this close together read as one change.
const TOGETHER: time::Duration = time::Duration::minutes(10);

/// Every change against what stood before it, oldest first: one person's
/// saves in a burst as one. The first says the mission was made, with what.
pub fn changes(revisions: &[Mission]) -> Vec<Change> {
    let at = |m: &Mission| super::words::parse_time(&m.updated_at);
    let mut out = vec![];
    let mut start: usize = 0;
    for i in 0..revisions.len() {
        let m = &revisions[i];
        let next = revisions.get(i + 1);
        let joins = next.is_some_and(|n| {
            n.updated_by == m.updated_by
                && matches!((at(m), at(n)), (Some(a), Some(b)) if b - a <= TOGETHER)
        });
        if joins {
            continue;
        }
        let lines = match start.checked_sub(1).map(|j| &revisions[j]) {
            Some(before) => diff(before, m),
            None => vec![Line {
                sign: Sign::Added,
                text: made(m),
                nodes: vec![],
            }],
        };
        start = i + 1;
        if lines.is_empty() {
            continue;
        }
        out.push(Change {
            rev: m.rev,
            by: m.updated_by.clone(),
            at: m.updated_at.clone(),
            lines,
        });
    }
    out
}

fn made(m: &Mission) -> String {
    let n = m.formations.len();
    let gates = m.gates.len();
    let mut s = format!("made it, {n} step{}", if n == 1 { "" } else { "s" });
    if gates > 0 {
        s += &format!(" and {gates} gate{}", if gates == 1 { "" } else { "s" });
    }
    s
}

fn line(sign: Sign, text: String, nodes: &[&str]) -> Line {
    Line {
        sign,
        text,
        nodes: nodes.iter().map(|n| n.to_string()).collect(),
    }
}

fn worker(s: &Slot) -> String {
    let agent = if s.agent_id.is_empty() {
        &s.agent
    } else {
        &s.agent_id
    };
    if s.harness.is_empty() {
        "nobody".into()
    } else if agent.is_empty() {
        harness_name(&s.harness)
    } else {
        format!("{} on {}", role_name(agent), harness_name(&s.harness))
    }
}

fn step(before: &Formation, after: &Formation) -> Vec<Line> {
    let id = after.id.as_str();
    let name = &after.title;
    let mut out = vec![];
    if before.title != after.title {
        out.push(line(
            Sign::Changed,
            format!("{} renamed {name}", before.title),
            &[id],
        ));
    }
    let goal = |f: &Formation| f.brief.as_ref().map(|b| b.goal.clone()).unwrap_or_default();
    if goal(before) != goal(after) {
        out.push(line(
            Sign::Changed,
            format!("{name} · brief rewritten"),
            &[id],
        ));
    }
    if before.fresh != after.fresh {
        out.push(line(
            Sign::Changed,
            format!(
                "{name} · {}",
                if after.fresh {
                    "each try starts fresh"
                } else {
                    "tries keep their session"
                }
            ),
            &[id],
        ));
    }
    for s in &after.slots {
        match before.slots.iter().find(|b| b.id == s.id) {
            None => out.push(line(Sign::Added, format!("{name} · {}", worker(s)), &[id])),
            Some(b) => {
                if b.harness != s.harness && b.agent == s.agent && b.agent_id == s.agent_id {
                    out.push(line(
                        Sign::Changed,
                        format!(
                            "{name} · on {} (was {})",
                            harness_name(&s.harness),
                            harness_name(&b.harness)
                        ),
                        &[id],
                    ));
                } else if worker(b) != worker(s) {
                    out.push(line(
                        Sign::Changed,
                        format!("{name} · {} (was {})", worker(s), worker(b)),
                        &[id],
                    ));
                }
                if b.effort != s.effort && !s.effort.is_empty() {
                    out.push(line(
                        Sign::Changed,
                        format!("{name} · effort {} (was {})", s.effort, or_none(&b.effort)),
                        &[id],
                    ));
                }
            }
        }
    }
    for b in &before.slots {
        if !after.slots.iter().any(|s| s.id == b.id) {
            out.push(line(
                Sign::Removed,
                format!("{name} · {}", worker(b)),
                &[id],
            ));
        }
    }
    out
}

fn or_none(s: &str) -> &str {
    if s.is_empty() { "none" } else { s }
}

/// What `after` changed from `before`.
pub fn diff(before: &Mission, after: &Mission) -> Vec<Line> {
    let mut out = vec![];
    if before.title != after.title {
        out.push(line(
            Sign::Changed,
            format!("renamed {} (was {})", after.title, before.title),
            &[],
        ));
    }
    for c in &after.input_cards {
        if let Some(b) = before.input_cards.iter().find(|b| b.id == c.id)
            && b.goal != c.goal
        {
            out.push(line(
                Sign::Changed,
                "what it is for, rewritten".into(),
                &[&c.id],
            ));
        }
    }
    for f in &after.formations {
        match before.formations.iter().find(|b| b.id == f.id) {
            None => out.push(line(Sign::Added, f.title.clone(), &[&f.id])),
            Some(b) => out.extend(step(b, f)),
        }
    }
    for b in &before.formations {
        if !after.formations.iter().any(|f| f.id == b.id) {
            out.push(line(Sign::Removed, b.title.clone(), &[]));
        }
    }
    for g in &after.gates {
        match before.gates.iter().find(|b| b.id == g.id) {
            None => out.push(line(Sign::Added, g.title.clone(), &[&g.id])),
            Some(b) => {
                if b.title != g.title {
                    out.push(line(
                        Sign::Changed,
                        format!("{} renamed {}", b.title, g.title),
                        &[&g.id],
                    ));
                }
                if b.criterion != g.criterion {
                    out.push(line(
                        Sign::Changed,
                        format!("{} · asks: {}", g.title, g.criterion),
                        &[&g.id],
                    ));
                }
                if b.kinds != g.kinds {
                    let who = |k: &[String]| {
                        k.iter()
                            .map(|k| match k.as_str() {
                                "human" => "you",
                                "formation" => "a judge",
                                other => other,
                            })
                            .collect::<Vec<_>>()
                            .join(" and ")
                    };
                    out.push(line(
                        Sign::Changed,
                        format!(
                            "{} · answered by {} (was {})",
                            g.title,
                            who(&g.kinds),
                            who(&b.kinds)
                        ),
                        &[&g.id],
                    ));
                }
            }
        }
    }
    for b in &before.gates {
        if !after.gates.iter().any(|g| g.id == b.id) {
            out.push(line(Sign::Removed, b.title.clone(), &[]));
        }
    }
    for e in &after.ends {
        if !before.ends.iter().any(|b| b.id == e.id) {
            out.push(line(Sign::Added, format!("end: {}", e.title), &[&e.id]));
        }
    }
    for b in &before.ends {
        if !after.ends.iter().any(|e| e.id == b.id) {
            out.push(line(Sign::Removed, format!("end: {}", b.title), &[]));
        }
    }
    let cap = |l: &ahjo_missions_contract::Limit| {
        let mut parts = vec![];
        if let Some(r) = l.rounds {
            parts.push(format!("{r} tries"));
        }
        if let Some(s) = l.seconds {
            parts.push(span(s));
        }
        if let Some(t) = l.tokens {
            parts.push(format!("{}k tokens", t / 1000));
        }
        parts.join(", ")
    };
    for l in &after.limits {
        let on = after.node_name(&l.target).to_owned();
        match before.limits.iter().find(|b| b.id == l.id) {
            None => out.push(line(
                Sign::Added,
                format!("{on} · at most {}", cap(l)),
                &[&l.target],
            )),
            Some(b) if cap(b) != cap(l) || b.when_spent != l.when_spent => out.push(line(
                Sign::Changed,
                format!("{on} · at most {} (was {})", cap(l), cap(b)),
                &[&l.target],
            )),
            _ => {}
        }
    }
    for b in &before.limits {
        if !after.limits.iter().any(|l| l.id == b.id) {
            let on = before.node_name(&b.target).to_owned();
            out.push(line(
                Sign::Removed,
                format!("{on} · limit of {}", cap(b)),
                &[],
            ));
        }
    }
    // Wires between cards that were there before and are still: a new
    // card's own wires go without saying.
    let node = |e: &str| e.split(':').next().unwrap_or(e).to_owned();
    let both = |id: &str| {
        let has = |m: &Mission| {
            m.formations.iter().any(|f| f.id == id)
                || m.gates.iter().any(|g| g.id == id)
                || m.ends.iter().any(|e| e.id == id)
                || m.input_cards.iter().any(|c| c.id == id)
        };
        has(before) && has(after)
    };
    let says = |m: &Mission, c: &ahjo_missions_contract::Connection| {
        let (from, to) = (node(&c.from), node(&c.to));
        let port = c.from.split_once(':').map_or("", |(_, p)| p);
        let how = match port {
            "pass" => " passes",
            "fail" => " fails",
            "judge" => return None,
            _ => "",
        };
        Some(format!(
            "{}{how} → {}",
            m.node_name(&from),
            m.node_name(&to)
        ))
    };
    for c in &after.connections {
        let (from, to) = (node(&c.from), node(&c.to));
        if both(&from)
            && both(&to)
            && !before
                .connections
                .iter()
                .any(|b| b.from == c.from && b.to == c.to)
            && let Some(text) = says(after, c)
        {
            out.push(line(Sign::Added, text, &[&from, &to]));
        }
    }
    for c in &before.connections {
        let (from, to) = (node(&c.from), node(&c.to));
        if both(&from)
            && both(&to)
            && !after
                .connections
                .iter()
                .any(|a| a.from == c.from && a.to == c.to)
            && let Some(text) = says(before, c)
        {
            out.push(line(Sign::Removed, text, &[&from, &to]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission(raw: &str) -> Mission {
        Mission::parse(raw).unwrap()
    }

    const BEFORE: &str = r#"
schema = 2
id = "msn_x"
slug = "x"
title = "X"
rev = 1
[[formation]]
id = "fmn_build"
type = "solo"
title = "Build"
[[formation.slot]]
id = "slot_build"
label = "Builder"
agent = "builder"
harness = "claude-code"
effort = "medium"
"#;

    #[test]
    fn a_changed_effort_and_a_new_step_read_as_lines_on_their_cards() {
        let before = mission(BEFORE);
        let after = mission(
            &(BEFORE
                .replace("rev = 1", "rev = 2")
                .replace("effort = \"medium\"", "effort = \"high\"")
                + r#"
[[formation]]
id = "fmn_ship"
type = "solo"
title = "Ship"
"#),
        );
        let lines = diff(&before, &after);
        assert_eq!(
            lines,
            vec![
                line(
                    Sign::Changed,
                    "Build · effort high (was medium)".into(),
                    &["fmn_build"]
                ),
                line(Sign::Added, "Ship".into(), &["fmn_ship"]),
            ]
        );
    }

    #[test]
    fn saves_in_a_burst_by_one_person_read_as_one_change() {
        let at = |rev: i64, by: &str, when: &str| {
            mission(&BEFORE.replace(
                "rev = 1",
                &format!("rev = {rev}\nupdated_by = \"{by}\"\nupdated_at = \"{when}\""),
            ))
        };
        let mut second = at(2, "human:ui", "2026-11-18T09:05:00Z");
        second.formations[0].slots[0].effort = "high".into();
        let mut third = at(3, "human:ui", "2026-11-18T09:09:00Z");
        third.formations[0].slots[0].effort = "low".into();
        let all = changes(&[
            at(1, "agent:planner", "2026-11-18T09:00:00Z"),
            second,
            third,
        ]);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].rev, 3);
        assert_eq!(all[1].lines[0].text, "Build · effort low (was medium)");
    }

    #[test]
    fn a_revision_that_changed_nothing_shown_is_left_out() {
        let a = mission(BEFORE);
        let b = mission(&BEFORE.replace("rev = 1", "rev = 2"));
        let all = changes(&[a, b]);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].lines[0].text, "made it, 1 step");
    }
}
