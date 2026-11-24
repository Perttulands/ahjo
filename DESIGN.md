# Design

This file says what Ahjo is meant to be, and the principles every screen is
built on. `VISION.md` says why Swarm exists, and `ARCHITECTURE.md` says how it
is built. This file changes only when the design changes. Open questions and
work in progress live in Beads and on the desk. The operator's own words behind
these principles are kept in the brain (`interface-taste`, and the Ahjo desk
records).

## Three apps, one family

- **Ahjo Swarm** makes tmux sessions great. You start agents in the folder
  where they belong, see and reach every session, see what shapes each agent
  and what work is open, and chain agents into missions.
- **Ahjo Desk** is where an agent shows the operator something real, and he
  answers it in his own words.
- **Ahjo Brain** is where we explore what we know and add to it.

The three share one look and one kit, and each works on its own. Agents make
things through the command line. The apps are where a person understands what
was made, and makes small edits right where he reads.

## What they are made of

- A **mission** is a plan of steps, gates and ends. It is a document an agent
  writes and a person can understand at a glance.
- A **step** is one piece of the work. **Agents** sit in its seats and work on
  an **issue**.
- A **gate** decides on a step's result. It has a condition, someone who
  decides it (an agent, code or the operator), and for each answer, where the
  work goes next.
- An **agent** is a named setup you can reuse. It is made good and keeps
  getting better. What it has learned is written down beside it.
- A **run** is one time a mission is carried out. Its agents work in ordinary
  sessions, and the operator can talk to them while it runs. A step's seats
  share one workspace, and keep it when the step is tried again. The run closes its sessions,
  unless the operator started it with the option to keep them.
- A **note** is a thought a person or an agent puts on the wall, about a
  mission or one of its steps.
- A **desk ask** is something real put in front of the operator (pictures, a
  clip, a page, a document), with the agent's reasoned view, waiting for his
  answer.
- **The brain** keeps three kinds of thing: originals, which never change;
  extractions, which models can write again; and understanding, which is
  edited with care.

## Principles

**One purpose per surface.** Every app, page and panel exists to answer one
question, and is shaped for that question alone. Use a chart when the question
is about time, a canvas when it is about a flow, and a terminal when it is
about a session. A page that could be any page is not finished. The first
screen answers its question without a click, and the next step is one action
away.

**Understanding comes first.** A person should see at a glance what is in front
of him: what a mission does, what changed, what is being asked. Things look
like what they are: a gate looks like a gate, a loop like a loop, and a choice
like something chosen.

**Everything earns its place.** Each thing on screen does at least one of three
jobs: it tells him something he would not otherwise know, it is what he acts
on, or it delights him. Filler that says nothing never earns its place. Filler
is, for example, a label that repeats what the layout already shows, a small
line of text above a title, helper text for a control everyone knows, or a
decorative dot. Delight is not filler: character, play and a beautiful detail
are reasons enough.

**Show, then say.** What someone did is drawn. What someone says is written,
under their name. You see a choice in how the chosen thing looks, not in a tag
beside it.

**Understand change; approve nothing.** The operator sees what changed and who
changed it, as a difference he can read. He is never asked to sign off on
edits. When a decision truly is his, it is part of the plan, made in the place
where it belongs.

**Free expression.** He answers in his own words and by marking what he sees.
He throws a note onto the wall without choosing where it belongs. Nothing
forces him into a form, a fixed set of options, or a structure he did not ask
for.

**Never disturb the work.** What opens beside the work makes room for itself:
the windows get narrower. It never covers the work and never moves anything.
It takes the keyboard only when the operator opens it, and then puts the
keyboard into what he came for. For example, Alt+L gives the keyboard to the
search when the list opens, and to the reader when a page reopens. It never
holds on to the keyboard: a click on the work takes the keyboard back, and
closing it gives the keyboard back to the work.

**Act where it is.** An action you start from a thing happens at that thing. A
rename edits the name where the name is shown. A small form opens beside what
you clicked, moved as needed to stay on screen. Your eyes and hand stay where
they were. The place shows what the action works on, so it needs no title. It
covers the neighbours, not the work. Only a decision about the whole window
sits in the centre.

**Fast and forgiving.** A page shows what it already has at once and fills in
the rest. Every search finds results as you type, and forgives a mistyped
letter. The same gesture does the same thing everywhere: one way to find, one
way to send, one way to zoom.

**Reviewed before it is shown.** No screen reaches the operator before someone
has looked at it. A person, helped by cheap checks, looks at every picture
before it is sent.

## Look and feel

- **Odin's ravens.** Huginn and Muninn (thought and memory) fly out and report
  back. They give Ahjo its art and its own dark themes, and empty places show
  their art. Beside these stand the themes people know from their editors,
  light and dark. Every page reads well in each theme.
- **Colour has fixed meanings.** There is one colour each for action and focus,
  done, failed, needs the operator, and running. A colour never carries two of
  these meanings, and colour is never decoration.
- **Emphasis is shaped, not striped.** To select or emphasise a thing, lift the
  thing itself: its border or its background. A thin coloured bar at an edge is
  not used.
- **Type has a hierarchy**: headers, group labels, numbers big enough to read
  at a glance, and comfortable body text. All of it comes from tokens (the
  theme's shared values).
- **Reads everywhere**: on a laptop and on a wide desktop, at any zoom. Text
  that matters is never cut off.

## The surfaces

**Swarm**

- **Sessions**: start an agent in the right folder in seconds; see and reach
  every session; arrange them the way the operator likes and keep that
  arrangement.
- **Files**: read and change what agents wrote, beside the session that wrote
  it, at least as well as a good editor does.
- **Harness**: what shapes an agent started in a folder: its instructions and
  where they come from, skills, hooks, settings and tools.
- **Agents**: each reusable agent, where it is used, and how it has done.
- **Issues**: what work is open across projects, and how it moves over time.
- **Missions**: a mission understood at a glance (its steps, who does them, its
  gates and loops), its notes, and what has changed in it. A run shows on the
  same board, with a way to talk to its agents.
- **Scheduled**: what will run when, and how the last run went.
- **Host**: is this machine all right now?
- **Settings**: every setting, explained plainly, changed where it is shown.

**Desk**: an immersive review. The thing being asked about fills the screen.
Options are real made things to compare, and each carries the asking agent's
reason where it has one. The operator's answer is his own words and marks.

**Brain**: a really good editor and note app over what we know. A quick note
lands without any fuss, and agents file it. Exploring the whole brain is a
pleasure.

## When in doubt

Ask what the surface is for, and whether this thing serves that. A
half-finished feature, or a number nobody acts on, makes the surface worse:
remove it, and say that you did.
