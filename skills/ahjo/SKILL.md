---
name: ahjo
description: Drive Ahjo Swarm, the operator's agent workspace, from the command line with `ahjo`. Use when inspecting or acting on sessions, files, work, the brain, host services, scheduled prompts, roles, missions, runs, or the open app through Ahjo Swarm.
---

# Ahjo Swarm

Ahjo Swarm runs a node in each machine environment. Use `ahjo help` for the running local node's commands, `ahjo <noun>` for its verbs and `ahjo <noun> <verb> --help` for arguments. Commands depend on the areas enabled on that node. `ahjo version` says whether the local node answers (exit 3 when none does); where `ahjo install` put it in place it runs as the systemd user unit `ahjo`. `ahjo mcp` exposes the same tools over stdio.

Commands use `ahjo <noun> <verb>`. Required arguments can be positional or named flags; object arguments are JSON. Add `--node <id>` to act on a node through the local node, or `--node <url>` to contact a node directly. Without it, commands address the local node. Use the command's help for current arguments and defaults.

Add `--json` whenever you read the answer: stdout is then the answer as JSON, and a failure is `{"error":{"code","message"}}` on stderr. Plain output is for a person and leaves out empty fields. The exit code is 0 when the command was done, 1 when it ran and failed (match on the error's `code`), 2 when the command line was wrong (the usage follows), and 3 when no node answers.

| Noun | Purpose and verbs |
|---|---|
| `session` | Sessions: `list`, `read`, `history`, `search`, `send`, `keys`, `launch`, `copy`, `rename`, `kill`, `mouse`, `seen`. |
| `file` | Browse and change files: `list`, `read`, `diff`, `recent`, `download`, `upload`, `write`, `mkdir`, `move`, `delete`, `find`, `locate`, `search`, `copy`, `resume`, `transfer`. Use the base version on writes to detect stale edits. |
| `work` | Read the configured work board and item family, or file an item: `list`, `show`, `create`. |
| `brain` | Find, read and save knowledge: `search`, `read`, `save`, `collections`, `changes`, `send`. |
| `host` | Read host information: `show`. |
| `service` | Inspect and control services: `list`, `show`, `log`, `start`, `stop`, `restart`. |
| `scheduled` | Manage prompts sent into sessions: `list`, `show`, `create`, `edit`, `copy`, `pause`, `resume`, `run`, `delete`. |
| `role` | Manage the role library: `list`, `inspect`, `usage`, `new`, `edit`, `retire`, `restore`, `delete`, `characters`. |
| `agents` | Inspect agent tools, skills and checkouts with `show`; `sync` a skills checkout and `link` its skills into the skills folders missing them. `shape --dir D [--harness claude\|codex]` shows what an agent started in D gets (instructions in load order, skills, hooks, settings, servers, each with its file); `read`/`write` an instruction or skill file (write names the etag read); `hook-add`, `hook-set`, `hook-remove`, `hook-switch` and `setting-set` change the file `shape` lists. |
| `mission`, `formation`, `layout`, `gate`, `end`, `limit`, `run` | Author mission graphs and drive runs; read [missions](references/missions.md). |
| `view` | Inspect connected apps with `show` and open a page or item with `open`. |
| `config` | Read the node configuration with `show`. |

The Swarm app’s own settings are in `app.json` on the app’s device: `AHJO_SWARM_CONFIG` overrides its path; otherwise it is `$XDG_CONFIG_HOME/ahjo/app.json` (default `~/.config/ahjo/app.json`) or `%APPDATA%\Ahjo\app.json` on Windows. Read and preserve the rest of the JSON when editing it; the running app reloads changes. `ahjo config show` reads the separate node configuration.

## Sessions and scheduled prompts

Read a session before sending into it; the operator may be working there. `session send` pastes text and normally presses Enter; `--no-enter` leaves it unsubmitted. A receipt says tmux took the input, not that the agent consumed it or finished. If delivery is uncertain, inspect the pane before retrying. Kill only sessions you launched.

The app's Send to Session composes a reference, note and attached file paths into a message; attachments are copied to the target node first. `session send` sends the supplied text only, so prepare any reference and paths yourself. The app leaves a message unsubmitted when it launches a new session.

For recurring work, inspect `scheduled list` and update a matching task instead of duplicating it. Target existing session names (or `user/name`), inspect the receiving session and its input, and write a durable prompt with the purpose, context and expected result. `scheduled create` takes a name, prompt, sessions and either `--every` minutes or a five-field `--cron`; specify an IANA `--timezone`. Use command help for exact syntax. Read back the task and its next run after changing it. `scheduled run` sends immediately; use it only when that is appropriate. The scheduler does not launch an agent. Check the receiving conversation and artifacts to verify consumption and completion.

For authoring a mission, staffing its steps, or driving and recovering a run, read [missions](references/missions.md).

For the complete command inventory, see [commands](references/commands.md).
