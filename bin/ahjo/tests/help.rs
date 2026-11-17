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
            assert!(stdout.contains("ahjo beta show"), "{stdout}");
            assert!(!stdout.contains("ahjo alpha show"), "{stdout}");
        }
    }
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_ahjo"))
        .arg("help")
        .env("XDG_CONFIG_HOME", &xdg)
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("ahjo alpha show"), "{stdout}");
    assert!(!stdout.contains("ahjo beta show"), "{stdout}");
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
