# Architecture

This repository holds Ahjo's three apps.

**Ahjo Swarm** is two programs (binaries):

- `ahjo` is the node and the command line (CLI).
- `ahjo-swarm` is the native app. It talks to a node only through the node's
  WebSocket and HTTP interface.

An app with a purpose of its own is a separate binary, built on the same app
kit. It looks like Swarm but does not use Swarm's shell. There are two.

**Ahjo Brain** (`ahjo-brain`) shows the company brain as a night sky, with its
search, reader and editor.

- It reads the brain's files directly.
- It writes a page only through Context Citadel's write route, the brain's one
  writer. That route runs `brain check` and commits. If another writer changed
  the page in the meantime, the app does not write over it unasked.
- A note caught in the app (and the note for each day) goes the same way, into
  `knowledge/_inbox`, with its header filled in. Agents file inbox notes into
  the brain. When the inbox is opened, the app reads from the brain's git
  history which notes they filed, and where to.
- It can read another machine's brain (one of Swarm's places, or
  `--from <node>`) through that machine's node. That node's brain area lists
  its page files and hands over their text. The app keeps a copy in its cache,
  and each time its window comes forward it fetches only what changed. It
  reads the copy as a brain, read only, and sends its searches to that node.

**Ahjo Desk** (`ahjo-desk`) is where agents publish and the operator answers in
his words and marks.

- It talks only to the decision desks' HTTP API (`/srv/desk`), never to a node.
- It keeps marks in the desk's one format.
- It decodes H.264 video itself, so clips step frame by frame with nothing
  installed.
- A published page renders inside its window only on Windows, in WebView2
  (`wry`), laid over the surface as a child window. On Linux a system web view
  would need GTK's loop, which a GPUI window does not run, so there the page
  opens in the browser.

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
app's package is named `ahjo-app-<name>` (`ahjo-app-swarm`, `ahjo-app-brain`,
`ahjo-app-desk`), because `ahjo-brain` is already the brain area's node crate.
Its binary has the name the operator runs.

The areas are terminals, files, issue, brain, host, scheduled, harness and
missions. Fleet becomes an area after the first release.

- The brain area's only part in Swarm is the Brain that Alt+L opens in the
  window's one panel, beside the terminals. It comes from a page without a
  tab. Ahjo Brain is where the brain is read at length and edited. The brain
  area's node side serves the `ahjo brain` commands and the panel.
- Launching sessions and agent events live in the terminals area.
- Settings is not an area: the shell's Settings page lists what the pages
  declare.

## Words

The words and what they mean are in `GLOSSARY.md`. In code, a word keeps its
name from the CLI to the stored file to the screen; only the casing changes
(`issue_id`, `--issue-id`). New code, schema fields and command names use only
glossary words. An old word left in the code is renamed everywhere the next
time that code is touched.

Where the screen words live in code: the panel is `Host::show_panel` with a
`PanelView`, a side section is `Host::lend_side` with its `SideOn`, and a popup
is `Host::popup`.

## Rules the compiler holds

The build fails when one of these rules is broken.

1. **One contract per area.** Both sides of an area use the same `contract`
   crate. Standalone apps that use an area's data use that same contract. So
   when a field changes, the build fails for every caller.
2. **Areas register themselves.** A node area hands `node-kit` its routes, its
   client actions and its commands. One command definition gives both the CLI
   verb and the MCP tool. An app area hands `app-kit` its page, which declares
   its frame kinds, channels, the asks it answers, and its chords.
   The binaries hold only the list of areas. So adding to an area touches only
   that area's directory. Adding a new area adds one line to each list and one
   dependency line to that binary's `Cargo.toml`. The workspace finds crates by
   where they sit, and CI finds them by their names (`ahjo-<area>-app`).
