//! Each node's services as its catalog lists them,
//! and CHROTE's rules for showing a change: a row says what was asked
//! ("starting…") until the node answers, and a failure stays in systemd's
//! words until the next action on that row.
//!
//! An action whose end is not known (the node said `unconfirmed`, or did
//! not answer in time) or that left its service moving is followed: that
//! one service is read again a few times, further apart each time, until it
//! settles. One that has not settled by then says so and is left to
//! Refresh. Nothing is read on a timer otherwise.

use std::collections::{BTreeMap, HashMap};

use ahjo_core::Error;
use ahjo_host_contract::commands::{ServiceActed, ServiceLogged, ServiceShown, ServicesListed};
pub use ahjo_host_contract::{LogLine, Service};
use ahjo_host_contract::{Outcome, SERVICE_UNCONFIRMED};

/// How long after an action, or the read before, each follow-up read waits;
/// the row is left to Refresh after the last.
pub const FOLLOW: [u64; 5] = [1_000, 2_000, 4_000, 8_000, 16_000];

/// What the app knows of how an action ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Ended {
    /// It did as told and the service has settled where the change aims.
    Done,
    /// It did as told, but the service settled elsewhere (a unit that
    /// failed as it started): the row shows that, and nothing claims the
    /// change, as in CHROTE.
    Elsewhere,
    /// systemd's words.
    Failed(String),
    /// Not known yet, or the service is still moving: read it again.
    Follow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Start,
    Stop,
    Restart,
}

impl Change {
    pub fn verb(self) -> &'static str {
        match self {
            Change::Start => "start",
            Change::Stop => "stop",
            Change::Restart => "restart",
        }
    }
    /// The action's name where it is offered.
    pub fn label(self) -> &'static str {
        match self {
            Change::Start => "Start",
            Change::Stop => "Stop",
            Change::Restart => "Restart",
        }
    }
    /// What the row says while the change runs.
    pub fn word(self) -> &'static str {
        match self {
            Change::Start => "starting…",
            Change::Stop => "stopping…",
            Change::Restart => "restarting…",
        }
    }
    /// The state the change aims at.
    fn aim(self) -> &'static str {
        match self {
            Change::Stop => "stopped",
            Change::Start | Change::Restart => "running",
        }
    }
}

/// One node's catalog as last read.
#[derive(Default)]
pub struct NodeServices {
    pub list: Option<Vec<Service>>,
    /// Why the last read failed, in the node's words.
    pub error: Option<String>,
    pub reading: bool,
    /// Another read was requested while the current one was in flight.
    pub reread: bool,
    /// When the last read landed, Unix ms.
    pub read_at: Option<u64>,
    /// Rows whose change runs, until a read after it lands.
    pub changing: HashMap<String, Change>,
    /// A row's last failure, until the next action on it.
    pub failures: HashMap<String, String>,
    /// Rows whose action's end is not known yet, and what the row says
    /// meanwhile; cleared when a read finds the service settled.
    pub unsettled: HashMap<String, String>,
    /// Each service's log as last read.
    pub logs: HashMap<String, Log>,
}

/// A service's latest log lines, or why they could not be read.
#[derive(Default)]
pub struct Log {
    pub lines: Vec<LogLine>,
    pub errors: Vec<String>,
    pub reading: bool,
}

impl Log {
    /// A log read landed.
    pub fn read(&mut self, answer: Result<ServiceLogged, Error>) {
        self.reading = false;
        match answer {
            Ok(log) => {
                self.lines = log.log.lines;
                self.errors = log.log.errors;
            }
            Err(error) => self.errors = vec![error.message],
        }
    }
}

#[derive(Default)]
pub struct Services {
    pub nodes: BTreeMap<String, NodeServices>,
}

/// The word a state is shown with.
pub fn word(state: &str) -> &str {
    match state {
        "starting" => "starting…",
        "stopping" => "stopping…",
        s => s,
    }
}

/// A key that moves the chosen row, as CHROTE's list does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Down,
    Up,
    Home,
    End,
}

/// The list's groups, in its order: what needs the operator, what runs,
/// what is stopped (on purpose: a stop that failed is in Failed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Failed,
    Running,
    Stopped,
}

