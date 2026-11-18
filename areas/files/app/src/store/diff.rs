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

/// Unchanged lines kept in view either side of a change.
pub const CONTEXT: usize = 3;
/// A run of unchanged lines folds only when it hides more than this: a
/// fold of one or two lines takes the room the lines would.
const FOLD_LEAST: usize = 3;

/// A row of the changes view: one line of the diff (an index among its
/// lines), or unchanged lines folded away, `from..to` among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Line(usize),
    Fold { from: usize, to: usize },
}

impl Shown {
    pub fn changed(&self, changed: &[bool]) -> bool {
        match self {
            Shown::Line(i) => changed.get(*i).copied().unwrap_or(false),
            Shown::Fold { .. } => false,
        }
    }
}

/// The diff's lines as the view shows them: each change with `CONTEXT`
/// unchanged lines either side, and every longer unchanged run folded,
/// unless its fold (named by its first line) was opened. `hidden` lines
/// (a whole file's one hunk head, which says nothing) never show.
pub fn fold(
    changed: &[bool],
    hidden: &[bool],
    open: &std::collections::HashSet<usize>,
) -> Vec<Shown> {
    let lines: Vec<usize> = (0..changed.len())
        .filter(|&i| !hidden.get(i).copied().unwrap_or(false))
        .collect();
    let mut out = vec![];
    let mut k = 0;
    while k < lines.len() {
        if changed[lines[k]] {
            out.push(Shown::Line(lines[k]));
            k += 1;
            continue;
        }
        let start = k;
        while k < lines.len() && !changed[lines[k]] {
            k += 1;
        }
        // Context after the change before the run, and before the next.
        let keep_head = if start > 0 { CONTEXT } else { 0 };
        let keep_tail = if k < lines.len() { CONTEXT } else { 0 };
        let run = k - start;
        let hide = run.saturating_sub(keep_head + keep_tail);
        let from = lines[start + keep_head.min(run)];
        if hide <= FOLD_LEAST || open.contains(&from) {
            out.extend(lines[start..k].iter().map(|&i| Shown::Line(i)));
            continue;
        }
        let fold_at = start + keep_head;
        let fold_end = fold_at + hide;
        out.extend(lines[start..fold_at].iter().map(|&i| Shown::Line(i)));
        out.push(Shown::Fold {
            from: lines[fold_at],
            to: lines[fold_end - 1] + 1,
        });
        out.extend(lines[fold_end..k].iter().map(|&i| Shown::Line(i)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_runs_fold_to_their_context_and_open_on_asking() {
        // 20 lines, a change at 10 and one at the last line.
        let mut changed = vec![false; 20];
        changed[10] = true;
        changed[19] = true;
        let none = std::collections::HashSet::new();
        let shown = fold(&changed, &[], &none);
        assert_eq!(shown[0], Shown::Fold { from: 0, to: 7 }, "the start folds");
        assert_eq!(
            &shown[1..5],
            [7, 8, 9, 10].map(Shown::Line),
            "three lines before the change"
        );
        // 11..19 is eight unchanged lines: three after, a fold of two
        // would hide too little, so all of them show.
        assert_eq!(shown.len(), 1 + 4 + 8 + 1);
        let starts = change_starts(shown.iter().map(|s| s.changed(&changed)));
        assert_eq!(starts, [4, 13], "the view opens near the first change");
        let open: std::collections::HashSet<usize> = [0].into();
        assert_eq!(
            fold(&changed, &[], &open).len(),
            20,
            "an opened fold shows all"
        );
        // A long run between changes keeps context either side.
        let mut far = vec![false; 40];
        far[0] = true;
        far[39] = true;
        let shown = fold(&far, &[], &none);
        assert_eq!(shown[4], Shown::Fold { from: 4, to: 36 });
        // The hidden hunk head never shows.
        let mut hidden = vec![false; 40];
        hidden[0] = true;
        assert!(!fold(&far, &hidden, &none).contains(&Shown::Line(0)));
    }

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
