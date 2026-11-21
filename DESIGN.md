# Design

The one map of Ahjo's three apps: what each is for, the things they are made
of, the rules every screen keeps, and what is still open. `VISION.md` says
why Swarm exists; `ARCHITECTURE.md` says how it is built. Every new Bead and
prototype is checked against this file before it is built or shown, and a
desk answer that changes the map changes this file in the same piece of work.
Desk answers are cited by number (`desk show AH13`); the brain keeps them word
for word.

## The family

- **Ahjo Swarm** makes tmux sessions great: launch agents, see and reach every
  session, see what shapes them and what is open, chain them into missions.
- **Ahjo Desk** is the shared surface where an agent publishes something (a
  picture, a clip, a rendered page, a question) and the operator answers
  freely, in prose and by marking what he sees.
- **Ahjo Brain** is for exploring what we know and adding to it: a good editor
  and note app over the brain's pages.

One app kit, one look, separate apps; none opens another. Each has a CLI an
agent can use for everything the app does, and the app is where the operator
understands it. Agents make most things (missions, agents, notes) through the
CLI.

## The things

One word for one thing, from the CLI to the stored file to the screen (the
full glossary goes into `ARCHITECTURE.md` with `ahjo-biq.34`).

- A **mission** is a plan: steps, gates and ends, written as one document an
  agent exports, edits and applies. The operator sketches it; an agent builds
  it.
- A **step** is one piece of the work. Its **seats** hold **agents**, working
  together in a **formation** (solo, peers, or a lead with workers). A step
  works on an **issue** (Beads or GitHub).
- A **gate** is the question a step's result must pass, with its condition in
  words and an exit for each answer. A gate is decided by an agent, by code,
  or by the operator.
- An **agent** is a reusable named setup (instructions, harness, model,
  effort) that is made good and kept improving; agents write notes on how it
  has done. The **harness** is the program it runs in (Claude Code, Codex).
- A **run** is one execution of a mission. Each seat has a **session** that
  lives as long as the run; a retry continues in the same workspace, and each
  attempt is kept. A run has its own chat between the operator and its agents.
- A **note** is text on the wall, written by the operator or an agent: on the
  mission or on a step. Nothing forces a note onto an anchor.
- A **desk ask** is a question, a look or a document put to the operator, with
  options in the real medium, the asking agent's reasoned recommendation, and
  his free answer.
- A **brain page** is one of three layers: originals that never change (desk
  answers, grillings, stored sources), extractions that models write and can
  write again, and understanding (topics, project homes, Self pages).

## How a page earns its place

Delightful and opinionated. Every page has one purpose and is shaped for it
alone: a chart where the question is about time, a canvas where it is about a
graph, a terminal where it is about a session. A page that could be any page
is not finished. A page is judged by three questions, in this order:

1. **The first screen answers the page's question** without a click, a scroll
   or a choice, and uses the whole window to do it.
2. **The next step is one action away**, and whatever can be viewed can be
   edited where it is viewed.
3. **It is a pleasure to use**: it responds at once, it has something worth
   looking at, and it has made the choices so the operator does not have to.

## Rules every screen keeps

- **Every pixel earns its place** (AH9). Whatever does not help the operator
  understand what is in front of him or act on it is a cost to remove.
- **Words only when they add information.** A label earns its place only if he
  would otherwise not know what it says. A label that repeats what the layout
  shows ("You decide", "Agent's pick", "Your lean"), a kicker above a title,
  helper text for a standard control, or a decorative dot or ring reads as
  unreviewed AI output (AH13, AH14). What someone did is drawn; what someone
  says is written, in their name. Keys live in tooltips and the Keys overlay.
- **Show what changed; ask for nothing to be approved** (AH13). Changes are
  readable as diffs with who and when. A gate the operator decides is a
  decision in the plan, not a sign-off on edits.
- **No structure nobody asked for** (AH13). No grouping layer between a mission
  and its steps, no intermediate screen before the real thing.
