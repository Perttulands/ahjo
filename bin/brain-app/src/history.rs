//! The brain's history, from its Git log: which pages were worked on, day by
//! day, for the replay.
//!
//! A page moved keeps its history: renames are followed back, so a change
//! made before a move is told under the page's name now. A commit that
//! touches a great many pages at once is a reorganisation (a move of whole
//! collections, a reformatting pass), not attention paid to them, so it is
//! left out and only counted.

use std::collections::HashMap;
use std::path::Path;

/// A commit touching more pages than this reshuffled the brain.
pub const RESHUFFLE: usize = 300;

/// One day the brain was worked on, and the pages, by their paths now.
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    /// A Julian day, in UTC.
    pub day: i32,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    /// Oldest first, only days something was done.
    pub days: Vec<Day>,
    /// Commits left out as reorganisations.
    pub reshuffles: usize,
}

/// Reads the brain's log with Git. Blocking: run it off the UI.
pub fn read(root: &Path) -> Result<History, String> {
    let mut git = std::process::Command::new("git");
    git.arg("-C")
        .arg(root)
        // A brain reached across WSL's share is owned by another user as
        // Windows sees it; reading its log is all this does.
        .args(["-c", "safe.directory=*"])
        .args(["-c", "diff.renameLimit=20000", "-c", "core.quotePath=false"])
        .args(["log", "-M", "--format=@%at", "--name-status", "--", "*.md"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        git.creation_flags(CREATE_NO_WINDOW);
    }
    let out = git.output().map_err(|e| {
        format!("the replay reads the brain's history with git, which did not run: {e}")
    })?;
    if !out.status.success() {
        return Err(format!(
            "git could not read the brain's history: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// `git log -M --format=@%at --name-status`, newest first, as days.
pub fn parse(log: &str) -> History {
    // Newest first: a rename seen now tells what an older name became.
    let mut became: HashMap<String, String> = HashMap::new();
    let now = |path: &str, became: &HashMap<String, String>| {
        became
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_string())
    };
    let mut commits: Vec<(i64, Vec<String>)> = vec![];
    for line in log.lines() {
        if let Some(at) = line.strip_prefix('@') {
            commits.push((at.trim().parse().unwrap_or(0), vec![]));
            continue;
        }
        let Some((_, touched)) = commits.last_mut() else {
            continue;
        };
        let mut fields = line.split('\t');
        let status = fields.next().unwrap_or("");
        match (status.chars().next(), fields.next(), fields.next()) {
            (Some('R'), Some(old), Some(new)) => {
                let current = now(new, &became);
                became.insert(old.to_string(), current.clone());
                touched.push(current);
            }
            (Some('A' | 'M' | 'C'), Some(path), _) => touched.push(now(path, &became)),
            _ => {}
        }
    }
    let mut by_day: HashMap<i32, Vec<String>> = HashMap::new();
    let mut reshuffles = 0;
    for (at, touched) in commits {
        if touched.len() > RESHUFFLE {
            reshuffles += 1;
            continue;
        }
        let day = (at.div_euclid(86_400) + 2_440_588) as i32;
        by_day.entry(day).or_default().extend(touched);
    }
    let mut days: Vec<Day> = by_day
        .into_iter()
        .map(|(day, mut paths)| {
            paths.sort();
            paths.dedup();
            Day { day, paths }
        })
        .collect();
    days.sort_by_key(|d| d.day);
    History { days, reshuffles }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_is_told_by_day_under_the_names_pages_have_now() {
        let day = 86_400;
        let big: String = (0..=RESHUFFLE).map(|i| format!("M\tp{i}.md\n")).collect();
        let log = format!(
            "@{}\nM\tknowledge/sea/tide.md\n\n@{}\n{big}\n@{}\nR087\tnotes/tide.md\tknowledge/sea/tide.md\nA\tknowledge/sea/moon.md\n\n@{}\nM\tnotes/tide.md\n@{}\nA\tnotes/tide.md\n",
            3 * day + 50,
            3 * day,
            2 * day + 10,
            day + 30,
            day + 20,
        );
        let h = parse(&log);
        assert_eq!(h.reshuffles, 1);
        let jd = |d: i64| (d + 2_440_588) as i32;
        assert_eq!(
            h.days,
            [
                Day {
                    day: jd(1),
                    paths: vec!["knowledge/sea/tide.md".into()],
                },
                Day {
                    day: jd(2),
                    paths: vec![
                        "knowledge/sea/moon.md".into(),
                        "knowledge/sea/tide.md".into()
                    ],
                },
                Day {
                    day: jd(3),
                    paths: vec!["knowledge/sea/tide.md".into()],
                },
            ]
        );
    }
}
