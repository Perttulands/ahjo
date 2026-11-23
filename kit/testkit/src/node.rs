//! A node in the test's own process, holding only the areas the test names,
//! with its config folder in a directory of the test's own and listening on
//! a port of its own.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use ahjo_core::Error;
use ahjo_node_kit::{Config, Listing, NodeClient, Options, Running};
use axum::http::Method;
use serde_json::{Value, json};

use crate::fresh_dir;

/// A node a test started. Stop it with [`Node::stop`]; its folder goes
/// when it drops.
pub struct Node {
    running: Option<Running>,
    dir: PathBuf,
}

impl Node {
    /// Starts a node with `areas` and the config file `config`, on a port
    /// of its own on loopback. Refused as the node refuses: an unknown
    /// section, a bad one, or areas that conflict.
    pub async fn start(areas: &[Listing], config: &str) -> anyhow::Result<Node> {
        Node::start_in(fresh_dir("ahjo-node"), areas, config).await
    }

    /// Like [`Node::start`], with `dir` as its config folder: one the test
    /// made and filled first, as an older node left it. The node owns it
    /// from here and removes it when it drops.
    pub async fn start_in(dir: PathBuf, areas: &[Listing], config: &str) -> anyhow::Result<Node> {
        let started = async {
            let config = Config::parse(config, dir.clone())?;
            let options = Options {
                dir: dir.clone(),
                config,
                listen: Some("127.0.0.1:0".into()),
            };
            ahjo_node_kit::start(options, areas).await
        };
        let running = started.await;
        let running = match running {
            Ok(running) => running,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(error);
            }
        };
        Ok(Node {
            running: Some(running),
            dir,
        })
    }

    fn running(&self) -> &Running {
        self.running.as_ref().expect("a running node")
    }

    pub fn address(&self) -> SocketAddr {
        self.running().address
    }

    /// `http://<address>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.address())
    }

    /// `ws://<address>/ws`.
    pub fn ws(&self) -> String {
        format!("ws://{}/ws", self.address())
    }

    /// The node's id.
    pub fn id(&self) -> String {
        self.running().node.id.to_string()
    }

    /// The node's sessions, as the area providing them provides them.
    pub fn sessions(&self) -> std::sync::Arc<dyn ahjo_term::Sessions> {
        self.running()
            .provided
            .get::<dyn ahjo_term::Sessions>()
            .expect("an area provides sessions")
    }

    /// What fills the node's interface for `T`, as its areas read it; an empty
    /// interface reads as `not_provided`.
    pub fn provided<T: ?Sized + Send + Sync + 'static>(&self) -> Result<std::sync::Arc<T>, Error> {
        self.running().provided.get::<T>()
    }

    /// The stand-in sessions, when the node lists [`crate::sessions`]'s area.
    pub fn fake_sessions(&self) -> std::sync::Arc<crate::sessions::FakeSessions> {
        crate::sessions::of(&self.id()).expect("the node lists the fake sessions area")
    }

    /// The node's config folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Calls the command `name` as an MCP tool, and answers with what it
    /// answered, or its error.
    pub async fn call(&self, name: &str, arguments: Value) -> Result<Value, Error> {
        let client = NodeClient::new(Some(&format!("{}/", self.url()))).expect("the node's URL");
        let message = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        });
        let (answer, _) = client
            .request(Method::POST, "/mcp", Some(message), None)
            .await
            .expect("the node answers");
        let result = &answer["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} answered no text: {answer}"));
        match result["isError"].as_bool() {
            Some(false) => Ok(serde_json::from_str(text).expect("a command answers JSON")),
            _ => Err(serde_json::from_str(text).expect("a failure is an error")),
        }
    }

    /// Stops serving and stops every area.
    pub async fn stop(mut self) {
        if let Some(running) = self.running.take() {
            running.stop().await.expect("stopping the node");
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
