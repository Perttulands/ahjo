# Send's target: the session it was opened from, else the named one, else the focused one

Send's target rule was rewritten seven times in two weeks. Opened from a
session, Send is aimed at that session. Asked by another page, it is aimed at
the session the ask names, and asks for a target when that session has gone;
only an ask that names no session is aimed at the focused window's. The reason
is one rule the operator can predict, which never sends somewhere the operator
did not mean.

## Decision

- **From a session** (its window's Send, Alt+S in its terminal, Peek's Send, its
  menu): Send is aimed at that session, which is listed with the others. The
  line naming that session goes along only when another session is picked
  instead.
- **From another page, naming a session** (such as a run's seat): Send is
  aimed at that session while it is listed. When it is no longer listed, Send
  opens with no target and asks for one; it does not fall back to the focused
  window's session.
- **From another page, naming none:** Send is aimed at the session of the
  terminal window that last had focus, if it is still listed; else it asks for
  a target.

Send is never aimed by the list's order. It sends to one session or several, and
in a session with several panes the operator picks the pane. The rule is
`store::send::first_target`, and the doc comment of
`areas/terminals/app/src/ui/send.rs`.

## Why

The code says why: "one rule the operator can predict". The last two changes
also have recorded reasons. Before 41225d42, Send opened from a session made that
session a source and left it out of the list, so Send asked for a target and
could not send to the session it came from. A named session that has gone no
longer falls back to the focused one (the operator, 2026-11-23): sending
somewhere other than the session the ask named is the guessed-target failure
that made 7f23c74b drop the other fallbacks.

## What it replaced

- 2026-11-10, a5cdd9af: an ask can name a target, and a named session wins
  over the focused one.
- 11-11, df3da61b (ahjo-j0f.4): a pane choice for a session with several panes.
  Before, Send always used the first pane, while CHROTE needs an explicit pane.
- 11-11, f7af0608 (ahjo-j0f.11): Send from a terminal carries its session's
  reference.
- 11-11, 029de234 (ahjo-j0f.19): Send from a window uses that window as its
  source, leaves it out of the targets, and starts with no target. Before, it
  offered the window's own session, with a reference to itself.
- 11-12, 367a2c38: "one rule": the terminal the asking view sits beside, else
  the session the ask names, else the last one sent to, and never one picked by
  list order. Work stopped guessing a session from an item's folder.
- 11-12, 7f23c74b: "one Send target rule" again: the named target, else the
  focused window, from every page.
- 11-23, 41225d42: opened from a session, Send is aimed at that session.
- 11-23 (ahjo-x3w): a named session that has gone leaves Send asking for a
  target instead of falling back to the focused window's session.

ADR 0001 covers the separate changes to where Send stands.

**Why 7f23c74b dropped two fallbacks** (the operator, 2026-11-23): "the
terminal the asking view sits beside" and "the last one sent to" did not work
in use, and the rule should be simple. Send is used from many pages (Files,
Issues, Harness, the Brain and the terminals), so its rule has to be one the
operator can predict from any of them.