impl Group {
    pub fn of(state: &str) -> Group {
        match state {
            "running" | "starting" | "stopping" => Group::Running,
            "stopped" => Group::Stopped,
            _ => Group::Failed,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Group::Failed => "Failed",
            Group::Running => "Running",
            Group::Stopped => "Stopped",
        }
    }
}

/// Where a key moves the chosen row in `order` (every row, in the page's
/// order). With none chosen, Down and Up choose the first; an empty page
/// moves nothing.
pub fn step<T: Clone + PartialEq>(order: &[T], at: Option<&T>, step: Step) -> Option<T> {
    if order.is_empty() {
        return None;
    }
    let at = at.and_then(|s| order.iter().position(|o| o == s));
    let last = order.len() - 1;
    let to = match (step, at) {
        (Step::Home, _) | (Step::Up | Step::Down, None) => 0,
        (Step::End, _) => last,
        (Step::Down, Some(i)) => (i + 1).min(last),
        (Step::Up, Some(i)) => i.saturating_sub(1),
    };
    Some(order[to].clone())
}

impl Services {
    /// The service on `node` with `id`, as last read.
    pub fn service(&self, node: &str, id: &str) -> Option<&Service> {
        self.nodes.get(node)?.row(id)
    }
}

/// Whether a service's address is offered to open: it has one, and
/// something may answer there (nothing does while it is stopped or failed).
pub fn opens(s: &Service) -> bool {
    !s.link.is_empty() && !matches!(s.state.as_str(), "stopped" | "failed")
}

/// An address as a row has room for: no scheme, no closing slash, and
/// shortened in its middle past `chars`, so its host and port both read.
pub fn address(link: &str, chars: usize) -> String {
    let bare = link
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    ahjo_app_kit::widgets::pane::middle(bare, chars)
}

/// Whether systemd keeps restarting a service that keeps dying.
pub fn looping(s: &Service) -> bool {
    s.components.iter().any(|c| c.sub == "auto-restart")
}

/// Whether a service needs the operator: what the list puts under
/// Failed. The verdict and the machine's row count the same.
pub fn needs_you(s: &Service) -> bool {
    Group::of(&s.state) == Group::Failed
}

/// A service's state in the list's few words: a failed one that systemd
/// keeps restarting says so, and one whose unit this machine does not
/// have says that.
pub fn state_word(s: &Service) -> &str {
    if s.state == "failed" && looping(s) {
        "keeps restarting"
    } else if missing(s).is_some() {
        "no such unit"
    } else {
        word(&s.state)
    }
}

/// The unit of a service that this machine does not have, if any.
pub fn missing(s: &Service) -> Option<&str> {
    (s.state == "unknown")
        .then(|| s.components.iter().find(|c| c.sub == "not-found"))
        .flatten()
        .map(|c| c.unit.as_str())
}

/// Why a service's state is not known, in a sentence: a unit this machine
/// does not have, one systemd will not load, or one it did not answer
/// about. None for a known state.
pub fn unknown_why(s: &Service) -> Option<String> {
    if s.state != "unknown" {
        return None;
    }
    if let Some(unit) = missing(s) {
        return Some(format!("No unit named {unit} on this machine"));
    }
    let c = s.components.iter().find(|c| c.state == "unknown")?;
    Some(match c.sub.as_str() {
        "" => format!("systemd did not say how {} is", c.unit),
        "masked" => format!("{} is masked: systemd will not start it", c.unit),
        why => format!("systemd cannot load {}: {why}", c.unit),
    })
}

/// The lines of a log from before the service's current run, when it
/// runs: how many of the first lines are older than `since`.
pub fn earlier(lines: &[LogLine], since: Option<u64>) -> usize {
    since.map_or(0, |since| {
        lines
            .iter()
            .take_while(|l| l.at.saturating_add(RUN_SLACK) < since)
            .count()
    })
}
/// A run's first lines are written a little before systemd counts it as
/// entered.
const RUN_SLACK: u64 = 2_000;

