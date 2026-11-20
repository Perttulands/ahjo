//! Work's tracker readers, commands and watched snapshot channel, and
//! reference resolution for other areas ([`ahjo_work_contract::Resolver`]).
mod work;

use ahjo_core::{Error, NodeId};
use ahjo_node_kit::args::{flag, object, text};
use ahjo_node_kit::{Area, BoxFuture, Built, Channel, Client, Command, Effect, Listing, Setup};
use ahjo_work_contract::{List, Resolver, Resolving};
use anyhow::Result;
use axum::Router;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
pub use work::{Bd, Gh};
use work::{Config, Work};

pub const LISTING: Listing = Listing {
    name: "work",
    kinds: ahjo_work_contract::KINDS,
    commands: &["work_list", "work_search", "work_show", "work_create"],
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
    let section: Section = setup.config.section("work")?;
    let config = Config::parse(&section.projects.join("\n"))?;
    let work = Work::keeping(
        config,
        setup.node.id.as_str(),
        gh,
        bd,
        Some(setup.dir.join("work-reads")),
    );
    setup
        .provided
        .provide::<dyn Resolver>(Arc::new(Provided(work.clone())))?;
    Built::now(WorkArea {
        work,
        node: setup.node.id.clone(),
    })
}

/// Work as other areas reach it ([`ahjo_work_contract::Resolver`]).
struct Provided(Work);

impl Resolver for Provided {
    fn resolve<'a>(&'a self, written: &'a str) -> Resolving<'a> {
        Box::pin(async move {
            self.0
                .resolve(written)
                .await
                .map_err(|error| Error::new(ahjo_work_contract::BAD_REQUEST, format!("{error:#}")))
        })
    }
}

struct WorkArea {
    work: Work,
    node: NodeId,
}
impl Area for WorkArea {
    fn routes(&self) -> Router {
        work::routes(self.work.clone())
    }
    fn commands(&self) -> Vec<Command> {
        let (work, node) = (self.work.clone(), self.node.clone());
        let list = Command::new(
            "work_list",
            Effect::Reads,
            "List each project's open work items, blocked and deferred ones included, one line each: id, title, status, kind, priority, parent and blockers. --all adds closed and merged ones; work show gives an item's text and family.",
            || object(json!({"all": {"type":"boolean", "default":false}}), &[]),
            move |args| {
                let (work, node) = (work.clone(), node.clone());
                async move {
                    let all = flag(&args, "all", false)
                        .map_err(|e| Error::new("bad_request", e.to_string()))?;
                    let _watching = work.subscribe();
                    let requested = work.refresh();
                    let read = async {
                        let list = List::of(&*work.refreshed(requested).await, all);
                        let mut answer = serde_json::to_value(&list).map_err(failed)?;
                        if list.projects.is_empty() {
                            answer["none"] = json!(format!(
                                "this node reads no projects; to read one, {}",
                                work::ADDING
                            ));
                        }
                        answer["node"] = json!(node);
                        Ok(answer)
                    };
                    tokio::time::timeout(Duration::from_secs(30), read)
                        .await
                        .map_err(|_| Error::new("tracker_error", "work read timed out"))?
                }
            },
        );
        let (work, node) = (self.work.clone(), self.node.clone());
        let search = Command::new(
            "work_search",
            Effect::Reads,
            "Find the items, closed ones too, holding every word of the query in their id, title, labels or text, as the Work page's search does.",
            || object(json!({"query": {"type":"string"}}), &["query"]),
            move |args| {
                let (work, node) = (work.clone(), node.clone());
                async move {
                    let query = text(&args, "query")
                        .map_err(|e| Error::new("bad_request", e.to_string()))?;
                    let terms = ahjo_work_contract::text::terms(query);
                    let _watching = work.subscribe();
                    let requested = work.refresh();
                    let read = async {
                        let snapshot = work.refreshed(requested).await;
                        let found = snapshot.matching(&terms);
                        let list = List::keeping(&snapshot, |reference, _| {
                            found.iter().any(|f| f == reference)
                        });
                        // Each found item, with its project.
                        let mut items = Vec::new();
                        for listed in &list.projects {
                            for line in &listed.items {
                                let mut item = json!({"project": listed.project});
                                if let serde_json::Value::Object(fields) =
                                    serde_json::to_value(line).map_err(failed)?
                                {
                                    item.as_object_mut().expect("an object").extend(fields);
                                }
                                items.push(item);
                            }
                        }
                        Ok(json!({"node": node, "items": items}))
                    };
                    tokio::time::timeout(Duration::from_secs(30), read)
                        .await
                        .map_err(|_| Error::new("tracker_error", "work read timed out"))?
                }
            },
        );
        let (work, node) = (self.work.clone(), self.node.clone());
        let show = Command::new(
            "work_show",
            Effect::Reads,
            "One work item and its family.",
            || object(json!({"ref": {"type":"string"}}), &["ref"]),
            move |args| {
                let (work, node) = (work.clone(), node.clone());
                async move {
                    let written =
                        text(&args, "ref").map_err(|e| Error::new("bad_request", e.to_string()))?;
                    let reference = work.resolve(written).await.map_err(failed)?;
                    let item = work.item(&reference).await.map_err(|error| {
                        if error.downcast_ref::<work::MissingItem>().is_some() {
                            Error::new("not_found", error.to_string())
                        } else {
                            failed(error)
                        }
                    })?;
                    let line = work.reference_line(&item).await;
                    Ok(json!({"node":node,"item":item,"line":line}))
                }
            },
        );
        let work = self.work.clone();
        let create = Command::new(
            "work_create",
            Effect::Writes,
            "File an item in a configured project, for example: \
             ahjo work create /srv/app 'Login fails' 'What fails, and how to see it'.",
            || {
                object(
                    json!({"project":{"type":"string", "description":"The project as ahjo work list shows it: a Beads store's absolute folder, or owner/name on GitHub."}, "title":{"type":"string"}, "description":{"type":"string"}, "type":{"type":"string", "enum":["bug", "task", "feature"]}}),
                    &["project", "title", "description"],
                )
            },
            move |args| {
                let work = work.clone();
                async move {
                    let request = serde_json::from_value(args)
                        .map_err(|e| Error::new("bad_request", e.to_string()))?;
                    let reference = work.create(&request).await?;
                    Ok(json!({"reference": reference}))
                }
            },
        );
        // Each may reach GitHub, for a project that lives there.
        [list, search, show, create]
            .into_iter()
            .map(Command::open_world)
            .collect()
    }
    fn channel(&self, _: Client) -> Option<Box<dyn Channel>> {
        Some(Box::new(Socket(work::Channel::new(self.work.clone()))))
    }
}
fn failed(e: impl std::fmt::Display) -> Error {
    Error::new("tracker_error", e.to_string())
}
struct Socket(work::Channel);
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
