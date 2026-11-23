# Send's target: the session it was opened from, else the one named, else the focused window's

Send has two ways in, and each aims it differently:

- **From a session** (its window's Send, Alt+S in its terminal, Peek's Send, its
  menu): Send is aimed at that session, which is listed with the others. The
  line naming that session goes along only when another session is picked
  instead.
- **From another page:** Send is aimed at the session the ask names, such as a
  run's seat. If none is named, it is aimed at the session of the terminal
  window that last had focus. Either way the session must still be listed.

Send is never aimed by the list's order. With no target, it asks for one. It can
send to one session or several, and in a session with several panes the
operator picks the pane. The rule is `store::send::first_target`, and the doc
comment of `areas/terminals/app/src/ui/send.rs`.

**Why.** The code gives the reason as "one rule the operator can predict". The
last change is recorded: before 41225d42, Send opened from a session made that
session a source and left it out of the list. Send then asked for a target and
could not send to the session it came from.

**What it replaced.** The rule was rewritten at least five times:

- 2026-11-10, a5cdd9af: an ask can name a target, and a named session wins
  over the focused one.
- 11-11, df3da61b (ahjo-j0f.4): a pane choice for a session with several panes.
  Before, Send always used the first pane, while CHROTE needs an explicit pane.
- 11-11, f7af0608 (ahjo-j0f.11): Send from a terminal carries its session's
  reference.
- 11-11, 029de234 (ahjo-j0f.19): Send from a window uses that window as its
  source, leaves it out of the targets and starts with no target. Before, it
  offered the window's own session with a reference to itself.
- 11-12, 367a2c38: "one rule": the terminal the asking view sits beside, else
  the session the ask names, else the last one sent to, and never one picked by
  list order. Work stopped guessing a session from an item's folder.
- 11-12, 7f23c74b: "one Send target rule" again: the named target, else the
  focused window, from every page.
- 11-23, 41225d42: opened from a session, Send is aimed at that session (above).

Where Send stands changed separately (ADR 0001).

**Not recorded.** No source says why 7f23c74b dropped "the terminal the asking
view sits beside" and "the last one sent to" the same afternoon that
367a2c38 brought them in (15:17, then 16:38).
