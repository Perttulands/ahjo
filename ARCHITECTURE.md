# Architecture

This repository holds Ahjo's three apps. Ahjo Swarm is two binaries: `ahjo` is
the node and the CLI, and `ahjo-swarm` is the native app; it talks to a node
only over the node's WebSocket and HTTP interface.

Apps with one purpose of their own are separate binaries on the same app kit,
so they share Swarm's look but not its shell. `ahjo-brain` is Ahjo Brain, the
company brain as a night sky with its search, reader and editor. It reads the
brain's files directly and writes a page only through Context Citadel's write
route, the brain's one writer, which runs `brain check` and commits; a page
another writer changed meanwhile is not written over unasked. A note caught
in it (and the note for each day) goes the same way into `knowledge/_inbox`,
its header filled in for it, where agents file inbox notes into the brain;
which they filed, and where to, the app reads from the brain's git history
when its inbox is opened. Another
machine's brain (one of Swarm's places, or `--from <node>`) is read through
that machine's node, whose brain area lists its page files and hands their
texts; the app keeps a copy in its cache, takes only what changed each time
its window comes forward, and reads the copy as a brain, read only, its
search asked of that node. `ahjo-desk` is Ahjo
Desk, the surface where agents publish and the operator answers in his words
and marks: it talks only to the decision desks' HTTP API (`/srv/desk`), never
to a node, keeps marks in the desk's one format, and decodes H.264 itself so
clips step by the frame with nothing installed. A published page renders inside
its window only on Windows, in WebView2 (`wry`) laid over the surface as a
child window; on Linux a system web view would need GTK's loop, which a GPUI
window does not run, so there the page opens in the browser.

```
areas/<area>/
    contract/   the area's wire types: requests, events, errors. No I/O.
    node/       the node side: state, routes, commands
    app/        the app side: store and views
kit/
    core/       ids (node + local id), the error shape, the frame envelope
    node-kit/   what a node area plugs into: events, config, registration
    app-kit/    what an app area plugs into: tokens, widgets (fields, a code editor),
                node link, Host and Page
    app-shell/  the app's shell: window chrome, tab bar, keys, view control
    testkit/    starts a node holding only the areas a test names
bin/
    ahjo/        the node and CLI: a list of areas, install and upgrade
    ahjo-swarm/  Swarm's native app: the shell in a window, and its list of pages
    ahjo-brain/  Ahjo Brain: the brain's sky, search and reader
    ahjo-desk/   Ahjo Desk: agents' asks answered in words and marks, over the desks' API
engine/         terminal engine, session holder, issue references, mission model and engine
```

Each directory under an area, `kit/`, `bin/` and `engine/` is one crate. An
app's package is `ahjo-app-<name>` (`ahjo-app-swarm`, `ahjo-app-brain`,
`ahjo-app-desk`), since `ahjo-brain` is the brain area's node crate; its
binary is the name the operator runs. The areas are terminals, files, issue,
brain, host, scheduled, harness and missions; fleet becomes one after the first
release. The brain area's only part in Swarm is its Alt+L Brain in the
window's one panel, beside the terminals, from a page without a tab: Ahjo Brain is where the brain is read at
length and edited, and its node side serves the `ahjo brain` commands and the
panel. Launching sessions and agent events live in terminals. Settings is not
an area: the shell's Settings page lists what pages declare.

## Words

The words and what they mean are in `GLOSSARY.md`. In code, a word keeps its
name from the CLI to the stored file to the screen; only the casing changes
(`issue_id`, `--issue-id`). New code, schema fields and command names use only
glossary words. An old word left in the code is renamed everywhere the next
time that code is touched.

Where the screen words live in code: the panel is `Host::show_panel` with a
`PanelView`, a side section is `Host::lend_side` with its `SideOn`, a popup is
`Host::popup`.

## Rules the compiler holds

1. **One contract per area.** Both sides of an area use the same `contract`
   crate. Standalone apps that use an area's data share that contract, so a
   changed field fails the build for every caller.
