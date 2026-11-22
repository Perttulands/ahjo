//! What a node area plugs into (ARCHITECTURE.md, Areas register themselves):
//! its routes, the frames it owns on the socket, and its commands, each of
//! which is both an MCP tool and an `ahjo <noun> <verb>`.

use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ahjo_core::{Error, TypedCommand};
use axum::Router;
use axum::http::StatusCode;
use schemars::{JsonSchema, generate::SchemaSettings};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{Provided, Setup};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Builds an area from the node's setup, once, when the node starts. The
/// binary lists it in the area's [`crate::Listing`]; an area whose config
/// section says `enabled = false` is not built.
pub type Build = fn(&Setup) -> anyhow::Result<Built>;

/// What a [`Build`] answers. Areas are built in two steps, so their order in
/// the binary's list never matters: each is built and fills the interfaces it
/// provides ([`Setup::provided`]); then those that use what others provide
/// finish, reading those interfaces.
pub enum Built {
    /// The area, which uses nothing another area provides.
    Now(Arc<dyn Area>),
    /// How the area finishes once every area is built.
    Then(Finish),
}

/// The second step of building an area: given what areas provide.
pub type Finish = Box<dyn FnOnce(&Provided) -> anyhow::Result<Arc<dyn Area>> + Send>;

impl Built {
    pub fn now(area: impl Area) -> anyhow::Result<Built> {
        Ok(Built::Now(Arc::new(area)))
    }

    pub fn then(
        finish: impl FnOnce(&Provided) -> anyhow::Result<Arc<dyn Area>> + Send + 'static,
    ) -> anyhow::Result<Built> {
        Ok(Built::Then(Box::new(finish)))
    }
}

/// A built area. What it is called and the frame kinds it owns are in its
/// [`crate::Listing`].
pub trait Area: Send + Sync + 'static {
    fn routes(&self) -> Router {
        Router::new()
    }

    fn commands(&self) -> Vec<Command> {
        Vec::new()
    }

    /// Its side of one client's socket, for frames of its listed kinds;
    /// `None` for an area with no frames.
    fn channel(&self, _client: Client) -> Option<Box<dyn Channel>> {
        None
    }

    /// Stops what it runs, as the node stops.
    fn stop(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// One client's socket, as an area sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Client {
    /// Unique among this node's connections.
    pub id: u64,
}

/// An area's side of one socket. Dropped when the client goes.
pub trait Channel: Send {
    /// A frame from the client, of one of the area's kinds.
    fn receive(&mut self, frame: Vec<u8>) -> BoxFuture<'_, ()>;

    /// The next frame to push to the client: pending while there is none,
    /// `None` once the channel has ended, which closes the socket. It must
    /// be cancel-safe: the node drops it whenever another channel or the
    /// client is first.
    fn next(&mut self) -> BoxFuture<'_, Option<Vec<u8>>>;
}

type Call = dyn Fn(Value) -> BoxFuture<'static, Result<Value, Error>> + Send + Sync;

/// What running a command does to the node and what it reaches, declared
/// with the command: an MCP client reads it as the tool's annotations
/// (ChatGPT asks before every call of a tool that does not only read).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Only reads: changes nothing.
    Reads,
    /// Adds or changes something, and doing it again does it again.
    Writes,
    /// Changes something to the state the arguments name, so doing it
    /// again changes nothing more.
    Sets,
    /// Deletes, ends or overwrites something that cannot be had back.
    Destroys,
}

impl Effect {
    /// Whether it changes nothing.
    pub fn reads(self) -> bool {
        self == Effect::Reads
    }

    /// Its word in what lists commands: reads, writes, sets or destroys.
    pub fn word(self) -> &'static str {
        match self {
            Effect::Reads => "reads",
            Effect::Writes => "writes",
            Effect::Sets => "sets",
            Effect::Destroys => "destroys",
        }
    }
}

