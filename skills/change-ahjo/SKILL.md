---
name: change-ahjo
description: Change Ahjo itself, in its repository at /srv/ahjo, and put the change on the operator's machine. Use when adding or fixing an area, command, page or setting, upgrading the landmass node, or building the Windows app.
---

# Change Ahjo

`AGENTS.md` holds the rules and the checks, `ARCHITECTURE.md` the areas and the
boundaries the build holds, `DESIGN.md` what good looks like in the app. Read
all three before you change anything.

## Where a change lives

| Change | Place |
|---|---|
| An area's wire types: requests, events, errors | `areas/<area>/contract` |
| Node side: state, routes, commands (one definition is the CLI verb and the MCP tool), the area's `config.toml` section | `areas/<area>/node` |
| App side: store, views, the page's settings | `areas/<area>/app` |
| What every area plugs into | `kit/`; tell the other owners when you change its interface or a `contract` they use |
| Terminal engine, session holder, mission model and engine | `engine/` |
| A new area | its directory, plus one line in `bin/ahjo`'s `AREAS`, one in `bin/ahjo-swarm`'s `PAGES` and a dependency line in each `Cargo.toml` |
| `ahjo install` and `ahjo upgrade` | `bin/ahjo/src/install` |

An area that needs another goes through that area's `contract` or an interface
it provides. When a change cannot stay inside these lines, stop and say so.

## Build and check

Run the `AGENTS.md` checks from `/srv/ahjo`, one build at a time, through
`heavy`. `tmux` on `PATH` here is a wrapper that refuses `kill-server`, so give
the tests the real one:

```sh
cargo fmt --check
heavy run --memory 8 -- cargo clippy -p <crate> --all-targets -- -D warnings
heavy run --memory 8 -- env AHJO_REQUIRE_TMUX=1 AHJO_TMUX=/home/linuxbrew/.linuxbrew/bin/tmux cargo test -p <crate>
```

## See it on screen

Run the app against a node of your own on a private display, then look at the
screenshot. Pick a display number and port nobody uses, and keep every file in
a scratch folder `$S`:

1. A tmux server of your own: `tmux -S $S/tmux.sock new-session -d -s <name>`.
2. `$S/xdg/ahjo/config.toml` with `[node] listen = "127.0.0.1:<port>"` and
   `[terminals] socket = "$S/tmux.sock"`, then
   `XDG_CONFIG_HOME=$S/xdg target/release/ahjo serve &`.
3. `Xvfb :<n> -screen 0 1600x1000x24 -nolisten tcp &`, then
   `env -u WAYLAND_DISPLAY DISPLAY=:<n> VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json XDG_CONFIG_HOME=$S/xdg AHJO_SWARM_CONFIG=$S/app.json AHJO_SWARM_BOUNDS=0,0,1600,1000 target/release/ahjo-swarm --node http://127.0.0.1:<port> &`.
4. Open the page with `ahjo view open <page> --node http://127.0.0.1:<port>`
   (this host has no XTEST, so no synthetic clicks) and take
   `ffmpeg -f x11grab -i :<n> -frames:v 1 $S/shot.png`.
5. Stop what you started: the app, Xvfb, the node, and your tmux session.

## Upgrade the operator's node

The node is the systemd user unit `ahjo`, listening on `127.0.0.1:7310`
(Windows Tailscale serves tailnet port 7300 to it, and the ChatGPT tunnel
`ahjo-tunnel` reaches its `/mcp` there). Upgrade only to what is on main:

```sh
cd /srv/ahjo && git pull --rebase
heavy run --memory 8 -- cargo build --release -p ahjo -p ahjo-app-swarm
target/release/ahjo upgrade --app target/release/ahjo-swarm
ahjo version
```

The upgrade restarts the node and only the node, so every tmux session lives
on; `ahjo-tunnel` restarts with it. A build that is not healthy in time is
put back and the upgrade fails with `upgrade_failed`. `ahjo version` names
the build both the CLI and the node run.

Where the install lives:

- Unit `~/.config/systemd/user/ahjo.service`, a link to `deploy/ahjo.service`,
  which is what `ahjo install` wrote; `/srv/landmass-host/boot.toml` records
  it with `deploy/ahjo-tunnel.service` and `deploy/ahjo-tunnel.yaml`.
- Binaries in `~/.local/share/ahjo/bin`; `~/.local/bin/ahjo` links there.
- State in `~/.config/ahjo`: `config.toml`, `launch.conf`, the node id in
  `node`, missions, scheduled tasks, agent hooks and work reads, and beside
  them the tunnel's `tunnel/ahjo.yaml` (a copy of the deploy file) and
  `tunnel.env` (its secrets, never in the repository). A `config.toml` edit
  takes effect at `systemctl --user restart ahjo`.

`ahjo install` is for a unit that must change. It refuses while the unit runs
another build, so upgrade first. Give it the unit's own `PATH`, and no
`XDG_CONFIG_HOME`, which would move the unit out of `~/.config/systemd/user`:

```sh
env -u XDG_CONFIG_HOME PATH="$(sed -n 's/^Environment=PATH=//p' deploy/ahjo.service)" \
  target/release/ahjo install --root ~/.local/share/ahjo --app target/release/ahjo-swarm
```

A changed unit is written over the link: copy it to `deploy/ahjo.service`,
link it back, run `systemctl --user daemon-reload` and commit the file.

## Build and install the Windows app

The Windows Swarm app is built on Windows from the checkout `C:\Users\pertt\ahjo-build`,
whose origin is GitHub, so push first:

```sh
cd /mnt/c/Users/pertt/ahjo-build && git pull --ff-only
heavy run --memory 8 -- /mnt/c/Users/pertt/.cargo/bin/cargo.exe build --release -p ahjo-app-swarm
```

It runs from `C:\Users\pertt\Ahjo` through `Ahjo.cmd` (the Start-menu entry
Ahjo Swarm), which points `AHJO_SWARM_CONFIG` at `app.json` beside it and the app at
`http://127.0.0.1:7310`. Replacing it closes the operator's window, so do it
when he asked for the new app. A node whose protocol changed (`protocol` in
`/api/health`) needs the app built from the same commit.

Windows programs are not on this host's `PATH`; `W=/mnt/c/Windows/System32`.

```sh
$W/taskkill.exe /F /IM ahjo-swarm.exe
cp /mnt/c/Users/pertt/ahjo-build/target/release/ahjo-swarm.exe /mnt/c/Users/pertt/Ahjo/
```

WSL runs in Windows session 0, where a started window never shows. Start the
app on his desktop through a one-off scheduled task, then delete it:

```sh
$W/schtasks.exe /Create /F /TN ahjo-swarm-start /SC ONCE /ST 23:59 /IT /TR "C:\Users\pertt\Ahjo\Ahjo.cmd"
$W/schtasks.exe /Run /TN ahjo-swarm-start
$W/schtasks.exe /Delete /F /TN ahjo-swarm-start
```

The app is connected when
`$W/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command "Get-NetTCPConnection -RemotePort 7310 -State Established"`
names its process.