3. **Dependencies point one way.**
   - `contract` depends on `core` and on two crates in `engine/`. One is
     `issue-ref`, an issue's reference: plain data with no dependencies, shared
     by the issue area and the mission engine. The only other is
     `mission-model`, the missions' wire data. The mission engine uses it, and
     it depends only on `core`, serde's data formats and their schemas.
   - `node` depends on its `contract` and `node-kit`. `app` depends on its
     `contract` and `app-kit`.
   - An area that needs another area uses that area's `contract` or an
     interface it provides, never its `node` or `app` crate. Tests may.
   - Standalone apps under `bin/` can use `app-kit` and area contracts.
   - `kit/` and `engine/` use no area.
   - Node crates never depend on the GUI toolkit (`gpui`, `gpui-pre`,
     `gpui-pre-platform`). App crates never depend on the web server (`axum`).
     Contracts depend on neither. This holds on every target and under any
     rename. Their tests may depend on them.

   `bin/ahjo/tests/layout.rs` holds this.

## Node areas

A node area is a `Listing` in `bin/ahjo`'s `AREAS`. The listing gives the
area's name, frame kinds, command names and build function, so these are known
without building the area. The area is built from the node's `Setup` (its
identity, folder, address, config and event log). It answers with an `Area`:
HTTP routes, commands, a channel for each `/ws` client, and a stop.

Before building, `node-kit` refuses:

- listings that share a name, a frame kind or a command (the error names both);
- a listing that takes a kind `core` reserves (the hello, the fleet's peer
  list, view control).

After building, it also refuses an area whose commands are not the ones its
listing named. `bin/ahjo/tests/areas.rs` checks the real list, and
`ahjo kinds` prints which area owns which kinds.

- **Frames.** The first byte of a `/ws` frame is its kind. A frame from a
  client goes to the channel of the area that owns its kind. Each channel
  pushes its own frames.
- **Commands.** A command has a `noun_verb` name, a typed request and
  response, and a handler that answers with the response or the error shape.
  The verb may be several words: `harness_edit_hook` is
  `ahjo harness edit-hook`. The request gives the description and input
  schema; the response gives the output schema. `node-kit` serves each command
  as an MCP tool and as the CLI verb `ahjo noun verb`. A `node` argument names
  where it runs; MCP's tool lists leave that argument out while only this node
  can run it. Each command declares its effect: reads, writes, sets or
  destroys. Its tool carries that effect as MCP annotations. This matters
  because a client such as ChatGPT asks the operator before every tool that is
  not marked read-only, so a command that only reads must say so.
- **MCP servers.** Each area's tools are an MCP server of their own, at
  `/mcp/<area>`. The node's own commands are at `/mcp/node`. `/mcp/a,b` serves
  several areas, and `/mcp` serves every one. A client connects to what it
  needs and pays only for those tools. For example, the missions area alone is
  about half of all tools' schemas, and an agent reading the brain need not
  carry it. A tool asked of a server that does not have it is refused, and the
  error names the server that does. `ahjo mcp --area <area>` (repeat `--area`
  for more) does the same over stdio. The command line reads `/mcp`, so it
  sees every command in one request.
