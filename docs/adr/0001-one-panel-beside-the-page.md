# One panel for everything opened beside a page

Everything a page opens beside another page, whether asked to or not, stands in
the window's one panel. The shell draws that panel at the right edge, or at the
left if its setting says so. Pages show content in it with `Host::show_panel`, a
`PanelView`. The panel holds one content at a time, with Back to what that
content opened over. It never holds the keyboard. Only Peek still opens over the
page. The shell's left column, where pages lend side sections
(`Host::lend_side`), is a separate thing and stays. `ARCHITECTURE.md` (App
pages, The panel) says how the panel behaves.

**Why.** There were four kinds of side surface: a view over the terminals, a
drawer that held the keyboard, a squeezing Files panel and a page's own column.
Each had its own look, keys and bugs. One panel has one of each. Two duplicates
in the history show the cost. In 0c950913 the left column's Files section listed
folders itself, beside the Files page's own tree. In f9ed9583 a path clicked in
a terminal opened a second Files over the windows, while the edge panel showed a
tree with no menu and no New, so no file act worked there.

**What it replaced.** Between 2026-11-10 and 2026-11-23, five mechanisms each
showed something beside a page, and each was replaced:

- The dock column, `Host::show_dock` (40321d81, 11-10), deleted in ea9c59a0
  (11-14).
- Beside views, `Host::show_beside`, drawn by the page that asked (584d61f2,
  11-10). They were made resizable (900bbaa4, 11-13) and then laid over the
  terminal windows (8afca65a, 11-13). Work's and Brain's views moved onto them
  (ea9c59a0, 11-14; e3440b37, 11-19).
- Send and the output search as drawers, first over the windows (ff649000,
  11-13), then beside them (81b2af1e, 11-19).
- The Files edge panel, `Host::lend_panel`, toggled with Ctrl+B (d22c42cc,
  11-19), which became the one Files panel (f9ed9583, 11-23).
- Page-own columns: the Missions board's side panel (0db6e71e, 11-21) and
  `SideOn::OwnPage` with `Host::take_panel` (7456a6c7, 11-23), with the gate
  panel built on them (c4eda3cc).

6d9c62dc (authored 11-23 12:21, committed 14:59) removed all of them whole:
`lend_panel`, `take_panel`, `show_beside`, the beside overlay, the Send drawer
and the per-area panels. It also added `kit::grip` as the one edge drag. Its
width was set to 480 in 6b31fa25 (ahjo-egr).

**Not recorded.** No source gives a reason why Peek alone stays over the page.
6d9c62dc says only "Only Peek stays over".
