# What resizes a session's windows: the operator's act, and the windows squeeze

A shared session's terminal windows change size only for the operator's own
act: opening or putting away the panel (a path he clicks, Send or an item's card
opens it) and dragging an edge. What opens beside the windows makes room for
itself, so the windows squeeze rather than being covered. A page showing, the
panel's content changing, or Peek never resizes them. A resize settles before it
reaches tmux, so a sliding panel sends one. `DESIGN.md` (Never disturb the work)
and `ARCHITECTURE.md` (App pages, Window sizes) state the rule.

**Context.** Sessions are tmux sessions that are also shared with other clients
(the phone, CHROTE, other apps). Resizing a window reflows the program in it for
every one of those clients.

**What it replaced.** The rule flipped from "nothing resizes" to "the windows
squeeze" in steps:

- 2026-11-11, 693515b3 (ahjo-j0f.16): Send reserved space beside the terminal,
  because it opened over the terminal it sent to and hid its last lines.
- 11-13, 8afca65a and ff649000: nothing opened on the page changes a session's
  size. The view beside, Send, the output search and Overview all lay over the
  windows. The recorded reason: sessions are shared, so opening something
  beside them "must not reflow their programs", and a drawer that moved the
  windows "resized every shared session ... for a drawer soon closed".
  031cf360 (11-14) wrote this into DESIGN.md, and ea9c59a0 (11-14) held it with
  the test `nothing_opened_on_the_page_resizes_a_session`.
- 11-17, e48f1ffd: putting away the shell's left column resizes the windows.
  The reason given: "Only the operator's own act resizes them, as putting the
  panel away does, the way putting the rail away always did."
- 11-19, d22c42cc: showing and hiding the Files edge panel resizes the windows
  too.
- 11-19, 81b2af1e: Send makes room, "since one writes to sessions with it while
  reading them".
- 11-21, ef411da1: the rewritten DESIGN.md said again that "nothing opened
  beside a terminal resizes it", although ARCHITECTURE.md already let panels
  and Send resize.
- 11-23, f9ed9583: a path clicked in a terminal opens the Files panel, and the
  session's window narrows.
- 11-23, 6d9c62dc: everything beside a page moved into the one panel (ADR
  0001), and DESIGN.md became "makes room for itself (the windows squeeze)".

**Why.** The history records reasons for two parts: Send, whose target's lines
must stay readable while one writes, and the operator's own act as the only
trigger, following the rail. **Not recorded:** why the reason of 8afca65a and
ff649000 no longer outweighs covering for the rest of what the panel shows (a
file, an issue's card, the Brain, a diagram). That reason was that a resize
reflows the session for every other client. No source says why those squeeze
rather than lie over the windows.