/// The actions a service offers in its state, the one most wanted first
/// (it stands in the head; the rest wait under More): a running one
/// restarts or stops, a failed or stopped one starts, one that is changing
/// or not known offers none.
pub fn actions(state: &str, looping: bool) -> &'static [Change] {
    match state {
        "running" => &[Change::Restart, Change::Stop],
        // Stopping is what ends the restarts of one that keeps dying.
        "failed" if looping => &[Change::Start, Change::Stop],
        "failed" | "partial" | "stopped" => &[Change::Start],
        _ => &[],
    }
}

/// How long ago, in a few characters: `<1m`, `12m`, `4h 12m`, `3d 4h`.
pub fn ago(since: u64, now: u64) -> String {
    let secs = now.saturating_sub(since) / 1000;
    if secs < 60 {
        "<1m".into()
    } else {
        crate::host_store::uptime(secs)
    }
}

/// What Send hands over for a service: which, in what state since when,
/// its units' states and its address.
pub fn reference(s: &Service, now: u64) -> String {
    let mut out = format!("service {} ({}): {}", s.id, s.name, word(&s.state));
    if let Some(since) = s.since() {
        out.push_str(&format!(" for {}", ago(since, now)));
    }
    if s.restarts() > 0 {
        out.push_str(&format!(", restarted {} times by systemd", s.restarts()));
    }
    let units: Vec<String> = s
        .components
        .iter()
        .map(|c| format!("{} {}", c.unit, c.state))
        .collect();
    if !units.is_empty() {
        out.push_str(&format!("; {}", units.join(", ")));
    }
    if !s.link.is_empty() {
        out.push_str(&format!("; {}", s.link));
    }
    out
}

impl NodeServices {
    /// A list read landed, or the node refused it.
    pub fn listed(&mut self, answer: Result<ServicesListed, Error>, now: u64) {
        self.reading = false;
        match answer {
            Ok(l) => {
                self.unsettled.retain(|id, _| {
                    l.services
                        .iter()
                        .any(|s| s.id == *id && (s.state == "starting" || s.state == "stopping"))
                });
                self.list = Some(l.services);
                self.error = None;
                self.read_at = Some(now);
            }
            Err(error) => {
                let error = error.message;
                // A node with no catalog has no services: none of those
                // read before stays listed.
                if error.starts_with(ahjo_host_contract::NO_CATALOG) {
                    self.list = None;
                    self.unsettled.clear();
                }
                self.error = Some(error);
            }
        }
    }

    /// Why the services could not be read. A node with no catalog was
    /// only never told which services to show: that is not a failure.
    pub fn unread(&self) -> Option<&str> {
        self.error
            .as_deref()
            .filter(|e| !e.starts_with(ahjo_host_contract::NO_CATALOG))
    }

    /// Where a node without a catalog says to write one.
    pub fn no_catalog(&self) -> Option<&str> {
        self.error
            .as_deref()?
            .strip_prefix(ahjo_host_contract::NO_CATALOG)
    }

    /// The state shown for a row: the change asked for while it runs.
    pub fn shown<'a>(&'a self, s: &'a Service) -> &'a str {
        match self.changing.get(&s.id) {
            Some(c) => c.word(),
            None => word(&s.state),
        }
    }

    /// An action answered: the row takes the node's read of it. A node
    /// that did not answer in time may still be acting.
    pub fn answered(
        &mut self,
        id: &str,
        change: Change,
        answer: Result<ServiceActed, Error>,
    ) -> Ended {
        self.changing.remove(id);
        let ended = match answer {
            Err(error) if error.code == "unanswered" => {
                self.unsettled.insert(
                    id.into(),
                    format!(
                        "The node did not answer the {}; reading it again.",
                        change.verb()
                    ),
                );
                Ended::Follow
            }
            Err(error) if error.code == SERVICE_UNCONFIRMED => {
                self.unsettled.insert(id.into(), error.message);
                Ended::Follow
            }
            Err(error) => Ended::Failed(error.message),
            Ok(answer) => {
                self.put(answer.service);
                if answer.outcome == Outcome::Unconfirmed {
                    self.unsettled.insert(id.into(), answer.errors.join("; "));
                    Ended::Follow
                } else if !answer.errors.is_empty() {
                    Ended::Failed(answer.errors.join("; "))
                } else if self.moving(id) {
                    Ended::Follow
                } else if self.row(id).is_some_and(|s| s.state == change.aim()) {
                    Ended::Done
                } else {
                    Ended::Elsewhere
                }
            }
        };
        match &ended {
            Ended::Failed(f) => {
                self.failures.insert(id.to_string(), f.clone());
            }
            _ => {
                self.failures.remove(id);
            }
        }
        ended
    }