/// One definition behind an MCP tool `noun_verb` and the CLI's
/// `ahjo noun verb`.
#[derive(Clone)]
pub struct Command {
    /// `noun_verb`: a noun, then a verb of one or more words, joined by
    /// single underscores.
    pub name: &'static str,
    pub effect: Effect,
    /// It reaches beyond this machine (GitHub, a remote), not only the
    /// node and its files.
    pub open_world: bool,
    pub description: Cow<'static, str>,
    /// The arguments' JSON Schema, asked for each time the tools are listed,
    /// so it may offer what the node has now.
    pub schema: Arc<dyn Fn() -> Value + Send + Sync>,
    pub output_schema: Arc<Value>,
    pub call: Arc<Call>,
    pub(crate) error_status: fn(&Error) -> Option<StatusCode>,
}

impl Command {
    /// Derives the tool schemas from its request and response types. The
    /// request is an object, with its first doc line naming what it does.
    pub fn typed<Input, Output, F, Fut>(name: &'static str, effect: Effect, handler: F) -> Self
    where
        Input: DeserializeOwned + JsonSchema,
        Output: Serialize + JsonSchema,
        F: Fn(Input) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Output, Error>> + Send + 'static,
    {
        let settings = SchemaSettings::draft2020_12().with(|settings| {
            settings.inline_subschemas = true;
            settings.meta_schema = None;
        });
        let input = Value::from(
            settings
                .clone()
                .into_generator()
                .into_root_schema_for::<Input>(),
        );
        let output = settings
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<Output>();
        let description = input["description"]
            .as_str()
            .and_then(|doc| doc.lines().next())
            .unwrap_or_default()
            .to_owned();
        let fields: Vec<String> = input["properties"]
            .as_object()
            .into_iter()
            .flat_map(|properties| properties.keys().cloned())
            .collect();
        Self {
            name,
            effect,
            open_world: false,
            description: Cow::Owned(description),
            schema: Arc::new(move || input.clone()),
            output_schema: Arc::new(output.into()),
            call: Arc::new(move |args| {
                let pending = typed_arguments(args, &fields).map(&handler);
                Box::pin(async move {
                    serde_json::to_value(pending?.await?).map_err(|error| {
                        Error::new("internal", format!("cannot encode {name} output: {error}"))
                    })
                })
            }),
            error_status: |_| None,
        }
    }

    /// Defines a command using the request shared with app callers.
    pub fn for_request<C, F, Fut>(effect: Effect, handler: F) -> Self
    where
        C: TypedCommand + JsonSchema,
        C::Output: JsonSchema,
        F: Fn(C) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<C::Output, Error>> + Send + 'static,
    {
        Self::typed(C::NAME, effect, handler)
    }

    /// Supplies the HTTP status for an area's own error codes.
    pub fn with_error_status(mut self, status: fn(&Error) -> Option<StatusCode>) -> Self {
        self.error_status = status;
        self
    }

    /// Marks it as reaching beyond this machine.
    pub fn open_world(mut self) -> Self {
        self.open_world = true;
        self
    }

    /// What a person reads for it: its words, as `ahjo` takes them
    /// (`session_list` is "Session list").
    pub fn title(&self) -> String {
        let mut words = self.name.replace('_', " ");
        if let Some(first) = words.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        words
    }
}

/// Check the keys before deserializing so an unknown key is always named,
/// even when a required argument is missing too.
fn typed_arguments<Input: DeserializeOwned>(
    args: Value,
    fields: &[String],
) -> Result<Input, Error> {
    let invalid = |why: String| {
        let accepted = if fields.is_empty() {
            "none".to_owned()
        } else {
            fields.join(", ")
        };
        Error::new(
            "invalid_argument",
            format!("{why}; accepted fields: {accepted}"),
        )
    };
    let object = args
        .as_object()
        .ok_or_else(|| invalid("arguments must be an object".into()))?;
    let unknown: Vec<_> = object
        .keys()
        .filter(|key| !fields.contains(key))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(invalid(format!(
            "unknown arguments: {}",
            unknown.join(", ")
        )));
    }
    serde_json::from_value(args).map_err(|error| invalid(error.to_string()))
}
