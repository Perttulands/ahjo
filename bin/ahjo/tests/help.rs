//! Top-level help uses the requested node, even when a different local node
//! is recorded in the CLI's config directory.
use ahjo_node_kit::{Area, Built, Command, Listing, Setup};
use ahjo_testkit::node::Node;
use serde_json::json;

struct One(&'static str);
impl Area for One {
    fn commands(&self) -> Vec<Command> {
        vec![Command::new(
            self.0,
            "A test command.",
            || json!({"type":"object","properties":{}}),
            |_| async { Ok(json!({})) },
        )]
    }
}
fn alpha(_: &Setup) -> anyhow::Result<Built> {
    Built::now(One("alpha_show"))
}
fn beta(_: &Setup) -> anyhow::Result<Built> {
    Built::now(One("beta_show"))
}
const ALPHA: Listing = Listing {
    name: "alpha",
    kinds: &[],
    commands: &["alpha_show"],
    build: alpha,
};
const BETA: Listing = Listing {
    name: "beta",
    kinds: &[],
    commands: &["beta_show"],
    build: beta,
};

#[tokio::test(flavor = "multi_thread")]
async fn help_selects_the_requested_node_instead_of_the_local_node() {
    let local = Node::start(&[ALPHA], "").await.unwrap();
    let other = Node::start(&[BETA], "").await.unwrap();
    let xdg = ahjo_testkit::fresh_dir("ahjo-help-xdg");
    let config = xdg.join("ahjo");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("listen"), local.address().to_string()).unwrap();

    for help in ["help", "--help"] {
        for flag in [
            vec!["--node".to_owned(), other.url()],
            vec![format!("--node={}", other.url())],
        ] {
            let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_ahjo"))
                .arg(help)
                .args(flag)
                .env("XDG_CONFIG_HOME", &xdg)
                .output()
                .await
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let stdout = String::from_utf8(out.stdout).unwrap();
            assert!(lists(&stdout, "beta"), "{stdout}");
            assert!(!lists(&stdout, "alpha"), "{stdout}");
        }
    }
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_ahjo"))
        .arg("help")
        .env("XDG_CONFIG_HOME", &xdg)
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(lists(&stdout, "alpha"), "{stdout}");
    assert!(!lists(&stdout, "beta"), "{stdout}");
    // Every command, with what it does, only when asked for all.
    assert!(!stdout.contains("A test command"), "{stdout}");
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_ahjo"))
        .args(["help", "--all"])
        .env("XDG_CONFIG_HOME", &xdg)
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("ahjo alpha show  A test command"),
        "{stdout}"
    );
    for args in [
        vec!["help", "--node"],
        vec!["--help", "--node="],
        vec!["help", "--bogus"],
    ] {
        let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_ahjo"))
            .args(args)
            .env("XDG_CONFIG_HOME", &xdg)
            .output()
            .await
            .unwrap();
        assert!(!out.status.success());
    }
    local.stop().await;
    other.stop().await;
    std::fs::remove_dir_all(xdg).unwrap();
}

/// Whether help names `noun` on a line of its own with its verb.
fn lists(help: &str, noun: &str) -> bool {
    help.lines()
        .any(|line| line.split_whitespace().eq([noun, "show"]))
}

fn ahjo(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_ahjo"))
        .args(args)
        .output()
        .unwrap()
}

/// The binary's own commands answer `--help` without doing anything, and a
/// wrong command line is one usage line and exit 2, not the whole usage.
#[test]
fn own_commands_have_help_and_a_wrong_command_line_exits_2() {
    for command in [
        "serve",
        "install",
        "upgrade",
        "uninstall",
        "version",
        "mcp",
        "kinds",
        "hook",
        "help",
    ] {
        let out = ahjo(&[command, "--help"]);
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.status.code(), Some(0), "{command}: {out:?}");
        assert!(
            text.starts_with(&format!("usage: ahjo {command}")),
            "{command}: {text}"
        );
        assert!(text.lines().count() > 1, "{command} says only its usage");
        let asked = ahjo(&["help", command]);
        assert_eq!(asked.stdout, out.stdout, "ahjo help {command}");
    }
    for (args, says) in [
        (&["serve", "--bogus"][..], "usage: ahjo serve"),
        (&["version", "extra"], "usage: ahjo version"),
        (&["mcp", "--bogus"], "usage: ahjo mcp"),
        (&["install", "--bogus", "x"], "usage: ahjo install"),
        (&["upgrade", "--root", "x"], "usage: ahjo upgrade"),
        (&["--bogus"], "ahjo --help"),
    ] {
        let out = ahjo(args);
        let error = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {error}");
        assert!(error.contains(says), "{args:?}: {error}");
        assert!(error.lines().count() <= 3, "{args:?}: {error}");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}

/// With no node there, a command says so in one line and exits 3 soon, in
/// plain words or as the one JSON error shape.
#[test]
fn a_node_that_is_not_running_is_said_in_one_line_and_exits_3() {
    // Bound and dropped: a port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let node = format!("127.0.0.1:{port}");
    let started = std::time::Instant::now();
    let out = ahjo(&["session", "list", "--node", &node]);
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    let error = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3), "{error}");
    assert_eq!(error.lines().count(), 1, "{error}");
    assert!(
        error.starts_with(&format!("ahjo: no node answers on {node} (")),
        "{error}"
    );
    assert!(error.contains("ahjo serve"), "{error}");

    let out = ahjo(&["session", "list", "--json", "--node", &node]);
    assert_eq!(out.status.code(), Some(3));
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["code"], "no_node", "{error}");

    let out = ahjo(&["version", "--json", "--node", &node]);
    assert_eq!(out.status.code(), Some(3));
    let version: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(version["version"], ahjo_node_kit::VERSION);
    assert!(version["node"].is_null(), "{version}");
}