    /// A follow-up read of one service landed. Returns whether to read it
    /// again: it is still moving, or the read failed, and `last` is not set.
    /// After the last read the row says what it last knew, as of `at`.
    pub fn followed(
        &mut self,
        id: &str,
        answer: Result<ServiceShown, Error>,
        last: bool,
        at: &str,
    ) -> bool {
        let error = match answer {
            Ok(answer) => {
                self.put(answer.service);
                if !self.moving(id) {
                    self.unsettled.remove(id);
                    return false;
                }
                None
            }
            Err(error) => Some(error.message),
        };
        if !last {
            return true;
        }
        let note = match error {
            None => format!(
                "Still {} at {at}; Refresh reads it again.",
                self.row(id).map_or("moving", |s| s.state.as_str())
            ),
            Some(error) => format!("Not read since the action ({error}); Refresh reads it again."),
        };
        self.unsettled.insert(id.into(), note);
        false
    }

    pub fn row(&self, id: &str) -> Option<&Service> {
        self.list.iter().flatten().find(|r| r.id == id)
    }

    fn moving(&self, id: &str) -> bool {
        self.row(id)
            .is_some_and(|s| s.state == "starting" || s.state == "stopping")
    }

    fn put(&mut self, s: Service) {
        if let Some(list) = &mut self.list
            && let Some(row) = list.iter_mut().find(|r| r.id == s.id)
        {
            *row = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).unwrap()
    }

    fn node() -> NodeServices {
        let mut n = NodeServices::default();
        n.listed(
            Ok(fixture(r#"{"node":"n","services":[{"id":"example","name":"Example","description":"d","link":"http://h:8080/x","state":"stopped","controllable":true,"components":[{"unit":"example.service","state":"inactive","sub":"dead","since":1000}]},
                {"id":"room","name":"Room","state":"running","controllable":false,"components":[{"unit":"room.service","state":"active"}]}]}"#)),
            1,
        );
        n
    }

    fn action(state: &str) -> ServiceActed {
        ServiceActed {
            node: "n".into(),
            service: Service {
                id: "example".into(),
                name: "Example".into(),
                state: state.into(),
                controllable: true,
                ..Service::default()
            },
            errors: Vec::new(),
            outcome: Outcome::Done,
        }
    }

    fn service(state: &str) -> ServiceShown {
        ServiceShown {
            node: "n".into(),
            service: Service {
                id: "example".into(),
                name: "Example".into(),
                state: state.into(),
                controllable: true,
                ..Service::default()
            },
        }
    }

    #[test]
    fn keys_move_the_chosen_row_and_stay_at_the_ends() {
        let order = ["machine", "a", "b"];
        assert_eq!(
            step(&order, None, Step::Up),
            Some("machine"),
            "none chosen: the first"
        );
        assert_eq!(step(&order, Some(&"a"), Step::Down), Some("b"));
        assert_eq!(
            step(&order, Some(&"b"), Step::Down),
            Some("b"),
            "stays on the last"
        );
        assert_eq!(step(&order, Some(&"b"), Step::Home), Some("machine"));
        assert_eq!(step(&order, Some(&"machine"), Step::Up), Some("machine"));
        assert_eq!(step(&order, None, Step::End), Some("b"));
        assert_eq!(
            step::<&str>(&[], Some(&"a"), Step::Home),
            None,
            "an empty page moves nothing"
        );
    }

    #[test]
    fn an_address_reads_without_its_scheme_and_shortens_in_the_middle() {
        assert_eq!(address("http://127.0.0.1:8091/", 30), "127.0.0.1:8091");
        let long = address("http://landmass.tail1f2f3b.ts.net:18774/", 24);
        assert_eq!(long.chars().count(), 24);
        assert!(
            long.starts_with("landmass.") && long.ends_with(":18774"),
            "{long}"
        );
    }

