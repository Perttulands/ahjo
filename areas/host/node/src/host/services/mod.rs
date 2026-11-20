//! Services: the host's catalog of services with their state and log,
//! started and stopped through systemd. Ahjo owns this itself, with CHROTE's
//! catalog, group and partial-state rules; it reads only when asked, so it
//! costs nothing while nobody looks.
//!
//! Starting goes through the components in order, stopping in reverse,
//! and restarting stops them all before it starts them. An unconfirmed
//! action may still finish in systemd and only a later read can say.

mod catalog;
mod systemd;

use std::path::PathBuf;
use std::time::Duration;

use ahjo_core::Error;
use ahjo_host_contract::{BAD_REQUEST, NOT_FOUND, Outcome, Service, ServiceLog, UNAVAILABLE};
use tokio::sync::Mutex;

use catalog::Entry;
use systemd::Refusal;

/// The most one action may take, every unit of it: clients wait this long
/// for an answer, and a little more.
pub const ACTION: Duration = Duration::from_secs(40);

/// The node's services: where its catalog is, and one action at a time.
pub struct Services {
    catalog: PathBuf,
    events: std::sync::Arc<ahjo_node_kit::events::Log>,
    acting: Mutex<()>,
}

/// A service's state from its components' (CHROTE's controller, in order).
pub fn group(components: &[&str]) -> &'static str {
    let all = |s: &str| components.iter().all(|c| *c == s);
    let any = |s: &[&str]| components.iter().any(|c| s.contains(c));
    if all("active") {
        "running"
    } else if all("inactive") {
        "stopped"
    } else if any(&["failed"]) {
        "failed"
    } else if any(&["unknown"]) {
        "unknown"
    } else if any(&["activating", "reloading"]) {
        "starting"
    } else if any(&["deactivating"]) {
        "stopping"
    } else {
        "partial"
    }
}

async fn status(e: &Entry) -> Service {
    let reads = e.components.iter().map(systemd::state);
    let components = futures_util::future::join_all(reads).await;
    let states: Vec<&str> = components.iter().map(|c| c.state.as_str()).collect();
    Service {
        id: e.id.clone(),
        name: e.name.clone(),
        description: e.description.clone(),
        link: e.link.clone(),
        state: group(&states).to_string(),
        controllable: !e.read_only,
        components,
    }
}

/// How many log lines a read gives when it does not say.
pub const LOG_LINES: usize = 40;

impl Services {
    pub fn with_events(mut self, events: std::sync::Arc<ahjo_node_kit::events::Log>) -> Self {
        self.events = events;
        self
    }

    pub fn new(catalog: PathBuf) -> Services {
        Services {
            catalog,
            events: Default::default(),
            acting: Mutex::new(()),
        }
    }

    fn entries(&self) -> Result<Vec<Entry>, Error> {
        if !self.catalog.exists() {
            return Err(Error::new(
                UNAVAILABLE,
                format!(
                    "{}write one at {}",
                    ahjo_host_contract::NO_CATALOG,
                    self.catalog.display()
                ),
            ));
        }
        catalog::read(&self.catalog).map_err(|e| Error::new(UNAVAILABLE, e))
    }

    pub async fn list(&self) -> Result<Vec<Service>, Error> {
        let entries = self.entries()?;
        Ok(futures_util::future::join_all(entries.iter().map(status)).await)
    }

