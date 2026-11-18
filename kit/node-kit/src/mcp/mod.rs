//! MCP: the areas' commands as tools for agents and AI apps. Each area is an
//! MCP server of its own, at `/mcp/<area>` (several as `/mcp/<a>,<b>`), and
//! the node's own commands are `/mcp/node`; `/mcp` serves every one, which
//! is what the command line reads. `ahjo mcp [--area <area>]...` offers the
//! same over stdio by relaying to the node. Every call is logged. There are
//! no permissions here: a caller reaches what the node's user does.

mod call_log;
mod http;
mod rpc;
mod stdio;

use ahjo_core::{Error, NodeId};
use axum::Router;
use serde_json::{Value, json};

use crate::Command;

pub use http::PATH;
pub use stdio::stdio;

/// The group the node's own commands ([`crate::own`]) are served in. No
/// area takes this name: it is the config's `[node]` section.
pub const NODE: &str = crate::config::NODE;

/// One MCP server: an area's commands, or the node's own.
pub(crate) struct Group {
    pub name: &'static str,
    pub commands: Vec<Command>,
}

/// The tools: every built area's commands, by area.
pub(crate) struct Mcp {
    node: NodeId,
    groups: Vec<Group>,
    /// Areas the config turned off, named when a client asks for one.
    off: Vec<&'static str>,
}

/// The groups one endpoint serves, as indexes into [`Mcp::groups`].
pub(crate) struct Reach(Vec<usize>);

impl Mcp {
    pub(crate) fn new(node: NodeId, groups: Vec<Group>, off: Vec<&'static str>) -> Mcp {
        Mcp { node, groups, off }
    }

    pub(crate) fn routes(self) -> Router {
        http::routes(std::sync::Arc::new(self))
    }

    /// Every group, as `/mcp` serves them.
    fn everything(&self) -> Reach {
        Reach((0..self.groups.len()).collect())
    }

    /// The groups `names` (`a,b`) asks for, or why there is no such server.
    fn reach(&self, names: &str) -> Result<Reach, String> {
        let mut reach = Vec::new();
        for name in names.split(',') {
            match self.groups.iter().position(|g| g.name == name) {
                Some(at) if !reach.contains(&at) => reach.push(at),
                Some(_) => {}
                None if self.off.contains(&name) => {
                    return Err(format!("the area {name} is turned off on this node"));
                }
                None => {
                    return Err(format!(
                        "no MCP server {name}; this node's are {}",
                        self.names(&self.everything()).join(", ")
                    ));
                }
            }
        }
        // In the node's order, however they were named.
        reach.sort_unstable();
        Ok(Reach(reach))
    }

    fn names(&self, reach: &Reach) -> Vec<&'static str> {
        reach.0.iter().map(|&at| self.groups[at].name).collect()
    }

    fn commands<'a>(&'a self, reach: &'a Reach) -> impl Iterator<Item = &'a Command> {
        reach
            .0
            .iter()
            .flat_map(|&at| self.groups[at].commands.iter())
    }

    /// The tools `reach` serves, as `tools/list` answers. Each runs on this
    /// node; `node` is taken (the command line names the node with it) but
    /// not listed, since no other node is reached yet.
    fn list(&self, reach: &Reach) -> Value {
        let tools: Vec<Value> = self
            .commands(reach)
            .map(|command| {
                let mut schema = (command.schema)();
                // An empty `required` says nothing.
                if let Some(fields) = schema.as_object_mut()
                    && fields
                        .get("required")
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
                {
                    fields.remove("required");
                }
                json!({
                    "name": command.name,
                    "description": command.description,
                    "inputSchema": schema,
                })
            })
            .collect();
        json!({ "tools": tools })
    }

    fn command<'a>(&'a self, reach: &'a Reach, name: &str) -> Option<&'a Command> {
        self.commands(reach).find(|command| command.name == name)
    }

    /// The server a tool is on, for a client that asked another for it.
    fn server_of(&self, tool: &str) -> Option<&'static str> {
        self.groups
            .iter()
            .find(|g| g.commands.iter().any(|c| c.name == tool))
            .map(|g| g.name)
    }

    /// What `initialize` tells a client: which servers it reached, and the
    /// others it could.
    fn instructions(&self, reach: &Reach) -> String {
        let here = self.names(reach);
        let others: Vec<String> = self
            .groups
            .iter()
            .map(|g| g.name)
            .filter(|name| !here.contains(name))
            .map(|name| format!("{PATH}/{name}"))
            .collect();
        let mut text = format!(
            "Ahjo, the operator's node: {} tools. Each is one of its commands, also run as \
             `ahjo <noun> <verb>`, and runs on this node.",
            here.join(", ")
        );
        if !others.is_empty() {
            text.push_str(&format!(
                " Other areas are their own servers: {}.",
                others.join(", ")
            ));
        }
        text
    }

    /// Takes `node` out of a call's arguments: this node unless they name
    /// another, which this node cannot reach.
    fn target(&self, args: &mut Value) -> Result<(), Error> {
        let named = args.as_object_mut().and_then(|args| args.remove("node"));
        match named {
            None | Some(Value::Null) => Ok(()),
            Some(Value::String(wanted)) if wanted == self.node.as_str() => Ok(()),
            Some(Value::String(wanted)) => Err(crate::serve::elsewhere(&wanted)),
            Some(_) => Err(Error::new("invalid_argument", "node is a string")),
        }
    }
}
