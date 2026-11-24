# Design

This file says what Ahjo should be, and lists the principles every screen
follows. Read it before you change anything on screen.

- `VISION.md` says why Swarm exists.
- `ARCHITECTURE.md` says how Ahjo is built.
- `GLOSSARY.md` defines the words used here.

Change this file only when the design changes. Open questions and work in
progress belong in Beads and on the desk. The operator's own words behind these
principles are in the brain: the `interface-taste` page and the Ahjo desk
records.

## Three apps, one family

- **Ahjo Swarm** is the workspace for tmux sessions. In Swarm you:
  - start agents in the folder where they belong;
  - see and reach every session;
  - see what shapes each agent, and what work is open;
  - chain agents into missions.
- **Ahjo Desk** is where an agent shows the operator something real. The
  operator answers in his own words.
- **Ahjo Brain** is where we explore what we know and add to it.

The three apps share one look and one kit. Each app also works on its own.

Agents make things through the command line. A person uses the apps to
understand what agents made, and to make small edits in the place where he
reads.

## What they are made of

- A **mission** is a plan of steps, gates and ends. An agent writes it as a
  document, and a person can understand it at a glance.
- A **step** is one piece of the work. **Agents** sit in the step's seats and
  work on an **issue**.
- A **gate** decides on a step's result. A gate has three parts:
  - a condition;
  - who decides it: an agent, code or the operator;
  - for each answer, where the work goes next.
- An **agent** is a named setup you can reuse. It is made good and keeps
  getting better. What the agent has learned is written down beside it.
- A **run** is one time a mission is carried out.
  - The run's agents work in ordinary sessions. The operator can talk to them
    while the run is going.
  - A step's seats share one workspace. They keep that workspace when the step
    is tried again.
  - When the run ends, it closes its sessions. It keeps them only if the
    operator started it with the option to keep them.
- A **note** is a thought that a person or an agent puts on the wall. A note
  is about a mission or one of its steps.
- A **desk ask** puts something real in front of the operator, such as
  pictures, a clip, a page or a document. It carries the agent's reasoned view
  and waits for the operator's answer.
- **The brain** keeps three kinds of thing:
  - **originals**, which never change;
  - **extractions**, which models can write again;
  - **understanding**, which is edited with care.

## Principles

**One purpose per surface.** Give every app, page and panel one question to
answer, and shape it for that question alone.

- When the question is about time, use a chart.
- When the question is about a flow, use a canvas.
- When the question is about a session, use a terminal.

A page that could be any page is not finished. The first screen answers its
question without a click. The next step is one action away.

**Understanding comes first.** A person sees at a glance what is in front of
him: what a mission does, what changed, and what is being asked. Draw each
thing so it looks like what it is. A gate looks like a gate, a loop looks like
a loop, and a choice looks chosen.

**Everything earns its place.** Show a thing only if it does at least one of
these jobs:

1. **Informs**: it tells the operator something he would not otherwise know.
2. **Is acted on**: it is the thing he acts on.
3. **Delights**: it has character, play or a beautiful detail. Delight is a
   reason on its own, not filler.

Remove filler that says nothing. Examples of filler:

- a label that repeats what the layout already shows;
- a small line of text above a title;
- helper text for a control everyone knows;
- a decorative dot.

**Show, then say.** Draw what someone did. Write what someone says, under that
person's name. Show a choice through how the chosen thing looks, not with a
tag beside it.

**Understand change; approve nothing.** Show the operator what changed and who
changed it, as a difference he can read. Never ask him to sign off on edits.
When a decision really is his, make it a part of the plan, in the place where
that decision belongs.

**Free expression.** The operator answers in his own words and by marking what
he sees. He throws a note onto the wall without choosing where it belongs.
Never force him into a form, a fixed set of options, or a structure he did not
ask for.

**Never disturb the work.** Anything that opens beside the work, such as the
panel, follows these rules:

- It makes room for itself: the windows get narrower.
- It never covers the work and never moves anything.
- It takes the keyboard only when the operator opens it.
- When it takes the keyboard, it puts the keyboard into what he came for.
  Example: when Alt+L opens the list, the search gets the keyboard; when Alt+L
  reopens a page, the reader gets it.
- It never holds on to the keyboard. A click on the work takes the keyboard
  back to the work.
- When it closes, the keyboard goes back to the work.

**Act where it is.** When you start an action from a thing, the action happens
at that thing. The operator's eyes and hand stay where they were.

- A rename edits the name where the name is shown.
- A small form (a popup) opens beside what you clicked. It moves as needed to stay on screen.
- The form needs no title, because its place shows what it works on.
- The form may cover neighbouring things, but never the work.
- Only a decision about the whole window opens in the centre.

**Fast and forgiving.**

- A page shows what it already has at once, then fills in the rest.
- Every search shows results as you type, and forgives a mistyped letter.
- The same gesture does the same thing everywhere: one way to find, one way to
  send, one way to zoom.

**Reviewed before it is shown.** Someone looks at every screen before it
reaches the operator. A person, helped by cheap checks, looks at every picture
before it is sent.

## Look and feel

- **Odin's ravens.** Huginn and Muninn are thought and memory: they fly out and
  report back. They give Ahjo its art and its own dark themes. Empty places
  show the ravens' art. Ahjo also has the light and dark themes people know
  from their editors. Every page must read well in every theme.
- **Colour has fixed meanings.** Each of these meanings has one colour:
  - action and focus;
  - done;
  - failed;
  - needs the operator;
  - running.

  Never use one colour for two of these meanings. Never use colour as
  decoration.
- **Emphasis is shaped, not striped.** To select or emphasise a thing, change
  the thing itself: its border or its background. Do not add a thin coloured
  bar at its edge.
- **Type has a hierarchy.** It has headers, group labels, numbers big enough
  to read at a glance, and comfortable body text. Take every type style from
  the theme's tokens (its shared values).
- **Reads everywhere.** Every page reads well on a laptop and on a wide
  desktop, at any zoom. Never cut off text that matters.

## The surfaces

**Swarm**

- **Sessions**: start an agent in the right folder in seconds. See and reach
  every session. Arrange sessions the way the operator likes, and keep that
  arrangement.
- **Files**: read and change what agents wrote, beside the session that wrote
  it. Editing works at least as well as in a good editor.
- **Harness**: what shapes an agent started in a folder: its instructions and
  where they come from, skills, hooks, settings and tools.
- **Agents**: each reusable agent, where it is used, and how it has done.
- **Issues**: what work is open across projects, and how it moves over time.
- **Missions**: a mission understood at a glance, with its steps, who does
  them, its gates and its loops. The page also shows the mission's notes and
  what has changed in it. A run shows on the same board, with a way to talk to
  the run's agents.
- **Scheduled**: what will run when, and how the last run went.
- **Host**: whether this machine is all right now.
- **Settings**: every setting, explained plainly and changed where it is shown.

**Desk**: an immersive review.

- The thing being asked about fills the screen.
- Options are real made things to compare. Each option shows the asking
  agent's reason, when the agent gave one.
- The operator answers in his own words and with marks.

**Brain**: a really good editor and note app over what we know.

- A quick note goes in with no extra steps, and agents file it later.
- Exploring the whole brain is a pleasure.

## When in doubt

Ask what the surface is for, and whether this thing serves that purpose. A
half-finished feature, or a number nobody acts on, makes the surface worse.
Remove it, and say that you removed it.
