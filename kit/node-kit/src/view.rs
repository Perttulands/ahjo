//! View control, the node's side (docs/protocol.md, View): each connected
//! app reports the page it shows (0xb0); `view_open` and `view_send` send it
//! a view command (0xb8) and answer with the app's answer (0xb1).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ahjo_core::{Error, NodeId};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::{BoxFuture, Channel, Client, Command, Effect};

/// What an app shows, up.
const VIEW: u8 = 0xb0;
/// An app's answer to a view command, up.
const VIEW_DONE: u8 = 0xb1;
/// A view command, down.
const VIEW_DO: u8 = 0xb8;

/// How long `view_open` waits for the app.
const WAIT: Duration = Duration::from_secs(5);

/// The things `view_open` shows on a page, at most one at a time: each is
/// what one of Ahjo Swarm's pages reveals.
const THINGS: &[&str] = &[
    "path", "item", "run", "mission", "session", "setting", "service", "agent",
];

/// Pages Ahjo Swarm no longer has, and where what they showed is now. The
/// node answers these itself, app or none, so an agent is told where to go
/// rather than that the app has no such page.
const MOVED: &[(&str, &str)] = &[(
    "brain",
    "Ahjo Swarm has no brain page: the brain lives in the Ahjo Brain app. \
     From here, ahjo brain search and ahjo brain read reach it.",
)];

/// The connected apps, by their client.
#[derive(Default)]
pub(crate) struct Views {
    apps: Mutex<BTreeMap<u64, App>>,
    next: AtomicU32,
}

/// An app that has said what it shows.
struct App {
    shown: Value,
    down: mpsc::UnboundedSender<Vec<u8>>,
    /// Who waits for the answer to each view command sent to it.
    waiting: HashMap<u32, mpsc::UnboundedSender<Value>>,
}

impl Views {
    /// One client's side of view control: an app becomes one when it first
    /// says what it shows, and is one until it goes.
    pub(crate) fn channel(self: &Arc<Self>, client: Client) -> Box<dyn Channel> {
        let (down, pushed) = mpsc::unbounded_channel();
        Box::new(Link {
            client: client.id,
            views: self.clone(),
            down,
            pushed,
        })
    }

    fn show(&self) -> Value {
        let apps = self.apps.lock().expect("views");
        let shown: Vec<Value> = apps
            .iter()
            .map(|(client, app)| {
                let mut shown = app.shown.clone();
                shown["app"] = json!(client);
                shown
            })
            .collect();
        Value::Array(shown)
    }

    /// Sends `command` to every app and answers the first answer.
    async fn open(&self, command: Value) -> Result<Value, Error> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (answer, mut answered) = mpsc::unbounded_channel();
        let frame = [
            &[VIEW_DO][..],
            &id.to_le_bytes(),
            command.to_string().as_bytes(),
        ]
        .concat();
        let sent = {
            let mut apps = self.apps.lock().expect("views");
            let mut sent = 0;
            for app in apps.values_mut() {
                if app.down.send(frame.clone()).is_ok() {
                    app.waiting.insert(id, answer.clone());
                    sent += 1;
                }
            }
            sent
        };
        drop(answer);
        if sent == 0 {
            return Err(Error::new(
                "no_app",
                "No Ahjo Swarm app is connected to this node.",
            ));
        }
        let got = tokio::time::timeout(WAIT, answered.recv()).await;
        for app in self.apps.lock().expect("views").values_mut() {
            app.waiting.remove(&id);
        }
        match got {
            Ok(Some(answer)) if answer["ok"] == true => Ok(answer["view"].clone()),
            Ok(Some(answer)) => Err(Error::new(
                "view_refused",
                answer["error"].as_str().unwrap_or("The app refused it."),
            )),
            Ok(None) => Err(Error::new("no_app", "The app went before it answered.")),
            Err(_) => Err(Error::new(
                "timeout",
                format!("The app did not answer within {} s.", WAIT.as_secs()),
            )),
        }
    }
}

