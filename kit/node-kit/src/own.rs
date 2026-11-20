//! The node's own commands and routes, which every node has whatever areas
//! it serves: no area may list one ([`crate::listing::check`]). They show its
//! config, and what the apps connected to it show (view control).

use std::sync::Arc;

use axum::Router;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;

use crate::{Command, Effect};
use ahjo_core::{
    Error, NodeId,
    commands::{ConfigShow, ConfigShown},
};

use crate::config::Config;
use crate::serve::Failure;
use crate::view::{self, Views};

/// The names of the node's own commands.
pub const COMMANDS: &[&str] = &["config_show", "view_show", "view_open", "view_send"];

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
    fn config(&self, section: Option<&str>) -> Result<ConfigShown, Error> {
        self.config.show(section, &self.areas)
    }
}

pub(crate) fn commands(own: &Arc<Own>) -> Vec<Command> {
    let shown = own.clone();
    let mut commands = vec![Command::typed(
        "config_show",
        Effect::Reads,
        move |args: ConfigShow| {
            let answer = shown.config(args.section.as_deref());
            async move { answer }
        },
    )];
    commands.extend(view::commands(&own.views, own.node.clone()));
    commands
}

#[derive(serde::Deserialize)]
struct ConfigQuery {
    section: Option<String>,
}

/// `GET /api/config[?section=]`: what `config_show` answers.
pub(crate) fn routes(own: Arc<Own>) -> Router {
    Router::new().route(
        "/api/config",
        get(move |Query(q): Query<ConfigQuery>| async move {
            match own.config(q.section.as_deref()) {
                Ok(answer) => axum::Json(answer).into_response(),
                Err(e) => Failure(StatusCode::NOT_FOUND, e).into_response(),
            }
        }),
    )
}
