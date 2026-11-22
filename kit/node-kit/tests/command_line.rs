//! What a command line hands the node's tool: a value that looks like one of
//! the command line's own flags stays a value, and the node it names is the
//! one asked.
use ahjo_node_kit::{Area, Built, Command, Config, Listing, Options, Running, Setup, cli};
use serde_json::{Value, json};

/// Save a note.
#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
struct Save {
    path: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    new: Option<bool>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Saved {
    ran_on: &'static str,
    given: Save,
}

/// `note save <path> <text> [--new]`, answering what it was given and
/// which node it ran on.
struct Echo(&'static str);
impl Area for Echo {
    fn commands(&self) -> Vec<Command> {
        let node = self.0;
        vec![Command::typed(
            "note_save",
            ahjo_node_kit::Effect::Writes,
            move |given: Save| async move {
                Ok(Saved {
                    ran_on: node,
                    given,
                })
            },
        )]
    }
}
fn one(_: &Setup) -> anyhow::Result<Built> {
    Built::now(Echo("one"))
}
fn two(_: &Setup) -> anyhow::Result<Built> {
    Built::now(Echo("two"))
}
const fn listing(build: fn(&Setup) -> anyhow::Result<Built>) -> Listing {
    Listing {
        name: "notes",
        kinds: &[],
        commands: &["note_save"],
        build,
    }
}

async fn start(name: &str, build: fn(&Setup) -> anyhow::Result<Built>) -> (Running, String) {
    let dir = std::env::temp_dir().join(format!("ahjo-cli-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let node = ahjo_node_kit::start(
        Options {
            config: Config::parse("", dir.clone()).unwrap(),
            dir,
            listen: Some("127.0.0.1:0".into()),
        },
        &[listing(build)],
    )
    .await
    .unwrap();
    let url = format!("http://{}/", node.address);
    (node, url)
}

async fn run(args: &[&str]) -> (i32, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut out = Vec::new();
    let code = cli::run("note", &args, &mut out).await.unwrap();
    (code, String::from_utf8(out).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn values_stay_values_and_the_named_node_is_asked() {
    let (one, url_one) = start("one", one).await;
    let (two, url_two) = start("two", two).await;

    // A text that is the command line's own flag stays the text, and the
    // `--new` guard after it reaches the tool.
    for own in ["--json", "--node", "--help", "-h"] {
        let (code, out) = run(&[
            "save", "p", "--text", own, "--new", "--json", "--node", &url_one,
        ])
        .await;
        assert_eq!(code, 0, "{own}: {out}");
        let answer: Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("{own}: {out}"));
        assert_eq!(
            answer["given"],
            json!({"path": "p", "text": own, "new": true}),
            "{own}"
        );
    }

    // After `--`, a page with front matter is a value.
    let (code, out) = run(&[
        "save",
        "--node",
        &url_one,
        "--json",
        "--",
        "p.md",
        "---\ntitle: x\n---\n",
    ])
    .await;
    assert_eq!(code, 0, "{out}");
    let answer: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(answer["given"]["text"], "---\ntitle: x\n---\n");

    // `--help` where a flag stands is help, and calls nothing.
    let (code, out) = run(&["save", "p", "--text", "x", "--help", "--node", &url_one]).await;
    assert_eq!(code, 0);
    assert!(out.starts_with("usage: ahjo note save"), "{out}");

    // The node is the one the command line names once read with each
    // flag's arity, even where a first look would take another: here the
    // second `--node` is the text's value's neighbour, a path.
    let (code, out) = run(&[
        "save", "--node", &url_one, "--text", "--node", &url_two, "--json",
    ])
    .await;
    assert_eq!(code, 0, "{out}");
    let answer: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(answer["ran_on"], "one", "{answer}");
    assert_eq!(answer["given"], json!({"path": url_two, "text": "--node"}));
    let (_, out) = run(&["--node", &url_two, "save", "p", "t", "--json"]).await;
    let answer: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(answer["ran_on"], "two", "{answer}");

    one.stop().await.unwrap();
    two.stop().await.unwrap();
}
