---
name: change-ahjo
description: Change Ahjo itself, in its repository at /srv/ahjo, and put the change on the operator's machine. Use when adding or fixing an area, command, page or setting, upgrading the landmass node, or building the Windows app.
---

# Change Ahjo

`AGENTS.md`'s stop conditions (Stop before building) hold for every change.
Read what your kind of change needs, then build and check it as below.
`CODING_STANDARDS.md` is what the review will hold the change to; read the
rules your change touches before you build, not after the review:

| Change | Read |
|---|---|
| A page, or anything else on screen | `DESIGN.md` Principles and the surface it is on; `ARCHITECTURE.md` Kit and its page under App pages; then See it on screen |
| A page's behaviour: its store, pending choices, replies | `CODING_STANDARDS.md` State and Regression tests; its page under `ARCHITECTURE.md` App pages |
| A command, contract or node area | `GLOSSARY.md`; `ARCHITECTURE.md` Words, Rules the compiler holds, Errors and its area under Node areas |
| `kit/` | `ARCHITECTURE.md` Kit and Rules the compiler holds; tell the other owners when its interface changes |
| Install, upgrade or the Windows apps | `ARCHITECTURE.md` Install and upgrade, then [lead-and-deploy.md](lead-and-deploy.md) |
| Leading lanes, trying a batch on Windows, deploying | [lead-and-deploy.md](lead-and-deploy.md) |

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

## Build and check

Work in a worktree of your own, not in `/srv/ahjo`, which other agents share:

```sh
git -C /srv/ahjo fetch origin
git -C /srv/ahjo worktree add /srv/ahjo-worktrees/<name> -b <name> origin/main
```

Run the `AGENTS.md` checks there, one build at a time, through `heavy`. `tmux`
on `PATH` here is a wrapper that refuses `kill-server`, so give the tests the
real one, and the app crates the system `pkg-config`. The last line runs the
source guards in `bin/ahjo/tests` (`layout.rs`: dependencies, kit
interactions, literal sizes; `areas.rs`: command names and the `ahjo` skill),
which a page or area crate's own tests never run:

```sh
export AHJO_REQUIRE_TMUX=1 AHJO_TMUX=/home/linuxbrew/.linuxbrew/bin/tmux PKG_CONFIG=/usr/bin/pkg-config
cargo fmt --check
heavy run --memory 8 -- cargo clippy -p <crate> --all-targets -- -D warnings
heavy run --memory 8 -- cargo nextest run -p <crate>
heavy run --memory 8 -- cargo nextest run -p ahjo --test layout --test areas
```

Make style edits before the checks: a recheck waits for `heavy` again.

When they pass, push and bring the shared checkout up to it, which only
moves `/srv/ahjo` when it is on `main` with no tracked file changed.
Untracked files do not count: a fast-forward refuses to overwrite one, and
the operator keeps some there, so counting them would block every
fast-forward:

```sh
git fetch origin && git rebase origin/main && git push origin HEAD:main
test "$(git -C /srv/ahjo branch --show-current)" = main \
  && test -z "$(git -C /srv/ahjo status --porcelain --untracked-files=no)" \
  && git -C /srv/ahjo merge --ff-only origin/main
```

Then look at the CI run for your commit (`gh run list -R Perttulands/ahjo`).
Afterwards remove what you made:
`git -C /srv/ahjo worktree remove /srv/ahjo-worktrees/<name>` and
`git -C /srv/ahjo branch -D <name>`.

## See it on screen

Run the app against a node of your own on a private display, then look at the
screenshot. Keep every file in a scratch folder `$S`, and run the node,
display and app in a tmux server of their own, so they outlive the command
that started them, whatever runs your shell:

1. Build both binaries and name where they are:
   `heavy run --memory 8 -- cargo build -p ahjo -p ahjo-app-swarm`, then
   `B=${CARGO_TARGET_DIR:-$PWD/target}/debug`.
2. A tmux server for the app's terminals:
   `$AHJO_TMUX -S $S/tmux.sock new-session -d -s <name> -c $S`.
3. Pick a port and display number nobody uses. Write
   `$S/xdg/ahjo/config.toml` with `[node] listen = "127.0.0.1:<port>"`,
   `[terminals] socket = "$S/tmux.sock"` and `[files] roots = ["$S"]`, then
   start the node in a second tmux server that holds what you run:
   `R="$AHJO_TMUX -S $S/run.sock"`, then
   `$R new-session -d -s node "env HOME=$S XDG_CONFIG_HOME=$S/xdg $B/ahjo serve 2>$S/node.log"`.
   With `HOME` and the roots in your scratch folder, nothing of the
   operator's shows in a screenshot. `$B/ahjo version --node
   127.0.0.1:<port>` answers when it is up.
4. Start the display and keep its process id:
   `X=$($R new-window -P -F '#{pane_pid}' -t node "exec Xvfb :<n> -screen 0 1600x1000x24 -nolisten tcp")`.
   It is up, and yours, once `/tmp/.X<n>-lock` holds `$X`
   (`test "$(tr -d ' ' </tmp/.X<n>-lock)" = "$X"`); another id there means
   another lane took the display, so pick a new number. Then
   `$R new-window -t node "env -u WAYLAND_DISPLAY DISPLAY=:<n> VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json XDG_CONFIG_HOME=$S/xdg AHJO_SWARM_CONFIG=$S/app.json AHJO_SWARM_BOUNDS=0,0,1600,1000 $B/ahjo-swarm --node http://127.0.0.1:<port> 2>$S/app.log"`.
5. Open the page with `$B/ahjo view open <page> --node http://127.0.0.1:<port>`
   (every `ahjo` call names your node with `--node`, or it reaches the
   operator's) and take `ffmpeg -f x11grab -i :<n> -frames:v 1 $S/shot.png`.
   Use it as a person does with `xdotool` on your display: `DISPLAY=:<n>
   xdotool mousemove <x> <y> click 1`, `xdotool key Down Return Escape
   ctrl+p`, `xdotool type --delay 20 'text'`. Coordinates are the
   screenshot's pixels; give the app a moment after each action before the
   next screenshot. Walk a journey this way, not by opening pages alone:
   clicks, keys and typing are what show focus, order and dead ends.
6. Stop what you started: `$R kill-server` ends the app and the node,
   `kill $X` the display (Xvfb outlives the tmux server's hangup), and `$AHJO_TMUX -S $S/tmux.sock kill-server` the
   terminals.
