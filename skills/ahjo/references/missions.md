# Missions

A mission is a reusable graph. An Input card starts it, formations are steps with staffed slots and ports, gates route verdicts, and End nodes end paths as `done` or `rejected`. A run snapshots the graph and staffing at admission; later edits affect later runs. Each step's agents work in ordinary tmux sessions with the harness's access, named `<prefix>-<agent>` so the session list groups a mission's sessions under its prefix.

## Connect

Use the node that owns the mission, as described in [the Ahjo Swarm skill](../SKILL.md). `ahjo help`, `ahjo <noun>` and `ahjo <noun> <verb> --help` are the current command reference; use `--json` for IDs and `--node` on each command when targeting another node. The examples below use the local node. An area can be disabled or a node can run an older build: check its help before relying on a command.

## Pick up a mission

The operator sketches a mission roughly and writes notes on it: on the mission as a whole (what they are trying to build), on steps, gates, limits, the Input card, Ends and wires. Notes are their intent; your job is usually to turn the sketch into a mission that runs. `ahjo mission show <mission>` is the whole hand-over in one answer: the intent notes under `about`, the steps in the order a run meets them (numbered as the app numbers them, with who does each, each slot’s session, the brief, where it leads, gates with what they check and where pass and fail go), limits with what they cap, every note beside what it is on with its id, author and whether it is resolved, and the `problems` that still stop a run. `--full --json` is the whole definition with every port and slot id, for wiring. For a fresh attempt or a session name already in use, `ahjo run show <run-id> --seats` shows the session bound during the run.

Answer the notes where they are, so the operator sees on the board what you did: `ahjo mission reply <mission> <note-id> --text '<answer or question>' --author <your-name>` asks or answers in the note's thread, and `ahjo mission resolve <mission> <note-id> --text '<what you did about it>' --author <your-name>` closes it with what you did; a later note from anyone opens it again. Leave your own explanations with `ahjo mission note <mission> --target <id-or-title> --text '...' --author <your-name>` (no `--target`: the mission itself; a wire is `From->To`, or `Gate:fail->Step`). `--author claude-7` writes as `agent:claude-7`; give your session's name. `--edit <note-id>` and `--delete <note-id>` change only your own notes; reply rather than rewrite someone else's. Notes express intent; briefs instruct seats: translate notes into briefs, staffing and wiring. `ahjo mission notes <mission>` lists the threads as they are stored.

## Author

Export the mission you will edit, or start from an example.

```sh
ahjo mission export post > post.toml
ahjo mission export --example write-and-review > new-post.toml
ahjo mission apply post.toml --dry-run
ahjo mission apply post.toml
```

Give a new mission its own `slug` and `id`, or omit its `id` to assign one. Keep the exported `rev` when editing an existing mission. If apply reports a conflict, export again and merge your edits into that copy. Apply saves drafts and reports validation. Fix the errors before using its run command. Reply to open notes before removing their cards. Over MCP, give `mission_apply` the TOML text in `document`.

The file uses `[[inputCard]]`, `[[formation]]`, `[[gate]]`, `[[end]]`, `[[limit]]`, and `[[connection]]`. A formation nests `input`, `output`, and `slot` tables. Use saved ids in `from` and `to` as `node:port`, and give every card you connect an id. `ahjo mission export --json` uses the same keys.

### Individual changes

Read `ahjo mission show <mission> --full --json` for card and port ids. Sketch a draft with `ahjo mission create <slug> --title '<title>' --steps 'Collect; Draft; Review' --note '<intent>'`. Use `--example <name>` to copy a staffed, wired example from `ahjo mission examples`. For one change, run `ahjo mission edit <slug> --rev <rev> --change '{"title":"New title"}'`. Send several operations together as `{"rev":n,"changes":[...]}`. The edit help lists the operation keys and fields. Use export and apply for broader rewiring and staffing.

`ahjo layout show <slug>` reads positions. `ahjo mission arrange <slug>` arranges the board after rewiring. `ahjo layout edit <slug> '{"nodes":[{"id":"<card-id>","x":120,"y":80}]}'` moves a card.

## Agents and sessions

A slot's `agent` names its session. With no name it uses the step's title. Set `fresh = true` on a formation to start a fresh session for each attempt. The prefix is the slug's letters and digits unless the mission supplies `prefix`. A mission can reuse only a session that a run started. A busy or independently started session is left alone and the run uses a separate session. Do not type into a mission's session while its step runs.

## Inputs and outputs

