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
  for another device's file the status line and the reader say why it does
  not run.
- **The source.** Source (and Page) in the viewer's head switches between the
  page running and its text, which is the coloured, editable view every text
  file has. A path asked for at a line opens to the source at that line,
  from its first frame, so the page does not run; an unsaved draft keeps the
  file in its source until it is saved or discarded. A save of a file in the
  page's folder loads the page afresh when it shows again.
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
- **Whole files, bounded ranges.** The node streams a file or one byte range
  of it, with no size limit. The system web view takes each answer whole. A
  file the page asks for whole (a script, a stylesheet, an image) comes
  whole, however long, as a page needs it. A range, as a player asks for,
  holds at most 8 MiB: a longer one is cut to that much and said so in
  `Content-Range`, and the player asks for the rest.
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

- **Sound, video and PDF too.** The same web view plays a sound or video
  file and shows a PDF, from a terminal click and from Files alike. A PDF
  loads straight into the web view's own viewer. Sound and video play in a
  page of the app's own (`live::PLAYER`), which the scheme serves at the
  folder's root and which names the file in its query: the player reads the
  file from the folder in ranges as it plays and seeks, never whole. When
  the web view cannot play it (an error, or a video whose picture it does
  not decode) the page tells the app (`live::CannotShow`), and the reader
  says so in place of the player. On Linux these open in the system's own
  program, as before.
- **Every other file, its own program.** A file Files shows neither as text
  nor in a viewer (an office document, an archive, unknown bytes, one too
  large to read, a picture this computer does not decode) and media the
  web view cannot play offer Open in its Windows app beside Download. Both
  copy the file whole as it streams from the node, a piece at a time, so no
  size limit applies. They read it through a route of their own
  (`/api/files/file/whole`), the one file as the list shows it with its
  links resolved under the roots, as Download always read it: the folder
  route's bound is a page's, and a file the operator picked by name needs
  none. The program gets a copy in this computer's temporary files, under
  one safe local name (the extension kept), in a folder no other open
  shares; folders of opens more than a day old go. The app says it handed
  the file to Windows, which tells it no more.
- **3D models too.** A glb, a gltf, an obj, an fbx or an stl turns in the
  same web view, in a page of the app's own (`live::model_page`) that the scheme
  serves at the folder's root, naming the model in its query. It loads the
  model from the folder like any page's file, so what the model names
  beside it comes with it: a glTF's buffers and textures, an OBJ's
  materials and theirs. It draws with three.js and its loaders, which the
  app ships (`web/three.js`, rebuilt by `web/bundle-three` from a pinned
  release) and serves from a site no folder has (`live::APP_SITE`), so a
  model shows without the network. A drag turns it, the wheel zooms, Reset
  view goes back to where it opened. It draws when the view changes and
  when a file the model names arrives after it first showed (an OBJ's
  texture can), and at no other time. A file the model names that is not
  there is named on the page, whenever that turns out; a model that does not load at all tells the app why
  (`live::Failure::Model`), which says so with Open in its Windows app.
  Files no longer draws models itself: the small CPU preview of a glb it
  had drew one kind, untextured, and is gone. On Linux
  models open in the system's own program, as media do.
- **A page's word, on its own open.** The player or model page names the
  open it shows (`Opened::open_id`), and the app takes what it says only
  for that open, so a late error from an earlier open of the same file changes
  nothing.

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
- **The system's player, not one of our own.** WebView2 plays what a
  browser plays (H.264 and VP8/9 video, MP3, AAC, Opus, WAV) and shows PDFs,
  with its own controls and seeking; anything else goes to the program
  Windows chooses for it, which plays far more. A copy, not the node's
  address, because the program is on Windows and the file on the node.
- **three.js, shipped, not a viewer of our own.** A model needs its
  materials, textures and formats read as their makers meant; three.js's
  loaders do that for every format asked, and the web view draws on the
  GPU. Shipped with the app and served by it, so it works offline and is
  the version the app was tested with; a classic script from the app's own
  site, so no folder's page needs leave to load it.
- **Hide on overlap, not punch holes.** A child window cannot be drawn under
  the app's popups. Hiding it while anything overlaps keeps every popup
  visible and costs a moment of blank page; a popup elsewhere does not.