    #[test]
    fn a_row_says_starting_until_the_node_answers() {
        let mut n = node();
        n.changing.insert("example".into(), Change::Start);
        let s = n.list.as_ref().unwrap()[0].clone();
        assert_eq!(n.shown(&s), "starting…");
        let ended = n.answered("example", Change::Start, Ok(action("running")));
        assert_eq!(ended, Ended::Done);
        assert!(n.changing.is_empty());
        assert_eq!(n.list.as_ref().unwrap()[0].state, "running");
    }

    #[test]
    fn a_failure_stays_in_systemds_words_until_the_next_action() {
        let mut n = node();
        let ended = n.answered(
            "example",
            Change::Start,
            Err(Error::new("service_failed", "Job failed; See journal")),
        );
        assert_eq!(ended, Ended::Failed("Job failed; See journal".into()));
        n.listed(
            Ok(fixture(r#"{"node":"n","services":[{"id":"example","name":"Example","state":"stopped","controllable":true}]}"#)),
            2,
        );
        assert_eq!(
            n.failures["example"], "Job failed; See journal",
            "a read keeps it"
        );
        n.answered("example", Change::Start, Ok(action("running")));
        assert!(!n.failures.contains_key("example"));
    }

    #[test]
    fn an_unknown_end_is_followed_until_it_settles_or_is_left_to_refresh() {
        let mut n = node();
        assert_eq!(
            n.answered(
                "example",
                Change::Restart,
                Err(Error::new("unanswered", "50 s"))
            ),
            Ended::Follow,
            "the node may still be restarting it"
        );
        assert!(n.unsettled.contains_key("example"));
        assert!(
            !n.failures.contains_key("example"),
            "not a failure: not known"
        );
        assert!(n.followed("example", Ok(service("starting")), false, "01:00"));
        assert!(
            !n.followed("example", Ok(service("starting")), true, "01:00"),
            "the last read stops"
        );
        assert!(n.unsettled["example"].starts_with("Still starting at"));
        assert!(!n.followed("example", Ok(service("running")), false, "01:01"));
        assert!(!n.unsettled.contains_key("example"));

        assert_eq!(
            n.answered(
                "example",
                Change::Stop,
                Err(Error::new(
                    SERVICE_UNCONFIRMED,
                    "systemctl stop gave no answer in 15 s; systemd may still finish it"
                ))
            ),
            Ended::Follow
        );
        assert!(n.unsettled["example"].contains("may still finish"));
        n.listed(
            Ok(fixture(r#"{"node":"n","services":[{"id":"example","name":"Example","state":"stopped","controllable":true}]}"#)),
            3,
        );
        assert!(
            n.unsettled.is_empty(),
            "a Refresh that finds it settled clears it"
        );
    }

    #[test]
    fn does_not_claim_a_start_when_the_node_reports_it_failed() {
        let mut n = node();
        let ended = n.answered("example", Change::Start, Ok(action("failed")));
        assert_eq!(ended, Ended::Elsewhere);
        assert!(!n.failures.contains_key("example"), "its state says it");
        assert_eq!(n.list.as_ref().unwrap()[0].state, "failed");
        assert_eq!(
            n.answered("example", Change::Stop, Ok(action("stopped"))),
            Ended::Done
        );
    }

    #[test]
    fn a_plain_refusal_is_shown_as_its_text() {
        let mut n = NodeServices::default();
        n.listed(
            Err(Error::new(
                "unavailable",
                "Service controller is unavailable",
            )),
            1,
        );
        assert_eq!(
            n.error.as_deref(),
            Some("Service controller is unavailable")
        );
        n.listed(Err(Error::new("unavailable", "No service catalog")), 1);
        assert_eq!(n.error.as_deref(), Some("No service catalog"));
        assert_eq!(n.unread(), Some("No service catalog"), "a failure");
        // A node that was never given a catalog has not failed, and has
        // no services, whatever was listed before.
        n.list = Some(vec![Service::default()]);
        n.listed(
            Err(Error::new(
                "unavailable",
                "No service catalog on this node: write one at /x/services.json",
            )),
            3,
        );
        assert_eq!(n.unread(), None);
        assert_eq!(n.no_catalog(), Some("write one at /x/services.json"));
        assert!(n.list.is_none(), "nothing stays listed from before");
    }

    #[test]
    fn a_coded_action_refusal_shows_its_message() {
        let mut n = node();
        let ended = n.answered(
            "example",
            Change::Restart,
            Err(Error::new("bad_request", "This service is read-only")),
        );
        assert_eq!(ended, Ended::Failed("This service is read-only".into()));
        assert_eq!(n.failures["example"], "This service is read-only");
    }

    #[test]
    fn each_state_offers_its_own_actions_and_its_group() {
        assert_eq!(actions("running", false), [Change::Restart, Change::Stop]);
        assert_eq!(
            actions("stopped", false),
            [Change::Start],
            "not Start and Restart"
        );
        assert_eq!(actions("failed", false), [Change::Start]);
        assert_eq!(
            actions("failed", true),
            [Change::Start, Change::Stop],
            "Stop ends the restarts"
        );
        assert_eq!(actions("starting", false), []);
        assert_eq!(actions("unknown", false), []);
        assert_eq!(Group::of("stopped"), Group::Stopped, "stopped on purpose");
        assert_eq!(Group::of("failed"), Group::Failed);
        assert_eq!(Group::of("partial"), Group::Failed);
        assert_eq!(Group::of("unknown"), Group::Failed);
        let mut s = Service {
            state: "unknown".into(),
            components: vec![ahjo_host_contract::Component {
                unit: "ledger.service".into(),
                state: "unknown".into(),
                sub: "not-found".into(),
                ..Default::default()
            }],
            ..Service::default()
        };
        assert!(needs_you(&s), "what the list puts under Failed needs you");
        assert_eq!(state_word(&s), "no such unit");
        assert_eq!(
            unknown_why(&s).as_deref(),
            Some("No unit named ledger.service on this machine")
        );
        s.components[0].sub.clear();
        assert_eq!(
            unknown_why(&s).as_deref(),
            Some("systemd did not say how ledger.service is")
        );
        s.state = "stopped".into();
        assert!(!needs_you(&s) && unknown_why(&s).is_none());
        assert_eq!(Group::of("stopping"), Group::Running);
    }

    #[test]
    fn an_address_is_offered_only_where_something_may_answer() {
        let mut n = node();
        let list = n.list.as_mut().unwrap();
        assert!(!opens(&list[0]), "stopped");
        list[0].state = "running".into();
        assert!(opens(&list[0]));
        list[0].state = "failed".into();
        assert!(!opens(&list[0]));
        assert!(!opens(&list[1]), "running, but it has no address");
    }

    #[test]
    fn a_unit_systemd_keeps_restarting_says_so() {
        let mut n = node();
        let s = &mut n.list.as_mut().unwrap()[0];
        s.state = "failed".into();
        assert_eq!(state_word(s), "failed");
        s.components[0].sub = "auto-restart".into();
        assert_eq!(state_word(s), "keeps restarting");
    }

    #[test]
    fn a_log_knows_which_lines_are_from_before_this_run() {
        let line = |at: u64| LogLine {
            at,
            ..LogLine::default()
        };
        let lines = [line(1_000), line(50_000), line(99_000), line(120_000)];
        assert_eq!(earlier(&lines, Some(100_000)), 2);
        assert_eq!(earlier(&lines, Some(500)), 0);
        assert_eq!(
            earlier(&lines, None),
            0,
            "not running: no run to tell apart"
        );
    }

    #[test]
    fn a_service_is_handed_over_whole() {
        let n = node();
        assert_eq!(
            reference(&n.list.as_ref().unwrap()[0], 1000 + 12 * 60_000),
            "service example (Example): stopped for 12m; example.service inactive; http://h:8080/x"
        );
        assert_eq!(
            reference(&n.list.as_ref().unwrap()[1], 0),
            "service room (Room): running; room.service active"
        );
    }

    #[test]
    fn a_log_is_its_lines_or_why_not() {
        let mut log = Log {
            reading: true,
            ..Log::default()
        };
        log.read(Ok(fixture(
            r#"{"node":"n","lines":[{"at":1,"text":"listening","error":false}],"errors":[]}"#,
        )));
        assert!(!log.reading);
        assert_eq!(log.lines[0].text, "listening");
        log.read(Err(Error::new("not_found", "No such service")));
        assert_eq!(log.errors, ["No such service"]);
    }
}
