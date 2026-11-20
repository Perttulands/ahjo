//! The node's own commands, which every node has whatever areas
//! it serves: no area may list one ([`crate::listing::check`]). They show its
//! config, and what the apps connected to it show (view control).

use std::sync::Arc;

use crate::{Command, Effect};
use ahjo_core::{
    Error, NodeId, TypedCommand,
    commands::{ConfigShow, ConfigShown},
};

use crate::config::Config;
use crate::view::{self, Views};

/// The names of the node's own commands.
pub const COMMANDS: &[&str] = &["config_show", "view_show", "view_open", "view_send"];

/// What the node's own commands read.
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
        ConfigShow::NAME,
        Effect::Reads,
        move |args: ConfigShow| {
            let answer = shown.config(args.section.as_deref());
            async move { answer }
        },
    )];
    commands.extend(view::commands(&own.views, own.node.clone()));
    commands
}
