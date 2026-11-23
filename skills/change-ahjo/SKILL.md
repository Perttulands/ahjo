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
| `ahjo install`, `ahjo upgrade` and `ahjo uninstall` | `bin/ahjo/src/install` |
| The binary's own words (`serve`, `help`, `version`), their help and the exit codes | `bin/ahjo/src/main.rs` |
| How every `ahjo <noun> <verb>` reads its arguments and prints its answer | `kit/node-kit/src/cli.rs` |
| What an agent is taught: every command is named in `skills/ahjo` (a test in `bin/ahjo` holds this) | `skills/ahjo` |

An area that needs another goes through that area's `contract` or an interface
it provides. When a change cannot stay inside these lines, stop and say so.

## Build and check

Work in a worktree of your own, not in `/srv/ahjo`, which other agents share:

```sh
git -C /srv/ahjo fetch origin
git -C /srv/ahjo worktree add /srv/ahjo-worktrees/<name> -b <name> origin/main
```

Run the `AGENTS.md` checks there, one build at a time, through `heavy`. `tmux`
on `PATH` here is a wrapper that refuses `kill-server`, so give the tests the
real one, and the app crates the system `pkg-config`:

```sh
export AHJO_REQUIRE_TMUX=1 AHJO_TMUX=/home/linuxbrew/.linuxbrew/bin/tmux PKG_CONFIG=/usr/bin/pkg-config
cargo fmt --check
heavy run --memory 8 -- cargo clippy -p <crate> --all-targets -- -D warnings
heavy run --memory 8 -- cargo nextest run -p <crate>
```

When they pass, `git fetch origin && git rebase origin/main`, then
`git push origin HEAD:main`, and look at the CI run for your commit
(`gh run list -R Perttulands/ahjo`). Afterwards remove what you made:
`git -C /srv/ahjo worktree remove /srv/ahjo-worktrees/<name>` and
`git -C /srv/ahjo branch -D <name>`.

## See it on screen

Run the app against a node of your own on a private display, then look at the
screenshot. Pick a display number and port nobody uses, and keep every file in
a scratch folder `$S`:

1. A tmux server of your own:
   `$AHJO_TMUX -S $S/tmux.sock new-session -d -s <name> -c $S`.
2. `$S/xdg/ahjo/config.toml` with `[node] listen = "127.0.0.1:<port>"`,
   `[terminals] socket = "$S/tmux.sock"` and `[files] roots = ["$S"]`, then
   `HOME=$S XDG_CONFIG_HOME=$S/xdg target/release/ahjo serve &`. With `HOME`
   and the roots in your scratch folder, nothing of the operator's shows in a
   screenshot. `ahjo version --node 127.0.0.1:<port>` answers when it is up.
3. `Xvfb :<n> -screen 0 1600x1000x24 -nolisten tcp &`, then
   `env -u WAYLAND_DISPLAY DISPLAY=:<n> VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json XDG_CONFIG_HOME=$S/xdg AHJO_SWARM_CONFIG=$S/app.json AHJO_SWARM_BOUNDS=0,0,1600,1000 target/release/ahjo-swarm --node http://127.0.0.1:<port> &`.
