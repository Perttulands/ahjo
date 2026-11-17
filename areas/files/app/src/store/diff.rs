//! A unified diff read into lines with their numbers in the file before
//! and after, and those lines paired for a side-by-side view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hunk,
    Add,
    Delete,
    Context,
}

/// One line of the diff: its numbers in the old file and the new (from 1),
/// where it is in them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

/// The lines of `diff` from its first hunk on, the git headers left out.
pub fn rows(diff: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut old, mut new) = (0, 0);
    let mut started = false;
    for line in diff.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(hunk) = line.strip_prefix("@@") {
            // `@@ -a,b +c,d @@`: where the hunk starts in each file.
            let mut starts = hunk.split_whitespace().take(2).map(|s| {
                s[1..]
                    .split(',')
                    .next()
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(1)
            });
            old = starts.next().unwrap_or(1);
            new = starts.next().unwrap_or(1);
            started = true;
            rows.push(Row {
                kind: Kind::Hunk,
                old: None,
                new: None,
                text: line.to_owned(),
            });
            continue;
        }
        if !started || line.starts_with('\\') {
            continue;
        }
        let (kind, text) = if let Some(t) = line.strip_prefix('+') {
            (Kind::Add, t)
        } else if let Some(t) = line.strip_prefix('-') {
            (Kind::Delete, t)
        } else {
            (Kind::Context, line.strip_prefix(' ').unwrap_or(line))
        };
        let (o, n) = match kind {
            Kind::Add => (None, Some(new)),
            Kind::Delete => (Some(old), None),
            _ => (Some(old), Some(new)),
        };
        if o.is_some() {
            old += 1;
        }
        if n.is_some() {
            new += 1;
        }
        rows.push(Row {
            kind,
            old: o,
            new: n,
            text: text.to_owned(),
        });
    }
    rows
}

/// A line of the side-by-side view: the old line on the left, the new on
/// the right; a change pairs what was taken out with what came in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub old: Option<(usize, String)>,
    pub new: Option<(usize, String)>,
    /// A hunk's head, across both sides.
    pub hunk: Option<String>,
    /// Taken out, put in or changed, not kept as it was.
    pub changed: bool,
}

pub fn pairs(rows: &[Row]) -> Vec<Pair> {
    let mut out = vec![];
    let mut i = 0;
    while i < rows.len() {
        let r = &rows[i];
        match r.kind {
            Kind::Hunk => {
                out.push(Pair {
                    old: None,
                    new: None,
                    hunk: Some(r.text.clone()),
                    changed: false,
                });
                i += 1;
            }
            Kind::Context => {
                out.push(Pair {
                    old: r.old.map(|n| (n, r.text.clone())),
                    new: r.new.map(|n| (n, r.text.clone())),
                    hunk: None,
                    changed: false,
                });
                i += 1;
            }
            Kind::Add | Kind::Delete => {
                // A run of lines taken out then put in, paired line by line.
                let gone: Vec<&Row> = rows[i..]
                    .iter()
                    .take_while(|r| r.kind == Kind::Delete)
                    .collect();
                i += gone.len();
                let came: Vec<&Row> = rows[i..]
                    .iter()
                    .take_while(|r| r.kind == Kind::Add)
                    .collect();
                i += came.len();
                for k in 0..gone.len().max(came.len()) {
                    out.push(Pair {
                        old: gone.get(k).map(|r| (r.old.unwrap_or(0), r.text.clone())),
                        new: came.get(k).map(|r| (r.new.unwrap_or(0), r.text.clone())),
                        hunk: None,
                        changed: true,
                    });
                }
            }
        }
    }
    out
}

/// Where each run of changed lines starts, among `changed` (one flag per
/// line shown): what stepping through the changes stops at.
pub fn change_starts(changed: impl Iterator<Item = bool>) -> Vec<usize> {
    let mut out = vec![];
    let mut before = false;
    for (i, c) in changed.enumerate() {
        if c && !before {
            out.push(i);
        }
        before = c;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -3,4 +3,4 @@ fn x\n kept\n-old one\n-old two\n+new one\n same\n\\ No newline at end of file\n+added\n";

    #[test]
    fn lines_carry_their_numbers_in_each_file() {
        let got: Vec<_> = rows(DIFF)
            .into_iter()
            .map(|r| (r.kind, r.old, r.new, r.text))
            .collect();
        assert_eq!(
            got,
            [
                (Kind::Hunk, None, None, "@@ -3,4 +3,4 @@ fn x".into()),
                (Kind::Context, Some(3), Some(3), "kept".into()),
                (Kind::Delete, Some(4), None, "old one".into()),
                (Kind::Delete, Some(5), None, "old two".into()),
                (Kind::Add, None, Some(4), "new one".into()),
                (Kind::Context, Some(6), Some(5), "same".into()),
                (Kind::Add, None, Some(6), "added".into()),
            ]
        );
        assert!(rows("diff --git a/x b/x\n").is_empty());
    }

    #[test]
    fn side_by_side_pairs_what_went_with_what_came() {
        let p = pairs(&rows(DIFF));
        let sides: Vec<_> = p
            .iter()
            .map(|p| {
                (
                    p.old.as_ref().map(|o| o.0),
                    p.new.as_ref().map(|n| n.0),
                    p.changed,
                )
            })
            .collect();
        assert_eq!(
            sides,
            [
                (None, None, false),
                (Some(3), Some(3), false),
                (Some(4), Some(4), true),
                (Some(5), None, true),
                (Some(6), Some(5), false),
                (None, Some(6), true),
            ]
        );
        assert_eq!(
            change_starts(p.iter().map(|p| p.changed)),
            [2, 5],
            "a run of changes is one stop"
        );
    }
}