- **Tool sets.** A binary may name tool sets (`ahjo`'s `TOOL_SETS`). Each set is
  an MCP server at `/mcp/<set>` holding chosen commands from any area, shaped
  for one client. For example, `chatgpt` holds the handoff, issue and host
  tools. A set may add three tools that exist only in sets:
  - `swarm_search` finds every other command by words.
  - `swarm_read` and `swarm_run` run one of them, through the same call and
    `node` check as `tools/call`. Each takes only its own kind of command: a
    read never needs confirming, and a change always does.

  So the client carries the schemas of about a dozen tools and can still reach
  all of them.
- **One node at a time.** A request names its node with `?node=` on any route
  and on `/ws`. A command call names it with its `node` argument. Passing a
  request on to another node (relaying) is not built. A request naming another
  node is refused as `unknown_node` (HTTP 421), never run here.
- **Config.** `config.toml` in the node's folder has a `[node]` section and one
  section per area, which that area reads. `enabled = false` leaves an area
  unbuilt, and `/api/health` lists it as off. If the file has a section no area
  reads, the node refuses to start. `ahjo config show [--section]` answers with
  the file's path and each section as the node loaded it. The Settings page
  shows the connected node's config, read only. To change it, an agent edits
  the file, and a restart applies it.
- **The node's own commands.** Every node has a few commands of its own,
  whatever areas it serves (`node-kit`'s `own::COMMANDS`: `config_show`,
  `view_show`, `view_open`, `view_send`). A listing that names one of them is
  refused. So is an area named `node`, because that is the name of the node's
  own config section and MCP server.
- **View control.** The node keeps the view-control kinds for itself. A client
  that reports which page it shows (0xb0) counts as an app until it goes.
  - `ahjo view show` lists what each app shows.
  - `ahjo view open --page <id> [--path|item|run|mission|session|setting|service|agent <id>] [--step]`
    sends the apps a view command (0xb8) and answers with the first answer
    (0xb1). It answers `no_app` when no app is connected, and `timeout` after
    5 s. The app shows the page and hands it the thing as an `Ask::Reveal`, on
    this node. A page Swarm no longer has (`brain`) is refused by the node
    itself as `no_page`, saying where it went.
  - `ahjo view send --text <text>` is how another app reaches Swarm's Send
    (Ahjo Brain's Send hands over `brain page <file>`). The app brings forward
    the page that answers `Ask::Send` and hands it the text, so the operator
    picks the session in Swarm's Send to Session.
- **Provided interfaces.** What one area offers other areas is a narrow trait.
  It lives in its provider's contract or engine crate, never in `node-kit`.
  While it is built, the provider fills the interface, keyed by that trait
  (`setup.provided.provide`). A reader answers `Built::then` and reads the
  interface once every area is built, so the order of `AREAS` never matters.
  An interface filled twice is refused, and an empty one reads as
  `not_provided`. There are two:
  - `Sessions` (in `engine/term`) lists sessions, follows them, delivers to
    them, starts them, and runs a mission's seats. It was the first; the
    terminals area provides it.
  - `Resolver` (in `areas/issue/contract`) gives the canonical reference for a
    written one; a bare Bead id is placed by the node's issue configuration.
    The issue area provides it.
- **Hooks.** `ahjo hook <url> [--then command...]` is how an outside program's
  own hook reports to the node. It posts the payload to the URL an area gave
  it, and runs `--then` whether or not that worked. The URL carries what the
  area needs, such as the session and the event.

## App pages

A page is a build function in `bin/ahjo-swarm`'s `PAGES`, a list held by the
shell in `kit/app-shell`. A page is built from a `Host` and answers with one
entity that implements `app-kit`'s `Page`. The `Page` gives:

- an id and a title;
- whether it has a tab (a page without one is never shown; it lives by its
  chords and what it lends);
- the node area it needs (its tab hides while every node has that area off);
- its frame kinds;
- the channels it turns on while shown;
- the asks it answers;
- its chords.

The shell owns the window, the tab bar, the dismissal stack, the leader, the
status line and the node connection. It refuses pages that share an id or a
frame kind, that take a kind `core` reserves, or whose `everywhere` chords
share a key; the error names both. `bin/ahjo-swarm/tests/pages.rs` mounts the
real list.

- **Host, Hub and AppFrame.** A `Host` is a cheap handle on the `Hub`. The Hub
  is the state the shell and every page share: the fleet link and the settings
  in `app.json`, beside the window's `AppFrame` (`Hub::frame`). The `AppFrame`
  is the half with no node behind it: the stack of popups, menus and layers,
  the status line and the toast, and the functions that draw them and handle
  their keys. Ahjo Brain and Ahjo Desk each hold an `AppFrame` of their own and
  no hub, so a gesture behaves the same in all three apps. Through the Host a
  page can list live nodes and where each device is, make HTTP requests, send
  frames, turn channels on, report, open popups, menus and layers, post
  desktop notifications (the shell owns the one click handler and runs the
  page's click), redial, keep its section of `app.json`, and ask.
- **Taps.** Some pages get more frames than the UI thread can take (terminal
  screens). Such a page registers a tap: the network thread hands it every
  frame of the page's own kinds first, and the page wakes itself at most once
  per paint.
- **Events.** The hub never calls a page. Instead, these reach the shell as a
  hub event, after the update that caused them: a frame of a page's kinds, a
  link to a node opening or going, an ask the page answers, and a closed
  popup's or layer's dismiss. So a page may use its Host inside its own
  update.
- **Tabs and chords.** A page may draw its own part of the tab bar. (The
  Sessions page, which belongs to the terminals area, draws its workspace tabs
  there.) A chord marked `everywhere` works from every page, as CHROTE's
  terminal chords do, and brings its page forward when it acts.
- **Asks.** Pages never depend on each other. An `Ask` is plain data: send a
  reference, open a path, reveal a thing, or launch a session in a folder on a
  node. It goes to the page that declares it answers that ask. When no page
  does, the asker gets an `unanswered` error and the status line says so. View
  control's `open` reaches pages the same way. A beside ask leaves the asking
  page shown, and the answering page shows its answer in the window's panel
  (below). The beside asks are Send, opening a path beside, and a reveal marked
  `beside` (as when an issue named in a terminal opens its card).
- **Side sections.** The column at the window's left belongs to the shell and
  is on every page. Each page may lend it one side section
  (`Host::lend_side`). The operator can put the column away to a narrow edge
  and bring it back (`Host::set_side_open`, kept in `app.json`). Sessions lends
  its rail, on every page. On the Sessions page that column is the rail that
  was always there; on other pages it makes the page narrower. A lent section
  keeps its page's channels on while the column is open. It acts through its
  page (a session goes to Sessions), so a page's own lists stay its own. A
  section says which pages it shows on (`SideOn`).
- **The panel.** Everything a page opens beside another, asked or not, stands
  in the window's one panel. The panel belongs to the shell. It is at the right
  edge, unless the shell's setting says left. What shows there: a file (Files,
  in one column: its list, or the file opened from it), an item's card, Issues'
  work (Alt+B), the Brain (Alt+L), Send, the output search, a diagram, and a
  mission's open card.

  A page shows its content there with `Host::show_panel`, passing a
  `PanelView` that says what its head is. The shell draws the one header
  around it (Back where it applies, the head, and its close control) and the
  panel's edge.

  One content shows at a time. What it opened over waits under it. Back brings
  that back, and so does its page taking the newer content out
  (`Host::close_panel`, as a send that arrived does). Content opened while the
  panel is put away starts the panel over, so Back leads only to what was
  shown. Content says where it stands (`SideOn`): Files does not stand in the
  panel on the Files page, which holds the same, and a mission's card stands
  only on Missions.

  Four things put the panel away: its ×, Escape with the keyboard in it,
  Ctrl+B, and the content's own chord. Each works only once the content lets
  it (`PanelView::may_close`: unsaved edits are asked about first). Ctrl+B
  brings it back.

  The panel never holds the keyboard. A click outside it takes the keyboard
  away and leaves the panel open. When the panel goes with the keyboard in it,
  the keyboard goes to the page. Ctrl+B is the shell's only when whatever has
  the keyboard did not take it. A terminal always takes it (tmux's prefix, a
  harness's background key, readline), so from a terminal each content's own
  chord is the way.

  While a page's content shows, that page's channels stay on. The width
  dragged at the panel's inner edge is kept in `app.json`; whether the panel is
  open is not.

  Every edge you drag to size what it edges (the panel, Issues' list) is
  `kit::grip`. It has one hit width, the window catches the release (not a
  cover laid over it), and a double click puts back the first width. The
  panel's column (head, Back, ×, grip, slide) is `panel::EdgeColumn`, which
  Desk's list of asks uses too.

  Only Peek still opens over the page: a glance that any click outside closes.
  Why one panel: [ADR 0001](docs/adr/0001-one-panel-beside-the-page.md).
- **Popups.** Every menu, confirm and small form a page opens over itself is a
  popup on the dismissal stack (`Host::popup`, `app-kit`'s `popup`). The shell
  does everything but its content, the same on every page.

  Where it stands: beside what was clicked (`Spot`: the pressed button, or else
  the point). It goes below that, turned to the side with room, flipped above
  it when needed, and kept inside the window, all by one function
  (`popup::beside`). It is never larger than the window.

  Closing: Escape closes only the top popup. A press outside either closes it
  (`Outside::Close`) or leaves it (`Outside::Keep`, for a form holding what was
  typed), and either way the press reaches nothing under it. A popup may say
  not now (`Popup::may_close`: a start not yet answered, a draft). When its
  own page closes it, it is never asked.

  The keyboard: a popup takes the keyboard into its declared focus. When it
  closes with the keyboard in it, the keyboard goes back where it was when the
  popup opened, a terminal included, or to the page if that was only the
  window.

  The frame: the shell draws the popup's frame and, at the frame's top right,
  the close control every surface shows (`kit::close`). A click on it closes
  the popup as Escape does, asking `may_close`. The content draws no frame or
  close of its own.

  Menus and confirms: a menu is a popup of rows (`MenuRow`) that the shell
  draws and steps through with the keys; anything else opening closes it. A
  danger confirm is a one-row menu that is already armed (`Host::confirm`).

  Content: a popup's content is a view. A page draws it through
  `popup::drawn`, which redraws with the page. A form in a popup is
  `popup::form`: its fields scroll when the window is short, and its actions
  stand under them, always in view.

  What opens in place stays the page's own (a rename in a head or a tab, a
  mission's note paper, the files palette). Only a decision about the whole
  window is centred (the close guard), as DESIGN.md's Act where it is says.
  Layers are the stack's other kind: Overview and Peek, which cover the page,
  and a rename in place while it holds the keyboard.
- **Renames in place.** Every name that is renamed where it is shown is an
  `InlineEdit`, with one rule: Enter, or the keyboard going elsewhere, keeps
  what was typed; Escape puts the old name back. It tells its page once:
  `Committed(name)` or `Reverted`. If the page or node refuses the name, the
  edit goes on (`resume`), with the reason beside it.
- **Scroll areas.** Use `pane::scroll` for a column, `scroll_x` for a strip and
  `scroll_both` for a picture or board. Each draws its own thumb and keeps its
  place, unless the page holds its handle. For long lists of rows of equal
  height, `pane::long_list` renders only the rows in view and draws the same
  thumb. A scroll area once built by hand as a row stretched its content to
  its own height, so nothing past it was measured (Desk's document did not
  scroll). `bin/ahjo/tests/layout.rs` refuses a scroll area built outside
  `pane.rs`.
- **Window sizes.** A shared session's terminal windows change size only when
  the operator does something himself: opens the panel or puts it away (as a
  path he clicks, Send or an item's card opens it), or drags an edge. What
  opens beside the windows makes room for itself rather than covering them. A
  page showing, the panel's content changing, or Peek never resizes them.
  Resizes settle before they reach tmux, so a sliding panel sends only one.
- **Unsaved work.** The shell owns the window's one close handler. A page
  guards itself, or its content in the panel, with `Host::guard_close`, naming
  what that view holds unsaved. While any guarded view names something,
  closing the window brings its page forward and asks, over the window,
  whether to leave it or keep editing. A view that is gone guards nothing.
- **Settings.** A page declares its settings beside it (`Page::SETTINGS`: key,
  label, control, default). The shell's Settings page lists the shell's own
  settings (node, keys) and each page's settings under its title. It keeps a
  change under its key in the page's section of `app.json`, and tells the page
  (`Page::settings_changed`). A key declared twice stops the app from
  starting. The app watches `app.json` (`Hub::watch`). An edit made outside
  the app, as an agent makes, applies at once, and each page whose section
  changed hears `settings_changed`. The app's own writes do not count as
  edits. A file that cannot be read is reported, and the last good settings
  stay. That file is a `SettingsFile`, from the kit, as Desk's `desk.json` and
  Brain's `brain.json` are. So every app has one way to read, write, set aside
  a broken file and pick up an outside edit (`KeepsSettings`).
- **Read models.** Some things one page knows are shown by other pages (the
  Sessions page's session list). Such a thing is a plain type in its area's
  `contract`. The page that knows it publishes it (`Host::publish`), and any
  page reads and watches it (`Host::model`), built in any order. A second
  publisher is refused.
- **Tests.** An app area's test drives `ahjo-app-shell` with its own page, or
  pages it defines, against a testkit or fake node. It builds no other area.

## Kit

Every interaction has one element in the kit, the same in Swarm, Brain and
Desk. Build with it. Where it falls short, extend it in `kit/`. How popups,
the panel, renames, scroll areas and settings behave is said once, above (App
pages); this section says where to find each element. Paths are under
`kit/app-kit/src` (`ahjo_app_kit`); `kit::` is `widgets::kit` and `pane::` is
`widgets::pane`.

| Element | Use it for | Where |
| --- | --- | --- |
| Popup | anything small opened over the page, beside what was clicked: `Host::popup(Popup::beside(spot, view))`, with its `Outside`, `focus` and `may_close` | `popup`, `Host::popup` |
| Form in a popup | a small form whose fields scroll and whose actions stay in view; a page draws it with `popup::drawn` | `popup::form` |
| Menu | a list of acts, with keys, rules, submenus and refused rows (`MenuRow`) | `Host::menu`, `menu_keyed`, `menu_armed`; `menu` |
| Confirm | a dangerous act asked about before it runs: one armed row beside the button | `Host::confirm` |
| Layer | a page-wide surface over the page or window (Overview, Peek); `keeps_escape` when Escape belongs to its content. Its shape says how it stands: covering its room and drawing its own head (`Shape::Cover`, the default); in a card the shell draws at the top or middle of the room (`Layer::card`, `middle`, `closes_outside`); or an edit in place such as a rename (`Layer::in_place`). `takes_drops` takes files dropped anywhere in its room. A card that grows with its content asks `frame::card_tallest` how tall it may grow | `Host::layer`, `frame::Layer`, `frame::Shape` |
| Close control | the small × that every surface that opens shows, with the tip Close (ADR 0002). The shell draws it on popups, on layers (at a card's top right, or at the top right of a covering layer's room; an edit in place has none), on the panel and on the status line. What opens in place in a page (Brain's reading sheet, a find bar, Desk's pen) puts it at its head, wired to close itself. A toast, which fades and takes no action, has none. A × that takes something away without closing a surface is `kit::cross`, with its own tip | `kit::close`, `kit::cross` |
| Escape and the stack | Escape closes only the top popup, menu or layer; an app's own Escape is for its own modes, when the stack is empty | `frame::key`, `frame::dismiss_top` |
| Panel | content opened beside the page (a file, a card, Send): a `PanelView` with its head, Back and `may_close` | `Host::show_panel`, `panel::Panel`, `panel::PanelView` |
| Edge column | a column at the window's edge in an app without the shell (Desk's asks): head, ×, grip, slide | `panel::EdgeColumn` |
| Side section | a page's part of the shell's left column, shown on the pages its `SideOn` names | `Host::lend_side` |
| Grip | an edge dragged to size what it edges; a double click puts the first width back | `kit::grip` |
| Scroll area | content scrolling down, across, or both (a picture, a board) | `pane::scroll`, `pane::scroll_x`, `pane::scroll_both` |
| Long list | a scrolling list of rows of equal height, rendered as they come into view | `pane::long_list` |
| Page skeleton | a list beside the thing, with its heads, rows, tables, lines, empty states and Send | `pane::page`, `list`, `detail`, `row`, `table`, `empty`, `send` |
| Text field | any typed text, one line or a wrapping note, with undo | `widgets::field::TextField` (`widgets::undo`) |
| Code | a file's text: line numbers, syntax colours, find, and edits when asked | `widgets::code::Code` |
| Rename in place | a name renamed where it is shown | `widgets::inline_edit::InlineEdit` |
| Search | every filter of a list: forgives a mistyped letter and marks what matched | `text::fuzzy::filter`, `fuzzy::matches` |
| Finder | picking one or several things from a list by typing, keyboard first | `widgets::finder::Finder` |
| Notices | the status line and the toast: `Host::report`, `Host::toast`; drawn over the page, or in the flow where an app gives them a place | `frame::notices`, `frame::notices_in` |
| Settings file | an app's JSON settings: read, written, a broken file set aside, an outside edit picked up | `settings_file::SettingsFile`, `KeepsSettings` |
| Page settings | a setting shown on Swarm's Settings page | `Page::SETTINGS`, `declared::Setting` |
| Unsaved work | a view holding edits not yet kept, asked about when the window closes | `Host::guard_close` |
| Theme picker | choosing the theme, in every app | `widgets::theme_picker::button`, `open` |
| Tip | an explanation shown on hover: `.tooltip(kit::tip(why))` | `kit::tip` |
| Buttons and toggles | an act (`Kind::Primary`, `Plain`, `Quiet`), a chosen option, a switch; a quiet act in a head | `kit::button`, `kit::option`, `kit::toggle`, `pane::act` |
| Tabs | a bar of tabs with the shown one marked | `kit::tab_bar`, `kit::tab`, `kit::tab_label` |
| Dragged file | a file on a node, dragged from one page onto another | `drag::NodeFile` |
| File picking | the desktop's file picker, or a typed path where there is none | `widgets::path_picker::pick`, `pick_folder` |
| Markdown | prose drawn in the theme, its links handed to the page | `widgets::markdown` |
| Selectable text | prose selected and copied across a column | `widgets::select::Selectable` |
| Tokens | every colour, size, corner, type size and motion | `theme` (`size`, `ty`, `board`, `fade`, `motion`), and a page's own tokens beside it |
| Window evidence | what a report attaches: the window's own pixels (`PrintWindow` on Windows, the X window's image on X11; on Wayland it says it cannot), and its page, panel, open surfaces, builds and size | `Host::capture` (`capture::Picture`; the binary names how, as Swarm's `capture` does), `Host::window_state` |
| AppFrame and Hub | `AppFrame` is a window's stack, status and toast, with no node. Swarm's `Hub` holds one beside the fleet and settings; Brain and Desk hold an `AppFrame` alone (App pages, Host, Hub and AppFrame) | `frame::AppFrame`, `hub::Hub` |

`bin/ahjo/tests/layout.rs` refuses hand-made versions of these outside the
kit: a page's own scroll area or scrollbar, a popup placed with `anchored`,
something drawn over the page with `deferred`, a press outside caught with
`on_mouse_down_out`, a page's own × or Close button, and a type size, corner or
colour written as a number. Its failure names the element to use. What it
allows is listed in it, each with its reason, and that list only shrinks.

## One repository

Swarm, Brain and Desk live in one repository. They share the app kit (theme,
fonts, art, widgets). The kit changed in 9 of the 29 commits before this
section was written. Four commits in one week changed the kit and a standalone
app together. In separate repositories, each of those would have been a
release, a version pin bump and three CI runs. One repository gives the three
apps one theme, one check, one install procedure and one Windows build
checkout.

The app kit and the area contracts give standalone apps their shared UI and
wire types. For example, Brain uses the Brain contract to read a remote node's
brain. A standalone app takes from the kit what Swarm's shell takes, except the
node:

- its `AppFrame` (popups, menus, layers, status, toast, and Escape closing only
  the top one);
- its settings as a `SettingsFile`;
- the theme picker (`widgets::theme_picker`, which in Brain and Desk can follow
  Swarm's theme);
- the edge column and grip;
- its errors as `ahjo_core::Error`.

An app keeps its own Escape for its own modes (Desk's drawing, Brain's open
page) only for when the frame's stack is empty. The build holds this boundary
(`bin/ahjo/tests/layout.rs`). Revisit this choice when a product gets users
outside this machine with a release rhythm of its own, or when the kit has
been stable long enough to version.

## The app's zoom

The app's zoom (`zoom::app`, the `size` setting) is the window's own scale in
GPUI. `vendor/gpui-pre` adds it to the GPUI release this repository pins
(`Window::set_ui_scale`, marked `Ahjo:` in its `window.rs` and
`platform.rs`). The display's scale factor is multiplied by the zoom, and the
viewport and every input position are divided by it, as a browser's zoom does.
So every measure grows together: tokens, a page's own pixels, what a canvas
draws, a measured text width. No view knows the zoom, and a layout that is
right at 100% is right at 130%. GPUI's rem size would scale only what is
written in rems, but the apps size everything in pixels, much of it in
arithmetic with measured widths. Raising `gpui-pre` to a new release means
applying the same few lines to it.

## Why this shape

Many agents work on Ahjo at once. This shape keeps one piece of work inside one
directory, keeps a build to the area being changed, and turns the boundaries
from a habit into something the build refuses to break.

## Errors

There is one error shape everywhere, defined in `core`: a stable `code` and a
`message` for people. Each area owns its codes in its `contract`. Routes,
commands and the app all carry that shape; nothing returns a bare string.

The command line says how a command ended in its exit code:

- 0: done;
- 1: it ran and failed;
- 2: the command line was wrong (`bad_arguments`);
- 3: no node answers (`no_node`).

With `--json`, the answer is JSON on stdout, and a failure is the error shape
on stderr, as `{"error": {code, message}}`.

## Sessions survive

Terminal sessions belong to the holder process, not to the node. Installing,
upgrading or restarting Ahjo never stops a session.

When a node attaches a control client, tmux records the session as active.
Ahjo keeps the pane's earlier output time until the pane writes again.

## Install and upgrade

`ahjo install`, `ahjo upgrade` and `ahjo uninstall` are the binary's own verbs,
not an area's commands. They put the binary itself in place and restart the
node, so no node can own them. They have no MCP tool, because a node
upgrading itself would be stopped by its own restart before it could answer.

On Linux and WSL the node is a systemd user unit (`ahjo`, unless `--unit` names
another). It runs `<root>/bin/ahjo serve --listen <addr>`, with
`KillMode=process`, so a restart signals only the node, and a tmux server the
node started lives on.

- **Install** copies the build (and `--app`) to `<root>/bin` (`~/.local` by
  default), writes the unit, enables and starts it, and waits until
  `/api/health` answers with the build's version. Run again with nothing
  changed, it changes nothing. If the unit is already running another build,
  install refuses: that is an upgrade.
- **Upgrade** reads the binary and address from the unit's `ExecStart`, sets
  the installed build aside, puts the new one in its place and restarts the
  unit. If the new node is not healthy in time, it puts back the build it set
  aside, restarts it and fails with `upgrade_failed`. Once the new node is
  healthy, the old build is deleted. A unit without `KillMode=process` is
  refused before anything changes.
- **Uninstall** stops and disables the unit, and removes it and the binaries
  its `ExecStart` names. Sessions, and the node's config and state, stay.
- **Version.** A build's version is the package version plus its commit
  (`0.0.1+8cdb58b`). It shows in `ahjo --version`, `ahjo version` and the
  health.
- **The address.** A node records the address it bound in its config folder
  (`listen`), so `ahjo` commands on the same machine reach it with no
  `--node`.
- **Apps for other machines.** A node hands out the Windows builds of Swarm,
  Brain and Desk made from its own commit. Another machine on the tailnet
  installs them from it, and so stays in step with it. Nothing is published.
  - `ahjo upgrade --windows-apps <dir>` checks that each app carries the new
    build's version, and puts them in `<root>/apps/windows/<version>/` with a
    manifest of sizes and SHA-256s. It removes every other version.
  - The node serves the manifest at `/apps/<platform>`, each file the manifest
    names, and `/install.ps1`. That script points at the node as the request
    named it, and at `[node] desk`.
  - `irm <node>/install.ps1 | iex` puts the apps in `%LOCALAPPDATA%\Ahjo`, with
    `install.json` naming the node. It sets Swarm's node and Desk's first desk,
    and adds Start-menu entries. It needs no administrator rights.
  - An app with `install.json` beside it asks the node for its build at start,
    when its window comes forward, and (Swarm only) when its link opens or is
    refused. When the node's build is not its own, it shows Update in its
    chrome.
  - Update fetches every file and checks each. It renames the running files
    aside (Windows lets a running program's file be renamed, not replaced),
    puts the new ones in place, and restarts the app with its arguments. In
    Swarm this happens after the unsaved-work guard. If a node refused the
    app's protocol, the app says so, with Update as its action.
  - The operator's own apps have no `install.json` and never update this way.

`deploy/` is the operator's own machine. It holds:

- `ahjo.service`, as `ahjo install --root ~/.local/share/ahjo` wrote it there
  (listening on 127.0.0.1:7310, which Windows Tailscale serves as tailnet port
  7300), linked into `~/.config/systemd/user`;
- the ChatGPT tunnel's unit and profile, which reach the node's `/mcp/chatgpt`
  tool set.

The `change-ahjo` skill has the steps.

`bin/ahjo/tests/install.rs` runs all three verbs against a stand-in
`systemctl` on `PATH`, with a node, a tmux server and a session of the test's
own.

## Tests

An area's tests live in its crates and run against `testkit`. `testkit` starts
a node with only the named areas, on a port and tmux server of its own.
`bin/ahjo` holds the few tests that need the whole binary.
