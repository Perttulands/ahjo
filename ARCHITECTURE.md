# Architecture

This file says how Ahjo is built: its crates, the boundaries between them, and
the kit every app is made from. Read it before you decide where a change goes.

This repository holds Ahjo's three apps.

**Ahjo Swarm** is two binaries:

- `ahjo` is the node and the command line (CLI).
- `ahjo-swarm` is the native app. It talks to a node only over the node's
  WebSocket and HTTP interface.

**Standalone apps** have one purpose of their own. Each is a separate binary
built on the same app kit. They look like Swarm but do not use Swarm's shell.
There are two: Ahjo Brain and Ahjo Desk.

**Ahjo Brain** (`ahjo-brain`) shows the company brain as a night sky, with a
search, a reader and an editor.

- Brain reads the brain's files directly.
- Brain writes a page only through Context Citadel's write route. That route
  is the brain's one writer: it runs `brain check` and then commits.
- When another writer has changed a page in the meantime, Brain does not write
  over it without asking.
- Brain saves a captured note, and each daily note, through the same route
  into `knowledge/_inbox`, with the note's header filled in.
- Agents later file inbox notes into the brain. When the operator opens the
  inbox, Brain reads the brain's git history to show which notes agents filed,
  and where.
- Brain can read another machine's brain through that machine's node. The
  other machine is one of Swarm's places, or is named with `--from <node>`.
  That node's brain area lists its page files and returns their text.
- Brain keeps a copy of a remote brain in its cache. Each time its window comes
  forward, it fetches only what changed. It reads the copy as a read-only brain
  and sends searches to that node.

**Ahjo Desk** (`ahjo-desk`) is where agents publish and the operator answers in
his own words and marks.

- Desk talks only to the decision desks' HTTP API (`/srv/desk`), never to a
  node.
- Desk keeps marks in the desk's one format.
- Desk decodes H.264 video itself, so clips step frame by frame with nothing
  installed.
- On Windows, Desk renders a published page inside its own window, in the
  kit's web view (Kit), with its own marking script, marks and Comment mode.
- On Linux, Desk opens a published page in the browser, where the kit's web
  view cannot show.

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

Each directory under an area, `kit/`, `bin/` and `engine/` is one crate.

An app's package is named `ahjo-app-<name>`: `ahjo-app-swarm`, `ahjo-app-brain`
and `ahjo-app-desk`. The prefix is needed because `ahjo-brain` is already the
name of the brain area's node crate. The app's binary has the name the operator
runs.

The areas are terminals, files, issue, brain, host, scheduled, harness and
missions. Fleet is planned as an area after the first release.

- **Brain in Swarm.** The brain area appears in Swarm only as the Brain that
  Alt+L opens in the window's one panel, beside the terminals. A page without a
  tab provides it. The brain is read at length and edited in Ahjo Brain. The
  brain area's node side serves the `ahjo brain` commands and the panel.
- **Terminals.** The terminals area launches sessions and carries agent events.
- **Settings.** Settings is not an area. The shell's Settings page lists the
  settings that pages declare.

## Words

The words and what they mean are in `GLOSSARY.md`.

- A word keeps the same name in the CLI, in the stored file and on the screen.
  Only the casing changes, as in `issue_id` and `--issue-id`.
- New code, schema fields and command names use only glossary words.
- When you change code that uses an old word, rename that word everywhere.

These code names stand for screen words:

- The panel is `Host::show_panel` with a `PanelView`.
- A side section is `Host::lend_side` with its `SideOn`.
- A popup is `Host::popup`.

## Rules the compiler holds

The build fails when code breaks one of these rules.

1. **One contract per area.** Both sides of an area use the same `contract`
   crate. A standalone app that uses an area's data uses that contract too. So
   when a field changes, every caller fails to build.
2. **Areas register themselves.**
   - A node area gives `node-kit` its routes, its client actions and its
     commands. One command definition produces both the CLI verb and the MCP
     tool.
   - An app area gives `app-kit` its page. The page declares its frame kinds,
     its channels, the asks it answers and its chords.
   - The binaries hold only the list of areas.

   So a change inside an area touches only that area's directory. A new area
   adds one line to each list and one dependency line to the binary's
   `Cargo.toml`. The workspace finds crates by their location. CI finds them by
   their names (`ahjo-<area>-app`).
3. **Dependencies point one way.**
   - `contract` depends on `core` and on two crates in `engine/`:
     - `issue-ref` is an issue's reference. It is plain data with no
       dependencies, and the issue area and the mission engine share it.
     - `mission-model` is the missions' wire data. The mission engine uses it.
       It depends only on `core`, serde's data formats and their schemas.
   - `node` depends on its own `contract` and on `node-kit`.
   - `app` depends on its own `contract` and on `app-kit`.
   - When an area needs another area, it uses that area's `contract` or an
     interface that area provides. It never uses the other area's `node` or
     `app` crate. Tests may.
   - Standalone apps under `bin/` may use `app-kit` and area contracts.
   - `kit/` and `engine/` use no area.
   - Node crates never depend on the GUI toolkit (`gpui`, `gpui-pre`,
     `gpui-pre-platform`).
   - App crates never depend on the web server (`axum`).
   - Contracts depend on neither.
   - These three bans hold on every target and under any rename. Tests of
     these crates may still use the banned dependencies.

   `bin/ahjo/tests/layout.rs` enforces this rule.

