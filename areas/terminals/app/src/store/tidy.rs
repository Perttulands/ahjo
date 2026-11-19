//! What could be cleared away: tags whose session has ended, and sessions
//! nobody has used for a day or more. The page offers them once, quietly;
//! clearing an ended tag frees a window, and a live session goes only on
//! the operator's own click. What was offered and waved away is not
//! offered again until something new joins it.

use super::sessions::{Key, waiting};
use super::workspace::Workspace;
use ahjo_terminals_contract::Session;

/// How long a session sits unused before it is offered for clearing.
pub const IDLE_LONG_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tidy {
    /// Bound tags whose session has ended, each once.
    pub ended: Vec<Key>,
    /// Live sessions unused a day or more, the longest first, with how
    /// long.
    pub idle: Vec<(Key, u64)>,
}

impl Tidy {
    pub fn is_empty(&self) -> bool {
        self.ended.is_empty() && self.idle.is_empty()
    }

    /// Every key it holds, as `Key::text`, to remember it waved away.
    pub fn texts(&self) -> Vec<String> {
        let mut out: Vec<String> = self.ended.iter().map(Key::text).collect();
        out.extend(self.idle.iter().map(|(k, _)| k.text()));
        out.sort();
        out
    }

    /// Whether it holds anything not among `seen`.
    pub fn news(&self, seen: &[String]) -> bool {
        self.texts().iter().any(|k| !seen.contains(k))
    }

    /// The line that offers it: what there is, in a few words.
    pub fn line(&self) -> String {
        let mut parts = vec![];
        match self.ended.len() {
            0 => {}
            1 => parts.push("1 ended session".to_string()),
            n => parts.push(format!("{n} ended sessions")),
        }
        match self.idle.len() {
            0 => {}
            n => parts.push(format!("{n} idle over a day")),
        }
        parts.join(" · ")
    }
}

/// What could be cleared: tags `ended` says have ended, and `sessions`
/// unused since a day before `now_ms`. A session that works, waits for
/// an answer, or is the one a window shows is in use.
pub fn find(
    ws: &Workspace,
    ended: impl Fn(&Key) -> bool,
    sessions: &[Session],
    now_ms: u64,
) -> Tidy {
    let mut out = Tidy::default();
    for b in ws.tabs[..ws.count]
        .iter()
        .flat_map(|t| t.shown())
        .flat_map(|w| &w.sessions)
    {
        if ended(&b.key) && !out.ended.contains(&b.key) {
            out.ended.push(b.key.clone());
        }
    }
    let on_screen: Vec<Key> = ws.on_screen().map(|b| b.key.clone()).collect();
    for s in sessions {
        let key = Key::of(s);
        let quiet = now_ms.saturating_sub(s.last.unwrap_or(now_ms));
        let asks = waiting(s).is_some()
            || s.event.as_ref().is_some_and(|e| {
                !e.seen && e.kind == ahjo_terminals_contract::EventKind::NeedsInput
            });
        if s.active != Some(true) && !asks && quiet >= IDLE_LONG_MS && !on_screen.contains(&key) {
            out.idle.push((key, quiet));
        }
    }
    out.idle.sort_by_key(|(_, quiet)| std::cmp::Reverse(*quiet));
    out
}

impl Workspace {
    /// Takes the tags of `ended` sessions out of every window; a window
    /// left empty closes, unless it is its tab's last. Returns how many
    /// tags went.
    pub fn clear_ended(&mut self, ended: &[Key]) -> usize {
        let mut gone = 0;
        let focused = self.shown(self.focus).map(|b| b.key.clone());
        for t in &mut self.tabs {
            for w in (0..t.count).rev() {
                let before = t.windows[w].sessions.len();
                t.windows[w].sessions.retain(|b| !ended.contains(&b.key));
                let after = t.windows[w].sessions.len();
                if after == before {
                    continue;
                }
                gone += before - after;
                let win = &mut t.windows[w];
                win.active = win.active.min(after.saturating_sub(1));
                if after == 0 {
                    t.hide(w);
                }
            }
        }
        let n = self.windows().len();
        self.focus = focused
            .and_then(|k| {
                self.windows()
                    .iter()
                    .position(|w| w.shown().is_some_and(|b| b.key == k))
            })
            .unwrap_or(self.focus)
            .min(n - 1);
        gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::layout::Side;

    const NOW: u64 = 10 * IDLE_LONG_MS;

    fn session(name: &str, idle: u64) -> Session {
        let mut s: Session = serde_json::from_value(serde_json::json!({
            "id": format!("${name}"), "name": name, "panes": [],
            "active": false, "last": NOW - idle,
        }))
        .unwrap();
        s.node = "n".into();
        s
    }

    fn key(name: &str) -> Key {
        Key {
            node: "n".into(),
            user: String::new(),
            name: name.into(),
        }
    }

    #[test]
    fn ended_tags_and_long_idle_sessions_are_offered_and_used_ones_are_not() {
        let mut ws = Workspace::default();
        ws.bind(0, key("gone"), None);
        ws.split(0, Side::Right);
        ws.bind(1, key("looked-at"), None);
        let mut working = session("working", 3 * IDLE_LONG_MS);
        working.active = Some(true);
        let mut asking = session("asking", 3 * IDLE_LONG_MS);
        asking.event = serde_json::from_value(
            serde_json::json!({"kind": "needs-input", "at": 1, "seen": false}),
        )
        .unwrap();
        let sessions = [
            session("old", 2 * IDLE_LONG_MS),
            session("older", 5 * IDLE_LONG_MS),
            session("fresh", IDLE_LONG_MS / 2),
            session("looked-at", 4 * IDLE_LONG_MS),
            working,
            asking,
        ];
        let t = find(&ws, |k| k.name == "gone", &sessions, NOW);
        assert_eq!(t.ended, [key("gone")]);
        let idle: Vec<&str> = t.idle.iter().map(|(k, _)| k.name.as_str()).collect();
        assert_eq!(idle, ["older", "old"], "the longest first");
        assert_eq!(t.line(), "1 ended session · 2 idle over a day");
        // Waved away, it stays quiet until something new joins it.
        let seen = t.texts();
        assert!(!t.news(&seen));
        let more = find(
            &ws,
            |k| k.name == "gone",
            &[session("other", 2 * IDLE_LONG_MS)],
            NOW,
        );
        assert!(more.news(&seen));
        assert!(find(&Workspace::default(), |_| true, &[], NOW).is_empty());
    }

    #[test]
    fn clearing_ended_tags_closes_the_windows_they_leave_empty() {
        let mut ws = Workspace::default();
        ws.bind(0, key("gone"), None);
        ws.split(0, Side::Right);
        ws.bind(1, key("kept"), None);
        ws.bind(1, key("gone-too"), None);
        ws.focus = 1;
        assert_eq!(ws.clear_ended(&[key("gone"), key("gone-too")]), 2);
        assert_eq!(ws.windows().len(), 1);
        assert_eq!(ws.shown(0).unwrap().key, key("kept"));
        assert_eq!(ws.focus, 0);
        // The last window of a tab stays, empty.
        assert_eq!(ws.clear_ended(&[key("kept")]), 1);
        assert_eq!(ws.windows().len(), 1);
        assert!(ws.windows()[0].is_empty());
    }
}