/// One client's view-control frames.
struct Link {
    client: u64,
    views: Arc<Views>,
    down: mpsc::UnboundedSender<Vec<u8>>,
    pushed: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl Channel for Link {
    fn receive(&mut self, frame: Vec<u8>) -> BoxFuture<'_, ()> {
        let mut apps = self.views.apps.lock().expect("views");
        match frame.split_first() {
            Some((&VIEW, json)) => match serde_json::from_slice(json) {
                Ok(shown) => {
                    let app = apps.entry(self.client).or_insert_with(|| App {
                        shown: Value::Null,
                        down: self.down.clone(),
                        waiting: HashMap::new(),
                    });
                    app.shown = shown;
                }
                Err(e) => eprintln!("ahjo: an app's view is not JSON: {e}"),
            },
            Some((&VIEW_DONE, body)) if body.len() >= 4 => {
                let id = u32::from_le_bytes(body[..4].try_into().expect("four bytes"));
                let answer = serde_json::from_slice(&body[4..]).unwrap_or(Value::Null);
                if let Some(waiting) = apps
                    .get_mut(&self.client)
                    .and_then(|app| app.waiting.remove(&id))
                {
                    let _ = waiting.send(answer);
                }
            }
            _ => eprintln!("ahjo: a view-control frame the node does not take"),
        }
        Box::pin(async {})
    }

    fn next(&mut self) -> BoxFuture<'_, Option<Vec<u8>>> {
        Box::pin(self.pushed.recv())
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Ok(mut apps) = self.views.apps.lock() {
            apps.remove(&self.client);
        }
    }
}

/// `view_show`, `view_open` and `view_send`. A thing opened is on `node`,
/// this node.
pub(crate) fn commands(views: &Arc<Views>, node: NodeId) -> Vec<Command> {
    let shown = views.clone();
    let opening = views.clone();
    let sending = views.clone();
    let mut properties = Map::new();
    properties.insert(
        "page".into(),
        json!({ "type": "string", "description": "sessions, files, agents, work, missions, scheduled, host or settings" }),
    );
    for thing in THINGS {
        properties.insert((*thing).into(), json!({ "type": "string" }));
    }
    properties.insert(
        "step".into(),
        json!({ "type": "string", "description": "A run's step, or an agent's harness." }),
    );
    let schema = json!({ "type": "object", "properties": properties, "required": ["page"] });
    vec![
        Command::new(
            "view_show",
            Effect::Reads,
            "What each Ahjo Swarm app connected to this node shows: its page.",
            || json!({ "type": "object", "properties": {} }),
            move |_| {
                let shown = shown.show();
                async move { Ok(shown) }
            },
        ),
        Command::new(
            "view_open",
            Effect::Sets,
            "Open a page in the operator's Ahjo Swarm app, and at most one thing on it: a session, \
             path, agent (a folder; --step names the harness), item, mission, run, service or \
             setting. Answers with what the app shows now.",
            move || schema.clone(),
            move |args: Value| {
                let views = opening.clone();
                let command = open_command(args, &node);
                async move { views.open(command?).await }
            },
        ),
        Command::new(
            "view_send",
            Effect::Writes,
            "Hand text to a session through the operator's Ahjo Swarm app, as its Send does: \
             the app opens Send to Session holding the text, and the operator picks the \
             session and sends it. Another app (Ahjo Brain's Send) reaches Swarm this way. \
             Answers with what the app shows now.",
            || {
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "What to send, such as brain page knowledge/x/y.md." },
                    },
                    "required": ["text"],
                })
            },
            move |args: Value| {
                let views = sending.clone();
                let command = send_command(&args);
                async move { views.open(command?).await }
            },
        ),
    ]
}

/// The view command `view_send`'s arguments ask for.
fn send_command(args: &Value) -> Result<Value, Error> {
    match &args["text"] {
        Value::String(t) if !t.trim().is_empty() => Ok(json!({ "do": "send", "reference": t })),
        _ => Err(Error::new(
            "invalid_argument",
            "view send needs the text to send",
        )),
    }
}

/// The view command `view_open`'s arguments ask for, as the app reads it.
fn open_command(args: Value, node: &NodeId) -> Result<Value, Error> {
    let invalid = |m: String| Error::new("invalid_argument", m);
    let text = |key: &str| match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(invalid(format!("{key} is text"))),
    };
    let page = text("page")?.ok_or_else(|| invalid("view open names a page".into()))?;
    if let Some((_, moved)) = MOVED.iter().find(|(gone, _)| *gone == page) {
        return Err(Error::new("no_page", *moved));
    }
    let mut command = json!({ "do": "open", "page": page });
    let mut things = Vec::new();
    for thing in THINGS {
        if let Some(id) = text(thing)? {
            things.push(*thing);
            command[*thing] = json!(id);
        }
    }
    if things.len() > 1 {
        return Err(invalid(format!(
            "view open shows one thing at a time, not {}",
            things.join(" and ")
        )));
    }
    if let Some(step) = text("step")? {
        command["step"] = json!(step);
    }
    if !things.is_empty() {
        command["node"] = json!(node);
    }
    Ok(command)
}
