# What resizes a session's windows: the operator's act, and the windows squeeze

Sessions are tmux sessions shared with other clients (the phone, CHROTE, other
apps), and a resize reflows their programs for every client. The windows change
size only for the operator's own act, and what opens beside them makes room
rather than covering them. The recorded reasons cover Send and the operator's
own act. The rest of the panel squeezes for the same reason as Send (below).

## Decision

A shared session's terminal windows change size only for the operator's own
act: opening or putting away the panel (a path he clicks, Send or an item's card
opens it), and dragging an edge. What opens beside the windows makes room for
itself, so the windows squeeze rather than being covered. A page showing, the
panel's content changing, and Peek never resize them. A resize settles before it
reaches tmux, so a sliding panel sends one. `DESIGN.md` (Never disturb the work)
and `ARCHITECTURE.md` (App pages, Window sizes) state the rule.

## What it replaced

The rule flipped from "nothing resizes" to "the windows squeeze" in steps:

- 2026-11-11, 693515b3 (ahjo-j0f.16): Send reserved space beside the terminal.
  It had opened over the terminal it sent to and hidden its last lines.
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
- 11-19, d22c42cc: showing and hiding the Files edge panel resizes them too.
- 11-19, 81b2af1e: Send makes room, "since one writes to sessions with it while
  reading them".
- 11-21, ef411da1: the rewritten DESIGN.md said again that "nothing opened
  beside a terminal resizes it". ARCHITECTURE.md already let panels and Send
  resize.
- 11-23, f9ed9583: a path clicked in a terminal opens the Files panel, and the
  session's window narrows.
- 11-23, 6d9c62dc: everything beside a page moved into the one panel (ADR
  0001), and DESIGN.md became "makes room for itself (the windows squeeze)".

**Why the whole panel squeezes** (the operator, 2026-11-23): what the panel
shows is something needed while working in a terminal. Covering the windows
would mean it cannot be open while the terminal is used, so the windows
squeeze. That outweighs the reflow for other clients (8afca65a, ff649000),
which happens only when the operator opens or puts away the panel.
