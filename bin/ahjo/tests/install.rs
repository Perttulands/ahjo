//! `ahjo install`, `ahjo upgrade` and `ahjo uninstall`, the whole binary, against a stand-in
//! for `systemctl --user` on PATH: it runs a unit's ExecStart as a process of
//! its own and, as `KillMode=process` does, stops only that process. The
//! node serves a tmux server of the test's own. Nothing here reaches the real
//! systemd, units, install folders or tmux. Linux only; skips without tmux
//! unless `AHJO_REQUIRE_TMUX=1`.
#![cfg(target_os = "linux")]

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ahjo::install::Unit;
use ahjo_testkit::fresh_dir;
use ahjo_testkit::tmux::{Server, eventually, write_script};
use serde_json::Value;

const AHJO: &str = env!("CARGO_BIN_EXE_ahjo");
const UNIT: &str = "ahjo-install-test";

/// `systemctl --user` for one scratch folder: calls go to `calls`, a
/// started unit's process id to `<unit>.pid`, its output to `<unit>.log`.
const FAKE_SYSTEMCTL: &str = r#"#!/bin/sh
echo "$*" >> "$FAKE_SYSTEMD/calls"
[ "$1" = --user ] || { echo "fake systemctl: only --user" >&2; exit 2; }
shift
command=$1
shift
while [ "${1#--}" != "$1" ]; do shift; done
service=$1
pidfile="$FAKE_SYSTEMD/$service.pid"
running() { [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; }
start() {
  line=$(sed -n 's/^ExecStart=//p' "$FAKE_SYSTEMD_UNITS/$service")
  eval "set -- $line"
  setsid "$@" >> "$FAKE_SYSTEMD/$service.log" 2>&1 < /dev/null &
  echo $! > "$pidfile"
}
stop() {
  if running; then
    pid=$(cat "$pidfile")
    kill "$pid"
    while kill -0 "$pid" 2>/dev/null; do sleep 0.05; done
  fi
  rm -f "$pidfile"
}
case $command in
  daemon-reload) ;;
  enable) touch "$FAKE_SYSTEMD/$service.enabled" ;;
  disable) rm -f "$FAKE_SYSTEMD/$service.enabled" ;;
  start) running || start ;;
  restart) stop; start ;;
  stop) stop ;;
  is-active) if running; then echo active; else echo inactive; exit 3; fi ;;
  *) echo "fake systemctl: no $command" >&2; exit 1 ;;
esac
"#;

/// One machine's worth of scratch: the fake systemd, the unit folder, the
/// install root, HOME and the config. Its node is stopped when it drops.
struct Machine {
    dir: PathBuf,
    listen: SocketAddr,
}

/// An address for a machine's node that nothing else listens on. The port
/// is free when chosen but nothing holds it until the node starts, and on
/// 127.0.0.1 another test's node could take it meanwhile; then this node
/// fails to listen while that one, at the same version, passes install's
/// health check. So each test process listens on a loopback address of its
/// own, from its process id (Linux routes all of 127.0.0.0/8 to loopback),
/// and its machines never share a port.
fn own_address() -> SocketAddr {
    static GIVEN: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    let [_, high, mid, low] = std::process::id().to_be_bytes();
    // pid_max is at most 2^22, so the second octet stays within 1..=64 and
    // never names 127.0.0.1.
    let ip = Ipv4Addr::new(127, 1 + high, mid, low);
    let mut given = GIVEN.lock().unwrap();
    loop {
        let address = TcpListener::bind((ip, 0)).unwrap().local_addr().unwrap();
        if !given.contains(&address.port()) {
            given.push(address.port());
            return address;
        }
    }
}

