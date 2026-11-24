# A web page in Files runs in a web view, served from its own folder by the app

Agents make HTML prototypes: a page with its own scripts, styles and assets
beside it, sometimes a library from a CDN. Files showed them as text, and the
operator had to find the file on disk to see one run. Now a `.html` or `.htm`
file opened in Files, from the tree or a path clicked in a terminal, runs in
the reader or the panel, its source a switch away.

## Decision

- **A web view.** On Windows the page runs in the kit's web view
  (`web_view::WebSurface`, WebView2 through `wry`), laid over the reader's
  body. On Linux a GPUI window has no web view it can hold (one needs GTK's
  event loop), so the page opens in the system's browser, from its own path,
  once each time the operator opens it: not as it draws, not as a tab coming
  back, not at start, and not when it was asked for at a line. Only the
  serving node's files reached on this computer's own address open that way;
  for another device's file the reader says why it does not run.
- **The source.** Source (and Page) in the viewer's head switches between the
  page running and its text, which is the coloured, editable view every text
  file has. A path asked for at a line opens to the source at that line, and
  an unsaved draft keeps the file in its source until it is saved or
  discarded.
- **Its own folder, through the app.** The page loads from a scheme of the
  app's own (`ahjo-file`), at `http://ahjo-file.<site>.localhost/<path>`,
  where `<site>` is a hash of the node and folder. The app answers every
  request itself (`live::Live`), reading it over its existing link from the
  node's read-only folder route (`/api/files/folder/file`), which names the
  file by the opened folder and a relative path. Both refuse a path that
  climbs out of the folder; the node also refuses one whose links resolve
  outside it, even into another folder under the same root. The page never
  gets the node's origin, its authority or another folder: each folder is a
  site of its own, and it may reach the network (a CDN) as any page can.
- **Bounded answers.** The node streams a file or one byte range of it, with
  no size limit. The system web view takes each answer whole, so the app
  answers at most 8 MiB at a time: a longer range is cut to that much and
  said so in `Content-Range`, and a player asks for the rest.
- **Hidden while covered.** A web view is a child window that draws above
  everything the app draws, so the kit decides once a frame, after the stack
  is drawn (`frame::stack` → `web_view::settle`), whether each one shows: it
  shows where its surface was painted this frame and no popup, menu or layer
  overlaps it. A popup elsewhere leaves it shown; Back, another file and the
  panel put away stop painting it, which hides it.
- **Escape order.** With the keyboard in the page, Escape goes to the page
  first. The kit's script in every page hands it back when the page left it
  (it did not prevent it, no dialog of its own was open, nothing in the page
  changed while the key ran). The app then takes the keyboard back from the
  page and presses Escape where the page's owner has its stand-in focus
  (`WebViewSetup::focus`), so it closes what ADR 0002 says, once: the panel,
  or its close guard when the file has a draft (`PanelView::may_close`).
  Cancelling the guard returns the keyboard to the panel. A press anywhere
  else in the window takes the keyboard back too. Terminal and Peek keys
  never pass through a page.

## Why

- **Not a viewer of our own.** A prototype is only seen by running it:
  its scripts, its layout, its libraries. Rendering HTML natively would be a
  browser engine; the system has one.
- **Not only the system's program.** Opening a copy in the browser breaks
  its relative files and takes the operator out of the app, which is what
  the panel exists to avoid (ADR 0001). Where the window can hold a web view,
  it runs beside the work. Linux opens the real path so the relative files
  still load.
- **The app's own scheme, not the node's address.** Loading from the node
  would give the page the node's origin, where its scripts could call the
  node's API (the node's route sends `Content-Security-Policy: sandbox` for
  that reason), and would need the node reachable from the web view. Served
  through the app, the page has only its folder.
- **Hide on overlap, not punch holes.** A child window cannot be drawn under
  the app's popups. Hiding it while anything overlaps keeps every popup
  visible and costs a moment of blank page; a popup elsewhere does not.
