# Coding standards

The reviewer reads this file. It holds Ahjo's seven review rules. A finding
under them cites the rule by its bold sentence, the changed line, and the
caller, interaction or stored data that the line affects. Two kinds of finding require action: a
cited breach of a rule here, and an unmet acceptance item of the owning Bead.
The implementer weighs smells and other observations and decides on them.
Where a rule here and a smell disagree, the rule wins.

Checks already hold the mechanical forms, so a review skips them: `cargo fmt`,
Clippy with `-D warnings`, and the tests in `bin/ahjo/tests`. `layout.rs`
covers dependency direction, hand-built scroll areas, popups, overlays and
outside presses, and literal type sizes, corners and colours. `areas.rs`
covers command registration, verbs, retired words in command and schema names,
and a skill that names every command. A handler's failure is typed as
`kit/core`'s error.

## Names

**A name says what its value is, with one meaning on every layer.**

Domain words come from `ARCHITECTURE.md` (Words). An id is named after what it
identifies: `step_id`, or `card_id` where it may be a gate or end, never
`node_id`. A boolean reads as a question or as its effect: `is_open`,
`install_completion_hooks`. A shared type in a contract or engine crate carries
its domain: `ScheduledTarget`, not `Target`. A name that lives through a long
function is a word; one letter is for a closure or a few lines, plus `cx` and
`t: &Theme`. Each action has one verb across nouns: `list`, `show`, `create`,
`edit`, `delete`. Names are fixed where the change touches them, not in a
campaign.

Why: `--role` was stored as `agentId` and shown as role, and `sessionStem` held
a role id (ahjo-biq.1). Both pass a spelling check, and readers built on the
false word (48cd8f1f).

## Contracts

**A caller gets the facts it needs from the owner that knows them, through the
owner's contract.**

Output carries derived values, so no reader rebuilds them (a seat shows its
session's name). Saved state says how it is to be read (a card placed by hand
is saved as placed by hand).

Why: a seat's dispatch record carried a stem for readers to build the
session's name from, and the stem was really a role id; the record now carries
the session's name (ahjo-biq.1). Saved board positions lack the fact that a
person placed them, so every load lays the board out again (ahjo-jlr).

## Interactions

**An interaction uses the kit element for the gesture it promises
(`ARCHITECTURE.md`, Kit).**

Focus, dismissal, sizing and scrolling live in that element; the page keeps
only its content and the target of its action. A page-local copy is a breach
even where the source checks cannot see it, such as a panel built from plain
elements. A kit element that falls short is extended in `kit/`.

Why: five side-surface mechanisms appeared in 13 days, each with its own keys
and bugs, until 6d9c62dc replaced them with one panel. After Send, the keyboard
landed in a terminal hidden under the Brain panel (ahjo-4y1, 07cb1a3f).

## State

**A choice or pending edit settles in the page's store, on the target it was
made for, on every completion path.**

Views draw from the store. The completion paths are success, failure, a newer
write, undo and redo. A late reply applies only to the request that caused it.

Why: a seat pick made during an undo's write was dropped, because only the
board write drained it (ahjo-hhw). A test that waited for Redo before picking
hid the defect (cd220aad). A late "Launched" overwrote a newer launch failure
(ahjo-j0f.9).

## Changes across callers

**A rename or reshape lands whole in one change, through readers, writers,
saved data, commands, help and skill text.**

Everything the old shape supported still works, and no shim, alias or dead
path is left behind.

Why: moving the Missions page dropped interactions that had to be restored
(ahjo-7n7.12, e2611364). Schema 6 left the `ahjo` skill teaching schema 5
(ahjo-b7q, 1b3c2eb1).

## Regression tests

**A test for a fixed bug reproduces the real order of actions and asserts on
the resource that broke, not only on Ahjo's report of it.**

The real conditions include the second client, the real program's behaviour
(tmux, a full-screen agent) and the action made while a write is in flight.
Where the failure was in tmux, a file or another program's state, the test
reads that state directly.

Why: the test for idle sessions looking active (ahjo-j0f.12, 9bf76114) started
a second node and checked that Ahjo's reported `last_output` and `active` did
not change. It passed, while connecting the app on the operator's server moved
tmux's own `session_activity` on 12 of 15 sessions (ahjo-j0f.17). The test in
a3bf9cc0 compares tmux's `session_activity`, window size and contents before
and after a listing with other clients attached; b89682a1 fixed the cause.

## Decisions and docs

**A change to a decided behaviour updates the `DESIGN.md`, `ARCHITECTURE.md`
or ADR passage that states it in the same commit, and the commit message says
why.**

Help and skill text that describes the behaviour changes in the same commit.

Why: `DESIGN.md` said nothing opened beside a terminal resizes it (031cf360).
f9ed9583 made the windows narrow for a panel and left that sentence standing;
the decision reached `DESIGN.md` only in 6d9c62dc. Send's target rule was
rewritten at least four times (ahjo-j0f.19, 41225d42). Help for `--cwd` does
not say the work happens in a copy (ahjo-lvo).
