# Coding standards

This file is for the reviewer. It holds Ahjo's seven review rules. A finding
under a rule cites three things: the rule's bold sentence, the changed line,
and what that line affects (a caller, an interaction or stored data).

Two kinds of finding must be acted on:

- a cited breach of a rule in this file;
- an acceptance item of the owning Bead that is not met.

Smells and other observations are for the implementer to weigh and decide on.
When a rule here and a smell disagree, the rule wins.

Automatic checks already cover the mechanical rules, so a review skips them:
`cargo fmt`, Clippy with `-D warnings`, and the tests in `bin/ahjo/tests`.

- `layout.rs` checks dependency direction; scroll areas, popups, overlays and
  outside presses built by hand; and type sizes, corners and colours written as
  literal numbers.
- `areas.rs` checks command registration, verbs, retired words in command and
  schema names, and that a skill names every command.
- A handler's failure is typed as `kit/core`'s error.

## Names

**A name says what its value is, with one meaning on every layer.**

Domain words come from `GLOSSARY.md`.

- An id is named after what it identifies: `step_id`, or `card_id` where it
  may be a gate or an end; never `node_id`.
- A boolean reads as a question or as its effect: `is_open`,
  `install_completion_hooks`.
- A shared type in a contract or engine crate carries its domain in its name:
  `ScheduledTarget`, not `Target`.
- A name that lives through a long function is a word. One letter is for a
  closure or a few lines. `cx` and `t: &Theme` are also allowed.
- Each action has one verb, whatever the noun: `list`, `show`, `create`,
  `edit`, `delete`.

Fix names where your change touches them, not in a renaming campaign.

Why: `--role` was stored as `agentId` and shown as role, and `sessionStem` held
a role id (ahjo-biq.1). Both pass a spelling check, and readers built on the
wrong word (48cd8f1f).

## Contracts

**A caller gets the facts it needs from the owner that knows them, through the
owner's contract.**

Output carries values that are already worked out, so no reader rebuilds them
(a seat shows its session's name). Saved state says how it is to be read (a
card placed by hand is saved as placed by hand).

Why: a seat's dispatch record carried a stem, and readers built the session's
name from it. The stem was really a role id. The record now carries the
session's name (ahjo-biq.1). Saved board positions do not record that a person
placed them, so every load lays out the board again (ahjo-jlr).

## Interactions

**An interaction uses the kit element for the gesture it promises
(`ARCHITECTURE.md`, Kit).**

Focus, closing, sizing and scrolling live in that element. The page keeps only
its content and the thing its action works on. A copy of that behaviour inside
a page is a breach, even where the source checks cannot see it, for example a
panel built from plain elements. When a kit element falls short, extend it in
`kit/`.

Why: five different side-surface mechanisms appeared in 13 days, each with its
own keys and bugs, until 6d9c62dc replaced them with one panel. After Send, the
keyboard landed in a terminal hidden under the Brain panel (ahjo-4y1,
07cb1a3f).

## State

**A choice or pending edit settles in the page's store, on the target it was
made for, on every completion path.**

In plain words: whatever way the action ends, the choice or unsaved edit ends
up in the page's store, on the thing it was made for. Views draw from the
store. An action can end in five ways (the completion paths): success,
failure, a newer write, undo and redo. A late reply applies only to the request
that caused it.

Why: a seat pick made while an undo was being written was lost, because only
the board write applied it (ahjo-hhw). A test that waited for Redo before
picking hid the defect (cd220aad). A late "Launched" overwrote a newer launch
failure (ahjo-j0f.9).

## Changes across callers

**A rename or reshape lands whole in one change, through readers, writers,
saved data, commands, help and skill text.**

Everything the old shape supported still works. No shim, alias or dead path is
left behind.

Why: moving the Missions page dropped interactions, which then had to be
restored (ahjo-7n7.12, e2611364). Schema 6 left the `ahjo` skill still
teaching schema 5 (ahjo-b7q, 1b3c2eb1).

## Regression tests

**A test for a fixed bug reproduces the real order of actions and asserts on
the resource that broke, not only on Ahjo's report of it.**

The real conditions include a second client, how the real program behaves
(tmux, a full-screen agent), and an action made while a write is still in
flight. When the failure was in tmux, in a file or in another program's state,
the test reads that state directly.

Why: the test for idle sessions looking active (ahjo-j0f.12, 9bf76114) started
a second node and checked that Ahjo's reported `last_output` and `active` did
not change. It passed. Meanwhile, connecting the app on the operator's server
moved tmux's own `session_activity` on 12 of 15 sessions (ahjo-j0f.17). The
test in a3bf9cc0 compares tmux's `session_activity`, window size and contents
before and after a listing, with other clients attached. b89682a1 fixed the
cause.

## Decisions and docs

**A change to a decided behaviour updates the `DESIGN.md`, `ARCHITECTURE.md`
or ADR passage that states it in the same commit, and the commit message says
why.**

Help and skill text that describes the behaviour changes in the same commit
too.

Why: `DESIGN.md` said nothing opened beside a terminal resizes it (031cf360).
f9ed9583 made the windows narrower for a panel and left that sentence
standing; the decision reached `DESIGN.md` only in 6d9c62dc. Send's rule for
where it sends was rewritten at least four times (ahjo-j0f.19, 41225d42). Help
for `--cwd` does not say the work happens in a copy (ahjo-lvo).
