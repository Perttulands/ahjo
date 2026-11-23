# Popups on the dismissal stack, beside what was clicked; Escape closes the top one

Popups were placed, dismissed and focused differently on each page. Every menu,
confirm and small form is now a popup on the shell's one dismissal stack,
standing beside what was clicked. The shell does everything but its content, so
one gesture behaves the same on every page and in Brain and Desk.

## Decision

Every menu, confirm and small form that a page opens over itself is a `Popup` on
the dismissal stack. The shell places it beside what was clicked and keeps it
inside the window (`popup::beside`). What opens in place, such as a rename,
stays the page's. Only a decision about the whole window is centred.

- **Escape** closes the top surface on the stack. A layer that keeps Escape for
  its content (`Layer::keeps_escape`) does not close on it. Peek is that layer:
  plain Escape goes to the session's program, and Shift+Escape closes Peek.
- **The close control.** Every surface that opens shows the same small ×
  (`kit::close`, with a "Close" tip naming its keys), so Escape is not the
  only way to close it. The shell draws it on every popup (menus and
  confirms among them) at the frame's top right, on the panel's header and
  on the status line; a layer, and what opens in place, put it at their
  head. A click on it closes that surface as Escape does, a popup's
  `may_close` included, and the keyboard settles as below. No page draws a
  close button of its own; a × that takes something away without closing
  a surface is the same drawing (`kit::cross`) with its own tip.
- **A press outside** either closes the popup or leaves it, as the popup
  declares (`Outside`). Either way it reaches nothing underneath. A popup may
  refuse to close (`may_close`).
- **Focus** after popups close (`frame::settle`):
  - If a surface still on the stack takes the keyboard, it gets it.
  - Otherwise, if the keyboard was in a closed popup (or nowhere) and the
    popup's dismiss did not move it, it goes back where it was when the popup
    opened. If that was only the window, it goes to the page.
  - Otherwise, the keyboard stays where it already is.

`ARCHITECTURE.md` (App pages, Popups) and the Kit table hold the details.

## Why

- **Placement.** The reason is recorded on ahjo-hmw (2026-11-23), which found
  a rename opening as a centred scrim near the top of the window: the eye and
  hand stay at the thing, and the place says what it acts on, so no title is
  needed. The popup covers neighbours, not the work, and a small action feels
  small. DESIGN.md's "Act where it is" says this.
- **The stack.** ahjo-83o.2 and its epic's audit (ahjo-83o) found that the
  first anchored popup, `widgets::popover`, was not on the stack. It left
  Escape, outside clicks and focus to each page, and each page rolled its own.
  Missions had five popups with their own Escape ladder, Terminals mirrored the
  stack in `surfaces`, and the theme picker was separate.

## What it replaced

- The stack itself is older. The shell has owned one dismissal stack of menus
  and layers since 4a79cf95 (2026-11-10), with Escape closing its top
  (`Hub::dismiss`). Popups grew outside it.
- ef851ee0 (11-23 09:42, ahjo-hmw) moved renames into place, and small forms
  beside the click through `widgets::popover`, still off the stack.
- e6e53c2d (11-23 15:59, ahjo-83o.2) deleted `popover()` and put every popup on
  the stack. It cut Missions' Escape ladder down to what opens in place, and
  removed Terminals' `surfaces` mirror and the centred 900x650 scrim for
  attempts.
- a0b65e5c (11-23 17:08) added `popup::form`, so a popup form keeps its actions
  in view when the window is short. Before, New mission hid Create at 1366x768.

**Why Escape closes only the top** (the operator, 2026-11-23): closing closes
the thing the operator is trying to close, and nothing else. Escape also
matters to a terminal program, which is why a layer over a terminal (Peek)
keeps it; so Escape is not the only way to close: every surface that opens
also shows a small close button. Before ahjo-6yx (2026-11-24) the surfaces
that had one each drew their own: the word Close on the panel, Overview,
Peek and Missions' attempts, a font × on the status line (failures only)
and Missions' note paper, a drawn cross on Brain's reading sheet, Done on
Desk's settings; menus, confirms, the keys panel and most forms had none.

**Not recorded:** why a press outside never reaches the page underneath.
4a79cf95 and e6e53c2d state the rule without a reason.
