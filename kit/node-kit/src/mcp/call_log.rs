//! Every tool call, without recording input or error details.

use super::rpc::Caller;

pub(super) fn record(caller: &Caller, tool: &str, target: &str, outcome: &str) {
    eprintln!("{}", line(caller, tool, target, outcome));
}

/// A command a tool set's `swarm_read` or `swarm_run` ran, beside the
/// line that tool's own call makes.
pub(super) fn relayed(through: &str, tool: &str, target: &str, outcome: &str) {
    eprintln!("ahjo: mcp {through} ran {tool} target={target:?} {outcome}");
}

fn line(caller: &Caller, tool: &str, target: &str, outcome: &str) -> String {
    format!(
        "ahjo: mcp {:?} {tool} target={:?} {outcome}",
        caller.who, target
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_only_safe_fields() {
        let caller = Caller {
            who: "client".into(),
        };
        assert_eq!(
            line(&caller, "session_send", "node-a", "failed"),
            "ahjo: mcp \"client\" session_send target=\"node-a\" failed"
        );
    }
}
