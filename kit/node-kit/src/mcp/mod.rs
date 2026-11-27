//! MCP: the areas' commands as tools for agents and AI apps. Each area is an
//! MCP server of its own, at `/mcp/<area>` (several as `/mcp/<a>,<b>`), and
//! the node's own commands are `/mcp/node`; `/mcp` serves every one, which
//! is what the command line reads. `ahjo mcp [--area <area>]...` offers the
//! same over stdio by relaying to the node. Every call is logged. There are
//! no permissions here: a caller reaches what the node's user does.

mod call_log;
mod http;
mod rpc;
mod sets;
mod stdio;

use ahjo_core::{Error, NodeId};
use axum::Router;
use serde_json::{Value, json};

use crate::{Command, Effect};

pub use http::PATH;
pub use sets::{SET_TOOLS, ToolSet, check};
pub use stdio::stdio;

/// The group the node's own commands ([`crate::own`]) are served in. No
/// area takes this name: it is the config's `[node]` section.
pub const NODE: &str = crate::config::NODE;

/// One MCP server: an area's commands, or the node's own.
pub(crate) struct Group {
    pub name: &'static str,
    pub commands: Vec<Command>,
}

/// A tool set as served: its tools, found among the groups, and its own.
struct Set {
    name: &'static str,
    instructions: &'static str,
    tools: Vec<Command>,
}

/// The tools: every built area's commands, by area, and the tool sets.
pub(crate) struct Mcp {
    node: NodeId,
    groups: Vec<Group>,
    sets: Vec<Set>,
    /// Areas the config turned off, named when a client asks for one.
    off: Vec<&'static str>,
}

/// What one endpoint serves: groups, as indexes into [`Mcp::groups`], or
/// one tool set.
pub(crate) enum Reach {
    Groups(Vec<usize>),
    Set(usize),
}

impl Mcp {
    pub(crate) fn new(
        node: NodeId,
        groups: Vec<Group>,
        off: Vec<&'static str>,
        sets: &[ToolSet],
    ) -> Mcp {
        let every: std::sync::Arc<[Command]> = groups
            .iter()
            .flat_map(|g| g.commands.iter().cloned())
            .collect();
        let sets = sets
            .iter()
            .map(|set| Set {
                name: set.name,
                instructions: set.instructions,
                // A command of an area turned off is not there to serve.
                tools: set
                    .tools
                    .iter()
                    .filter_map(|&tool| {
                        sets::own(tool, &every, &node)
                            .or_else(|| every.iter().find(|c| c.name == tool).cloned())
                    })
                    .collect(),
            })
            .collect();
        Mcp {
            node,
            groups,
            sets,
            off,
        }
    }

    pub(crate) fn routes(self) -> Router {
        http::routes(std::sync::Arc::new(self))
    }

    /// Every group, as `/mcp` serves them.
    fn everything(&self) -> Reach {
        Reach::Groups((0..self.groups.len()).collect())
    }

    /// The groups `names` (`a,b`) asks for, or the one tool set it names,
    /// or why there is no such server.
    fn reach(&self, names: &str) -> Result<Reach, String> {
        if let Some(at) = self.sets.iter().position(|s| s.name == names) {
            return Ok(Reach::Set(at));
        }
        let mut reach = Vec::new();
        for name in names.split(',') {
            match self.groups.iter().position(|g| g.name == name) {
                Some(at) if !reach.contains(&at) => reach.push(at),
                Some(_) => {}
                None if self.off.contains(&name) => {
                    return Err(format!("the area {name} is turned off on this node"));
                }
                None if self.sets.iter().any(|s| s.name == name) => {
                    return Err(format!("{PATH}/{name} is a tool set of its own"));
                }
                None => {
                    let mut servers = self.names(&self.everything());
                    servers.extend(self.sets.iter().map(|s| s.name));
                    return Err(format!(
                        "no MCP server {name}; this node's are {}",
                        servers.join(", ")
                    ));
                }
            }
        }
        // In the node's order, however they were named.
        reach.sort_unstable();
        Ok(Reach::Groups(reach))
    }