2. **Areas register themselves.** A node area hands `node-kit` its routes, its
   client actions and its commands; one command definition yields the CLI verb
   and the MCP tool. An app area hands `app-kit` its page, which declares its
   frame kinds, channels, the asks it answers and its chords.
   The binaries hold only the list of areas. Adding to an area touches that
   area's directory and nothing else; adding an area adds one line to each list
   and one dependency line to that binary's `Cargo.toml`; the workspace finds
   crates by where they sit, and CI by their names (`ahjo-<area>-app`).
3. **Dependencies point one way.** `contract` depends on `core` and on plain
   data in `engine/` that has no dependencies (`issue-ref`, an issue's
   reference, shared by the issue area and the mission engine). The one other
   is `mission-model`, the missions' wire data, which the mission engine uses
   and which depends only on `core`, serde's data formats and their schemas. `node` depends on
   its `contract` and `node-kit`; `app` on its `contract` and `app-kit`. An
   area that needs another uses that area's `contract` or an interface it
   provides, never its `node` or `app` crate (tests may). Standalone apps
   under `bin/` can use `app-kit` and area contracts. `kit/` and `engine/` use
   no area. Node crates never depend on the GUI toolkit (`gpui`, `gpui-pre`,
   `gpui-pre-platform`), app crates never on the web server (`axum`), and
   contracts on neither, on any target and under any rename; their tests
   may. `bin/ahjo/tests/layout.rs` holds this.

## Node areas