4. Open the page with `target/release/ahjo view open <page> --node http://127.0.0.1:<port>`
   (every `ahjo` call names your node with `--node`, or it reaches the
   operator's) and take `ffmpeg -f x11grab -i :<n> -frames:v 1 $S/shot.png`.
   Use it as a person does with `xdotool` on your display: `DISPLAY=:<n>
   xdotool mousemove <x> <y> click 1`, `xdotool key Down Return Escape
   ctrl+p`, `xdotool type --delay 20 'text'`. Coordinates are the
   screenshot's pixels; give the app a moment after each action before the
   next screenshot. Walk a journey this way, not by opening pages alone:
   clicks, keys and typing are what show focus, order and dead ends.
5. Stop what you started: the app, Xvfb, the node, and your tmux session.

## Upgrade the operator's node

The node is the systemd user unit `ahjo`, listening on `127.0.0.1:7310`
(Windows Tailscale serves tailnet port 7300 to it, and the ChatGPT tunnel
`ahjo-tunnel` reaches its `/mcp` there). Upgrade only to what is on main,
built in the deploy worktree, which holds nothing but main, together with
the Windows apps of the same commit, which the node hands to other machines
(the laptop installs and updates from it):

```sh
cd /home/perttu/.cache/ahjo-dev/deploy
git fetch origin && git checkout --detach origin/main
C=$(git rev-parse HEAD)
heavy run --memory 8 -- env PKG_CONFIG=/usr/bin/pkg-config cargo build --release -p ahjo -p ahjo-app-swarm
git -C /mnt/c/Users/pertt/ahjo-build fetch origin && git -C /mnt/c/Users/pertt/ahjo-build merge --ff-only $C
(cd /mnt/c/Users/pertt/ahjo-build && heavy run --memory 8 -- /mnt/c/Users/pertt/.cargo/bin/cargo.exe build --release -p ahjo-app-swarm -p ahjo-app-brain -p ahjo-app-desk)
target/release/ahjo upgrade --app target/release/ahjo-swarm --windows-apps /mnt/c/Users/pertt/ahjo-build/target/release
ahjo version
```

The upgrade restarts the node and only the node, so every tmux session lives
on; `ahjo-tunnel` restarts with it. A build that is not healthy in time is
put back and the upgrade fails with `upgrade_failed`. `ahjo version` names
the build both the CLI and the node run. `--windows-apps` refuses builds
of another commit (`apps_mismatch`) before anything changes; the apps go
in `~/.local/share/ahjo/apps/windows/<version>/` and the upgrade checks the
new node hands them out. Run from the installed build (`ahjo upgrade
--windows-apps <dir>`) it puts them beside the running node and restarts
nothing, for when the Windows build came later.

The laptop installs once, in PowerShell, and then updates itself from each
window's Update, which shows when the node runs another build:

```powershell
irm http://landmass.tail1f2f3b.ts.net:7300/install.ps1 | iex
```

`[node] desk` in `~/.config/ahjo/config.toml` is the desk the installed
Desk opens on, as the laptop reaches it
(`http://landmass.tail1f2f3b.ts.net:18782`). The operator's own apps on
this machine are such an install; try a changed script with `AHJO_INSTALL_DIR`, `AHJO_CONFIG_DIR` and `AHJO_START_MENU`
set to a scratch folder, against a node of your own.

Where the install lives:

- Unit `~/.config/systemd/user/ahjo.service`, a link to
  `/srv/ahjo/deploy/ahjo.service`, which is what `ahjo install` wrote; `/srv/landmass-host/boot.toml` records
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

`ahjo uninstall` stops the unit and removes it and the installed binaries;
sessions, `~/.config/ahjo` and the tunnel stay. Never run it, `install` or
`upgrade` to try a change: the install tests (`bin/ahjo/tests/install.rs`) do
that against a stand-in `systemctl` with a unit, root and config of their own,
and are where a change to them is tried.

## Build and install the Windows app

The Windows Swarm app is built on Windows from the checkout `C:\Users\pertt\ahjo-build`,
whose origin is GitHub, so push first:

```sh
cd /mnt/c/Users/pertt/ahjo-build && git pull --ff-only
heavy run --memory 8 -- /mnt/c/Users/pertt/.cargo/bin/cargo.exe build --release -p ahjo-app-swarm
```

The apps are installed as any machine's are, by the node's install script, in
`%LOCALAPPDATA%\Ahjo` with Start-menu entries, each started bare, so the Start
menu and a taskbar pin start the same app the same way. `install.json` beside them names the local node
(`http://127.0.0.1:7310/`), so they update themselves like the laptop's: after
an upgrade with `--windows-apps`, each app shows Update in its top bar when the
node hands out another build, and the operator presses it. Do not copy builds
over them or close his windows to deploy. Their settings are in `%APPDATA%\Ahjo`: `app.json`
(Swarm, naming the node `http://127.0.0.1:7310`) and `desk.json` (Desk);
Brain's are in `%APPDATA%\ahjo-brain\settings.json`. Each app keeps its log
beside its settings (`app.log`, `desk.log`), with every panic's message,
place and backtrace; the `.pdb` files in `C:\Users\pertt\ahjo-build\target\release` name the frames. A node whose protocol changed (`protocol` in
`/api/health`) needs the app built from the same commit.

Windows programs are not on this host's `PATH`; `W=/mnt/c/Windows/System32`.

WSL runs in Windows session 0, where a started window never shows. To start
an app on his desktop (only when he asks), use a one-off scheduled task, then delete it:

```sh
$W/schtasks.exe /Create /F /TN ahjo-swarm-start /SC ONCE /ST 23:59 /IT /TR "C:\Users\pertt\AppData\Local\Ahjo\ahjo-swarm.exe"
$W/schtasks.exe /Run /TN ahjo-swarm-start
$W/schtasks.exe /Delete /F /TN ahjo-swarm-start
```

The app is connected when
`$W/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command "Get-NetTCPConnection -RemotePort 7310 -State Established"`
names its process.