## Node areas

A node area is a `Listing` in `bin/ahjo`'s `AREAS`. The listing gives the
area's name, frame kinds, command names and build function. So the node knows
these without building the area.

The build function takes the node's `Setup`: its identity, folder, address,
config and event log. It returns an `Area`, which holds HTTP routes, commands,
a channel for each `/ws` client, and a stop.

`node-kit` refuses these listings before it builds any area:

- two listings that share a name, a frame kind or a command. The error names
  both.
- a listing that takes a kind `core` reserves: the hello, the fleet's peer list
  or view control.

After the build, `node-kit` also refuses an area whose commands differ from the
ones its listing named. `bin/ahjo/tests/areas.rs` checks the real list.
`ahjo kinds` prints which area owns which kinds.

- **Frames.** The first byte of a `/ws` frame is its kind. The node sends a
  client's frame to the channel of the area that owns that kind. Each channel
  pushes its own frames to clients.
- **Commands.** A command has a `noun_verb` name, a typed request, a typed
  response, and a handler. The handler returns the response or the error
  shape.
  - The verb may be several words: `harness_edit_hook` is
    `ahjo harness edit-hook`.
  - The request type gives the description and the input schema. The response
    type gives the output schema.
  - `node-kit` serves each command as an MCP tool and as the CLI verb
    `ahjo noun verb`.
  - A `node` argument names the node that runs the command. MCP's tool lists
    leave this argument out while only this node can run the command.
  - A command that answers a picture marks that field `x-ahjo-image` (an
    object of `mime_type` and base64 `data`). MCP gives it as image content
    after the text, and leaves it out of the text and the structured result;
    the command line puts it back for `--json`. Files' `file_read_image`
    does this for an image in a folder under the roots.
  - Each command declares its effect: reads, writes, sets or destroys. Its MCP
    tool carries the effect as annotations. Mark every command that only reads
    as a read. A client such as ChatGPT asks the operator before it runs any
    tool not marked read-only.
- **MCP servers.** Each area's tools form an MCP server of their own at
  `/mcp/<area>`.
  - The node's own commands are at `/mcp/node`.
  - `/mcp/a,b` serves several areas. `/mcp` serves every area.
  - A client connects only to the servers it needs, and carries only those
    tools' schemas. For example, the missions area alone is about half of all
    tool schemas, and an agent that reads the brain does not need it.
  - When a client calls a tool on a server that lacks it, the server refuses.
    The error names the server that has the tool.
  - `ahjo mcp --area <area>` serves the same over stdio. Repeat `--area` for
    more areas.
  - The command line reads `/mcp`, so it sees every command in one request.
- **Tool sets.** A binary may name tool sets, as `ahjo` does in `TOOL_SETS`.
  Each set is an MCP server at `/mcp/<set>`. It holds chosen commands from any
  area, shaped for one client. For example, `chatgpt` holds the handoff, issue, image
  and host tools. A set may also add three tools that exist only in tool sets:
  - `swarm_search` finds any other command by words.
  - `swarm_read` runs a command that reads. It never needs confirming.
  - `swarm_run` runs a command that changes something. It always needs
    confirming.

  `swarm_read` and `swarm_run` go through the same call and the same `node`
  check as `tools/call`. With them, a client carries about a dozen tools'
  schemas and can still reach every command.
- **One node at a time.** A request names its node with `?node=`, on any route
  and on `/ws`. A command call names its node with the `node` argument. The
  node does not relay requests to other nodes. It refuses a request that names
  another node as `unknown_node` (HTTP 421) and never runs it locally.
- **Config.** `config.toml` in the node's folder has a `[node]` section and one
  section per area. Each area reads its own section.
  - `enabled = false` leaves an area unbuilt. `/api/health` lists it as off.
  - When the file has a section that no area reads, the node refuses to start.
  - `ahjo config show [--section]` prints the file's path and each section as
    the node loaded it.
  - The Settings page shows the connected node's config, read only.
  - To change the config, an agent edits the file. A restart applies it.
