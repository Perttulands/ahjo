//! Issues's tracker readers, commands and watched snapshot channel, and
//! reference resolution for other areas ([`ahjo_issue_contract::Resolver`]).
mod issues;

use ahjo_core::{Error, NodeId};
use ahjo_issue_contract::commands::*;
use ahjo_issue_contract::{List, Resolver, Resolving};
use ahjo_node_kit::{Area, BoxFuture, Built, Channel, Client, Command, Effect, Listing, Setup};
use anyhow::Result;
pub use issues::{Bd, Gh};
use issues::{Config, Issues};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

pub const LISTING: Listing = Listing {
    name: "issue",
    kinds: ahjo_issue_contract::KINDS,
    commands: &[
        "issue_list",
        "issue_search",
        "issue_show",
        "issue_create",
        "issue_snapshot_text",
        "issue_snapshot_search",
    ],
    build,
};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Section {
    /// Project lines in the same form as the former work.conf.
    projects: Vec<String>,
}

pub fn build(setup: &Setup) -> Result<Built> {
    build_with(setup, Bd::default(), Gh::default())
}

/// Build with tracker programs supplied by a test or embedding node.
pub fn build_with(setup: &Setup, bd: Bd, gh: Gh) -> Result<Built> {
    let section: Section = setup.config.section("issue")?;
    let config = Config::parse(&section.projects.join("\n"))?;
    let reads = migrate_reads(&setup.dir)?;
    let work = Issues::keeping(config, setup.node.id.as_str(), gh, bd, Some(reads));
    setup
        .provided
        .provide::<dyn Resolver>(Arc::new(Provided(work.clone())))?;
    Built::now(WorkArea {
        work,
        node: setup.node.id.clone(),
    })
}

/// Preserve existing cache files if a previous startup already made the new directory.
fn migrate_reads(dir: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let old = dir.join("work-reads");
    let new = dir.join("issue-reads");
    if old.is_dir() {
        if !new.exists() {
            std::fs::rename(&old, &new)?;
        } else {
            for entry in std::fs::read_dir(&old)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    let destination = new.join(entry.file_name());
                    match std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(destination)
                    {
                        Ok(mut output) => {
                            std::io::copy(&mut std::fs::File::open(entry.path())?, &mut output)?;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error),
                    }
                }
            }
        }
    }
    Ok(new)
}

/// Issues as other areas reach it ([`ahjo_issue_contract::Resolver`]).
struct Provided(Issues);

impl Resolver for Provided {
    fn resolve<'a>(&'a self, written: &'a str) -> Resolving<'a> {
        Box::pin(async move {
            self.0
                .resolve(written)
                .await
                .map_err(|error| Error::new(ahjo_issue_contract::BAD_REQUEST, format!("{error:#}")))
        })
    }
}

