//! Requests and responses for the commands every node serves.

use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Show the config file's path and the keys and values this node loaded.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigShow {
    /// Only this section. The file has [node] and a section for each area.
    pub section: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ConfigShown {
    pub path: PathBuf,
    pub sections: BTreeMap<String, Map<String, Value>>,
}

impl crate::command::TypedCommand for ConfigShow {
    const NAME: &'static str = "config_show";
    type Output = ConfigShown;
}

/// Show which page each connected Ahjo Swarm app is on.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewShow {}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ViewsShown {
    pub apps: Vec<AppView>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AppView {
    pub app: u64,
    #[serde(flatten)]
    pub view: View,
}

/// Open a page in Swarm and optionally reveal one thing on it.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewOpen {
    /// sessions, files, harness, issue, missions, scheduled, host or settings.
    pub page: String,
    /// A file path to reveal.
    pub path: Option<String>,
    /// An issue to reveal.
    pub item: Option<String>,
    /// A mission run to reveal.
    pub run: Option<String>,
    /// A mission to reveal.
    pub mission: Option<String>,
    /// A session to reveal.
    pub session: Option<String>,
    /// A setting to reveal.
    pub setting: Option<String>,
    /// A service to reveal.
    pub service: Option<String>,
    /// A folder to reveal on the Harness page.
    pub agent: Option<String>,
    /// A run's step, or the harness for an agent.
    pub step: Option<String>,
}

/// Open Swarm's Send to Session with text for the operator to send.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewSend {
    /// What to send, such as brain page knowledge/x/y.md.
    pub text: String,
}

/// The page an app shows, reported over its socket and returned by view commands.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct View {
    pub page: Option<String>,
}