- **The node's own commands.** Every node has a few commands of its own,
  whatever areas it serves: `config_show`, `view_show`, `view_open` and
  `view_send` (`node-kit`'s `own::COMMANDS`).
  - The node refuses a listing that names one of these commands.
  - The node refuses an area named `node`. That name belongs to the node's own
    config section and MCP server.
- **View control.** The node owns the view-control kinds itself. A client that
  reports which page it shows (0xb0) counts as an app until it disconnects.
  - `ahjo view show` lists what each app shows.
  - `ahjo view open` sends the apps a view command (0xb8) and returns the first
    app's reply (0xb1). If no app is connected, it returns `no_app`. If no app
    replies within 5 s, it returns `timeout`. Its full form is:

    ```
    ahjo view open --page <id> [--path|item|run|mission|session|setting|service|agent <id>] [--step]
    ```

    The app shows the page and passes the thing to it as an `Ask::Reveal`, on
    this node.
  - When the page is one Swarm no longer has, such as `brain`, the node itself
    refuses with `no_page`. The error says where the page went.
  - `ahjo view send --text <text>` lets another app use Swarm's Send. For
    example, Ahjo Brain's Send sends `brain page <file>`. Swarm brings forward
    the page that answers `Ask::Send` and passes it the text. The operator then
    picks the session in Swarm's Send to Session.
- **Provided interfaces.** When one area offers something to other areas, it
  offers a narrow trait.
  - The trait lives in the provider's contract or engine crate, never in
    `node-kit`.
  - While the provider is built, it fills the interface, keyed by that trait
    (`setup.provided.provide`).
  - A reader returns `Built::then` and reads the interface after every area is
    built. So the order of `AREAS` never matters.
  - The node refuses an interface filled twice. An empty interface reads as
    `not_provided`.

  There are two interfaces:
  - `Sessions` (in `engine/term`) lists sessions, follows them, delivers to
    them, starts them and runs a mission's seats. The terminals area provides
    it. It was the first.
  - `Resolver` (in `areas/issue/contract`) turns a written reference into the
    canonical one. It places a bare Bead id using the node's issue
    configuration. The issue area provides it.
- **Hooks.** An outside program's own hook reports to the node with this
  command:

  ```
  ahjo hook <url> [--then command...]
  ```

  It posts the payload to the URL an area gave the program. Then it runs
  `--then`, whether or not the post worked. The URL carries what the area
  needs, such as the session and the event.

## App pages

A page is a build function in `bin/ahjo-swarm`'s `PAGES`. The shell in
`kit/app-shell` holds that list. The build function takes a `Host` and returns
one entity that implements `app-kit`'s `Page`. The `Page` declares:

- an id and a title;
- whether it has a tab. A page without a tab is never shown itself. It works
  only through its chords and what it lends to the side column.
- the node area it needs. Its tab hides while that area is off on every node.
- its frame kinds;
- the channels it turns on while shown;
- the asks it answers;
- its chords.

The shell owns the window, the tab bar, the dismissal stack, the leader, the
status line and the node connection. The shell refuses pages that share an id
or a frame kind, that take a kind `core` reserves, or whose `everywhere` chords
share a key. The error names both pages. `bin/ahjo-swarm/tests/pages.rs` mounts
the real list.

- **Host, Hub and AppFrame.**
  - The **`Hub`** is the state the shell and every page share: the fleet link
    and the settings in `app.json`. It holds the window's `AppFrame`
    (`Hub::frame`).
  - The **`AppFrame`** is the part with no node behind it: the stack of popups,
    menus and layers, the status line, the toast, and the functions that draw
    them and handle their keys.
  - A **`Host`** is a cheap handle on the `Hub`.

  Ahjo Brain and Ahjo Desk each hold an `AppFrame` of their own and no `Hub`.
  So a gesture behaves the same in all three apps.

  Through its `Host`, a page can:
  - list live nodes and see where each device is;
  - make HTTP requests, send frames and turn channels on;
  - report, and open popups, menus and layers;
  - post desktop notifications. The shell owns the one click handler and runs
    the page's click action.
  - redial;
  - keep its own section of `app.json`;
  - send asks.
- **Taps.** Some pages receive more frames than the UI thread can handle, such
  as terminal screens. Such a page registers a tap. The network thread then
  gives the page every frame of its own kinds first. The page wakes itself at
  most once per paint.
- **Events.** The hub never calls a page. These reach the shell as hub events,
  after the update that caused them:
  - a frame of the page's kinds;
  - a link to a node opening or closing;
  - an ask the page answers;
  - the dismiss of a closed popup or layer.

  So a page may use its `Host` inside its own update.
- **Tabs and chords.** A page may draw its own part of the tab bar. For
  example, the Sessions page, which belongs to the terminals area, draws its
  workspace tabs there. A chord marked `everywhere` works from every page, as
  CHROTE's terminal chords do. When it acts, it brings its page forward.
- **Asks.** Pages never depend on each other. Instead, a page sends an `Ask`.
  - An ask is plain data: send a reference, open a path, reveal a thing, or
    launch a session in a folder on a node.
  - The ask goes to the page that declares it answers that ask.
  - When no page answers it, the asker gets an `unanswered` error and the
    status line says so.
  - View control's `open` reaches pages the same way.
  - A **beside ask** leaves the asking page shown. The answering page shows its
    answer in the window's panel (below). The beside asks are Send, open a path
    beside, and a reveal marked `beside`. For example, an issue named in a
    terminal opens its card beside the terminal.
- **Side sections.** The column at the window's left belongs to the shell and
  is on every page.
  - Each page may lend the column one side section (`Host::lend_side`).
  - The operator can shrink the column to a narrow edge and open it again
    (`Host::set_side_open`). The app keeps this in `app.json`.
  - Sessions lends its rail, on every page. On the Sessions page, the column is
    the rail the page always had. On other pages, the column makes the page
    narrower.
  - A lent section keeps its page's channels on while the column is open.
  - A lent section acts through its own page. For example, a session clicked
    there opens in Sessions. So each page's lists stay its own.
  - A section says on which pages it shows (`SideOn`).
- **The panel.** Everything a page opens beside another page, asked or not,
  stands in the window's one panel. The shell owns the panel. It sits at the
  right edge, unless the shell's setting says left.

  The panel shows: a file (Files, in one column: its list or the file opened
  from it), an item's card, Issues' work (Alt+B), the Brain (Alt+L), Send, the
  output search, a diagram, and a mission's open card.

  A page shows content there with `Host::show_panel`. It passes a `PanelView`,
  which says what the head shows. The shell draws the panel's edge and its one
  header: Back where it applies, the head, and the close control.

  One content shows at a time. When new content opens, the earlier content
  waits under it. Back brings the earlier content back. So does the page
  removing its newer content (`Host::close_panel`), as an arrived send does.
  Content opened while the panel is put away starts the panel's history over.
  So Back leads only to content the operator saw.

  Content says on which pages it stands (`SideOn`). For example, Files does not
  open in the panel on the Files page, which already shows the same thing. A
  mission's card stands in the panel only on Missions.

  Four things put the panel away: its ×, Escape while the panel has the
  keyboard, Ctrl+B, and the content's own chord. Each works only when the
  content allows it (`PanelView::may_close`). For example, content with
  unsaved edits asks first. Ctrl+B brings the panel back.

  The panel never holds on to the keyboard. A click outside the panel takes
  the keyboard away and leaves the panel open. When the panel closes while it
  has the keyboard, the keyboard goes to the page. The shell gets Ctrl+B only
  when whatever has the keyboard does not take it. A terminal always takes
  Ctrl+B, for tmux's prefix, a harness's background key and readline. So from
  a terminal, the operator uses each content's own chord.

  While a page's content shows in the panel, that page's channels stay on. The
  app keeps the panel's width, dragged at its inner edge, in `app.json`. It does
  not keep whether the panel is open.

  Every edge the operator drags to size something (the panel, Issues' list) is
  `kit::grip`. A grip has one hit width. The window catches the release, not a
  cover laid over it. A double click puts back the first width. The panel's
  column (head, Back, ×, grip, slide) is `panel::EdgeColumn`. Desk's list of
  asks uses it too.

  Only Peek still opens over the page. Peek is a glance that any click outside
  closes. The reasons for one panel are in
  [ADR 0001](docs/adr/0001-one-panel-beside-the-page.md).
- **Web pages in Files.** A `.html` or `.htm` file opened in Files, in the
  reader or the panel, runs; Source in its head switches to its text. On
  Windows it runs in the kit's web view, loaded from the app's own scheme,
  which serves only the folder it was opened in: Files' `live::Live` reads
  each request over the link from the node's read-only folder route
  (`/api/files/folder/file`, streamed, one byte range at a time). On Linux it
  opens in the system's browser, once each time the operator opens it. Sound
  and video play in the same web view, in a player page the scheme serves at
  the folder's root, and a PDF shows in the web view's own viewer. A 3D
  model (glb, gltf, obj, fbx, stl) turns in a model page served there too,
  with three.js, which the app ships and serves from a site of its own
  (`live::THREE`, built by `areas/files/app/web/bundle-three`), so it needs
  no network. On Linux
  they open in the system's own program, as does any file Files does not
  show (Open in its Windows app, beside Download). That program gets a copy
  made whole as it streams from the node (`/api/files/file/whole`, the file
  as the list shows it, links resolved under the roots), so no size stops
  it.
  The reasons, the folder's confinement and the Escape order are in
  [ADR 0006](docs/adr/0006-web-pages-run-in-a-web-view.md).
- **Popups.** Every menu, confirm and small form that a page opens over itself
  is a popup on the dismissal stack (`Host::popup`, `app-kit`'s `popup`). The
  shell handles everything except the popup's content, the same way on every
  page.
  - **Placement.** A popup stands beside what was clicked (`Spot`: the pressed
    button, or else the point). It opens below that spot, toward the side with
    room. If needed, it flips above. One function (`popup::beside`) places it
    and keeps it inside the window. A popup is never larger than the window.
  - **Closing.** Escape closes only the top popup. A press outside either
    closes the popup (`Outside::Close`) or leaves it open (`Outside::Keep`, for
    a form holding typed text). Either way, the press reaches nothing under
    the popup.
  - **Refusing to close.** A popup may refuse to close for now
    (`Popup::may_close`), for example while a start is not yet answered or
    while it holds a draft. When its own page closes it, the popup is not
    asked.
  - **Keyboard.** A popup takes the keyboard into its declared focus. When it
    closes while it has the keyboard, the keyboard goes back to where it was
    when the popup opened, including a terminal. If that was only the window,
    the keyboard goes to the page.
  - **Frame.** The shell draws the popup's frame. At the frame's top right, it
    draws the close control every surface shows (`kit::close`). A click on it
    closes the popup as Escape does, and asks `may_close` first. The content
    draws no frame or close control of its own.
  - **Menus and confirms.** A menu is a popup of rows (`MenuRow`). The shell
    draws the rows and moves through them with the keys. Any other popup
    opening closes the menu. A danger confirm is a one-row menu that is already
    armed (`Host::confirm`).
  - **Content.** A popup's content is a view. A page draws it through
    `popup::drawn`, which redraws with the page. A form in a popup is
    `popup::form`. Its fields scroll when the window is short. Its actions stay
    under the fields, always in view.
  - **In place.** What opens in place stays the page's own, such as a rename in
    a head or a tab, a mission's note paper, or the files palette.
  - **Centred.** Only a decision about the whole window is centred, such as the
    close guard. This follows "Act where it is" in DESIGN.md.
  - **Layers.** Layers are the stack's other kind. They are Overview and Peek,
    which cover the page, and a rename in place while it has the keyboard.
- **Renames in place.** Every name renamed where it is shown is an
  `InlineEdit`. It follows one rule:
  - Enter, or the keyboard moving elsewhere, keeps what was typed.
  - Escape puts the old name back.

  The `InlineEdit` tells its page once: `Committed(name)` or `Reverted`. When
  the page or node refuses the name, editing continues (`resume`), with the
  reason shown beside it.
- **Scroll areas.** Use `pane::scroll` for a column, `scroll_x` for a strip and
  `scroll_both` for a picture or a board. Each draws its own thumb. Each keeps
  its scroll position, unless the page holds its handle. For long lists of
  rows of equal height, use `pane::long_list`. It renders only the rows in view
  and draws the same thumb. `bin/ahjo/tests/layout.rs` refuses a scroll area
  built outside `pane.rs`. The reason: a hand-built scroll area once stretched
  its content to its own height as a row, so nothing past it was measured, and
  Desk's document did not scroll.
- **Window sizes.** A shared session's terminal windows change size only when
  the operator acts himself:
  - he opens the panel or puts it away, for example when a path he clicks,
    Send or an item's card opens it;
  - he drags an edge.

  What opens beside the terminal windows makes room for itself and does not
  cover them. These never resize the terminal windows: a page showing, the
  panel's content changing, and Peek. Resizes settle before they reach tmux,
  so a sliding panel sends only one resize.
- **Unsaved work.** The shell owns the window's one close handler.
  - A page guards itself, or its content in the panel, with
    `Host::guard_close`. The guard names what that view holds unsaved.
  - While any guarded view names something, closing the window brings that
    view's page forward. The app then asks, over the window, whether to leave
    or keep editing.
  - A view that is gone guards nothing.
- **Settings.** A page declares its settings next to its code
  (`Page::SETTINGS`: key, label, control, default).
  - The shell's Settings page lists the shell's own settings (node, keys).
    Below them, it lists each page's settings under the page's title.
  - When the operator changes a setting, the Settings page stores it under its
    key in the page's section of `app.json`. It then tells the page
    (`Page::settings_changed`).
  - A key declared twice stops the app from starting.
  - The app watches `app.json` (`Hub::watch`). An edit made outside the app,
    such as an agent's, applies at once. Each page whose section changed gets
    `settings_changed`. The app's own writes do not count as outside edits.
  - When the file cannot be read, the app reports it and keeps the last good
    settings.
  - `app.json` is a kit `SettingsFile`, like Desk's `desk.json` and Brain's
    `brain.json`. So every app reads and writes its settings the same way,
    sets aside a broken file, and picks up an outside edit (`KeepsSettings`).
- **Read models.** Sometimes one page knows something that other pages show,
  such as the Sessions page's session list.
  - That thing is a plain type in its area's `contract`.
  - The page that knows it publishes it (`Host::publish`).
  - Any page reads and watches it (`Host::model`). Pages may be built in any
    order.
  - The app refuses a second publisher.
- **Tests.** An app area's test drives `ahjo-app-shell` with the area's own
  page, or pages the test defines. It runs against a testkit node or a fake
  node, and builds no other area.

## Kit

Every interaction has one element in the kit. The element is the same in
Swarm, Brain and Desk. Build with the kit's element. When an element falls
short, extend it in `kit/`.

App pages, above, says once how popups, the panel, renames, scroll areas and
settings behave. This section says where to find each element. Paths are under
`kit/app-kit/src` (`ahjo_app_kit`). `kit::` means `widgets::kit`, and `pane::`
means `widgets::pane`.

| Element | Use it for | Where |
| --- | --- | --- |
| Popup | anything small opened over the page, beside what was clicked: `Host::popup(Popup::beside(spot, view))`, with its `Outside`, `focus` and `may_close` | `popup`, `Host::popup` |
| Form in a popup | a small form whose fields scroll and whose actions stay in view. A page draws it with `popup::drawn` | `popup::form` |
| Menu | a list of acts, with keys, rules, submenus and refused rows (`MenuRow`) | `Host::menu`, `menu_keyed`, `menu_armed`; `menu` |
| Confirm | a dangerous act asked about before it runs: one armed row beside the button | `Host::confirm` |
| Layer | a surface across the page or window (Overview, Peek). Set `keeps_escape` when Escape belongs to its content. Its shape says how it stands: it covers its room and draws its own head (`Shape::Cover`, the default); or it is a card the shell draws at the top or middle of the room (`Layer::card`, `middle`, `closes_outside`); or it is an edit in place, such as a rename (`Layer::in_place`). `takes_drops` accepts files dropped anywhere in its room. A card that grows with its content asks `frame::card_tallest` how tall it may grow | `Host::layer`, `frame::Layer`, `frame::Shape` |
| Close control | the small × that every surface that opens shows, with the tip "Close" (ADR 0002). The shell draws it on popups, on layers, on the panel and on the status line. On a layer it sits at a card's top right, or at the top right of a covering layer's room. An edit in place has none. What opens in place in a page (Brain's reading sheet, a find bar, Desk's pen) puts it at its own head, wired to close itself. A toast fades and takes no action, so it has none. A × that removes something without closing a surface is `kit::cross`, with its own tip | `kit::close`, `kit::cross` |
| Escape and the stack | Escape closes only the top popup, menu or layer. An app uses its own Escape for its own modes, only when the stack is empty | `frame::key`, `frame::dismiss_top` |
| Panel | content opened beside the page (a file, a card, Send): a `PanelView` with its head, Back and `may_close` | `Host::show_panel`, `panel::Panel`, `panel::PanelView` |
| Edge column | a column at the window's edge in an app without the shell, such as Desk's asks: head, ×, grip, slide | `panel::EdgeColumn` |
| Side section | a page's part of the shell's left column, shown on the pages its `SideOn` names | `Host::lend_side` |
| Grip | an edge dragged to size what it edges. A double click puts the first width back | `kit::grip` |
| Scroll area | content that scrolls down, across, or both (a picture, a board) | `pane::scroll`, `pane::scroll_x`, `pane::scroll_both` |
| Long list | a scrolling list of rows of equal height, rendered as they come into view | `pane::long_list` |
| Page skeleton | a list beside the thing, with its heads, rows, tables, lines, empty states and Send | `pane::page`, `list`, `detail`, `row`, `table`, `empty`, `send` |
| Text field | any typed text, one line or a wrapping note, with undo | `widgets::field::TextField` (`widgets::undo`) |
| Code | a file's text: line numbers, syntax colours, find, and edits when asked | `widgets::code::Code` |
| Rename in place | a name renamed where it is shown | `widgets::inline_edit::InlineEdit` |
| Search | every filter of a list. It tolerates a mistyped letter and marks what matched | `text::fuzzy::filter`, `fuzzy::matches` |
| Finder | picking one or several things from a list by typing, keyboard first | `widgets::finder::Finder` |
| Notices | the status line and the toast (`Host::report`, `Host::toast`). They are drawn over the page, or in the flow where an app gives them a place | `frame::notices`, `frame::notices_in` |
| Settings file | an app's JSON settings: read, written, a broken file set aside, an outside edit picked up | `settings_file::SettingsFile`, `KeepsSettings` |
| Page settings | a setting shown on Swarm's Settings page | `Page::SETTINGS`, `declared::Setting` |
| Unsaved work | a view holding edits not yet kept, asked about when the window closes | `Host::guard_close` |
| Theme picker | choosing the theme, in every app | `widgets::theme_picker::button`, `open` |
| Tip | an explanation shown on hover: `.tooltip(kit::tip(why))` | `kit::tip` |
| Buttons and toggles | an act (`Kind::Primary`, `Plain`, `Quiet`), a chosen option, a switch, and a quiet act in a head | `kit::button`, `kit::option`, `kit::toggle`, `pane::act` |
| Tabs | a bar of tabs with the shown tab marked | `kit::tab_bar`, `kit::tab`, `kit::tab_label` |
| Dragged file | a file on a node, dragged from one page onto another | `drag::NodeFile` |
| File picking | the desktop's file picker, or a typed path where the desktop has none | `widgets::path_picker::pick`, `pick_folder` |
| Markdown | prose drawn in the theme. Its links go to the page | `widgets::markdown` |
| Selectable text | prose that can be selected and copied across a column | `widgets::select::Selectable` |
| Tokens | every colour, size, corner, type size and motion | `theme` (`size`, `ty`, `board`, `fade`, `motion`), and a page's own tokens beside the page |
| Web view | a web page inside the window, on Windows: WebView2 (`wry`) as a child window laid over a surface's bounds. A child window draws over everything the app draws there, so the kit decides once a frame, after the stack is drawn, whether it shows: where its surface was painted this frame and no popup, menu or layer overlaps it (`frame::stack` calls `web_view::settle`). The owner gives it an optional script that runs before the page's own, hears what the page posts, may serve it from a scheme of the app's own (`Scheme`, `scheme_url`), and names the focus that stands for the page's keyboard: an Escape the page leaves unhandled is pressed there, once (ADR 0006). On Linux it cannot show (a system web view would need GTK's event loop, which a GPUI window does not run): `SHOWS_INSIDE` is false and opening fails with `web_view_unsupported` | `web_view::WebSurface`, `WebViewSetup`, `Scheme` |
| Window evidence | what a report attaches: the window's own pixels, and its page, panel, open surfaces, builds and size. Windows captures with `PrintWindow`, X11 with the X window's image. On Wayland it reports that it cannot | `Host::capture` (`capture::Picture`; the binary names the capture method, as Swarm's `capture` does), `Host::window_state` |
| AppFrame and Hub | `AppFrame` is a window's stack, status line and toast, with no node. Swarm's `Hub` holds one, beside the fleet and settings. Brain and Desk hold an `AppFrame` alone (see App pages, Host, Hub and AppFrame) | `frame::AppFrame`, `hub::Hub` |

`bin/ahjo/tests/layout.rs` refuses these hand-made versions outside the kit:

- a page's own scroll area or scrollbar;
- a popup placed with `anchored`;
- something drawn over the page with `deferred`;
- a press outside caught with `on_mouse_down_out`;
- a page's own × or Close button;
- a type size, corner or colour written as a number.

When the test fails, it names the element to use. The test lists its allowed
exceptions, each with its reason. That list only shrinks.

## One repository

Swarm, Brain and Desk live in one repository. They share the app kit: theme,
fonts, art and widgets.

The reason is how often the kit changes. When this section was written, the
kit had changed in 9 of the last 29 commits. In one week, four commits changed
the kit and a standalone app together. With separate repositories, each of
those commits would have needed a release, a version pin bump and three CI
runs. One repository gives the three apps one theme, one check, one install
procedure and one Windows build checkout.

Standalone apps get their shared UI from the app kit and their wire types from
the area contracts. For example, Brain uses the brain area's contract to read a
remote node's brain. A standalone app takes from the kit everything Swarm's
shell takes, except the node:

- its `AppFrame`: popups, menus, layers, status line, toast, and Escape
  closing only the top one;
- its settings, as a `SettingsFile`;
- the theme picker (`widgets::theme_picker`). In Brain and Desk it can follow
  Swarm's theme.
- the edge column and the grip;
- its errors, as `ahjo_core::Error`.

An app uses its own Escape for its own modes, such as Desk's drawing or Brain's
open page, only when the frame's stack is empty. The build enforces this
boundary (`bin/ahjo/tests/layout.rs`).

Revisit the one-repository choice when either is true:

1. a product gets users outside this machine, with its own release rhythm;
2. the kit has been stable long enough to version.

## The app's zoom

The app's zoom (`zoom::app`, the `size` setting) is the window's own scale in
GPUI. `vendor/gpui-pre` adds this scale to the GPUI release this repository
pins (`Window::set_ui_scale`). The added lines are marked `Ahjo:` in its
`window.rs` and `platform.rs`.

The zoom works as a browser's zoom does. GPUI multiplies the display's scale
factor by the zoom, and divides the viewport and every input position by it.
So every measure grows together: tokens, a page's own pixels, what a canvas
draws, and a measured text width. No view needs to know the zoom. A layout
that is right at 100% is right at 130%.

GPUI's rem size is not used. It would scale only what is written in rems, and
the apps size everything in pixels, much of it computed from measured widths.

When you raise `gpui-pre` to a new release, apply the same few lines to it.

## Why this shape

Many agents work on Ahjo at the same time. This shape does three things for
them:

- It keeps one piece of work inside one directory.
- It limits a build to the area being changed.
- It turns the boundaries from a convention into something the build refuses
  to break.

## Errors

Every error has one shape, defined in `core`: a stable `code` and a `message`
for people. Each area owns its codes in its `contract`. Routes, commands and
the app all use that shape. Nothing returns a bare string.

The command line reports how a command ended in its exit code:

- 0: done.
- 1: the command ran and failed.
- 2: the command line was wrong (`bad_arguments`).
- 3: no node answers (`no_node`).

With `--json`, the command prints its result as JSON on stdout. It prints a
failure as the error shape on stderr: `{"error": {code, message}}`.

## Sessions survive

Terminal sessions belong to the holder process, not to the node. Installing,
upgrading or restarting Ahjo never stops a session.

When a node attaches a control client, tmux records the session as active.
Ahjo keeps the pane's earlier output time until the pane writes again.

## Install and upgrade

`ahjo install`, `ahjo upgrade` and `ahjo uninstall` are the binary's own verbs,
not commands of an area. They put the binary itself in place and restart the
node, so no node can own them. They have no MCP tool, because a node upgrading
itself would be stopped by its own restart before it could reply.

On Linux and WSL, the node runs as a systemd user unit. The unit is named
`ahjo`, unless `--unit` names another. It runs
`<root>/bin/ahjo serve --listen <addr>` with `KillMode=process`. So a restart
signals only the node, and a tmux server the node started keeps running.

- **Install** does the following:
  1. copies the build, and the app if `--app` is given, to `<root>/bin`
     (`<root>` is `~/.local` by default);
  2. writes the unit, enables it and starts it;
  3. waits until `/api/health` reports the build's version.

  Run again with nothing changed, install changes nothing. If the unit is
  already running another build, install refuses, because that is an upgrade.
- **Upgrade** refuses a unit without `KillMode=process`, before it changes
  anything. Otherwise it:
  1. reads the binary and the address from the unit's `ExecStart`;
  2. sets the installed build aside;
  3. puts the new build in its place;
  4. restarts the unit.

  If the new node is not healthy in time, upgrade puts back the build it set
  aside, restarts it, and fails with `upgrade_failed`. Once the new node is
  healthy, upgrade deletes the old build.
- **Uninstall** stops and disables the unit. It removes the unit and the
  binaries its `ExecStart` names. Sessions, and the node's config and state,
  stay.
- **Version.** A build's version is the package version plus its commit, such
  as `0.0.1+8cdb58b`. It shows in `ahjo --version`, in `ahjo version` and in
  the health response.
- **The address.** A node records the address it bound in its config folder
  (`listen`). So `ahjo` commands on the same machine reach the node without
  `--node`.
- **Apps for other machines.** A node serves the Windows builds of Swarm, Brain
  and Desk made from its own commit. Another machine on the tailnet installs
  the apps from that node, and so stays in step with it. Nothing is published.
  - `ahjo upgrade --windows-apps <dir>` checks that each app carries the new
    build's version. It puts the apps in `<root>/apps/windows/<version>/` with
    a manifest of sizes and SHA-256s. It removes every other version.
  - The node serves the manifest at `/apps/<platform>`, and each file the
    manifest names. It also serves `/install.ps1`. That script points at the
    node by the address the request used, and at `[node] desk`.
  - `irm <node>/install.ps1 | iex` puts the apps in `%LOCALAPPDATA%\Ahjo`, with
    an `install.json` that names the node. It sets Swarm's node and Desk's
    first desk, and adds Start-menu entries. It needs no administrator rights.
  - An app with `install.json` beside it asks the node for its build at these
    times: at start, when its window comes forward, and, for Swarm only, when
    its link opens or is refused. When the node's build differs from the app's
    own, the app shows Update in its chrome.
  - Update fetches every file and checks each one. It renames the running
    files aside, because Windows lets a running program's file be renamed but
    not replaced. It puts the new files in place and restarts the app with its
    arguments. In Swarm, the unsaved-work guard runs first.
  - When a node refused the app's protocol, the app says so, with Update as
    the action.
  - The operator's own apps have no `install.json` and never update this way.

`deploy/` holds the operator's own machine's setup:

- `ahjo.service`, as `ahjo install --root ~/.local/share/ahjo` wrote it,
  linked into `~/.config/systemd/user`. It listens on 127.0.0.1:7310. Windows
  Tailscale serves that as tailnet port 7300.
- the ChatGPT tunnel's unit and profile. They reach the node's `/mcp/chatgpt`
  tool set.

The `change-ahjo` skill has the steps.

`bin/ahjo/tests/install.rs` runs install, upgrade and uninstall against a
stand-in `systemctl` on `PATH`. The test uses a node, a tmux server and a
session of its own.

## Tests

An area's tests live in its crates and run against `testkit`. `testkit` starts
a node with only the named areas, on a port and a tmux server of its own.
`bin/ahjo` holds the few tests that need the whole binary.