struct WorkArea {
    work: Issues,
    node: NodeId,
}
impl Area for WorkArea {
    fn commands(&self) -> Vec<Command> {
        let (work, node) = (self.work.clone(), self.node.to_string());
        let list = Command::typed("issue_list", Effect::Reads, move |args: ListIssues| {
            let (work, node) = (work.clone(), node.clone());
            async move {
                let _watching = work.subscribe();
                let requested = work.refresh();
                let read = async {
                    let list = List::of(work.refreshed(requested).await.as_ref(), args.all);
                    let none = list.projects.is_empty().then(|| {
                        format!(
                            "this node reads no projects; to read one, {}",
                            issues::ADDING
                        )
                    });
                    Ok(IssuesListed {
                        node,
                        projects: list.projects,
                        none,
                    })
                };
                tokio::time::timeout(Duration::from_secs(30), read)
                    .await
                    .map_err(|_| Error::new("tracker_error", "issue read timed out"))?
            }
        });
        let (work, node) = (self.work.clone(), self.node.to_string());
        let search = Command::typed("issue_search", Effect::Reads, move |args: SearchIssues| {
            let (work, node) = (work.clone(), node.clone());
            async move {
                let terms = ahjo_issue_contract::text::terms(&args.query);
                let _watching = work.subscribe();
                let requested = work.refresh();
                let read = async {
                    let snapshot = work.refreshed(requested).await;
                    let found = snapshot.matching(&terms);
                    let list = List::keeping(&snapshot, |reference, _| {
                        found.iter().any(|f| f == reference)
                    });
                    let items = list
                        .projects
                        .into_iter()
                        .flat_map(|listed| {
                            listed.items.into_iter().map(move |item| IssueMatch {
                                project: listed.project.clone(),
                                item,
                            })
                        })
                        .collect();
                    Ok(IssuesFound { node, items })
                };
                tokio::time::timeout(Duration::from_secs(30), read)
                    .await
                    .map_err(|_| Error::new("tracker_error", "issue read timed out"))?
            }
        });
        let (work, node) = (self.work.clone(), self.node.to_string());
        let show = Command::typed("issue_show", Effect::Reads, move |args: ShowIssue| {
            let (work, node) = (work.clone(), node.clone());
            async move {
                let reference = work.resolve(&args.reference).await.map_err(failed)?;
                let item = work.item(&reference).await.map_err(|error| {
                    if error.downcast_ref::<issues::MissingItem>().is_some() {
                        Error::new("not_found", error.to_string())
                    } else {
                        failed(error)
                    }
                })?;
                let line = work.reference_line(&item).await;
                let family = work.family(&item);
                Ok(IssueShown {
                    node,
                    item,
                    line,
                    family,
                })
            }
        });
        let work = self.work.clone();
        let create = Command::typed("issue_create", Effect::Writes, move |args: CreateIssue| {
            let work = work.clone();
            async move {
                let request = ahjo_issue_contract::Create {
                    project: args.project,
                    title: args.title,
                    description: args.description,
                    kind: args.kind.as_str().into(),
                };
                Ok(IssueCreated {
                    reference: work.create(&request).await?,
                })
            }
        });
        let work = self.work.clone();
        let text = Command::for_request(Effect::Reads, move |args: SnapshotText| {
            let work = work.clone();
            async move {
                if let Some(text) = work.last().text(&args.reference) {
                    return Ok(SnapshotTextRead {
                        text: text.to_owned(),
                    });
                }
                let reference = work.resolve(&args.reference).await.map_err(failed)?;
                let item = work.item(&reference).await.map_err(|error| {
                    if error.downcast_ref::<issues::MissingItem>().is_some() {
                        Error::new("not_found", error.to_string())
                    } else {
                        failed(error)
                    }
                })?;
                Ok(SnapshotTextRead { text: item.body })
            }
        });
        let work = self.work.clone();
        let snapshot_search = Command::for_request(Effect::Reads, move |args: SnapshotSearch| {
            let snapshot = work.last();
            async move {
                let terms = ahjo_issue_contract::text::terms(&args.query);
                let references = tokio::task::spawn_blocking(move || snapshot.matching(&terms))
                    .await
                    .map_err(|error| Error::new("search_failed", error.to_string()))?;
                Ok(SnapshotMatches { references })
            }
        });
        // Tracker reads and writes may reach GitHub.
        [list, search, show, create, text, snapshot_search]
            .into_iter()
            .map(Command::open_world)
            .collect()
    }
    fn channel(&self, _: Client) -> Option<Box<dyn Channel>> {
        Some(Box::new(Socket(issues::Channel::new(self.work.clone()))))
    }
}
fn failed(e: impl std::fmt::Display) -> Error {
    Error::new("tracker_error", e.to_string())
}
struct Socket(issues::Channel);
impl Channel for Socket {
    fn receive(&mut self, frame: Vec<u8>) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let _ = self.0.receive(&frame);
        })
    }
    fn next(&mut self) -> BoxFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move { Some(self.0.next().await) })
    }
}

#[cfg(test)]
mod migration_tests {
    #[test]
    fn kept_reads_move_once_and_existing_new_files_win() {
        let dir = ahjo_testkit::fresh_dir("issue-reads-migration");
        let old = dir.join("work-reads");
        let new = dir.join("issue-reads");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("prefixes.json"), "old").unwrap();
        assert_eq!(super::migrate_reads(&dir).unwrap(), new);
        assert_eq!(
            std::fs::read_to_string(new.join("prefixes.json")).unwrap(),
            "old"
        );
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("prefixes.json"), "stale").unwrap();
        std::fs::write(old.join("other.json"), "other").unwrap();
        super::migrate_reads(&dir).unwrap();
        super::migrate_reads(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(new.join("prefixes.json")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("other.json")).unwrap(),
            "other"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
