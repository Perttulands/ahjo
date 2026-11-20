//! Host and service commands shared by the node and the Host page.

use ahjo_core::TypedCommand;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{Event, Facts, Outcome, Reading, Service};

/// Read this node's host facts and current reading.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostShow {
    /// Include the heaviest processes and recent events.
    pub full: Option<bool>,
}

impl TypedCommand for HostShow {
    const NAME: &'static str = "host_show";
    type Output = HostShown;
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HostShown {
    pub facts: Facts,
    pub latest: Reading,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<Event>>,
    pub node: String,
}

/// List this node's services and their state.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceList {}

impl TypedCommand for ServiceList {
    const NAME: &'static str = "service_list";
    type Output = ServicesListed;
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ServicesListed {
    pub node: String,
    pub services: Vec<Service>,
}

/// Show one service and the state of its units.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceShow {
    /// The service id.
    pub id: String,
}

impl TypedCommand for ServiceShow {
    const NAME: &'static str = "service_show";
    type Output = ServiceShown;
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ServiceShown {
    pub node: String,
    pub service: Service,
}

/// Start every unit of a service.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceStart {
    /// The service id.
    pub id: String,
}

impl TypedCommand for ServiceStart {
    const NAME: &'static str = "service_start";
    const TIMEOUT: Option<Duration> = Some(ACTION_WAIT);
    type Output = ServiceActed;
}

/// Stop every unit of a service.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceStop {
    /// The service id.
    pub id: String,
}

impl TypedCommand for ServiceStop {
    const NAME: &'static str = "service_stop";
    const TIMEOUT: Option<Duration> = Some(ACTION_WAIT);
    type Output = ServiceActed;
}

/// Stop every unit of a service, then start them.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceRestart {
    /// The service id.
    pub id: String,
}

impl TypedCommand for ServiceRestart {
    const NAME: &'static str = "service_restart";
    const TIMEOUT: Option<Duration> = Some(ACTION_WAIT);
    type Output = ServiceActed;
}

const ACTION_WAIT: Duration = Duration::from_secs(50);

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ServiceActed {
    pub node: String,
    pub service: Service,
    pub errors: Vec<String>,
    pub outcome: Outcome,
}

/// Read the last lines a service's units logged, oldest first.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceLog {
    /// The service id.
    pub id: String,
    /// How many lines to read. Defaults to 40 and is capped at 200 per unit.
    pub lines: Option<usize>,
}

impl TypedCommand for ServiceLog {
    const NAME: &'static str = "service_log";
    type Output = ServiceLogged;
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ServiceLogged {
    pub node: String,
    #[serde(flatten)]
    pub log: crate::ServiceLog,
}
