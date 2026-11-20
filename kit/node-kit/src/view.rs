//! View control, the node's side (docs/protocol.md, View): each connected
//! app reports the page it shows (0xb0); `view_open` and `view_send` send it
//! a view command (0xb8) and answer with the app's answer (0xb1).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ahjo_core::{
    Error, NodeId,
    commands::{AppView, View, ViewOpen, ViewSend, ViewShow, ViewsShown},
};
use serde::Deserialize;
use serde_json::{Value, json};
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
    shown: View,
    down: mpsc::UnboundedSender<Vec<u8>>,
    /// Who waits for the answer to each view command sent to it.
    waiting: HashMap<u32, mpsc::UnboundedSender<Result<View, Error>>>,
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

    fn show(&self) -> ViewsShown {
        let apps = self.apps.lock().expect("views");
        ViewsShown {
            apps: apps
                .iter()
                .map(|(&app, view)| AppView {
                    app,
                    view: view.shown.clone(),
                })
                .collect(),
        }
    }

    /// Sends `command` to every app and answers the first answer.
    async fn open(&self, command: Value) -> Result<View, Error> {
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
            Ok(Some(answer)) => answer,
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
                    if let Some(app) = apps.get_mut(&self.client) {
                        app.shown = shown;
                    } else {
                        apps.insert(
                            self.client,
                            App {
                                shown,
                                down: self.down.clone(),
                                waiting: HashMap::new(),
                            },
                        );
                    }
                }
                Err(e) => eprintln!("ahjo: an app's view is not JSON: {e}"),
            },
            Some((&VIEW_DONE, body)) if body.len() >= 4 => {
                let id = u32::from_le_bytes(body[..4].try_into().expect("four bytes"));
                if let Some(waiting) = apps
                    .get_mut(&self.client)
                    .and_then(|app| app.waiting.remove(&id))
                {
                    let _ = waiting.send(read_answer(&body[4..]));
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

#[derive(Deserialize)]
struct Answer {
    ok: bool,
    view: Option<View>,
    error: Option<String>,
}

fn read_answer(body: &[u8]) -> Result<View, Error> {
    let answer: Answer = serde_json::from_slice(body).map_err(|error| {
        Error::new(
            "view_refused",
            format!("The app's answer did not read: {error}"),
        )
    })?;
    if answer.ok {
        return answer
            .view
            .ok_or_else(|| Error::new("view_refused", "The app did not say what it shows."));
    }
    Err(Error::new(
        "view_refused",
        answer.error.unwrap_or_else(|| "The app refused it.".into()),
    ))
}

/// `view_show`, `view_open` and `view_send`. A thing opened is on `node`.
pub(crate) fn commands(views: &Arc<Views>, node: NodeId) -> Vec<Command> {
    let shown = views.clone();
    let opening = views.clone();
    let sending = views.clone();
    vec![
        Command::typed("view_show", Effect::Reads, move |_: ViewShow| {
            let answer = shown.show();
            async move { Ok(answer) }
        }),
        Command::typed("view_open", Effect::Sets, move |args: ViewOpen| {
            let views = opening.clone();
            let command = open_command(args, &node);
            async move { views.open(command?).await }
        }),
        Command::typed("view_send", Effect::Writes, move |args: ViewSend| {
            let views = sending.clone();
            async move {
                if args.text.trim().is_empty() {
                    return Err(Error::new(
                        "invalid_argument",
                        "view send needs the text to send",
                    ));
                }
                views
                    .open(json!({ "do": "send", "reference": args.text }))
                    .await
            }
        }),
    ]
}

/// What the app needs to open a page and reveal one thing.
fn open_command(args: ViewOpen, node: &NodeId) -> Result<Value, Error> {
    if let Some((_, moved)) = MOVED.iter().find(|(gone, _)| *gone == args.page) {
        return Err(Error::new("no_page", *moved));
    }
    let things = [
        ("path", args.path),
        ("item", args.item),
        ("run", args.run),
        ("mission", args.mission),
        ("session", args.session),
        ("setting", args.setting),
        ("service", args.service),
        ("agent", args.agent),
    ];
    let mut command = json!({ "do": "open", "page": args.page });
    let mut shown = Vec::new();
    for (what, id) in things {
        if let Some(id) = id {
            shown.push(what);
            command[what] = json!(id);
        }
    }
    if shown.len() > 1 {
        return Err(Error::new(
            "invalid_argument",
            format!(
                "view open shows one thing at a time, not {}",
                shown.join(" and ")
            ),
        ));
    }
    if let Some(step) = args.step {
        command["step"] = json!(step);
    }
    if !shown.is_empty() {
        command["node"] = json!(node);
    }
    Ok(command)
}
