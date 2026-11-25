# Lead, try on Windows, deploy

The lead's part of [change-ahjo](SKILL.md): briefing lanes, trying a commit
on Windows, and putting main on the operator's machine.

## Lead the work

Work that runs in parallel goes to lanes: one agent owns one outcome end to
end. A lane's brief stands on its own and names:

- the outcome and why, in the operator's words where he gave them;
- what is known (files, commits, beads) and what is ruled out;
- the kit elements to use, so the lane extends the kit rather than its page;
- the stop conditions (`AGENTS.md`, Stop before building), and that a lane
  meeting one brings the decision back to the lead;
- that the lane looks at its screens on a private display, lands on main
  when green, chases any red run to its cause, and does not deploy;
- what to report: commits, screenshots, what it did not check.

Pick the model by the work: the strongest for design, hard bugs and review;
a cheaper one for mechanical moves across many files. The lead reads each
lane's screenshots and tries the batch on Windows before deploying, and
deploys in batches, once or twice a day or when the operator asks, with one
short note of what to try.

## Try it on Windows

Before every upgrade the lead runs the commit's Windows apps on this machine
and looks at every screenshot: Xvfb on Linux is not what the operator sees.

```sh
skills/change-ahjo/try-windows <commit> --page 'sessions --session agent' \
  --page 'files --path README.md' --page missions
```

It builds the commit's node (in `~/.cache/ahjo-dev/try`) and Windows apps
(in `C:\Users\pertt\ahjo-try`), never the deploy builds, starts a scratch
node on a free port from 7350 with its own tmux server (sessions `shell`,
`agent`, `build`, `protos`), files and brain, and a fake desk, then Swarm, Desk and
Brain on his desktop with settings, logs and caches in a scratch
`%APPDATA%` and `%LOCALAPPDATA%` (his theme and size are copied, nothing of
his is written). Each `--page` is what `ahjo view open` takes, a `--path`
relative to the scratch files; `--app` limits the apps. The screenshots,
`swarm-<page>.png`, `desk.png` and `brain.png`, go to the folder it prints,
with the build and node logs. It stops only what it started, by process
id, on any exit. A try takes about a minute when the try builds are warm;
a commit far from the last one tried adds its compile time.

A journey is walked with `--step`s, run in Swarm in order after the
pages: `open` (as `--page`), `click <x> <y>` at the pixels of the last
screenshot, `type <text>`, `key enter|escape|tab|...` and `shot <name>`
(`swarm-<name>.png`). Clicks and keys are posted to the scratch window
alone, never through his keyboard or mouse. They carry no Alt or Ctrl, so
a chord runs from the Keys panel: click Keys at the tab bar's right, then
its row. Take a first try's screenshot to find the coordinates:

```sh
skills/change-ahjo/try-windows <commit> --app swarm --page 'sessions --session agent' \
  --step 'click 1419 55' --step 'shot keys' --step 'click 424 521' \
  --step 'type This panel will not close' --step 'shot report' --step 'key escape'
```

A step that starts with `desk` or `brain` runs in that app instead
(`shot <name>` then saves `desk-<name>.png`). A click lands in the deepest
window under its pixel, so a click on a web view's page reaches the page,
and `eval <script>` reads a value back from the app's web page through its
DevTools port, printed and kept in `eval.txt`. The fake desk publishes an
HTML page (`try-page`, shown first); Comment, then a mark on its heading
(the pixels are for the default size):

```sh
skills/change-ahjo/try-windows <commit> --app desk --step 'desk eval document.title' \
  --step 'desk click 850 51' --step 'desk click 900 120' --step 'desk shot marked' \
  --step "desk eval document.querySelectorAll('[data-desk-mark]').length"
```

The scratch files hold two HTML prototypes for Files to run (ADR 0006):
`protos/counter/index.html` (its own css and js, an image in `my assets`, a
`<dialog>`) and `protos/cdn/index.html` (lodash from a CDN). Each counts its
button's clicks in `window.count`; the session `protos` prints both paths
to click. With several web views open, `eval` reads the one shown:

```sh
skills/change-ahjo/try-windows <commit> --app swarm --page 'files --path protos/counter/index.html' \
  --step 'click 700 160' --step 'eval window.count' --step 'shot counted'
```

`media/` in the scratch files holds silent fixtures for the media viewers
(made once into `~/.cache/ahjo-dev/try-media`): `big.mp4` (H.264, over
64 MiB), `clip.webm`, `voice.mp3`, `paper.pdf` (three pages), `odd.avi`
(MPEG-4 Part 2, over 64 MiB, which WebView2 does not play), `letter.docx`
and `bundle.zip`; the session `media` prints their paths to click. The
player's element is `#media`. `wait <seconds>` waits for what the app does
on its own. After Open in its Windows app, `launched <name>` finds the
program that took the newest copy (by its command line, by what holds the
copy open, an Explorer window on it, or else a packaged app or Windows'
Open with started since the copy was made) and says whether the copy is
as large as the file. It closes nothing: he works on this desktop, so the
handler (name, pid, start time) is only reported and stays open. A
packaged app such as Media Player gets no path, and while his session is
locked shows no window. The apps' copies go to the run's own
`TEMP`:

```sh
skills/change-ahjo/try-windows <commit> --app swarm --page 'files --path media/odd.avi' \
  --step 'wait 2' --step 'click 652 224' --step 'launched odd.avi'
```

His desktop sees little: each window shows on his main screen for about
30 ms before it is moved right of every screen, without taking focus, and
has a taskbar button while the try runs. A second Swarm beside his is
fine: the apps hold no lock or single-instance check.

## Upgrade the operator's node

The node is the systemd user unit `ahjo`, listening on `127.0.0.1:7310`
(Windows Tailscale serves tailnet port 7300 to it, and the ChatGPT tunnel
`ahjo-tunnel` reaches its `/mcp/chatgpt` there; ChatGPT keeps the tool list
it last loaded, so after changing the `chatgpt` set press Refresh tools under
Plugins > Ahjo Swarm in ChatGPT's settings). Upgrade only to what is on main,
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

## The Windows apps

They are built with the node, as the upgrade above does, in the checkout
`C:\Users\pertt\ahjo-build`, whose origin is GitHub. The apps are installed as any machine's are, by the node's install script, in
`%LOCALAPPDATA%\Ahjo` with Start-menu entries, each started bare, so the Start
menu and a taskbar pin start the same app the same way. `install.json` beside them names the local node
(`http://127.0.0.1:7310/`), so they update themselves like the laptop's: after
an upgrade with `--windows-apps`, each app shows Update in its top bar when the
node hands out another build, and the operator presses it. Do not copy builds
over them or close his windows to deploy. Their settings are in `%APPDATA%\Ahjo`: `app.json`
(Swarm, naming the node `http://127.0.0.1:7310`) and `desk.json` (Desk) and `brain.json` (Brain, which kept them in
`%APPDATA%\ahjo-brain\settings.json` until ahjo-ctz: its first start, or
the install script, carries that file over and removes it once the new one is
written; `AHJO_BRAIN_CONFIG` names the file). Each app keeps its log
beside its settings (`app.log`, `desk.log`, `brain.log`), with every panic's message,
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
