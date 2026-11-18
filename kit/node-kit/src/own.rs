//! The node's own commands and routes, which every node has whatever areas
//! it serves: no area may list one ([`crate::listing::check`]). They show its
//! config, and what the apps connected to it show (view control).

use std::sync::Arc;

use axum::Router;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::{Value, json};

use crate::Command;
use ahjo_core::NodeId;

use crate::config::Config;
use crate::serve::Failure;
use crate::view::{self, Views};

/// The names of the node's own commands.
pub const COMMANDS: &[&str] = &["config_show", "view_show", "view_open"];

/// What the node's own commands and routes read.
pub(crate) struct Own {
    pub config: Config,
    /// Every listed area's name: the sections the config may have.
    pub areas: Vec<&'static str>,
    pub node: NodeId,
    /// The apps connected to this node, for view control.
    pub views: Arc<Views>,
}

impl Own {
    fn config(&self, section: Option<&str>) -> Result<Value, ahjo_core::Error> {
        self.config.show(section, &self.areas)
    }
}

pub(crate) fn commands(own: &Arc<Own>) -> Vec<Command> {
    let shown = own.clone();
    // Built once when the node starts, so naming its sections costs one
    // short string for the node's life.
    let described: &'static str = Box::leak(
        format!(
            "The node's config file and each section's keys and values as the node loaded them. \
             An agent edits the file; restarting the node applies it, and sessions survive. \
             Its sections are [node] and one per area: {}. A section is TOML, for example \
             [work] projects = [\"beads /abs/folder\"]; a key the area does not know is refused \
             when the node starts, with the reason.",
            own.areas
                .iter()
                .map(|a| format!("[{a}]"))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into_boxed_str(),
    );
    let mut commands = vec![Command::new(
        "config_show",
        described,
        || {
            json!({
                "type": "object",
                "properties": {
                    "section": { "type": "string", "description": "Only this section." },
                },
            })
        },
        move |args: Value| {
            let answer = shown.config(args["section"].as_str());
            async move { answer }
        },
    )];
    commands.extend(view::commands(&own.views, own.node.clone()));
    commands
}

#[derive(serde::Deserialize)]
struct Section {
    section: Option<String>,
}

/// `GET /api/config[?section=]`: what `config_show` answers.
pub(crate) fn routes(own: Arc<Own>) -> Router {
    Router::new().route(
        "/api/config",
        get(move |Query(q): Query<Section>| async move {
            match own.config(q.section.as_deref()) {
                Ok(answer) => axum::Json(answer).into_response(),
                Err(e) => Failure(StatusCode::NOT_FOUND, e).into_response(),
            }
        }),
    )
}
