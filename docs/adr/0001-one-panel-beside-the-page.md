# One panel for everything opened beside a page

For thirteen days, five mechanisms showed something beside a page, and each had
its own look, keys and bugs. Everything a page opens beside another now stands
in the window's one panel at its edge, so there is one look, one set of keys
and one place for bugs.

## Decision

Everything a page opens beside another page, whether asked to or not, stands in
the window's one panel. The shell draws the panel at the right edge, or at the
left if its setting says so. A page shows content there with `Host::show_panel`,
a `PanelView`. The panel holds one content at a time, with Back to what that
content opened over. It never holds the keyboard. Only Peek still opens over the
page. The shell's left column, where pages lend side sections
(`Host::lend_side`), is a separate thing and stays. `ARCHITECTURE.md` (App
pages, The panel) says how the panel behaves.

## Why

Four kinds of side surface each had their own look, keys and bugs: a view over
the terminals, a drawer that held the keyboard, a squeezing Files panel and a
page's own column. One panel has one of each. Two duplicates in the history show
the cost. In 0c950913 the left column's Files section listed folders itself,
beside the Files page's own tree. In f9ed9583 a path clicked in a terminal
opened a second Files over the windows, while the edge panel showed a tree with
no menu and no New, so no file act worked there.

## What it replaced

Between 2026-11-10 and 2026-11-23:

- The dock column, `Host::show_dock` (40321d81, 11-10), deleted in ea9c59a0
  (11-14).
- Beside views, `Host::show_beside`, drawn by the page that asked (584d61f2,
  11-10). They were made resizable (900bbaa4, 11-13), then laid over the
  terminal windows (8afca65a, 11-13). Work's and Brain's views were moved onto
  them (ea9c59a0, 11-14; e3440b37, 11-19).
- Send and the output search as drawers over the windows (ff649000, 11-13). Send
  moved beside the windows in 81b2af1e (11-19). The output search stayed over
  them until 6d9c62dc.
- The Files edge panel, `Host::lend_panel`, toggled with Ctrl+B (d22c42cc,
  11-19), which became the one Files panel (f9ed9583, 11-23).
- Columns of a page's own: the Missions board's side panel (0db6e71e, 11-21),
  and `SideOn::OwnPage` with `Host::take_panel` (7456a6c7, 11-23), with the gate
  panel built on them (c4eda3cc).

6d9c62dc (written 11-23 12:21, committed 14:59) removed all of them whole and
put the output search in the panel too. It removed `lend_panel`, `take_panel`,
`show_beside`, the beside overlay, the Send drawer and the per-area panels, and
added `kit::grip` as the one edge drag. 6b31fa25 (ahjo-egr) set the panel's
first width to 480.

**Why Peek stays over the page** (the operator, 2026-11-23): Peek is a glance,
and over the page is right for a glance. Squeezing the windows earns its cost
only for something kept open while working in a terminal (ADR 0003).
