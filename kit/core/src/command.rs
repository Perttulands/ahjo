//! The typed command a node defines and an app calls.

use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

pub trait TypedCommand: Serialize + DeserializeOwned + 'static {
    /// The same command name used by the CLI and MCP.
    const NAME: &'static str;

    /// A longer wait for commands whose work outlasts an ordinary request.
    const TIMEOUT: Option<Duration> = None;

    type Output: Serialize + DeserializeOwned + Send + 'static;
}
