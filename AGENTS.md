# Ahjo

Ahjo is three apps on one app kit. Ahjo Swarm is the operator's agent
workspace: terminals, files, work, host and missions in one Rust binary, used
through a native app and the `ahjo` CLI, which also reaches the brain. Ahjo
Brain and Ahjo Desk are standalone apps. `VISION.md` says what Swarm is for,
`DESIGN.md` what good looks like in the app and `ARCHITECTURE.md` how it is
built.

## The operator works on this machine while you do

- The operator's tmux sessions survive everything you do. Tests start their own
  tmux servers and Ahjo nodes through `kit/testkit` and act only on those,
  never on the real fleet or the real Tailscale. A test needs only Rust and
  tmux: any other program (`bd`, `gh`, a harness) is a fake the test writes.
- Other agents run at the same time. Pick unique scratch names and ports.
- Build and test through `heavy run --memory <GiB>`.

## Owning a piece

You own an outcome, such as a page being good on its journeys, and keep going
until it is. Fix what you find where it lives, in any directory. Push to main
when the crates you changed pass the checks; CI runs the whole workspace, and a
red main is fixed forward by whoever broke it. Tell the other owners only when
you change a `contract` or `kit/` interface they use. File a bead only for work
you will not finish yourself.

For anything on screen, run the app against a node of your own on a private
display and look at the screenshot. A passing test does not show that a page
is good.

## Code

- `ARCHITECTURE.md` lists the boundaries the build holds. If a change needs to
  break one, the design is wrong: stop and say so.
- In the native app every visual value is a token, and stores stay apart from
  views.
- Land a rename or reshape whole: no shims, aliases or dead code.
- Errors carry a code and a message in the one shape `kit/core` defines.

## Names

`ARCHITECTURE.md` (Words) is the vocabulary. Names follow these rules:

1. One meaning per domain word on every layer; only casing changes. Before:
   `--role` was stored as `agentId` and shown as role. After: a seat's `agent`
   is an agent in the CLI, the file and the board.
2. Name an id after what it identifies: `step_id` for a step, `card_id` where
   it may be a gate or end, never `node_id`.
3. A name states only what is true: a value that is something else is renamed
   or removed (`session_stem` held a role id; the real session name replaced it).
4. Output carries derived values: a seat shows its session's name, so no
   reader needs the rule that builds it.
5. A boolean reads as a question or names its effect: `is_open`, not `open`;
   `install_completion_hooks`, not `notify`.
6. Shared types carry their domain: `ScheduledTarget`, `LimitScope`, not
   `Target` in a contract or engine crate; a page's private type may be short.
7. A name that lives through a long function is a word (`mission`, not `m`);
   one letter is for a closure or a few lines, plus `cx` and `t: &Theme`.
   Rename when you touch the code, not as a campaign.
8. One verb per action across nouns: `list`, `show`, `create`, `edit`,
   `delete` (not `new`, `inspect`, `retire`).

## Checks

Before pushing: `cargo fmt --check`, then `cargo clippy -- -D warnings` and
`AHJO_REQUIRE_TMUX=1 cargo test` for the crates you changed (`-p <crate>`).
If `tmux` on `PATH` is a wrapper that refuses `kill-server`, set `AHJO_TMUX` to
the real binary. Write a test for behaviour that could break again, not for how
something looks. Chase a flake by reading the code and writing a deterministic
test, not by rerunning.