impl Machine {
    fn new(tmux: &Server) -> Machine {
        let dir = fresh_dir("ahjo-install");
        for sub in ["fakebin", "systemd", "units", "home", "xdg", "builds"] {
            std::fs::create_dir(dir.join(sub)).unwrap();
        }
        write_script(&dir.join("fakebin/systemctl"), FAKE_SYSTEMCTL);
        std::fs::write(
            dir.join("config.toml"),
            format!(
                "[node]\ndevice = \"install-test\"\n[terminals]\nsocket = {:?}\n",
                tmux.socket_path()
            ),
        )
        .unwrap();
        Machine {
            dir,
            listen: own_address(),
        }
    }

    /// A build of ahjo at `builds/<name>/ahjo`: a copy of this one.
    fn build(&self, name: &str) -> PathBuf {
        let dir = self.dir.join("builds").join(name);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("ahjo");
        // Only the child holds the executable open for writing. Another
        // test's fork cannot inherit that handle and keep the build busy.
        let copied = Command::new("cp").arg(AHJO).arg(&path).status().unwrap();
        assert!(copied.success(), "copying {}", path.display());
        path
    }

    fn listen(&self) -> String {
        self.listen.to_string()
    }

    fn installed(&self) -> PathBuf {
        self.dir.join("root/bin/ahjo")
    }

    fn unit_file(&self) -> PathBuf {
        self.dir.join(format!("units/{UNIT}.service"))
    }

