# Design

What good looks like in the app. `VISION.md` says what Ahjo Swarm is for; this says
how a page earns its place and how the pages become one app.

## The aim

Delightful and opinionated. Every tab has one purpose, and its page is shaped
for that purpose alone: a chart where the question is about time, a canvas
where it is about a graph, a terminal where it is about a session. A page
that could be any page is not finished.

A page is judged by three questions, in this order:

1. **The first screen answers the page's question** without a click, a scroll
   or a choice, and uses the whole window to do it.
2. **The next step is one action away**, and whatever can be viewed can be
   edited where it is viewed.
3. **It is a pleasure to use**: it responds at once, it has something worth
   looking at, and it has made the choices so the operator does not have to.

## The purpose of each tab

- **Sessions.** Start an agent in the right folder in seconds, and see and
  reach every session. The launcher finds any project from a fragment of its
  name and remembers how the operator launches. Sessions are shared with
  other places. A session fits the window it is shown in; nothing opened over
  it resizes or disturbs it, and nothing over it lets a click or a key
  through to it. Peek is a full terminal on a session without a window:
  typed into, scrolled and selected as in a window, at the session's own
  size; Esc goes to the session, Shift+Esc or a click outside closes it,
  and Attach gives it a window sized to it.
- **Files.** Read and change what agents wrote, beside the session that wrote
  it. At least as good as VS Code at finding, reading and editing a file.
  Being redesigned from a prototype.
- **Agents.** See and edit what shapes an agent started in a given folder:
  instructions and where each comes from, skills, hooks as "this event does
  that", settings and tools. Skills has its own page for managing them and
  for what is out of sync; Roles has another.
- **Work.** See the open work across projects and how it moves over time.
  The strip comes first: a chart of what opened and finished each day over
  the open line (folded away to one line when the operator wants the
  room), facets for state and project, and the rows with type,
  state, priority and age. Items read at first sight to someone who has
  never used the tracker: states, kinds and priorities in plain words
  that say what they mean, the state marks keyed in the facets, and a
  group's items under it with how far it has come. Time gives the chart the page and says what
  opened, finished or sat idle in the days dragged across it. Tree keeps
  one project's work as parents and children. Every chart reads out the
  day under the mouse, and the item beside the list shows its relations
  as cards.
- **Missions.** Chain sessions into repeatable runs of several agents. A
  person does not build a mission by hand: they sketch it in seconds (a
  name, what it is for, a few rough steps) and write notes on it, on the
  mission, a step, a gate, a limit or a wire, as plain multi-line text where
  they read it. An agent picks it up from one command (`ahjo mission
  inspect`), builds a version that runs, and answers the notes in place,
  resolving the ones it took up. The page exists so a person understands a
  complex flow: in what order the steps run, who does each, and where the
  gates and limits are and what each checks or caps, readable without
  opening a card. Where the window is wide, the words and the picture
  stand together: Flow, the steps in order, beside the whole board, and a
  step chosen in one is marked and brought into view in the other. Notes
  read as annotations, quiet beside the flow, not banners over it. The
  graph is a diagram: typed cards, coloured ports, visible loops and
  gates. A step uses a named agent's live session or
  starts one; a mission's sessions are ordinary sessions under the
  mission's prefix.
- **Scheduled.** What will run when, and how the last run went.
- **Host.** Is this machine all right now, with services as part of the
  answer.
- **Settings.** Every setting, said plainly, changed in place.

The Sessions and Files panels are available on every tab. The Missions tab
adds its own panel of missions and runs.

## One app

- **Odin's ravens.** Ahjo's art and theme come from Huginn and Muninn,
  thought and memory, who fly out and report back. Empty places carry the
  art; the app's own themes are dark, with their own character. Beside
  them stand the colour themes people know from their editors, two of them
  light, with the raven drawn in their colours; every page reads in each.
- **Colour has fixed meanings.** One colour for action and focus, one for
  done, one for failed, one for "needs you", one for running. A colour never
  carries two of them.
- **Type has a hierarchy.** Headers, group labels, numbers worth reading at a
  glance and comfortable body text are different sizes and weights, all from
  tokens.
- **One zoom.** What the operator reads or works in (a terminal, code and
  its changes, a picture, a page read) zooms the same way: Ctrl+wheel over
  it, Ctrl+= and Ctrl+-, Ctrl+0 back. Each keeps its own size across runs
  and says it when it changes. Anywhere else (a list, a form, the bars, a
  dialog) the same gestures zoom the whole app, kept as Zoom in Settings:
  everything grows together, so every page lays out at 130% as at 100%.
  Terminals keep their own text size through it; the rest zoom on top.
- **Words only when they change what you do.** Keys live in tooltips and the
  Keys overlay.
- **Send is the same everywhere.** Anything you can look at can be sent to a
  session, the same way, from the same place, and Send opens over the page
  you are on, which stays where it was.
- **Beside, not over.** What opens from a session docks beside the terminals
  and leaves them as they were.
- **Fast.** A page shows what it has at once and fills in; nothing blocks on
  a read, nothing waits a fixed time, and typing in a search answers as you
  type.
- **Sizes.** Right at 1456x939, at a wide desktop and narrow; names shorten
  in the middle, primary actions never clip.

## When in doubt

Ask what the tab is for and whether this serves it. A half-finished feature
or a number nobody acts on makes the page worse; remove it and say so.
