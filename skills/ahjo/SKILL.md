---
name: ahjo
description: Drive Ahjo Swarm, the operator's agent workspace, from the command line with `ahjo`. Use when inspecting or acting on sessions, files, work, the brain, host services, scheduled prompts, roles, missions, runs, or the open app through Ahjo Swarm.
---

# Ahjo Swarm

Ahjo Swarm runs a node in each machine environment. `ahjo help` lists the running node's nouns, each with its verbs; `ahjo <noun>` says what each verb does and `ahjo <noun> <verb> --help` gives its arguments (`ahjo help --all`: every command at once). Commands depend on the areas enabled on that node. `ahjo version` says whether the local node answers (exit 3 when none does); where `ahjo install` put it in place it runs as the systemd user unit `ahjo`. `ahjo mcp` exposes the same tools over stdio.

Commands use `ahjo <noun> <verb>`. Required arguments can be positional or named flags, as can a command's one optional argument (`ahjo file list <path>`); object arguments are JSON. Add `--node <id>` to act on a node through the local node, or `--node <url>` to contact a node directly. Without it, commands address the local node.

Plain output is compact: it leaves out empty fields and says `none` for an empty list. Lists and readings are short by default: `session history` gives the newest 200 lines and says when it cut (`--last N`, `--all`), `role list` and `host show` give a summary (`--full` for every field; `role inspect <id>` for one role). Add `--json` when a program parses the answer: stdout is then the answer as JSON, and a failure is `{"error":{"code","message"}}` on stderr. The exit code is 0 when the command was done, 1 when it ran and failed (match on the error's `code`), 2 when the command line was wrong, and 3 when no node answers. An error names the command to run next.

Nouns: `session` (terminal sessions), `file`, `work` (the work board), `brain`, `host` and `service`, `scheduled` (prompts sent into sessions), `role` and `agents` (agent tools, skills and instruction files), `mission`, `formation`, `layout`, `gate`, `end`, `limit` and `run` (mission graphs and their runs; read [missions](references/missions.md)), `view` (the open app) and `config`. Before a file `write`, read the file and pass its version as the base, so a stale edit is refused. `agents shape --dir D` shows what an agent started in D gets (instructions in load order, skills, hooks, settings, servers, each with its file); the `hook-*` and `setting-set` verbs change the files it lists.

The Swarm app’s own settings are in `app.json` on the app’s device: `AHJO_SWARM_CONFIG` overrides its path; otherwise it is `$XDG_CONFIG_HOME/ahjo/app.json` (default `~/.config/ahjo/app.json`) or `%APPDATA%\Ahjo\app.json` on Windows. Read and preserve the rest of the JSON when editing it; the running app reloads changes. `ahjo config show` reads the separate node configuration.

## Sessions and scheduled prompts

Read a session before sending into it; the operator may be working there. `session send` pastes text and normally presses Enter; `--no-enter` leaves it unsubmitted. A receipt says tmux took the input, not that the agent consumed it or finished. If delivery is uncertain, inspect the pane before retrying. Kill only sessions you launched.

The app's Send to Session composes a reference, note and attached file paths into a message; attachments are copied to the target node first. `session send` sends the supplied text only, so prepare any reference and paths yourself. The app leaves a message unsubmitted when it launches a new session.

For recurring work, inspect `scheduled list` and update a matching task instead of duplicating it. Target existing session names (or `user/name`), inspect the receiving session and its input, and write a durable prompt with the purpose, context and expected result. `scheduled create` takes a name, prompt, sessions and either `--every` minutes or a five-field `--cron`; specify an IANA `--timezone`. Use command help for exact syntax. A task sends only to the sessions it was made for, through renames and across a tmux restart (a reboot), where it follows their names: when another session takes one's name on the same server, the task is held (`held` in `scheduled list`) until `scheduled edit <task> --sessions <name>` aims it again. Read back the task and its next run after changing it. `scheduled run` sends immediately; use it only when that is appropriate. The scheduler does not launch an agent. Check the receiving conversation and artifacts to verify consumption and completion.

For authoring a mission, staffing its steps, or driving and recovering a run, read [missions](references/missions.md).

For the complete command inventory, see [commands](references/commands.md).