- **Checked before it reaches him.** A screen goes to the operator only after
  `ui-check` and a person's look at every picture; a clean check means no
  obvious noise, not approval.
- **Colour has fixed meanings.** One colour for action and focus, one for
  done, one for failed, one for "needs you", one for running. A colour never
  carries two of them.
- **Type has a hierarchy.** Headers, group labels, numbers worth reading at a
  glance and comfortable body text are different sizes and weights, all from
  tokens.
- **Odin's ravens.** Ahjo's art and theme come from Huginn and Muninn, thought
  and memory, who fly out and report back. Empty places carry the art; the
  app's own themes are dark, with their own character. Beside them stand the
  colour themes people know from their editors, two of them light, with the
  raven drawn in their colours; every page reads in each.
- **One zoom.** What the operator reads or works in (a terminal, code and its
  changes, a picture, a page read) zooms the same way: Ctrl+wheel over it,
  Ctrl+= and Ctrl+-, Ctrl+0 back. Each keeps its own size across runs and
  says it when it changes. Anywhere else the same gestures zoom the whole app,
  kept as Zoom in Settings, so every page lays out at 130% as at 100%.
  Terminals keep their own text size through it.
- **Send is the same everywhere.** Anything you can look at can be sent to a
  session or several, the same way, from the same place, and Send opens over
  the page you are on, which stays where it was. Where a rule holds part of it
  back, it says so and offers to send anyway.
- **One way to find a thing.** Every list one searches finds as one types,
  forgivingly: letters apart, words in any order, a slipped letter. A list to
  pick from marks the letters that found each row, the keyboard does it all
  (Up, Down, Enter, Tab to mark another), and chips narrow a long one by kind.
- **Beside, not over.** What opens from a session docks beside the terminals
  and leaves them as they were.
- **Fast.** A page shows what it has at once and fills in; nothing blocks on a
  read, nothing waits a fixed time, and typing in a search answers as you type.
- **Sizes.** Right at 1366x768, 1456x939 and a wide desktop; text that matters
  is never cut off, names shorten in the middle, primary actions never clip.

## Swarm's tabs

- **Sessions.** Start an agent in the right folder in seconds, and see and
  reach every session. The launcher finds any project from a fragment of its
  name and remembers how the operator launches. Sessions are shared with
  other places. A session fits the window it is shown in; nothing opened over
  it resizes or disturbs it, and nothing over it lets a click or a key
  through to it. Send is the one thing the windows make room for, since one
  writes to sessions with it while reading them. Peek is a full terminal on a
  session without a window: typed into, scrolled and selected as in a window,
  at the session's own size; Esc goes to the session, Shift+Esc or a click
  outside closes it, and Attach gives it a window sized to it. The windows are
  the operator's to arrange: a session dragged to any window's edge or middle,
  every divider moved, and an arrangement kept as a view on Alt+Shift+1 to 9,
  recalled as an RTS recalls a control group. Ended and long-idle sessions are
  offered for clearing once, quietly; a live one goes only on the operator's
  click.
- **Files.** Read and change what agents wrote, beside the session that wrote
  it. At least as good as VS Code at finding, reading and editing a file.
- **Harness.** See and edit what shapes an agent started in a given folder:
  instructions and where each comes from, skills, hooks as "this event does
  that", settings and tools. Skills has its own page for managing them and
  for what is out of sync.
- **Agents.** Each agent: what it is (instructions, harness, model, effort),
  where it is used, and its notes on how it has done, newest first. For
  understanding, with light editing; agents improve agents through the CLI.
- **Issues.** See the open issues across projects and how they move over
  time. The strip comes first: a chart of what opened and finished each day
  over the open line (folded away to one line when the operator wants the
  room), facets for state and project, and the rows with type, state,
  priority and age. Issues read at first sight to someone who has never used
  the tracker: states, kinds and priorities in plain words, the state marks
  keyed in the facets, and a group's issues under it with how far it has come.
  Time gives the chart the page and says what opened, finished or sat idle in
  the days dragged across it. Tree keeps one project's issues as parents and
  children. Every chart reads out the day under the mouse, and the issue
  beside the list shows its relations as cards.