    fn names(&self, reach: &Reach) -> Vec<&'static str> {
        match reach {
            Reach::Groups(at) => at.iter().map(|&at| self.groups[at].name).collect(),
            Reach::Set(at) => vec![self.sets[*at].name],
        }
    }

    fn commands<'a>(&'a self, reach: &'a Reach) -> Box<dyn Iterator<Item = &'a Command> + 'a> {
        match reach {
            Reach::Groups(at) => {
                Box::new(at.iter().flat_map(|&at| self.groups[at].commands.iter()))
            }
            Reach::Set(at) => Box::new(self.sets[*at].tools.iter()),
        }
    }

    /// The tools `reach` serves, as `tools/list` answers. Each runs on this
    /// node; `node` is taken (the command line names the node with it) but
    /// not listed, since no other node is reached yet.
    fn list(&self, reach: &Reach) -> Value {
        let tools: Vec<Value> = self.commands(reach).map(tool).collect();
        json!({ "tools": tools })
    }

    fn command<'a>(&'a self, reach: &'a Reach, name: &str) -> Option<&'a Command> {
        self.commands(reach).find(|command| command.name == name)
    }

    /// The command whose answer a call of `tool` with `args` gives: the
    /// tool's own, or the one `swarm_read` or `swarm_run` relays to.
    fn answering<'a>(&'a self, tool: &'a Command, args: &Value) -> Option<&'a Command> {
        if tool.name != sets::READ && tool.name != sets::RUN {
            return Some(tool);
        }
        let name = args.get("command")?.as_str()?;
        self.groups
            .iter()
            .flat_map(|g| g.commands.iter())
            .find(|c| c.name == name)
    }

    async fn invoke(
        &self,
        caller: &rpc::Caller,
        command: &Command,
        mut args: Value,
    ) -> Result<Value, Error> {
        let outcome = match target(&self.node, &mut args) {
            Ok(()) => (command.call)(args).await,
            Err(error) => Err(error),
        };
        call_log::record(
            caller,
            command.name,
            self.node.as_str(),
            if outcome.is_ok() { "ok" } else { "failed" },
        );
        outcome
    }

    /// Where else a tool is, for a client that asked a server without it.
    fn elsewhere(&self, reach: &Reach, tool: &str) -> String {
        let server = self
            .groups
            .iter()
            .find(|g| g.commands.iter().any(|c| c.name == tool))
            .map(|g| g.name);
        match (reach, server) {
            (Reach::Set(at), Some(_)) if self.sets[*at].reaches_every() => {
                "; run it here with swarm_read or swarm_run".to_owned()
            }
            (_, Some(server)) => format!("; it is on {PATH}/{server}"),
            (_, None) => String::new(),
        }
    }

    /// What `initialize` tells a client: which servers it reached, and the
    /// others it could; a tool set says its own.
    fn instructions(&self, reach: &Reach) -> String {
        if let Reach::Set(at) = reach {
            return self.sets[*at].instructions.to_owned();
        }
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
}

impl Set {
    /// Whether it has the tools that run any command.
    fn reaches_every(&self) -> bool {
        self.tools.iter().any(|t| t.name == sets::RUN)
    }
}

/// One tool as `tools/list` gives it: name, title, description, schema and
/// what it does ([`Effect`]) as MCP's annotations.
fn tool(command: &Command) -> Value {
    let mut schema = (command.schema)();
    // The image a command answers is image content, not structured content,
    // so the structured result may go without it.
    let mut output = command.output_schema.as_ref().clone();
    if let Some(field) = image_field(&output).map(str::to_owned)
        && let Some(required) = output.get_mut("required").and_then(Value::as_array_mut)
    {
        required.retain(|name| *name != field.as_str());
    }
    // An empty `required` says nothing.
    if let Some(fields) = schema.as_object_mut()
        && fields
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        fields.remove("required");
    }
    // Each hint whose default is wrong for it; `idempotentHint` is false
    // unless said, the others true.
    let mut annotations = match command.effect {
        Effect::Reads => json!({ "readOnlyHint": true }),
        effect => json!({
            "readOnlyHint": false,
            "destructiveHint": effect == Effect::Destroys,
        }),
    };
    if command.effect == Effect::Sets {
        annotations["idempotentHint"] = json!(true);
    }
    annotations["openWorldHint"] = json!(command.open_world);
    json!({
        "name": command.name,
        "title": command.title(),
        "description": command.description,
        "inputSchema": schema,
        "outputSchema": output,
        "annotations": annotations,
    })
}

/// The output field a command answers as MCP image content: the one its
/// output schema marks `x-ahjo-image`, an object of `mime_type` and base64
/// `data`. `tools/call` gives it as an image after the text, and leaves it
/// out of the text and the structured result; the command line puts it back.
pub fn image_field(output_schema: &Value) -> Option<&str> {
    output_schema["properties"]
        .as_object()?
        .iter()
        .find_map(|(name, spec)| (spec["x-ahjo-image"] == true).then_some(name.as_str()))
}

/// Takes `node` out of a call's arguments: this node unless they name
/// another, which this node cannot reach.
fn target(node: &NodeId, args: &mut Value) -> Result<(), Error> {
    let named = args.as_object_mut().and_then(|args| args.remove("node"));
    match named {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(wanted)) if wanted == node.as_str() => Ok(()),
        Some(Value::String(wanted)) => Err(crate::serve::elsewhere(&wanted)),
        Some(_) => Err(Error::new("invalid_argument", "node is a string")),
    }
}
