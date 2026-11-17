//! An item's text travels apart from the snapshot. The channel sends each
//! item without it; a page reads one item's text when it shows the item,
//! and asks the node which items' text holds the words it searches for
//! (`docs/work.md`, The seam).

use crate::model::{ProjectView, Snapshot};

impl Snapshot {
    /// This snapshot with every item's text left out, as the channel sends
    /// it: the text is most of a snapshot's size.
    pub fn lean(&self) -> Snapshot {
        let mut lean = self.clone();
        for project in &mut lean.projects {
            match project {
                ProjectView::Beads(store) => {
                    for item in store.items.iter_mut().flatten() {
                        item.body = String::new();
                    }
                }
                ProjectView::Github(repo) => {
                    if let Some(work) = &mut repo.work {
                        work.issues.iter_mut().for_each(|i| i.body = String::new());
                        work.pulls.iter_mut().for_each(|p| p.body = String::new());
                    }
                }
            }
        }
        lean
    }

    /// The text of the item `reference` names, as last read; `None` when
    /// this snapshot does not hold it.
    pub fn text(&self, reference: &str) -> Option<&str> {
        self.texts()
            .find(|(r, _)| r == reference)
            .map(|(_, text)| text.body)
    }

    /// The references of the items holding every one of `terms` (see
    /// [`terms`]) in their id, title, labels or text.
    pub fn matching(&self, terms: &[String]) -> Vec<String> {
        if terms.is_empty() {
            return vec![];
        }
        self.texts()
            .filter(|(_, text)| {
                let haystack = text.haystack();
                terms.iter().all(|t| haystack.contains(t.as_str()))
            })
            .map(|(reference, _)| reference)
            .collect()
    }

    /// Every item's reference with its searchable words.
    fn texts(&self) -> impl Iterator<Item = (String, Text<'_>)> {
        self.projects.iter().flat_map(|project| {
            let all: Vec<(String, Text<'_>)> = match project {
                ProjectView::Beads(store) => store
                    .items
                    .iter()
                    .flatten()
                    .map(|i| {
                        let text = Text {
                            id: i.id.clone(),
                            title: &i.title,
                            labels: &i.labels,
                            body: &i.body,
                        };
                        (i.reference.clone(), text)
                    })
                    .collect(),
                ProjectView::Github(repo) => repo
                    .work
                    .iter()
                    .flat_map(|w| {
                        let issues = w
                            .issues
                            .iter()
                            .map(|i| (i.number, &i.title, &i.labels, &i.body));
                        let pulls = w
                            .pulls
                            .iter()
                            .map(|p| (p.number, &p.title, &p.labels, &p.body));
                        issues.chain(pulls)
                    })
                    .map(|(n, title, labels, body)| {
                        let text = Text {
                            id: format!("#{n}"),
                            title,
                            labels,
                            body,
                        };
                        (github_reference(&repo.name, n), text)
                    })
                    .collect(),
            };
            all
        })
    }
}

/// The canonical reference of a repository's issue or pull request.
pub fn github_reference(repo: &str, number: u32) -> String {
    format!("gh:{repo}#{number}")
}

/// The words a search holds every one of: lowercase, split on space, an
/// issue's `#` dropped.
pub fn terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(|t| t.trim_start_matches('#').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// What a search looks in, lowercase: the id, title, text and labels on
/// lines of their own.
pub fn haystack(id: &str, title: &str, body: &str, labels: &[String]) -> String {
    [id, title, body, &labels.join(" ")]
        .join("\n")
        .to_lowercase()
}

struct Text<'a> {
    id: String,
    title: &'a str,
    labels: &'a [String],
    body: &'a str,
}

impl Text<'_> {
    fn haystack(&self) -> String {
        haystack(&self.id, self.title, self.body, self.labels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Issue, IssueState, RepoView, StoreView, Work};
    use crate::{Item, Snapshot};

    fn snapshot() -> Snapshot {
        let bead = |id: &str, title: &str, body: &str| Item {
            reference: format!("bead:n:/srv/q#{id}"),
            id: id.into(),
            title: title.into(),
            body: body.into(),
            ..Item::default()
        };
        let issue = Issue {
            number: 12,
            title: "Crash on start".into(),
            url: String::new(),
            state: IssueState::Open,
            author: None,
            created: String::new(),
            updated: String::new(),
            body: "The window opens, then the GPU driver resets.".into(),
            labels: vec!["bug".into()],
            assignees: vec![],
            branches: vec![],
            pulls: vec![],
        };
        Snapshot {
            refreshed: 3,
            projects: vec![
                ProjectView::Beads(StoreView {
                    folder: "/srv/q".into(),
                    items: Some(vec![
                        bead("q-1", "Which stores", "Read every store the node names."),
                        bead("q-2", "Quiet pages", ""),
                    ]),
                    ..StoreView::default()
                }),
                ProjectView::Github(RepoView {
                    name: "o/r".into(),
                    work: Some(Work {
                        issues: vec![issue],
                        ..Work::default()
                    }),
                    ..RepoView::default()
                }),
            ],
        }
    }

    #[test]
    fn the_lean_snapshot_keeps_everything_but_the_text() {
        let full = snapshot();
        let lean = full.lean();
        assert_eq!(lean.text("bead:n:/srv/q#q-1"), Some(""));
        assert_eq!(lean.text("gh:o/r#12"), Some(""));
        assert!(lean.matching(&terms("driver")).is_empty());
        assert_eq!(lean.matching(&terms("crash bug")), ["gh:o/r#12"]);
        assert_eq!(lean.refreshed, full.refreshed);
        assert_eq!(lean.lean(), lean);
    }

    #[test]
    fn an_item_s_text_is_found_by_its_reference() {
        let s = snapshot();
        assert_eq!(
            s.text("bead:n:/srv/q#q-1"),
            Some("Read every store the node names.")
        );
        assert_eq!(
            s.text(&github_reference("o/r", 12)),
            Some("The window opens, then the GPU driver resets.")
        );
        assert_eq!(s.text("bead:n:/srv/q#q-9"), None);
    }

    #[test]
    fn a_search_needs_every_word_somewhere_in_the_item() {
        let s = snapshot();
        assert_eq!(s.matching(&terms("STORE node")), ["bead:n:/srv/q#q-1"]);
        assert_eq!(s.matching(&terms("#12 driver")), ["gh:o/r#12"]);
        assert_eq!(s.matching(&terms("crash bug")), ["gh:o/r#12"]);
        assert!(s.matching(&terms("store driver")).is_empty());
        assert!(s.matching(&terms("  ")).is_empty());
    }
}