- **Missions.** Understand a mission at a glance, and what has changed in it.
  - **The board** is a diagram: steps as compact cards (number, name, who
    works it); gates with their condition in full and an exit for each answer;
    loops drawn with their limit in words ("up to 3 tries, then it fails");
    ends. Colour marks only where a gate leads and what needs the operator. A
    long mission wraps into rows and keeps a loop whole.
  - **Notes** come at two levels (AH13): mission notes in the side panel, and
    step notes behind a small count on the step that opens them. They are a
    place to throw an idea or feedback onto the wall without opening a
    terminal; agents write them too.
  - **The side panel** holds what the mission is for, its notes, and every
    saved change as a diff with who and when, newest first. Choosing a change
    marks the cards it touched.
  - **Authoring** is a chat with an agent that edits the mission as a document;
    its changes land and appear in the history like anyone's.
  - **A run** shows on the same board, and its chat is where the operator and
    the run's agents reach each other.
  - Free shapes on the canvas (a coloured box around steps) may come later;
    nothing needs them yet (AH13).
- **Scheduled.** What will run when, and how the last run went.
- **Host.** Is this machine all right now, with services as part of the
  answer.
- **Settings.** Every setting, said plainly, changed in place.

The Sessions panel stands at the left on every tab. The Files panel stands at
an edge of its own on the other tabs, the right unless set to the left, and
goes and comes back with Ctrl+B as in VS Code (Alt+E from a terminal, which
keeps its Ctrl+B). The Missions tab adds its own panel of missions and runs.
Swarm has no Brain tab: Alt+L lays a Brain panel over the terminals to search
the brain, read a page and send it to a session.

## Desk

The point is immersion: the operator sees the thing itself, large, and
answers it in his own words (AH2, AH9).

- **The list** of asks is a collapsible side panel, sortable; the ask and its
  media fill the rest, with dividers he can drag. Settings and filler stay
  behind a button (AH9).
- **Options are cards**, each with its own media; the asking agent's reason
  sits on the option it argues for, in its name (AH14). What he chose is drawn
  on the card, not labelled.
- **His answer** is one free text field per question, with his marks at its
  head. He is never forced into multiple choice; "answered elsewhere" is
  always open (2026-11-17, AH14).
- **Marking** a picture or a rendered page uses tools with icons, including a
  note, and a mark sits next to what it marks (AH2, AH9).
- Agents post asks by the desk skill, with their screens checked first; the
  desk refuses media outside its work folder.

## Brain

- **A really good editor and note app** (AH6, AH8): pages edited in place with
  a live view, `[[links]]` that complete, backlinks, the YAML header hidden.
  Ctrl+N catches a note into the inbox that the gardener files; Ctrl+D opens
  today's note.
- **Exploring** what we know: search that says when nothing matches, and the
  atlas, the brain as a night sky. Other machines' brains read the same way.
- **Layers** decide who may change a page: originals are never edited,
  extractions are regenerated in bulk, understanding is edited carefully and
  reviewed (the gardener's plan, AH15).

## Open

- **The board's gate look** (fork, under its step, or its own card) and
  whether the side panel is right: AH13 round 2, prototype `proto/board-v3`.
- **The Desk answer panel**: how the chosen option is drawn and where the
  recommendation sits: AH14 round 2, prototype `proto/desk-panel-2`. The list's
  kickers ("LOOK", "DECIDE", "your draft") are next.
- **The gardener and the brain's review**: AH15 round 2. Search fixes are
  live; the gardener is stopped until it is settled.
- **Epic `ahjo-biq` predates AH13 in two places**: its notes model
  ("annotations and change threads", resolved comments) and an authoring chat
  whose changes wait for acceptance. Both are rewritten to this file before
  they are built.

## When in doubt

Ask what the page is for and whether this serves it. A half-finished feature
or a number nobody acts on makes the page worse; remove it and say so.