    fn entry(&self, id: &str) -> Result<Entry, Error> {
        self.entries()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| Error::new(NOT_FOUND, format!("No service {id} in the catalog")))
    }

    /// One service as its units are now.
    pub async fn one(&self, id: &str) -> Result<Service, Error> {
        Ok(status(&self.entry(id)?).await)
    }

    /// The last `lines` lines a service's units logged, oldest first; a
    /// unit whose log does not read says why instead.
    pub async fn log(&self, id: &str, lines: usize) -> Result<ServiceLog, Error> {
        let entry = self.entry(id)?;
        let units = entry
            .components
            .iter()
            .filter(|c| !matches!(c, catalog::Component::Http { .. }));
        let reads = futures_util::future::join_all(units.map(|c| systemd::log(c, lines))).await;
        let mut log = ServiceLog::default();
        for read in reads {
            match read {
                Ok(lines) => log.lines.extend(lines),
                Err(e) => log.errors.push(e),
            }
        }
        log.lines.sort_by_key(|l| l.at);
        let over = log.lines.len().saturating_sub(lines);
        log.lines.drain(..over);
        Ok(log)
    }

    /// Starts, stops or restarts every unit of a service within
    /// [`ACTION`]; each failure in systemd's words. Every unit of a start
    /// or a stop is tried, whatever the one before did; a restart starts
    /// only once every unit has stopped.
    pub async fn act(
        &self,
        id: &str,
        action: &str,
    ) -> Result<(Service, Vec<String>, Outcome), Error> {
        let bad = |message: &str| Error::new(BAD_REQUEST, message);
        let steps: &[&str] = match action {
            "start" => &["start"],
            "stop" => &["stop"],
            "restart" => &["stop", "start"],
            _ => return Err(bad("A service starts, stops or restarts.")),
        };
        let entry = self.entry(id)?;
        if entry.read_only {
            return Err(bad(
                "This service is read-only: the host shows it but does not let Ahjo change it.",
            ));
        }
        let _one = self.acting.lock().await;
        let deadline = tokio::time::Instant::now() + ACTION;
        let mut errors = vec![];
        let mut outcome = Outcome::Done;
        for (n, verb) in steps.iter().enumerate() {
            if n > 0 && outcome != Outcome::Done {
                errors.push(format!("Not {verb}ed: the stop did not finish."));
                break;
            }
            let mut order: Vec<_> = entry.components.iter().collect();
            if *verb == "stop" {
                order.reverse();
            }
            for c in order {
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                if left.is_zero() {
                    errors.push(format!("Not tried: the {action} ran out of time."));
                    outcome = max_outcome(outcome, Outcome::Failed);
                    continue;
                }
                match systemd::act(c, verb, left.min(systemd::SYSTEMCTL)).await {
                    Ok(()) => {}
                    Err(Refusal::Failed(e)) => {
                        errors.push(e);
                        outcome = max_outcome(outcome, Outcome::Failed);
                    }
                    Err(Refusal::Unconfirmed(e)) => {
                        errors.push(e);
                        outcome = Outcome::Unconfirmed;
                    }
                }
            }
        }
        let name = &entry.name;
        let (failure, text) = match outcome {
            Outcome::Done => {
                let done = match action {
                    "stop" => "stopped",
                    "start" => "started",
                    _ => "restarted",
                };
                (false, format!("{name} {done}"))
            }
            Outcome::Failed => (
                true,
                format!("{name}: {action} failed: {}", errors.join("; ")),
            ),
            Outcome::Unconfirmed => (
                true,
                format!("{name}: {action} unconfirmed: {}", errors.join("; ")),
            ),
        };
        self.events.record("service", failure, text);
        Ok((status(&entry).await, errors, outcome))
    }
}

/// Unconfirmed outweighs failed: something may still be moving.
fn max_outcome(a: Outcome, b: Outcome) -> Outcome {
    match (a, b) {
        (Outcome::Unconfirmed, _) | (_, Outcome::Unconfirmed) => Outcome::Unconfirmed,
        (Outcome::Failed, _) | (_, Outcome::Failed) => Outcome::Failed,
        _ => Outcome::Done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_reads_as_chrotes_controller_does() {
        assert_eq!(group(&["active", "active"]), "running");
        assert_eq!(group(&["inactive", "inactive"]), "stopped");
        assert_eq!(group(&["active", "inactive"]), "partial");
        assert_eq!(group(&["active", "failed"]), "failed");
        assert_eq!(group(&["active", "unknown"]), "unknown");
        assert_eq!(group(&["active", "activating"]), "starting");
        assert_eq!(group(&["inactive", "deactivating"]), "stopping");
        assert_eq!(
            group(&["failed", "unknown"]),
            "failed",
            "failed comes first"
        );
    }

    #[tokio::test]
    async fn refuses_an_unknown_action_an_unknown_id_and_a_read_only_service() {
        let dir = std::env::temp_dir().join(format!("ahjo-services-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("services.json");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let responder = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
        });
        std::fs::write(
            &file,
            format!(
                r#"{{"services":[{{"id":"room","name":"Room","readOnly":true,
                "components":[{{"kind":"http","url":"http://{address}/"}}]}}]}}"#
            ),
        )
        .unwrap();
        let s = Services::new(file);
        let code = |r: Result<_, Error>| r.unwrap_err().code;
        assert_eq!(code(s.act("room", "reload").await), BAD_REQUEST);
        assert_eq!(code(s.act("nope", "start").await), NOT_FOUND);
        assert_eq!(code(s.act("room", "start").await), BAD_REQUEST);
        let listed = s.list().await.unwrap();
        assert_eq!(listed[0].state, "running");
        responder.await.unwrap();
        assert!(!listed[0].controllable);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn without_a_catalog_the_node_says_where_to_write_one() {
        let s = Services::new(PathBuf::from("/nonexistent/ahjo/services.json"));
        let r = s.list().await.unwrap_err();
        assert_eq!(r.code, UNAVAILABLE);
        assert!(r.message.contains("/nonexistent/ahjo/services.json"));
    }
}
