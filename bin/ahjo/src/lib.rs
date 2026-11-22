//! The node's list of areas, which `main` serves and tests check, and
//! installing and upgrading the binary that serves them.

pub mod install;

use ahjo_node_kit::Listing;
use ahjo_node_kit::mcp::ToolSet;

/// The node's areas, in the order they are built. Adding an area adds one
/// line here and one in Cargo.toml.
pub const AREAS: &[Listing] = &[
    ahjo_harness::LISTING,
    ahjo_terminals::LISTING,
    ahjo_scheduled::LISTING,
    ahjo_host::LISTING,
    ahjo_issue::LISTING,
    ahjo_brain::LISTING,
    ahjo_files::LISTING,
    ahjo_missions::LISTING,
];

/// The tool sets the node serves besides its areas' servers, each at
/// `/mcp/<name>`.
pub const TOOL_SETS: &[ToolSet] = &[CHATGPT];

/// ChatGPT's: handing work to a coding agent and following it, the issues and the host, and the rest of Swarm through `swarm_search`.
/// The brain is left to Context Citadel's own connector, so ChatGPT has one
/// brain interface, not two.
pub const CHATGPT: ToolSet = ToolSet {
    name: "chatgpt",
    tools: &[
        "project_search",
        "handoff_launch",
        "handoff_show",
        "handoff_reply",
        "session_list",
        "issue_list",
        "issue_search",
        "issue_show",
        "issue_create",
        "host_show",
        "swarm_search",
        "swarm_read",
        "swarm_run",
    ],
    instructions: "Ahjo Swarm runs coding agents (Codex, Claude Code) in terminal sessions \
        and handles Beads and GitHub issues, missions and services. \
        To hand work to an agent: project_search, then handoff_launch with a brief that stands \
        on its own; follow with handoff_show (wait long-polls) and answer with \
        handoff_reply. The company brain is the Context Citadel app, not this one. \
        swarm_search finds any other Swarm command; run it with swarm_read if it only \
        reads, swarm_run if it changes something.\n\n\
        Every tool runs on the user's machine as the user. A handoff session shows in the \
        Swarm app, where the user can watch and type into it; its name is how to follow it \
        later. session_list shows every session, including ones the user started.",
};

/// The binary's own first words, which no area's command noun may take.
pub const VERBS: &[&str] = &[
    "serve",
    "mcp",
    "hook",
    "kinds",
    "help",
    "install",
    "upgrade",
    "uninstall",
    "version",
];
