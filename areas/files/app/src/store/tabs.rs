//! The files open in tabs: which, in what order, which one only previews
//! (the next file previewed takes its place), the order they were last
//! shown in, and the ones closed, to open again.

/// A file on a node: (node, path).
pub type Key = (String, String);

/// Tabs closed remembered to open again.
const CLOSED: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub key: Key,
    /// Shown in passing: the next file previewed replaces it. Editing it,
    /// opening it on purpose or a double click keeps it.
    pub preview: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Tabs {
    pub open: Vec<Tab>,
    /// The tabs as last shown, the one shown now first.
    shown: Vec<Key>,
    closed: Vec<Key>,
}

impl Tabs {
    pub fn at(&self, key: &Key) -> Option<usize> {
        self.open.iter().position(|t| &t.key == key)
    }

    /// The tabs in the order last shown, the one shown now first.
    pub fn recent(&self) -> &[Key] {
        &self.shown
    }

    /// Shows `key` in a tab: its own when it has one (kept when `keep`),
    /// else a new one after `after`. A preview takes the place of the
    /// preview there was.
    pub fn show(&mut self, key: Key, keep: bool, after: Option<usize>) {
        match self.at(&key) {
            Some(i) => {
                if keep {
                    self.open[i].preview = false;
                }
            }
            None => {
                let tab = Tab {
                    key: key.clone(),
                    preview: !keep,
                };
                // Only a preview takes the place of the one before it.
                match self.open.iter().position(|t| t.preview && !keep) {
                    Some(i) => {
                        let was = std::mem::replace(&mut self.open[i], tab);
                        self.shown.retain(|k| *k != was.key);
                    }
                    None => {
                        let at = after.map_or(self.open.len(), |a| (a + 1).min(self.open.len()));
                        self.open.insert(at, tab);
                    }
                }
            }
        }
        self.shown.retain(|k| *k != key);
        self.shown.insert(0, key);
    }

    /// Keeps a preview open: it is not replaced by the next one.
    pub fn keep(&mut self, key: &Key) {
        if let Some(i) = self.at(key) {
            self.open[i].preview = false;
        }
    }

    pub fn is_preview(&self, key: &Key) -> bool {
        self.at(key).is_some_and(|i| self.open[i].preview)
    }

    /// Closes `key`'s tab; answers the tab to show in its place: the one
    /// shown before it.
    pub fn close(&mut self, key: &Key) -> Option<Key> {
        let i = self.at(key)?;
        self.open.remove(i);
        self.shown.retain(|k| k != key);
        self.closed.retain(|k| k != key);
        self.closed.push(key.clone());
        if self.closed.len() > CLOSED {
            self.closed.remove(0);
        }
        self.shown.first().cloned()
    }

    /// The tab closed last that is not open again, taken off the list.
    pub fn reopen(&mut self) -> Option<Key> {
        while let Some(key) = self.closed.pop() {
            if self.at(&key).is_none() {
                return Some(key);
            }
        }
        None
    }

    /// Every tab but `key`'s and those `stays` says must stay (unsaved
    /// work): answers the tabs to close.
    pub fn others(&self, key: &Key, stays: impl Fn(&Key) -> bool) -> Vec<Key> {
        self.open
            .iter()
            .map(|t| &t.key)
            .filter(|k| *k != key && !stays(k))
            .cloned()
            .collect()
    }

    /// The tabs right of `key`'s.
    pub fn right_of(&self, key: &Key, stays: impl Fn(&Key) -> bool) -> Vec<Key> {
        let Some(i) = self.at(key) else {
            return vec![];
        };
        self.open[i + 1..]
            .iter()
            .map(|t| &t.key)
            .filter(|k| !stays(k))
            .cloned()
            .collect()
    }

    /// The tab `by` places from `key`'s, round the ends.
    pub fn beside(&self, key: &Key, by: isize) -> Option<Key> {
        let n = self.open.len() as isize;
        let i = self.at(key)? as isize;
        (n > 1).then(|| self.open[(i + by).rem_euclid(n) as usize].key.clone())
    }

    /// A file renamed or saved as another: its tab follows it.
    pub fn moved(&mut self, from: &Key, to: Key) {
        for t in &mut self.open {
            if &t.key == from {
                t.key = to.clone();
            }
        }
        for k in self.shown.iter_mut().chain(self.closed.iter_mut()) {
            if k == from {
                *k = to.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(p: &str) -> Key {
        ("n".into(), p.into())
    }
    fn paths(t: &Tabs) -> Vec<(String, bool)> {
        t.open
            .iter()
            .map(|t| (t.key.1.clone(), t.preview))
            .collect()
    }

    #[test]
    fn a_preview_gives_way_to_the_next_until_it_is_kept() {
        let mut t = Tabs::default();
        t.show(k("a"), true, None);
        t.show(k("b"), false, Some(0));
        t.show(k("c"), false, Some(0));
        assert_eq!(paths(&t), [("a".into(), false), ("c".into(), true)]);
        t.keep(&k("c"));
        t.show(k("d"), false, Some(1));
        assert_eq!(
            paths(&t),
            [("a".into(), false), ("c".into(), false), ("d".into(), true)]
        );
        // Opened on purpose, a previewed file is kept where it stands.
        t.show(k("d"), true, Some(0));
        assert!(!t.is_preview(&k("d")));
        t.show(k("e"), true, Some(0));
        assert_eq!(
            t.open[1].key,
            k("e"),
            "a kept tab opens beside the one shown"
        );
        // A kept tab opened beside a preview leaves the preview be.
        t.show(k("f"), false, None);
        t.show(k("g"), true, None);
        assert!(t.is_preview(&k("f")), "{:?}", paths(&t));
        assert!(t.at(&k("g")).is_some());
    }

    #[test]
    fn closing_shows_the_tab_shown_before_and_reopens_in_turn() {
        let mut t = Tabs::default();
        for p in ["a", "b", "c"] {
            t.show(k(p), true, None);
        }
        t.show(k("a"), false, None);
        assert_eq!(t.recent(), [k("a"), k("c"), k("b")]);
        assert_eq!(t.close(&k("a")), Some(k("c")));
        assert_eq!(t.close(&k("b")), Some(k("c")));
        assert_eq!(t.reopen(), Some(k("b")));
        t.show(k("b"), true, None);
        assert_eq!(t.reopen(), Some(k("a")));
        assert_eq!(t.reopen(), None);
        assert_eq!(t.beside(&k("c"), 1), Some(k("b")));
        assert_eq!(t.beside(&k("c"), -1), Some(k("b")));
        assert_eq!(t.others(&k("c"), |key| *key == k("b")), Vec::<Key>::new());
        assert_eq!(t.right_of(&k("c"), |_| false), [k("b")]);
    }
}
