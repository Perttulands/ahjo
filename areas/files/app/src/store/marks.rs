//! What changed in a file since its last commit, line by line, read from
//! its unified diff: the marks the reader shows beside the lines.
use parvi_app_kit::widgets::code::Mark;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Each new or changed line, and each place lines were taken out
    /// (0-based lines of the file as it is now), in order.
    pub marks: Vec<(usize, Mark)>,
    /// Lines in and out, as `git diff --numstat` counts them.
    pub added: usize,
    pub removed: usize,
    /// The commit does not have it: every line is new, and none is marked.
    pub new_file: bool,
}

impl Changes {
    /// How many separate changes there are to step through.
    pub fn count(&self) -> usize {
        parvi_app_kit::widgets::code::change_starts(&self.marks).len()
    }
}

/// `@@ -a,b +c,d @@`: where the hunk starts in the old and the new file.
fn hunk(line: &str) -> Option<(usize, usize, usize)> {
    let mut parts = line.strip_prefix("@@ ")?.split(' ');
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let start = |r: &str| r.split(',').next()?.parse::<usize>().ok();
    let len = |r: &str| r.split(',').nth(1).map_or(Some(1), |n| n.parse().ok());
    Some((start(old)?, len(old)?, start(new)?))
}

pub fn of_diff(diff: &str) -> Changes {
    let mut out = Changes::default();
    // The next line of the new file, 1-based, and lines taken out before
    // it that no new line has replaced yet.
    let mut at = 0usize;
    let mut gone = false;
    let mut in_hunk = false;
    let mut hunks = 0;
    let mut only_new = true;
    let flush = |gone: &mut bool, at: usize, out: &mut Changes| {
        if std::mem::take(gone) {
            out.marks.push((at.saturating_sub(1), Mark::Gone));
        }
    };
    for line in diff.lines() {
        let line = line.trim_end_matches('\r');
        if let Some((old_start, old_len, new_start)) = hunk(line) {
            flush(&mut gone, at, &mut out);
            in_hunk = true;
            hunks += 1;
            only_new &= old_start == 0 && old_len == 0;
            at = new_start.max(1);
            continue;
        }
        if !in_hunk || line.starts_with('\\') {
            continue;
        }
        if line.starts_with('+') {
            // A line in where one went out is the same line, changed.
            gone = false;
            out.marks.push((at - 1, Mark::New));
            out.added += 1;
            at += 1;
        } else if line.starts_with('-') {
            gone = true;
            out.removed += 1;
        } else {
            flush(&mut gone, at, &mut out);
            at += 1;
        }
    }
    flush(&mut gone, at, &mut out);
    if hunks > 0 && only_new {
        out.new_file = true;
        out.marks.clear();
    }
    out.marks.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_changed_lines_are_new_and_lines_taken_out_mark_where_they_were() {
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,6 +1,7 @@\n one\n-two\n+TWO\n three\n+three and a half\n four\n-five\n six\n@@ -20,2 +21,1 @@\n twenty\n-gone at the end\n\\ No newline at end of file\n";
        let c = of_diff(diff);
        assert_eq!(
            c.marks,
            vec![
                (1, Mark::New),
                (3, Mark::New),
                (5, Mark::Gone),
                (21, Mark::Gone)
            ]
        );
        assert_eq!((c.added, c.removed, c.new_file), (2, 3, false));
        assert_eq!(c.count(), 4);
        let runs = of_diff("@@ -1,2 +1,4 @@\n a\n+b\n+c\n d\n");
        assert_eq!(runs.count(), 1, "lines in a row are one change");
    }

    #[test]
    fn a_file_the_commit_does_not_have_is_new_not_marked_line_by_line() {
        let c = of_diff("@@ -0,0 +1,2 @@\n+a\n+b\n");
        assert!(c.new_file);
        assert!(c.marks.is_empty());
        assert_eq!(c.added, 2);
        assert_eq!(of_diff(""), Changes::default());
    }
}
