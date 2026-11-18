//! What `work_list` answers: each project's items, one short line each.
//! An item's text and its whole family are `work_show`'s.

use serde::{Deserialize, Serialize};

use crate::model::{IssueState, ProjectView, PullState, Snapshot};
use crate::text::github_reference;
use crate::work_ref::{self, WorkRef, Written};

/// Every configured project, in configuration order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub projects: Vec<Listed>,
}

/// One project and its items.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    /// As `work_create` takes it: a Beads store's folder, or `owner/name`.
    pub project: String,
    /// Why the last read failed; the items are then the last ones read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why Ahjo cannot see the store change, when it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unwatched: Option<String>,
    #[serde(default)]
    pub items: Vec<Line>,
}

/// One item: what tells it apart in a list. Its id is what `work_show`
/// takes: a Bead's id, or an issue's or pull request's `owner/name#number`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub id: String,
    pub title: String,
    /// Beads' status, or GitHub's state.
    pub status: String,
    /// Beads' type (`task`, `epic`, …), or `issue` or `pull` on GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// What must finish before this can.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
}

impl List {
    /// The list of `snapshot`, without closed and merged items unless `all`.
    pub fn of(snapshot: &Snapshot, all: bool) -> List {
        List::keeping(snapshot, |_, status| {
            all || !matches!(status, "closed" | "merged")
        })
    }

    /// The list of `snapshot`'s items that `keep` keeps, given each one's
    /// canonical reference and status.
    pub fn keeping(snapshot: &Snapshot, keep: impl Fn(&str, &str) -> bool) -> List {
        let projects = snapshot
            .projects
            .iter()
            .map(|project| match project {
                ProjectView::Beads(store) => Listed {
                    project: store.folder.clone(),
                    error: store.error.clone(),
                    unwatched: store.unwatched.clone(),
                    items: store
                        .items
                        .iter()
                        .flatten()
                        .filter(|item| keep(&item.reference, &item.status))
                        .map(|item| Line {
                            id: item.id.clone(),
                            title: item.title.clone(),
                            status: item.status.clone(),
                            kind: item.kind.clone(),
                            priority: item.priority,
                            parent: item.family.parent.as_deref().map(short),
                            blocked_by: item.family.blockers.iter().map(|b| short(b)).collect(),
                        })
                        .collect(),
                },
                ProjectView::Github(repo) => {
                    let mut items = Vec::new();
                    let mut add = |number: u32, title: &str, status: String, kind: &str| {
                        if keep(&github_reference(&repo.name, number), &status) {
                            items.push(Line {
                                id: format!("{}#{number}", repo.name),
                                title: title.to_owned(),
                                status,
                                kind: Some(kind.to_owned()),
                                ..Line::default()
                            });
                        }
                    };
                    for issue in repo.work.iter().flat_map(|work| &work.issues) {
                        add(
                            issue.number,
                            &issue.title,
                            issue_state(issue.state),
                            "issue",
                        );
                    }
                    for pull in repo.work.iter().flat_map(|work| &work.pulls) {
                        add(pull.number, &pull.title, pull_state(pull.state), "pull");
                    }
                    Listed {
                        project: repo.name.clone(),
                        error: repo.error.clone(),
                        unwatched: None,
                        items,
                    }
                }
            })
            .collect();
        List { projects }
    }
}

fn issue_state(state: IssueState) -> String {
    match state {
        IssueState::Open => "open",
        IssueState::Closed => "closed",
    }
    .into()
}

fn pull_state(state: PullState) -> String {
    match state {
        PullState::Open => "open",
        PullState::Draft => "draft",
        PullState::Merged => "merged",
        PullState::Closed => "closed",
    }
    .into()
}

/// A family member as `work_show` takes it, shortest: a Bead by its id, an
/// issue as `owner/name#number`.
fn short(reference: &str) -> String {
    match work_ref::parse(reference) {
        Ok(Written::Ref(WorkRef::Bead { id, .. }) | Written::Bead(id)) => id,
        Ok(Written::Ref(WorkRef::Issue {
            owner,
            repo,
            number,
        })) => format!("{owner}/{repo}#{number}"),
        Err(_) => reference.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Issue, Pull, RepoView, StoreView, Work};
    use crate::{Family, Item};

    fn bead(id: &str, status: &str, family: Family) -> Item {
        Item {
            reference: format!("bead:n1:/srv/app#{id}"),
            id: id.into(),
            title: format!("{id} title"),
            status: status.into(),
            kind: Some("task".into()),
            priority: Some(2),
            body: "a long body nobody lists".into(),
            family,
            ..Item::default()
        }
    }

    #[test]
    fn a_list_is_each_item_once_without_its_text_or_whole_family() {
        let snapshot = Snapshot {
            projects: vec![ProjectView::Beads(StoreView {
                folder: "/srv/app".into(),
                items: Some(vec![
                    bead(
                        "app-2",
                        "open",
                        Family {
                            parent: Some("bead:n1:/srv/app#app-1".into()),
                            blockers: vec!["bead:n1:/srv/app#app-3".into(), "ctx-9".into()],
                            ..Family::default()
                        },
                    ),
                    bead("app-3", "closed", Family::default()),
                ]),
                ..StoreView::default()
            })],
            refreshed: 4,
        };
        let open = serde_json::to_value(List::of(&snapshot, false)).unwrap();
        assert_eq!(
            open,
            serde_json::json!({"projects": [{"project": "/srv/app", "items": [{
                "id": "app-2", "title": "app-2 title", "status": "open", "kind": "task",
                "priority": 2, "parent": "app-1", "blocked_by": ["app-3", "ctx-9"],
            }]}]})
        );
        assert_eq!(List::of(&snapshot, true).projects[0].items.len(), 2);
    }

    #[test]
    fn github_items_are_named_as_work_show_takes_them() {
        let issue = Issue {
            number: 7,
            title: "Crash".into(),
            url: String::new(),
            state: IssueState::Open,
            author: None,
            created: String::new(),
            updated: String::new(),
            body: String::new(),
            labels: vec![],
            assignees: vec![],
            branches: vec![],
            pulls: vec![],
        };
        let pull = |number, state| Pull {
            number,
            title: "Fix".into(),
            url: String::new(),
            state,
            author: None,
            created: String::new(),
            updated: String::new(),
            body: String::new(),
            head: String::new(),
            base: String::new(),
            additions: 0,
            deletions: 0,
            labels: vec![],
            assignees: vec![],
            closes: vec![],
            checks: None,
        };
        let snapshot = Snapshot {
            projects: vec![ProjectView::Github(RepoView {
                name: "o/r".into(),
                work: Some(Work {
                    issues: vec![issue],
                    pulls: vec![pull(8, PullState::Draft), pull(9, PullState::Merged)],
                    ..Work::default()
                }),
                ..RepoView::default()
            })],
            refreshed: 0,
        };
        let listed = List::of(&snapshot, false);
        let items: Vec<(&str, &str, Option<&str>)> = listed.projects[0]
            .items
            .iter()
            .map(|i| (i.id.as_str(), i.status.as_str(), i.kind.as_deref()))
            .collect();
        assert_eq!(
            items,
            [
                ("o/r#7", "open", Some("issue")),
                ("o/r#8", "draft", Some("pull"))
            ]
        );
    }
}