Declare inputs and outputs on the mission's Input card. With no declarations the run takes a required `brief`. Reference inputs in step briefs as `{name}`. Use `{{name}}` for literal braces. File and folder inputs are existing absolute paths on the node. File and folder outputs are copied to the attempt's evidence and the copy's path is routed. A missing required output on a done path fails the run.

## Gates

Set a criterion and gate kinds in the mission file. A human-only gate is the default. A judge uses a staffed solo formation in the gate's judge chain. A code gate checks `output_contains` or `output_absent` version 1. Kinds execute in code, formation, human order and stop at failure. Wire `<gate-id>:in`, `<gate-id>:pass` and `<gate-id>:fail` to their respective ports. Cap send-back loops with a Limit card.

The runtime supplies each seat its output contract; a judge also returns a fenced verdict block containing `verdict` (`pass` or `fail`), `reason`, and `evidence`. Do not direct a seat by typing into its session. Use `ahjo run show <run-id> --evidence --seats` if a result fails to route.

## Limits

Runs have no automatic limits. Add a Limit card targeting a step id or the Input card id for the whole mission. Set a cap in rounds, seconds or tokens. Exhaustion blocks by default. With `whenSpent = "fail"` the run fails. With `whenSpent = "best"` a suitable send-back gate selects among judged attempts. A human pick or a grant needs the operator's authority.

## Run and watch

Start only with the authority and inputs appropriate to the work. `ahjo mission run <slug> --inputs '{"brief":"<work>"}' --actor 'agent:<session>' --json` returns `run_id`; use named values in the JSON object for declared inputs. `--cwd` selects an existing workspace; `--work` identifies the owning work item and repeated `--context-paths` supply references. `ahjo mission run <slug> --from <step>` starts at one step. If the receipt is lost, inspect `ahjo run list --mission <slug>` before starting another.

Drive with `ahjo run wait <run-id> --json`. The default `needs-you` returns when a gate needs a decision, the run blocks, or it ends. The result has `outcome`, `status`, `cursor`, `asks`, and exact answer commands in `next`; exit 0 means the wait answered, not that the run succeeded. After an answer, wait again using `--since <cursor>`. `--until final` waits for the end, `--until any-change` reports the next change, and `--timeout <seconds>` bounds a wait. On timeout, wait again with the returned cursor. Use `ahjo run show <run-id>` for the reason, exact next command, state and outputs. Add `--events`, `--seats`, `--attempts` or `--evidence` for details. Evidence includes problems, briefs, produced file paths and the files agents wrote in the run folder under `written`. Use `--step <id>` to narrow attempts or evidence. A picked result carries failed work selected for continuation, so inspect its findings before using it.

## Decide

For a human gate, `ahjo run show <run-id>` includes the criterion, inputs and possible routes under the pending ask. Decide only with the operator's authority. Follow the suggested `ahjo run approve` or `ahjo run reject` command with its current `--seq`. Add `--reason` to send the operator's words onward. When a gate asks for a pick, view `ahjo run show <run-id> --attempts` and follow the suggested `ahjo run pick` with `--attempt` and `--seq`. If the sequence is stale, read the run again. Wait again after the decision.

## Recover

Read `ahjo run show <run-id> --evidence` for the block and whether resume is allowed. A seat that went quiet without saying it was done is reminded twice, then its step blocks with `seat_idle_without_sentinel` (or `seat_stuck` when it never took its brief, with what its screen said); even `run wait --until final` returns then. Its terminal is kept: resuming asks that seat once to go on, and a closed terminal blocks again with `seat_gone`, which needs `--mode redispatch`. Resolve the named cause, then `ahjo run resume <run-id> --reason '<reason>'`; use `--mode redispatch` when a lost seat must run again. A spent limit may require an authorized `--grant` or `--pick-best`; otherwise `ahjo run abort <run-id> --reason '<reason>'` stops that run. Preserve its briefs and artifacts while investigating. In a repository run, what a step that failed or was stopped wrote is kept on the branch `ahjo/run-<first 8 of the run id>-kept/<step-name>-<attempt>` (the run's own work is on `ahjo/run-<first 8>`), named in the block reason and in a `workspace_kept` event in `run show --events`; `run delete` removes it. A final failed or canceled run needs a new run after fixing its cause. Follow a resumed run with `run wait` again.

`ahjo run delete <run-id>` removes a finished run with its record, artifacts and the folder Ahjo made for it; a repository run must be landed first. `ahjo mission delete <slug>` removes a mission. Delete only with the operator's say-so.
