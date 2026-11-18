//! The node's list of areas, which `main` serves and tests check, and
//! installing and upgrading the binary that serves them.

pub mod install;

use ahjo_node_kit::Listing;

/// The node's areas, in the order they are built. Adding an area adds one
/// line here and one in Cargo.toml.
pub const AREAS: &[Listing] = &[
    ahjo_agents::LISTING,
    ahjo_terminals::LISTING,
    ahjo_scheduled::LISTING,
    ahjo_host::LISTING,
    ahjo_work::LISTING,
    ahjo_brain::LISTING,
    ahjo_files::LISTING,
    ahjo_missions::LISTING,
];

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
