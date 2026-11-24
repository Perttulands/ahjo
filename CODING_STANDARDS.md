# Coding standards

This file holds Ahjo's seven review rules. Read it when you review a change.

A finding under a rule cites three things:

1. the rule's bold sentence;
2. the changed line;
3. what that line affects: a caller, an interaction or stored data.

Two kinds of finding must be acted on:

- a cited breach of a rule in this file;
- an acceptance item of the owning Bead that is not met.

Smells and other observations are for the implementer to weigh and decide on.
When a rule here and a smell disagree, the rule wins.

Skip the mechanical rules in a review. Automatic checks already cover them:

- `cargo fmt`, and Clippy with `-D warnings`.
- The tests in `bin/ahjo/tests`. `layout.rs` checks dependency direction,
  hand-built scroll areas, popups, overlays and outside presses, and type
  sizes, corners and colours written as literal numbers. `areas.rs` checks
  command registration, verbs, retired words in command and schema names, and
  that a skill names every command.
- A handler's failure is typed as `kit/core`'s error.

## Names

**A name says what its value is, with one meaning on every layer.**

Take domain words from `GLOSSARY.md`.

- Name an id after what it identifies: `step_id`, or `card_id` where it may be
  a gate or an end. Never `node_id`.
- Name a boolean as a question or as its effect: `is_open`,
  `install_completion_hooks`.
- Put the domain in the name of a shared type in a contract or engine crate:
  `ScheduledTarget`, not `Target`.
- Use a word for a name that lives through a long function. Use one letter
  only in a closure or over a few lines. `cx` and `t: &Theme` are also allowed.
- Use one verb per action, whatever the noun: `list`, `show`, `create`,
  `edit`, `delete`.

Fix names where your change touches them. Do not run a renaming campaign.

Why: `--role` was stored as `agentId` and shown as role, and `sessionStem` held
a role id (ahjo-biq.1). Both names pass a spelling check, and readers built on
the wrong word (48cd8f1f).

## Contracts

**A caller gets the facts it needs from the owner that knows them, through the
owner's contract.**

Output carries values that are already worked out, so no reader rebuilds them.
For example, a seat shows its session's name. Saved state says how it is to be
read. For example, a card placed by hand is saved as placed by hand.

Why: a seat's dispatch record carried a stem, and readers built the session's
name from it. The stem was really a role id. The record now carries the
session's name (ahjo-biq.1). Saved board positions do not record that a person
placed them, so every load lays out the board again (ahjo-jlr).

## Interactions

**An interaction uses the kit element for the gesture it promises
(`ARCHITECTURE.md`, Kit).**

Focus, closing, sizing and scrolling belong to that element. The page keeps
only its content and the thing its action works on. A page's own copy of that
behaviour is a breach, even where the source checks cannot see it. A panel
built from plain elements is one example. When a kit element falls short,
extend it in `kit/`.

Why: five different side-surface mechanisms appeared in 13 days, each with its
own keys and bugs, until 6d9c62dc replaced them with one panel. After Send, the
keyboard landed in a terminal hidden under the Brain panel (ahjo-4y1,
07cb1a3f).

## State

**A choice or pending edit settles in the page's store, on the target it was
made for, on every completion path.**

An action has five **completion paths**: success, failure, a newer write, undo
and redo. Views draw from the store. A late reply applies only to the request
that caused it.

Why: a seat pick made while an undo was being written was lost, because only
the board write applied it (ahjo-hhw). A test that waited for Redo before
picking hid the defect (cd220aad). A late "Launched" overwrote a newer launch
failure (ahjo-j0f.9).

## Changes across callers

**A rename or reshape lands whole in one change, through readers, writers,
saved data, commands, help and skill text.**

Everything the old shape supported still works. Leave no shim, alias or dead
path behind.

Why: moving the Missions page dropped interactions, which then had to be
restored (ahjo-7n7.12, e2611364). Schema 6 left the `ahjo` skill still
teaching schema 5 (ahjo-b7q, 1b3c2eb1).

## Regression tests

**A test for a fixed bug reproduces the real order of actions and asserts on
the resource that broke, not only on Ahjo's report of it.**

Reproduce the real conditions:

- a second client;
- how the real program behaves, such as tmux or a full-screen agent;
- an action made while a write is still in flight.

When the failure was in tmux, in a file or in another program's state, the
test reads that state directly.

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

Update help and skill text that describes the behaviour in the same commit too.

Why: `DESIGN.md` said nothing opened beside a terminal resizes it (031cf360).
f9ed9583 made the windows narrower for a panel and left that sentence
standing. The decision reached `DESIGN.md` only in 6d9c62dc. Send's rule for
where it sends was rewritten at least four times (ahjo-j0f.19, 41225d42). Help
for `--cwd` does not say the work happens in a copy (ahjo-lvo).