A node area is a `Listing` in `bin/ahjo`'s `AREAS`: its name, frame kinds,
command names and build function, known without building it. Built from the
node's `Setup` (its identity, folder, address, config and event log), it
answers with an `Area`: HTTP routes, commands, a channel for each `/ws` client,
and a stop. Before building, `node-kit` refuses listings that share a name, a
frame kind or a command, naming both, or that take a kind `core` reserves
(the hello, the fleet's peer list, view control); a built area whose commands
are not the ones listed is refused too. `bin/ahjo/tests/areas.rs` checks the
real list, and `ahjo kinds` prints who owns which kinds.

- **Frames.** The first byte of a `/ws` frame is its kind. A client's frame
  goes to the channel of the area owning its kind; each channel pushes its own.
- **Commands.** A command is a `noun_verb` name (the verb may be several words: `harness_edit_hook` is `ahjo harness edit-hook`), a typed request and
  response, and a handler answering the response or the error shape. The
  request supplies the description and input schema, and the response supplies
  the output schema. `node-kit` serves it as
  an MCP tool and as the CLI verb `ahjo noun verb`. A `node` argument names
  where it runs; MCP's lists leave it out while only this node can. Each
  command declares its effect (reads, writes, sets, destroys), which its
  tool carries as MCP annotations: a client such as ChatGPT asks the
  operator before every tool not marked read-only, so a read must say so.
- **MCP servers.** Each area's tools are an MCP server of their own at
  `/mcp/<area>`, and the node's own commands at `/mcp/node`; `/mcp/a,b`
  serves several and `/mcp` every one. A client connects to what it needs and
  pays only for those tools: the missions area alone is about half of every
  tool's schema, and an agent reading the brain need not carry it. A tool
  asked of a server that does not have it is refused naming the one that
  does. `ahjo mcp --area <area>`, again for more, is the same over stdio. The
  command line reads `/mcp`, so it sees every command in one request.
- **Tool sets.** A binary may name tool sets (`ahjo`'s `TOOL_SETS`), each an
  MCP server at `/mcp/<set>` of chosen commands from any area, shaped for one
  client: `chatgpt` holds the handoff, issue and host tools. A set may add
  `swarm_search`, which finds every other command by words, and
  `swarm_read` and `swarm_run`, which run one through the same call and
  `node` check as `tools/call`, each only for its kind: a read never needs
  confirming, a change always does. So the client carries a dozen tools'
  schemas and still reaches all of them. The three exist only in a set.
- **One node at a time.** A request names its node with `?node=` on any route
  and on `/ws`, a command call with its `node` argument. Relaying to other
  nodes is not built: one naming another node is refused as `unknown_node`
  (HTTP 421), never run here.
- **Config.** `config.toml` in the node's folder has a `[node]` section and one
  section per area, read by that area. `enabled = false` leaves an area unbuilt
  and listed as off in `/api/health`; a section no area reads refuses to start.
  `ahjo config show [--section]` answers the file's
  path and each section as the node loaded it, and the Settings page shows
  the connected node's read-only; an agent edits the file and a restart
  applies it.
- **The node's own commands.** Every node has a few commands whatever areas
  it serves (`node-kit`'s `own::COMMANDS`: `config_show`, `view_show`,
  `view_open`, `view_send`); a listing that names one is refused, as is an area named
  `node`, the name of the node's own config section and MCP server.
- **View control.** The node keeps the view-control kinds itself: a client
  that reports the page it shows (0xb0) is an app until it goes. `ahjo view
  show` lists what each app shows; `ahjo view open --page <id> [--path|item|
  run|mission|session|setting|service|agent <id>] [--step]` sends the apps a view command
  (0xb8) and answers the first answer (0xb1): `no_app` with none connected,
  `timeout` after 5 s. The app shows the page and hands it the thing as an
  `Ask::Reveal`, on this node. A page Swarm no longer has (`brain`) is
  refused by the node itself as `no_page`, saying where it went. `ahjo view
  send --text <text>` is how another app reaches Swarm's Send (Ahjo Brain's
  Send hands `brain page <file>`): the app brings forward the page that
  answers `Ask::Send` and hands it the text, so the operator picks the
  session in Swarm's Send to Session.
- **Provided interfaces.** What one area offers others is a narrow trait that
  lives with its provider's contract or engine crate, never in `node-kit`. The
  provider fills the interface keyed by that trait (`setup.provided.provide`) while
  it is built; a reader answers `Built::then` and reads the interface once every
  area is built, so the order of `AREAS` never matters. An interface filled twice is
  refused, and an empty one reads as `not_provided`. `Sessions` (in
  `engine/term`: list, follow, deliver to, start, and run a mission's seats)
  is the first; terminals provides it. `Resolver` (in `areas/issue/contract`:
  the canonical reference for a written one, a bare Bead id placed by the
  node's issue configuration) is provided by the issue area.
- **Hooks.** `ahjo hook <url> [--then command...]` is how an outside
  program's own hook reports to the node: it posts its payload to the URL an
  area gave it and runs `--then` regardless. The URL carries what the area
  needs, such as the session and event.

## App pages

A page is a build function in `bin/ahjo-swarm`'s `PAGES`, held by the shell in
`kit/app-shell`. Built from a `Host`, it answers with one entity implementing
`app-kit`'s `Page`: an id, a title, whether it has a tab (one without is
never shown and lives by its chords and what it lends), the node area it needs (its tab hides while
every node has that area off), its frame kinds, the channels it turns on while
shown, the asks it answers and its chords. The shell owns the window, the tab
bar, the dismissal stack, the leader, the status line and the node connection,
and refuses pages that share an id or a frame kind, take a kind `core`
reserves, or whose `everywhere` chords share a key, naming both;
`bin/ahjo-swarm/tests/pages.rs` mounts the real list.

- **Host, Hub and AppFrame.** A `Host` is a cheap handle on the `Hub`, the
  state the shell and every page share: the fleet link and the settings in
  `app.json`, beside the window's `AppFrame` (`Hub::frame`). The frame is
  the half with no node behind it: the stack of popups, menus and layers,
  the status line and the toast, and the functions that draw them and work
  their keys. Ahjo Brain and Ahjo Desk each hold an `AppFrame` of their own
  and no hub, so a gesture behaves the same in all three. Through the Host a page lists live nodes and where
  each device is, makes HTTP requests, sends frames, turns channels on,
  reports, opens popups, menus and layers, posts desktop notifications (the shell
  owns the one click handler and runs the page's click), redials, keeps its
  section of `app.json`, and asks.
- **Taps.** A page whose frames are too many for the UI thread (terminal
  screens) registers a tap: the network thread hands it every frame of the
  page's own kinds first, and the page wakes itself at most once per paint.
- **Events.** The hub never calls a page. A frame of a page's kinds, a link
  to a node opening or going, an ask it answers or a closed popup's or
  layer's dismiss reaches the shell as a hub event after the update that caused it, so a page
  may use its Host inside its own update.
- **Tabs and chords.** A page may draw its own part of the tab bar (the
  Sessions page, the terminals area's, draws its workspace tabs there). A chord marked `everywhere`
  works from every page, as CHROTE's terminal chords do, and brings its page
  forward when it acts.
- **Asks.** Pages never depend on each other. An `Ask` (send a reference, open
  a path, reveal a thing, launch a session in a folder on a node) is plain data that goes to the page declaring it
  answers it; when none does, the asker gets an `unanswered` error and the
  status line says so. View control's `open` reaches pages the same way. A
  beside ask (Send, open a path beside, or a reveal marked `beside`, as an
  issue named in a terminal opens its card) leaves the asker shown: the
  answering page shows its answer in the window's panel (below).
- **Side sections.** The column at the window's left is the shell's, on
  every page: each page may lend it one side section (`Host::lend_side`), and the
  operator puts it away to a narrow edge and back (`Host::set_side_open`,
  kept in `app.json`). Sessions lends its rail, on every page. On the
  Sessions page that column is the rail that was always there; on other
  pages it narrows the page. A lent section keeps its page's channels on
  while the column is open, and acts through its page (a session goes to
  Sessions), so a page's own lists stay its own. A section says where it
  shows (`SideOn`).
- **The panel.** Everything a page opens beside another, asked or not,
  stands in the window's one panel, the shell's, at the right edge unless
  the shell's setting says left: a file (Files, in one column, its list or
  the file opened from it), an item's card, Issues' work (Alt+B), the
  Brain (Alt+L), Send, the output search, a diagram, a mission's open card.
  A page shows its content there with `Host::show_panel`, a `PanelView`
  that says its head; the shell draws the one header around it (Back
  where it applies, the head, its close control) and its edge. One content shows at a
  time; what it opened over waits under it, and Back, or its page taking
  it out (`Host::close_panel`, as a send that arrived does), brings it
  back; content opened while the panel is put away starts it over, so
  Back leads only to what showed. Content says where it stands (`SideOn`): Files not on Files, which
  holds the same, a mission's card only on Missions. Its ×, Escape with
  the keyboard in it, Ctrl+B and the content's own chord put the panel
  away, once the content lets it (`PanelView::may_close`: unsaved edits are
  asked about first); Ctrl+B brings it back. It never holds the keyboard:
  a click outside it takes the keyboard away and leaves it open, and when
  it goes with the keyboard in it the keyboard goes to the page. Ctrl+B is
  the shell's only when what has the keyboard did not take it: a terminal
  always takes it (tmux's prefix, a harness's background key, readline), so
  each content's chord is the way from a terminal. While a page's content
  shows, that page's channels stay on. The width dragged at its inner edge
  is kept in `app.json`; whether it is open is not. Every edge dragged to
  size what it edges (the panel, Issues' list) is `kit::grip`: one hit
  width, the release caught by the window rather than a cover over it,
  and a double click back to the first width. The panel's column (head,
  Back, ×, grip, slide) is `panel::EdgeColumn`, which Desk's list of
  asks uses too. Only Peek still opens
  over the page, a glance that any click outside closes. Why one panel:
  [ADR 0001](docs/adr/0001-one-panel-beside-the-page.md).
- **Popups.** Every menu, confirm and small form a page opens over itself
  is a popup on the dismissal stack (`Host::popup`, `app-kit`'s `popup`),
  and the shell does everything but its content, the same on every page.
  It stands beside what was clicked (`Spot`: the pressed button, else the
  point), below it and turned to the side with room, flipped over it and
  kept inside the window by one function (`popup::beside`), and never
  larger than the window. Escape closes the top one only; a press outside
  closes it (`Outside::Close`) or leaves it (`Outside::Keep`, a form
  holding what was typed), and reaches nothing under it either way. A
  popup may say not now (`Popup::may_close`: a start not yet answered, a
  draft), its page closing it is never asked. It takes the keyboard into
  its declared focus, and when it closes with the keyboard in it the
  keyboard goes back where it was when it opened, a terminal included, or
  to the page when that was only the window. The shell draws its frame
  and, at the frame's top right, the close control every surface shows
  (`kit::close`): a click on it closes the popup as Escape does, asking
  `may_close`. Its content draws no frame or close of its own. A menu is a popup of rows
  (`MenuRow`) that the shell draws and steps through with the keys;
  anything else opening closes it. A danger confirm is a one-row menu,
  already armed (`Host::confirm`). Its content is a view; a page draws it
  through `popup::drawn`, which redraws with the page. A form in one is
  `popup::form`: its fields scroll when the window is short and its
  actions stand under them, always in view. What opens in place
  stays the page's (a rename in a head or a tab, a mission's note paper,
  the files palette). Only a decision about the whole window is centred
  (the close guard), per DESIGN.md's Act where it is. Layers are the
  stack's other kind: Overview and Peek, which cover the page, and a
  rename in place while it holds the keyboard.
- **Renames in place.** Every name renamed where it is shown is an
  `InlineEdit`, with one rule: Enter and the keyboard going elsewhere keep
  what was typed, Escape puts the name back. It tells its page once,
  `Committed(name)` or `Reverted`; a name the page or node refuses goes on
  being edited (`resume`) beside the reason.
- **Scroll areas.** Use `pane::scroll` for a column, `scroll_x` for a
  strip and `scroll_both` for a picture or board. Each draws its own
  thumb and keeps its place unless the page holds its handle. For long
  lists of equal-height rows, `pane::long_list` renders the visible range
  and draws the same thumb. A scroll area built by hand as a row
  stretched its content to its own height, so nothing past it was
  measured (Desk's document did not scroll); `bin/ahjo/tests/layout.rs`
  refuses one outside `pane.rs`.
- **Window sizes.** A shared session's terminal windows change size only for
  the operator's own act: opening the panel or putting it away (as a path
  he clicks, Send or an item's card opens it), dragging an edge. What
  opens beside the windows makes room for itself rather than covering
  them. A page showing, the panel's content changing, or Peek never
  resizes them. Resizes settle before they reach tmux, so a sliding panel
  sends one.
- **Unsaved work.** The shell owns the window's one close handler. A page
  guards itself, or its content in the panel, with
  `Host::guard_close`, which names what that view holds unsaved; while any
  guarded view names something, closing the window brings its page forward
  and asks over the window whether to leave it or keep editing. A view that
  is gone guards nothing.
- **Settings.** A page declares its settings beside it (`Page::SETTINGS`: key,
  label, control, default). The shell's Settings page lists the shell's own
  (node, keys) and each page's under its title, keeps a change under its key in
  the page's section of `app.json`, and tells the page
  (`Page::settings_changed`). A key declared twice refuses to start. The app
  watches `app.json` (`Hub::watch`): an edit made outside it, as an agent
  makes, applies at once and each page whose section changed hears
  `settings_changed`; the app's own writes are not edits, and a file that
  does not read is reported while the last good settings stay. That file
  is a `SettingsFile`, the kit's, as Desk's `desk.json` and Brain's
  `brain.json` are: one way to read, write, set aside a broken file and
  pick up an outside edit, whichever app holds it (`KeepsSettings`).
- **Read models.** What one page knows that others show (the Sessions page's session
  list) is a plain type in its area's `contract`, published by that page
  (`Host::publish`) and read and observed by any page (`Host::model`), built in
  any order. A second publisher is refused.
- **Tests.** An app area's test drives `ahjo-app-shell` with its own page, or
  pages it defines, against a testkit or fake node, building no other area.

## Kit

Every interaction has one element in the kit, the same in Swarm, Brain and
Desk. Build with it; where it falls short, extend it in `kit/`. How the
popups, the panel, renames, scroll areas and settings behave is said once,
above (App pages); this is where to find each. Paths are under
`kit/app-kit/src` (`ahjo_app_kit`); `kit::` is `widgets::kit`, `pane::`
is `widgets::pane`.

| Element | Use it for | Where |
| --- | --- | --- |
| Popup | anything small opened over the page beside what was clicked: `Host::popup(Popup::beside(spot, view))`, its `Outside`, `focus`, `may_close` | `popup`, `Host::popup` |
| Form in a popup | a small form whose fields scroll and whose actions stay in view; a page draws it with `popup::drawn` | `popup::form` |
| Menu | a list of acts, with keys, rules, submenus and refused rows (`MenuRow`) | `Host::menu`, `menu_keyed`, `menu_armed`; `menu` |
| Confirm | a danger asked about before it runs: one armed row beside the button | `Host::confirm` |
| Layer | a page-wide surface over the page or window (Overview, Peek); `keeps_escape` when Escape is its content's. Its shape says how it stands: covering its room and drawing its own head (`Shape::Cover`, the default), in a card the shell draws at the top or middle of the room (`Layer::card`, `middle`, `closes_outside`), or an edit in place such as a rename (`Layer::in_place`). `takes_drops` takes files dropped anywhere in its room; a card that grows with its content asks `frame::card_tallest` how tall it may grow | `Host::layer`, `frame::Layer`, `frame::Shape` |
| Close control | the small × every surface that opens shows, its tip saying Close (ADR 0002). The shell draws it on popups, on layers (at a card's top right, or at the top right of a covering layer's room; an edit in place has none), on the panel and on the status line; what opens in place in a page (Brain's reading sheet, a find bar, Desk's pen) puts it at its head wired to close itself. A toast, which fades and takes no action, has none. A × that takes something away without closing a surface is `kit::cross` with its own tip | `kit::close`, `kit::cross` |
| Escape and the stack | Escape closes only the top popup, menu or layer; an app's own Escape is for its own modes, when the stack is empty | `frame::key`, `frame::dismiss_top` |
| Panel | content opened beside the page (a file, a card, Send): a `PanelView` with its head, Back and `may_close` | `Host::show_panel`, `panel::Panel`, `panel::PanelView` |
| Edge column | a column at the window's edge in an app without the shell (Desk's asks): head, ×, grip, slide | `panel::EdgeColumn` |
| Side section | a page's part of the shell's left column, shown on the pages its `SideOn` names | `Host::lend_side` |
| Grip | an edge dragged to size what it edges; double click puts the first width back | `kit::grip` |
| Scroll area | content scrolling down, across, or both (a picture, a board) | `pane::scroll`, `pane::scroll_x`, `pane::scroll_both` |
| Long list | a scrolling list of equal-height rows, rendered as they come into view | `pane::long_list` |
| Page skeleton | a list beside the thing, its heads, rows, tables, lines, empty states and Send | `pane::page`, `list`, `detail`, `row`, `table`, `empty`, `send` |
| Text field | any typed text, one line or a wrapping note, with undo | `widgets::field::TextField` (`widgets::undo`) |
| Code | a file's text: line numbers, syntax, find, edits when asked | `widgets::code::Code` |
| Rename in place | a name renamed where it is shown | `widgets::inline_edit::InlineEdit` |
| Search | every filter of a list, forgiving a slipped letter, marking what matched | `text::fuzzy::filter`, `fuzzy::matches` |
| Finder | picking a thing from a list by typing, keyboard first, one or several | `widgets::finder::Finder` |
| Notices | the status line and the toast: `Host::report`, `Host::toast`; drawn over the page, or in the flow where an app gives them a place | `frame::notices`, `frame::notices_in` |
| Settings file | an app's JSON settings: read, written, a broken file set aside, an outside edit picked up | `settings_file::SettingsFile`, `KeepsSettings` |
| Page settings | a setting shown on Swarm's Settings page | `Page::SETTINGS`, `declared::Setting` |
| Unsaved work | a view that holds edits not yet kept, asked about when the window closes | `Host::guard_close` |
| Theme picker | choosing the theme, in every app | `widgets::theme_picker::button`, `open` |
| Tip | a hover explanation: `.tooltip(kit::tip(why))` | `kit::tip` |
| Buttons and toggles | an act (`Kind::Primary`, `Plain`, `Quiet`), a chosen option, a switch; a quiet act in a head | `kit::button`, `kit::option`, `kit::toggle`, `pane::act` |
| Tabs | a bar of tabs with the shown one marked | `kit::tab_bar`, `kit::tab`, `kit::tab_label` |
| Dragged file | a file on a node dragged from one page onto another | `drag::NodeFile` |
| File picking | the desktop's picker, with a typed path where it has none | `widgets::path_picker::pick`, `pick_folder` |
| Markdown | prose drawn in the theme, its links handed to the page | `widgets::markdown` |
| Selectable text | prose selected and copied across a column | `widgets::select::Selectable` |
| Tokens | every colour, size, corner, type size and motion | `theme` (`size`, `ty`, `board`, `fade`, `motion`), and a page's own tokens beside it |
| Window evidence | what a report attaches: the window's own pixels (`PrintWindow` on Windows, the X window's image on X11; Wayland says it cannot), and its page, panel, open surfaces, builds and size | `Host::capture` (`capture::Picture`; the binary names how, as Swarm's `capture` does), `Host::window_state` |
| AppFrame and Hub | `AppFrame` is a window's stack, status and toast, with no node; Swarm's `Hub` holds one beside the fleet and settings; Brain and Desk hold an `AppFrame` alone (App pages, Host, Hub and AppFrame) | `frame::AppFrame`, `hub::Hub` |

`bin/ahjo/tests/layout.rs` refuses the hand-rolled forms outside the kit:
a scroll area or scrollbar of a page's own, a popup placed with
`anchored`, something drawn over the page with `deferred`, a press outside
caught with `on_mouse_down_out`, a × or Close button of a page's own, and a type size, corner or colour written
as a number. Its failure names the element to use. What it allows is
listed in it, each with its reason, and only shrinks.

## One repository

Swarm, Brain and Desk live in one repository. They share the app kit (theme,
fonts, art, widgets), which changed in 9 of the 29 commits before this section
was written. Four commits in one week changed the kit and a standalone app
together. In separate
repositories each of those would be a release, a pin bump and three CI runs.
One repository gives the three one theme, one check, one install procedure and
one Windows build checkout.

The app kit and area contracts give standalone apps their shared UI and wire
types. For example, Brain uses the Brain contract to read a remote node's
corpus. A standalone app takes from the kit what Swarm's shell does, short of
the node: its `AppFrame` (popups, menus, layers, status, toast, and Escape
closing only the top one), its settings as a `SettingsFile`, the theme
picker (`widgets::theme_picker`, which in Brain and Desk can follow Swarm's
theme), the edge column and grip, and its errors as `ahjo_core::Error`. An
app keeps its own Escape for its own modes (Desk's drawing, Brain's open
page) only for when the frame's stack is empty. The build holds this boundary (`bin/ahjo/tests/layout.rs`). Revisit
this when a product gets users outside this machine with its own release
rhythm, or when the kit has been stable long enough to version.

## The app's zoom

The app's zoom (`zoom::app`, the `size` setting) is the window's own scale
in GPUI, which `vendor/gpui-pre` adds to the release this repository pins
(`Window::set_ui_scale`, marked `Ahjo:` in its `window.rs` and
`platform.rs`): the display's scale factor is multiplied by it and the
viewport and every input position divided, as a browser's zoom does. Every
measure (tokens, a page's own pixels, what a canvas draws, a measured text
width) grows together, so no view knows the zoom and a layout right at
100% is right at 130%. GPUI's rem size would scale only what is written in
rems, and the apps size everything in pixels, much of it in arithmetic
with measured widths. Raising `gpui-pre` means applying the same few lines
to the new release.

## Why this shape

Work on Ahjo is done by many agents at once. The shape keeps one piece of work
inside one directory, keeps a build to the area being changed, and turns the
boundaries from a convention into something the build refuses to break.

## Errors

One shape everywhere: a stable `code` and a human `message`, defined in `core`.
Each area owns its codes in its `contract`. Routes, commands and the app all
carry that shape; nothing returns a bare string.

The command line says how a command ended in its exit code: 0 done, 1 it ran
and failed, 2 the command line was wrong (`bad_arguments`), 3 no node answers
(`no_node`). With `--json` the answer is JSON on stdout and a failure the
error shape on stderr, as `{"error": {code, message}}`.

## Sessions survive

Terminal sessions belong to the holder process, not to the node. Installing,
upgrading or restarting Ahjo never stops a session.
tmux records a session as active when a node attaches a control client; Ahjo preserves the pane's earlier output time until it writes again.

## Install and upgrade

`ahjo install`, `ahjo upgrade` and `ahjo uninstall` are the binary's own verbs, not an area's
commands: they put the binary itself in place and restart the node, so no
node can own them. They have no MCP tool, because a node upgrading itself
would be stopped by its own restart before it could answer. On Linux and WSL
the node is a systemd user unit (`ahjo` unless `--unit` names another) that
runs `<root>/bin/ahjo serve --listen <addr>`, with `KillMode=process` so a
restart signals only the node and a tmux server it started lives on.

- **Install** copies the build (and `--app`) to `<root>/bin` (`~/.local` by
  default), writes the unit, enables and starts it, and waits for
  `/api/health` to answer as the build's version. Run again with nothing
  changed, it changes nothing. A running unit with another build is
  refused: that is an upgrade.
- **Upgrade** reads the binary and address from the unit's `ExecStart`,
  sets the installed build aside, puts the new one in its place and
  restarts the unit. If the new node is not healthy in time, it puts the
  build it set aside back, restarts it and fails with `upgrade_failed`;
  once the new node is healthy the old build is deleted.
  A unit without `KillMode=process` is refused before anything changes.
- **Uninstall** stops and disables the unit and removes it and the
  binaries its `ExecStart` names. Sessions, and the node's config and
  state, stay.
- **Version.** A build's version is the package version and its commit
  (`0.0.1+8cdb58b`), in `ahjo --version`, `ahjo version` and the health.
- **The address.** A node records the address it bound in its config folder
  (`listen`), so `ahjo` commands on the same machine reach it with no
  `--node`.
- **Apps for other machines.** A node hands out the Windows builds of
  Swarm, Brain and Desk made from its own commit, so another machine on
  the tailnet installs them from it and stays in step with it; nothing is
  published. `ahjo upgrade --windows-apps <dir>` checks that each carries
  the new build's version, puts them in `<root>/apps/windows/<version>/`
  with a manifest of sizes and SHA-256s, and removes every other
  version's. The node serves the manifest at `/apps/<platform>`, each file
  it names, and `/install.ps1`, a script pointed at the node as the
  request named it and at `[node] desk`. `irm <node>/install.ps1 | iex`
  puts the apps in `%LOCALAPPDATA%\Ahjo` with `install.json` naming the
  node, sets Swarm's node and Desk's first desk, and adds Start-menu
  entries; no administrator rights. An app with `install.json` beside it
  asks the node at start, when its window comes forward and (Swarm) when
  its link opens or is refused; when the node's build is not its own it
  shows Update in its chrome. Update fetches every file, checks each,
  renames the running files aside (Windows lets a running program's file
  be renamed, not replaced), puts the new ones in place and restarts the
  app with its arguments, after Swarm's unsaved-work guard; a node that
  refused the app's protocol says so with Update as its action. The
  operator's own apps have no `install.json` and never update this way.

`deploy/` is the operator's own machine: `ahjo.service` as
`ahjo install --root ~/.local/share/ahjo` wrote it there (listening on
127.0.0.1:7310, which Windows Tailscale serves as tailnet port 7300), linked
into `~/.config/systemd/user`, and the ChatGPT tunnel's unit and profile,
which reach the node's `/mcp/chatgpt` tool set. The `change-ahjo` skill has the steps.

`bin/ahjo/tests/install.rs` runs all three against a stand-in `systemctl` on
`PATH`, with a node, a tmux server and a session of the test's own.

## Tests

An area's tests live in its crates and run against `testkit`, which starts a
node with only the named areas on a port and tmux server of its own. `bin/ahjo`
holds the few tests that need the whole binary.
