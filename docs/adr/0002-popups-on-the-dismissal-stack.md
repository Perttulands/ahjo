# Popups on the dismissal stack, beside what was clicked; Escape closes the top one

Every menu, confirm and small form that a page opens over itself is a `Popup` on
the shell's one dismissal stack. The shell does everything except its content.
It places the popup beside what was clicked and keeps it inside the window
(`popup::beside`). Escape closes only the top surface on the stack. A press
outside either closes the popup or leaves it, as the popup declares (`Outside`),
and it reaches nothing underneath either way. A popup may refuse to close
(`may_close`). The keyboard goes back to where it was when the popup opened.
What opens in place, such as a rename, stays the page's. Only a decision about
the whole window is centred. `ARCHITECTURE.md` (App pages, Popups) holds the
details.

**Why.**

- **Placement.** The rule comes from ahjo-hmw (2026-11-23). A rename opened as
  a centred scrim near the top of the window, not where it was asked for. The
  bead gives the reason for the change: the eye and hand stay at the thing, and
  the place says what it acts on, so no title is needed. The popup covers
  neighbours, not the work, and a small action feels small. DESIGN.md's "Act
  where it is" says this.
- **The stack.** ahjo-83o.2 and its epic's audit (ahjo-83o) found that the
  first anchored popup, `widgets::popover`, was not on the stack. It left
  Escape, outside clicks and focus to each page, and each page rolled its own:
  Missions had five popups with their own Escape ladder, Terminals mirrored the
  stack in `surfaces`, and the theme picker was separate. With one owner, the
  same gesture behaves the same on every page and in Brain and Desk through
  `AppFrame`.

**What it replaced.**

- The stack itself is older. The shell has owned one dismissal stack of menus
  and layers since 4a79cf95 (2026-11-10), with Escape closing its top
  (`Hub::dismiss`). Popups grew outside it.
- ef851ee0 (11-23 09:42, ahjo-hmw) moved renames into place and small forms
  beside the click through `widgets::popover`, which was still off the stack.
- Six hours later e6e53c2d (11-23 15:59, ahjo-83o.2) deleted `popover()` and
  put every popup on the stack. It also removed Missions' Escape ladder except
  for what opens in place, Terminals' `surfaces` mirror, and the centred 900x650
  scrim for attempts.
- a0b65e5c (11-23 17:08) added `popup::form`, so a popup form keeps its actions
  in view when the window is short. Before, New mission hid Create at 1366x768.

**Not recorded.** No source says why Escape closes only the top surface rather
than everything open, or why a press outside never reaches the page underneath.
4a79cf95 and e6e53c2d state both rules without a reason.
