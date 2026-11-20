//! What shapes an agent, read from a scratch home and project the way each
//! harness reads them, and changed through the node's commands.
use ahjo_node_kit::{NodeClient, NodeError};
use ahjo_testkit::{fresh_dir, node::Node, sessions};
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

struct World {
    home: PathBuf,
    srv: PathBuf,
    project: PathBuf,
    dir: PathBuf,
}

/// A home and a tree: `<srv>/AGENTS.md` above a repository `<srv>/proj`
/// with its own files, and a folder `src` inside it.
fn world() -> World {
    let home = fresh_dir("ahjo-harness-shape-home");
    let srv = fresh_dir("ahjo-harness-shape-srv");
    let project = srv.join("proj");
    let dir = project.join("src");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    put(&project.join(".git/HEAD"), "ref: refs/heads/main\n");
    std::fs::create_dir_all(&dir).unwrap();
    put(&srv.join("AGENTS.md"), "Shared rules.\n");
    put(
        &project.join("AGENTS.md"),
        "Project rules. See @docs/more.md\n",
    );
    put(&project.join("docs/more.md"), "More.\n");
    put(&dir.join("sub/AGENTS.md"), "Deeper.\n");
    // Claude Code
    put(&home.join(".claude/CLAUDE.md"), "Mine.\n");
    put(
        &home.join(".claude/settings.json"),
        &json!({
            "model": "opus",
            "permissions": {"deny": ["WebFetch"]},
            "skillOverrides": {"quiet": "off"},
            "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "guard"}]}]}
        })
        .to_string(),
    );
    put(
        &dir.join(".claude/settings.json"),
        &json!({"model": "sonnet"}).to_string(),
    );
    put(
        &home.join(".claude/skills/handoff/SKILL.md"),
        "---\nname: handoff\ndescription: Leave a handoff.\n---\n",
    );
    put(
        &home.join(".claude/skills/quiet/SKILL.md"),
        "---\nname: quiet\ndescription: Off.\n---\n",
    );
    put(
        &project.join(".claude/skills/handoff/SKILL.md"),
        "---\nname: handoff\ndescription: The project's.\n---\n",
    );
    let key: String = project
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    put(
        &home
            .join(".claude/projects")
            .join(key)
            .join("memory/MEMORY.md"),
        "- a memory\n",
    );
    put(
        &home.join(".claude.json"),
        &json!({
            "mcpServers": {"calendar": {"command": "calendar-mcp"}},
            "projects": {project.to_string_lossy(): {"enabledMcpjsonServers": ["beds"]}}
        })
        .to_string(),
    );
    put(
        &project.join(".mcp.json"),
        &json!({"mcpServers": {"beds": {"command": "beds-mcp"}, "weather": {"url": "https://example.invalid/mcp"}}})
            .to_string(),
    );
    // Codex
    put(&home.join(".codex/AGENTS.md"), "Codex mine.\n");
    put(
        &home.join(".codex/config.toml"),
        &format!(
            "# kept\nmodel = \"gpt\"\n\n[projects.{:?}]\ntrust_level = \"trusted\"\n",
            project.to_string_lossy()
        ),
    );
    put(
        &home.join(".codex/hooks.json"),
        &json!({"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "first"}, {"type": "command", "command": "second"}]}]}})
            .to_string(),
    );
    put(
        &home.join(".agents/skills/tidy/SKILL.md"),
        "---\nname: tidy\ndescription: Tidy.\n---\n",
    );
    put(
        &home.join(".agents/skills/shelf/old/SKILL.md"),
        "---\nname: old\ndescription: Benched.\n---\n",
    );
    put(
        &project.join(".agents/skills/local/SKILL.md"),
        "---\nname: local\ndescription: Here.\n---\n",
    );
    World {
        home,
        srv,
        project,
        dir,
    }
}

fn paths(v: &Value, list: &str) -> Vec<(String, String, String)> {
    v[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["path"].as_str().unwrap_or_default().to_owned(),
                i["origin"].as_str().unwrap().to_owned(),
                i["load"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn each_harness_reads_its_own_files_in_its_own_order() {
    let w = world();
    let config = format!("[harness]\nhome = {:?}\npath = []\n", w.home);
    let node = Node::start(&[sessions::LISTING, ahjo_harness::LISTING], &config)
        .await
        .unwrap();
    let p = |path: &Path| path.to_string_lossy().into_owned();

    let claude = node
        .call(
            "harness_shape",
            json!({"dir": p(&w.dir), "harness": "claude"}),
        )
        .await
        .unwrap();
    let got = paths(&claude, "instructions");
    let want = [
        (p(&w.home.join(".claude/CLAUDE.md")), "user", "start"),
        (p(&w.srv.join("AGENTS.md")), "ancestor", "start"),
        (p(&w.project.join("AGENTS.md")), "project", "start"),
        (p(&w.project.join("docs/more.md")), "import", "start"),
    ];
    for (i, (path, origin, load)) in want.iter().enumerate() {
        assert_eq!(
            (&got[i].0, got[i].1.as_str(), got[i].2.as_str()),
            (path, *origin, *load),
            "{got:#?}"
        );
    }
    assert!(got.contains(&(
        p(&w.dir.join("sub/AGENTS.md")),
        "project".into(),
        "later".into()
    )));
    assert!(
        got.iter()
            .any(|(path, o, l)| path.ends_with("memory/MEMORY.md")
                && o == "memory"
                && l == "start")
    );
    // The nearer settings file wins the model; the user's is overridden.
    let model: Vec<(&str, bool)> = claude["settings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["key"] == "model")
        .map(|s| {
            (
                s["value"].as_str().unwrap(),
                s["overridden"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(model, [("opus", true), ("sonnet", false)]);
    // The user's skill wins over the project's of the same name; a skill
    // switched off is said to be.
    let skills = paths(&claude, "skills");
    assert!(skills.contains(&(
        p(&w.home.join(".claude/skills/handoff/SKILL.md")),
        "user".into(),
        "start".into()
    )));
    assert!(skills.contains(&(
        p(&w.project.join(".claude/skills/handoff/SKILL.md")),
        "project".into(),
        "off".into()
    )));
    assert!(skills.contains(&(
        p(&w.home.join(".claude/skills/quiet/SKILL.md")),
        "user".into(),
        "off".into()
    )));
    assert_eq!(claude["hooks"][0]["event"], "PreToolUse");
    let servers: Vec<(&str, &str)> = claude["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["why"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(
        servers,
        [
            ("calendar", ""),
            ("beds", ""),
            ("weather", "asks before its first use")
        ],
        "approved at Claude Code's prompt, the project's server needs no asking"
    );

    // A project under the home folder: the operator's own file is theirs,
    // once, and is no CLAUDE.md above the project that would hide its
    // AGENTS.md.
    let under = w.home.join("work/garden");
    put(&under.join("AGENTS.md"), "Garden.\n");
    let at_home = node
        .call(
            "harness_shape",
            json!({"dir": p(&under), "harness": "claude"}),
        )
        .await
        .unwrap();
    let got = paths(&at_home, "instructions");
    let mine = p(&w.home.join(".claude/CLAUDE.md"));
    assert_eq!(
        got.iter().filter(|(path, ..)| *path == mine).count(),
        1,
        "{got:#?}"
    );
    assert!(
        got.contains(&(
            p(&under.join("AGENTS.md")),
            "ancestor".into(),
            "start".into()
        )) || got.contains(&(
            p(&under.join("AGENTS.md")),
            "project".into(),
            "start".into()
        )),
        "{got:#?}"
    );
    assert_eq!(claude["hooks"][0]["matcher"], "Bash");
    assert_eq!(claude["hooks"][0]["run"], "guard");

    let codex = node
        .call(
            "harness_shape",
            json!({"dir": p(&w.dir), "harness": "codex"}),
        )
        .await
        .unwrap();
    let got = paths(&codex, "instructions");
    assert_eq!(
        got,
        [
            (
                p(&w.home.join(".codex/AGENTS.md")),
                "user".into(),
                "start".into()
            ),
            (
                p(&w.project.join("AGENTS.md")),
                "project".into(),
                "start".into()
            ),
        ],
        "Codex reads no folder above the project, and no import"
    );
    let names: Vec<&str> = codex["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"tidy") && names.contains(&"old") && names.contains(&"local"),
        "{names:?}"
    );
    assert!(!names.contains(&"handoff"));
    let hooks: Vec<(&str, bool)> = codex["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (h["run"].as_str().unwrap(), h["on"].as_bool().unwrap()))
        .collect();
    assert_eq!(hooks, [("first", true), ("second", true)]);
    assert!(
        codex["hooks"][0]["note"]
            .as_str()
            .unwrap()
            .contains("not trusted")
    );
    node.stop().await;
    for d in [&w.home, &w.srv] {
        std::fs::remove_dir_all(d).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_land_in_the_file_they_came_from() {
    let w = world();
    let config = format!("[harness]\nhome = {:?}\npath = []\n", w.home);
    let node = Node::start(&[sessions::LISTING, ahjo_harness::LISTING], &config)
        .await
        .unwrap();
    let p = |path: &Path| path.to_string_lossy().into_owned();
    let settings = p(&w.home.join(".claude/settings.json"));

    // A file is written whole, through its link, only over the version read.
    let target = w.home.join("AGENTS.md");
    put(&target, "Linked.\n");
    std::fs::remove_file(w.home.join(".claude/CLAUDE.md")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, w.home.join(".claude/CLAUDE.md")).unwrap();
    #[cfg(not(unix))]
    put(&w.home.join(".claude/CLAUDE.md"), "Linked.\n");
    let link = p(&w.home.join(".claude/CLAUDE.md"));
    let read = node
        .call("harness_read", json!({"path": link}))
        .await
        .unwrap();
    assert_eq!(read["text"], "Linked.\n");
    let stale = node
        .call(
            "harness_write",
            json!({"path": link, "text": "x", "etag": "nope"}),
        )
        .await
        .unwrap_err();
    assert_eq!(stale.code, "harness_stale");
    let client = NodeClient::new(Some(&node.url())).unwrap();
    let error = client
        .request(
            Method::POST,
            "/api/call/harness_write",
            Some(json!({"path": link, "text": "x", "etag": "nope"})),
            None,
        )
        .await
        .unwrap_err();
    let error = error.downcast_ref::<NodeError>().unwrap();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert_eq!(error.code, stale.code);
    node.call(
        "harness_write",
        json!({"path": link, "text": "Changed.\n", "etag": read["etag"]}),
    )
    .await
    .unwrap();
    #[cfg(unix)]
    {
        assert!(
            w.home
                .join(".claude/CLAUDE.md")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "Changed.\n");
    }

    // Claude Code hooks: add, switch off and on (Ahjo keeps it meanwhile),
    // change and remove.
    let shape = || async {
        node.call(
            "harness_shape",
            json!({"dir": p(&w.dir), "harness": "claude"}),
        )
        .await
        .unwrap()
    };
    node.call(
        "harness_hook_add",
        json!({"harness": "claude", "file": settings, "event": "Stop", "command": "say done"}),
    )
    .await
    .unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(saved["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
    assert_eq!(saved["model"], "opus", "the rest of the file stays");
    let at = json!({"harness": "claude", "file": settings, "event": "Stop", "group": 0, "index": 0, "was": "say done"});
    let mut off = at.clone();
    off["on"] = json!(false);
    node.call("harness_hook_switch", off).await.unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert!(saved["hooks"].get("Stop").is_none());
    let s = shape().await;
    let parked = s["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["run"] == "say done")
        .unwrap()
        .clone();
    assert_eq!(
        (parked["on"].clone(), parked["at"]["parked"].clone()),
        (json!(false), json!(true))
    );
    let mut on = at.clone();
    on["on"] = json!(true);
    on["parked"] = json!(true);
    on["index"] = parked["at"]["index"].clone();
    node.call("harness_hook_switch", on).await.unwrap();
    let mut set = at.clone();
    set["new_command"] = json!("say finished");
    set["new_matcher"] = json!("");
    node.call("harness_hook_set", set).await.unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(
        saved["hooks"]["Stop"][0]["hooks"][0]["command"],
        "say finished"
    );
    let mut gone = at.clone();
    gone["was"] = json!("say done");
    assert_eq!(
        node.call("harness_hook_remove", gone)
            .await
            .unwrap_err()
            .code,
        "harness_stale",
        "a hook that changed since it was read stays"
    );
    let mut gone = at.clone();
    gone["was"] = json!("say finished");
    node.call("harness_hook_remove", gone).await.unwrap();

    // A setting, set and taken out.
    node.call(
        "harness_setting_set",
        json!({"harness": "claude", "file": settings, "key": "env.FOO", "value": "bar"}),
    )
    .await
    .unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(saved["env"]["FOO"], "bar");
    node.call(
        "harness_setting_set",
        json!({"harness": "claude", "file": settings, "key": "env.FOO", "unset": true}),
    )
    .await
    .unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert!(saved.get("env").is_none());

    // Codex: a switch is Codex's own, and removing a hook moves the
    // switch of the hook after it with it.
    let hooks_json = p(&w.home.join(".codex/hooks.json"));
    let config_toml = w.home.join(".codex/config.toml");
    node.call(
        "harness_hook_switch",
        json!({"harness": "codex", "file": hooks_json, "event": "SessionStart", "group": 0, "index": 1, "was": "second", "on": false}),
    )
    .await
    .unwrap();
    let text = std::fs::read_to_string(&config_toml).unwrap();
    assert!(text.starts_with("# kept\n"), "{text}");
    assert!(
        text.contains(&format!("\"{hooks_json}:session_start:0:1\"")),
        "{text}"
    );
    node.call(
        "harness_hook_remove",
        json!({"harness": "codex", "file": hooks_json, "event": "SessionStart", "group": 0, "index": 0, "was": "first"}),
    )
    .await
    .unwrap();
    let text = std::fs::read_to_string(&config_toml).unwrap();
    assert!(
        text.contains(&format!("\"{hooks_json}:session_start:0:0\"")),
        "{text}"
    );
    assert!(!text.contains(":session_start:0:1"), "{text}");
    let codex = node
        .call(
            "harness_shape",
            json!({"dir": p(&w.dir), "harness": "codex"}),
        )
        .await
        .unwrap();
    assert_eq!(codex["hooks"][0]["run"], "second");
    assert_eq!(codex["hooks"][0]["on"], false);
    node.call(
        "harness_setting_set",
        json!({"harness": "codex", "file": p(&config_toml), "key": "model_reasoning_effort", "value": "high"}),
    )
    .await
    .unwrap();
    let text = std::fs::read_to_string(&config_toml).unwrap();
    assert!(
        text.contains("model_reasoning_effort = \"high\"") && text.starts_with("# kept\n"),
        "{text}"
    );

    node.stop().await;
    for d in [&w.home, &w.srv] {
        std::fs::remove_dir_all(d).unwrap();
    }
}
