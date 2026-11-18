# Ahjo

Ahjo is a family of three apps on one app kit: Ahjo Swarm, where you run your
agents; Ahjo Brain, for exploring what we know; and Ahjo Desk, where agents
publish and you answer.

Ahjo Swarm holds sessions, files, work, host and missions in one Rust binary,
`ahjo`, with a native app for you and a CLI for your agent; the CLI reaches the
brain too.

`VISION.md` says what Swarm is for. `ARCHITECTURE.md` says how it is built.

## Run Ahjo Swarm on Linux or WSL

You need [rustup](https://rustup.rs) (the repository pins its toolchain),
`tmux`, and a systemd user session. The app also needs the window system's
libraries; on Debian or Ubuntu:

```sh
sudo apt-get install -y tmux pkg-config libxcb1-dev libx11-xcb-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libfreetype-dev
```

Build the node and the app, put them in place and open the app:

```sh
cargo build --release -p ahjo -p ahjo-app-swarm
target/release/ahjo install --app target/release/ahjo-swarm
~/.local/bin/ahjo version     # the build, and the node that answers
~/.local/bin/ahjo-swarm       # the app
```

`ahjo install` copies both binaries to `~/.local/bin`, writes the systemd user
unit `ahjo`, starts it and waits until the node answers on `127.0.0.1:7300`,
where the app looks for it. The node runs with defaults until you write
`~/.config/ahjo/config.toml`; `ahjo config show` prints what it loaded, and the
app's Settings page shows the same.

To try it without installing anything, run the node in one terminal and the
app in another:

```sh
target/release/ahjo serve
target/release/ahjo-swarm
```

A later build goes in with `target/release/ahjo upgrade --app
target/release/ahjo-swarm`, which puts the previous build back if the new one
does not answer. `ahjo uninstall` takes the unit and the binaries away and
leaves your config. Terminal sessions belong to tmux and live on through all
three.

## The command line

Everything the app does, an agent can do with `ahjo <noun> <verb>`:
`ahjo help` lists the running node's commands, `ahjo <noun>` a noun's verbs
with their arguments, and `--json` answers in JSON. The same commands are MCP
tools, one server per area: `/mcp/<area>` on the node, or `ahjo mcp --area
<area>` over stdio. `skills/ahjo` teaches an agent to use them, and
`skills/change-ahjo` to change Ahjo itself.