    /// Runs `ahjo <verb> --json` from `build` with the scratch flags.
    fn ahjo(&self, build: &Path, verb: &str, extra: &[&str]) -> Output {
        let mut command = Command::new(build);
        command.arg(verb).arg("--json");
        command.args(["--unit", UNIT, "--unit-dir"]);
        command.arg(self.dir.join("units"));
        if verb != "uninstall" {
            command.args(["--timeout", "10"]);
        }
        command.args(extra);
        let path = format!(
            "{}:{}",
            self.dir.join("fakebin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        command
            .env("PATH", path)
            .env("HOME", self.dir.join("home"))
            .env("XDG_CONFIG_HOME", self.dir.join("xdg"))
            .env("FAKE_SYSTEMD", self.dir.join("systemd"))
            .env("FAKE_SYSTEMD_UNITS", self.dir.join("units"));
        command.output().unwrap()
    }

    fn install(&self, build: &Path) -> Output {
        let root = self.dir.join("root");
        let config = self.dir.join("config.toml");
        self.ahjo(
            build,
            "install",
            &[
                "--root",
                root.to_str().unwrap(),
                "--listen",
                &self.listen(),
                "--config",
                config.to_str().unwrap(),
            ],
        )
    }

    /// The node process the fake systemd runs now.
    fn node_pid(&self) -> Option<u32> {
        let text = std::fs::read_to_string(self.dir.join(format!("systemd/{UNIT}.service.pid")));
        text.ok()?.trim().parse().ok()
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("systemd/calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// `ahjo <args> --node <this node>` with the installed CLI.
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(self.installed())
            .args(args)
            .args(["--node", &format!("http://{}", self.listen())])
            .output()
            .unwrap()
    }

    fn screen(&self, session: &str) -> String {
        let out = self.cli(&["session", "read", session]);
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Some(pid) = self.node_pid() {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn succeeded(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

async fn health(listen: &str) -> Value {
    let client = ahjo_node_kit::NodeClient::new(Some(&format!("http://{listen}/"))).unwrap();
    let (health, _) = client
        .request(axum::http::Method::GET, "/api/health", None, None)
        .await
        .unwrap();
    health
}

/// Types `text` into `session` through the node, once the node lists it.
async fn send(machine: &Machine, session: &str, text: &str) {
    eventually(&format!("{text} sent to {session}"), || {
        machine
            .cli(&["session", "send", session, text])
            .status
            .success()
            .then_some(())
    })
    .await;
}

/// Waits until `session`'s screen through the node shows `line`.
async fn shows(machine: &Machine, session: &str, line: &str) {
    eventually(&format!("{session} shows {line}"), || {
        machine
            .screen(session)
            .lines()
            .any(|shown| shown.trim() == line)
            .then_some(())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_upgrade_keeps_every_session_and_the_new_node_serves_it() {
    let Some(tmux) = Server::start_in("ahjo-upgrade") else {
        return;
    };
    tmux.new_session("alpha", 100, 30);
    let machine = Machine::new(&tmux);
    let first = machine.build("first");
    succeeded(&machine.install(&first));
    let pane = tmux.display("alpha", "#{pane_pid}");
    send(&machine, "alpha", "echo before-$((6*7))").await;
    shows(&machine, "alpha", "before-42").await;
    let old_node = machine.node_pid().unwrap();

    let second = machine.build("second");
    let upgraded = succeeded(&machine.ahjo(&second, "upgrade", &[]));
    assert_eq!(upgraded["version"], ahjo_node_kit::VERSION);
    assert_eq!(upgraded["previous"], ahjo_node_kit::VERSION);

    let new_node = machine.node_pid().unwrap();
    assert_ne!(old_node, new_node, "the node did not restart");
    assert!(std::fs::read(machine.installed()).unwrap() == std::fs::read(&second).unwrap());
    let old_build = machine.installed().with_file_name("ahjo.previous");
    assert!(!old_build.exists(), "the replaced build was left behind");
    // The same session, its same process and its screen, through the new node.
    assert_eq!(tmux.display("alpha", "#{pane_pid}"), pane);
    shows(&machine, "alpha", "before-42").await;
    send(&machine, "alpha", "echo after-$((6*7))").await;
    shows(&machine, "alpha", "after-42").await;
    assert_eq!(
        health(&machine.listen()).await["version"],
        ahjo_node_kit::VERSION
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_build_that_fails_its_health_check_is_rolled_back() {
    let Some(tmux) = Server::start_in("ahjo-rollback") else {
        return;
    };
    tmux.new_session("alpha", 100, 30);
    let machine = Machine::new(&tmux);
    let first = machine.build("first");
    succeeded(&machine.install(&first));
    send(&machine, "alpha", "echo kept-$((6*7))").await;
    shows(&machine, "alpha", "kept-42").await;

    // A build that says its version and runs upgrade, but cannot serve.
    let broken = machine.dir.join("builds/broken");
    std::fs::create_dir(&broken).unwrap();
    let broken = broken.join("ahjo");
    write_script(
        &broken,
        format!(
            "#!/bin/sh\ncase \"$1\" in serve) echo 'broken build' >&2; exit 3 ;; esac\nexec {AHJO} \"$@\"\n"
        ),
    );
    let out = machine.ahjo(
        &first,
        "upgrade",
        &["--from", broken.to_str().unwrap(), "--timeout", "3"],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["code"], "upgrade_failed", "{error}");
    let message = error["error"]["message"].as_str().unwrap();
    assert!(message.contains("no healthy node"), "{message}");
    assert!(message.contains("inactive"), "{message}");

    // The previous build is back, serving, with the session.
    assert!(std::fs::read(machine.installed()).unwrap() == std::fs::read(&first).unwrap());
    let health = health(&machine.listen()).await;
    assert_eq!(health["status"], "ok");
    shows(&machine, "alpha", "kept-42").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn install_is_idempotent_and_writes_the_expected_unit() {
    let Some(tmux) = Server::start_in("ahjo-install") else {
        return;
    };
    let machine = Machine::new(&tmux);
    let first = machine.build("first");
    let installed = succeeded(&machine.install(&first));
    assert_eq!(installed["changed"], true);
    assert_eq!(installed["listen"], machine.listen());
    let unit = std::fs::read_to_string(machine.unit_file()).unwrap();
    let mut environment = vec![(
        "PATH".to_owned(),
        format!(
            "{}:{}",
            machine.dir.join("fakebin").display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    )];
    if let Some(tmux) = std::env::var_os("AHJO_TMUX").filter(|v| !v.is_empty()) {
        environment.push(("AHJO_TMUX".into(), tmux.to_string_lossy().into()));
    }
    let expected = Unit {
        binary: machine.installed(),
        listen: machine.listen(),
        config: Some(machine.dir.join("config.toml")),
        environment,
    };
    assert_eq!(unit, expected.render());
    let service = format!("{UNIT}.service");
    assert_eq!(
        machine.calls(),
        [
            format!("--user is-active {service}"),
            "--user daemon-reload".into(),
            format!("--user enable --quiet {service}"),
            format!("--user start {service}"),
        ]
    );
    let pid = machine.node_pid().unwrap();

    let again = succeeded(&machine.install(&first));
    assert_eq!(again["changed"], false, "{again}");
    assert!(
        again["summary"]
            .as_str()
            .unwrap()
            .starts_with("Already installed")
    );
    assert_eq!(std::fs::read_to_string(machine.unit_file()).unwrap(), unit);
    assert_eq!(
        machine.node_pid(),
        Some(pid),
        "a second install restarted it"
    );
    assert_eq!(
        machine.calls()[4..],
        [
            format!("--user is-active {service}"),
            format!("--user enable --quiet {service}"),
        ]
    );

    // A different build is an upgrade's job, not install's.
    let other = machine.build("other");
    let changed = Command::new("sh")
        .args(["-c", r#"printf '\0' >> "$1""#, "sh"])
        .arg(&other)
        .status()
        .unwrap();
    assert!(changed.success(), "changing {}", other.display());
    let refused = machine.install(&other);
    assert_eq!(refused.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&refused.stderr).unwrap();
    assert_eq!(error["error"]["code"], "install_running", "{error}");
    assert_eq!(machine.node_pid(), Some(pid));
}

#[tokio::test(flavor = "multi_thread")]
async fn uninstall_takes_the_unit_and_the_build_away_and_keeps_sessions_and_config() {
    let Some(tmux) = Server::start_in("ahjo-uninstall") else {
        return;
    };
    tmux.new_session("alpha", 100, 30);
    let machine = Machine::new(&tmux);
    let first = machine.build("first");
    succeeded(&machine.install(&first));
    let node = machine.node_pid().unwrap();
    let pane = tmux.display("alpha", "#{pane_pid}");
    let service = format!("{UNIT}.service");
    let enabled = machine.dir.join(format!("systemd/{service}.enabled"));
    assert!(enabled.exists());

    let gone = succeeded(&machine.ahjo(&first, "uninstall", &[]));
    assert_eq!(gone["unit"], service.as_str());
    assert_eq!(
        gone["removed"],
        serde_json::json!([machine.installed()]),
        "{gone}"
    );
    assert!(!machine.unit_file().exists());
    assert!(!machine.installed().exists());
    assert!(!enabled.exists(), "the unit is still enabled");
    assert_eq!(machine.node_pid(), None, "the node still runs");
    assert!(
        !Path::new(&format!("/proc/{node}")).exists(),
        "the node process outlived its unit"
    );
    // What is not the install's stays: the session, and the config.
    assert_eq!(tmux.display("alpha", "#{pane_pid}"), pane);
    assert!(machine.dir.join("config.toml").exists());

    // Nothing is left to take away, and a wrong flag is a wrong command line.
    let again = machine.ahjo(&first, "uninstall", &[]);
    assert_eq!(again.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&again.stderr).unwrap();
    assert_eq!(error["error"]["code"], "not_installed", "{error}");
    let wrong = machine.ahjo(&first, "uninstall", &["--root", "/nowhere"]);
    assert_eq!(wrong.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&wrong.stderr).unwrap();
    assert_eq!(error["error"]["code"], "bad_arguments", "{error}");
}

/// One GET to the node on `listen`: its status and body.
fn get(listen: &str, path: &str) -> (u16, Vec<u8>) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(listen).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {listen}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let end = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&all[..end]).into_owned();
    assert!(
        !head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked"),
        "{head}"
    );
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, all[end + 4..].to_vec())
}

/// Three Windows apps as cargo leaves them, each carrying `version`.
fn windows_build(dir: &Path, version: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    for name in ["ahjo-swarm.exe", "ahjo-brain.exe", "ahjo-desk.exe"] {
        let bytes = format!("MZ {name} ... {version} ...");
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    dir.to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_upgrade_puts_the_windows_apps_beside_the_node_which_hands_them_out() {
    let Some(tmux) = Server::start_in("ahjo-apps") else {
        return;
    };
    let machine = Machine::new(&tmux);
    let first = machine.build("first");
    succeeded(&machine.install(&first));
    let listen = machine.listen();
    // The node keeps none yet, and says how they come.
    let (status, body) = get(&listen, "/apps/windows");
    assert_eq!(status, 404);
    let error: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["error"]["code"], "no_apps", "{error}");
    let apps = machine.dir.join("root/apps/windows");
    let stale = apps.join("0.0.0+0000000");
    std::fs::create_dir_all(&stale).unwrap();

    // Builds of another commit are refused before anything changes.
    let wrong = windows_build(&machine.dir.join("win-wrong"), "0.0.0+0000000");
    let pid = machine.node_pid();
    let second = machine.build("second");
    let out = machine.ahjo(
        &second,
        "upgrade",
        &["--windows-apps", wrong.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["code"], "apps_mismatch", "{error}");
    assert_eq!(
        machine.node_pid(),
        pid,
        "a refused upgrade restarted the node"
    );
    assert!(std::fs::read(machine.installed()).unwrap() == std::fs::read(&first).unwrap());

    let version = ahjo_node_kit::VERSION;
    let windows = windows_build(&machine.dir.join("win"), version);
    let upgraded = succeeded(&machine.ahjo(
        &second,
        "upgrade",
        &["--windows-apps", windows.to_str().unwrap()],
    ));
    assert_eq!(
        upgraded["windows_apps"].as_str().map(PathBuf::from),
        Some(apps.join(version)),
        "{upgraded}"
    );
    assert!(!stale.exists(), "another version's apps were left");

    let (status, body) = get(&listen, "/apps/windows");
    assert_eq!(status, 200);
    let manifest: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(manifest["version"], version);
    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files.len(), 3, "{manifest}");
    for file in files {
        let name = file["name"].as_str().unwrap();
        let (status, bytes) = get(&listen, &format!("/apps/windows/{name}"));
        assert_eq!(status, 200, "{name}");
        assert_eq!(bytes, std::fs::read(windows.join(name)).unwrap());
        assert_eq!(file["size"], bytes.len());
        use sha2::Digest;
        let sum: String = sha2::Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(file["sha256"], sum);
    }
    // Only what the manifest names.
    assert_eq!(get(&listen, "/apps/windows/manifest.json").0, 404);
    assert_eq!(get(&listen, "/apps/windows/..%2Fmanifest.json").0, 404);

    // The script installs from this node, as the request named it.
    let (status, script) = get(&listen, "/install.ps1");
    assert_eq!(status, 200);
    let script = String::from_utf8(script).unwrap();
    assert!(
        script.contains(&format!("$node = 'http://{listen}'")),
        "{script}"
    );
    assert!(script.is_ascii(), "Windows PowerShell reads it as ANSI");

    // From the installed build, the apps alone: nothing restarts.
    let pid = machine.node_pid();
    let again = succeeded(&machine.ahjo(
        &machine.installed(),
        "upgrade",
        &["--windows-apps", windows.to_str().unwrap()],
    ));
    assert_eq!(again["version"], version, "{again}");
    assert_eq!(
        machine.node_pid(),
        pid,
        "placing the apps restarted the node"
    );
    assert_eq!(get(&listen, "/apps/windows").0, 200);

    // Uninstall takes them away with the build.
    succeeded(&machine.ahjo(&machine.installed(), "uninstall", &[]));
    assert!(!machine.dir.join("root/apps").exists());
}
