# Design

What Ahjo is designed to be, and the principles every screen is made from.
`VISION.md` says why Swarm exists and `ARCHITECTURE.md` how it is built. This
file changes only when the design does; open questions and work in progress
live in Beads and on the desk. The operator's own words behind these
principles are kept in the brain (`interface-taste`, and the Ahjo desk
records).

## Three apps, one family

- **Ahjo Swarm** makes tmux sessions great: start agents where they belong,
  see and reach every session, see what shapes them and what is open, and
  chain them into missions.
- **Ahjo Desk** is where an agent shows the operator something real and he
  answers it in his own words.
- **Ahjo Brain** is where we explore what we know and add to it.

They share one look and one kit, and each stands on its own. Agents make
things through the CLI; the apps are where a person understands them, and
edits lightly where he reads.

## What they are made of

- A **mission** is a plan of steps, gates and ends: a document an agent
  writes and a person can read at a glance.
- A **step** is one piece of the work. **Agents** sit in its seats and work
  on an **issue**.
- A **gate** is a decision on a step's result: a condition, who decides it
  (an agent, code or the operator), and where each answer leads.
- An **agent** is a named, reusable setup that is made good and keeps getting
  better; what it has learned is written down beside it.
- A **run** is one time a mission is carried out. Its agents work in ordinary
  sessions, and the operator can talk to them while it runs. A step's seats
  share a workspace across retries. The run closes its sessions unless the
  operator starts it with the option to keep them.
- A **note** is a thought put on the wall by a person or an agent, about a
  mission or one of its steps.
- A **desk ask** is something real put in front of the operator (pictures, a
  clip, a page, a document) with the agent's reasoned view, waiting for his
  answer.
- **The brain** keeps originals that never change, extractions models can
  write again, and understanding that is edited with care.

## Principles

**One purpose per surface.** Every app, page and panel exists for one
question and is shaped for it alone: a chart where the question is about
time, a canvas where it is about a flow, a terminal where it is about a
session. A page that could be any page is not finished. The first screen
answers its question without a click; the next step is one action away.

**Understanding comes first.** A person should grasp what is in front of
him at a glance: what a mission does, what changed, what is being asked.
Things look like what they are: a gate looks like a gate, a loop like a
loop, a choice like something chosen.

**Everything earns its place.** By telling him something he would not
otherwise know, by being what he acts on, or by delight. Filler that says
nothing never does: a label repeating what the layout shows, a kicker over a
title, helper text for a familiar control, a decorative dot. Delight is not
filler; character, play and a beautiful detail are reasons enough.

**Show, then say.** What someone did is drawn; what someone says is written,
in their name. A choice is visible in how the chosen thing looks, not in a
tag beside it.

**Understand change; approve nothing.** The operator sees what changed and
who changed it, as a readable difference. He is never asked to sign off on
edits. A decision that is truly his is part of the plan, made where it
belongs.

**Free expression.** He answers in prose and by marking what he sees, and
throws a note onto the wall without choosing where it belongs. Nothing forces
him into a form, a fixed set of options, or a structure he did not ask for.

**Never disturb the work.** What he is working in stays as it was: nothing
opened beside a terminal resizes it, takes its keys, or moves it. Things open
beside, not over.

**Act where it is.** An action opened from a thing happens at that thing:
a rename edits the name where it is shown, and a small form opens beside
what was clicked, moved to stay on screen. The eye and hand stay put, the
place says what it acts on, so it needs no title, and it covers the
neighbours, not the work. Only a decision about the whole window sits
centred.

**Fast and forgiving.** A page shows what it has at once and fills in. Every
search finds as one types and forgives a slipped letter. The same gesture
does the same thing everywhere: one way to find, one way to send, one way to
zoom.

**Reviewed before it is shown.** No screen reaches the operator that no one
has looked at. A person, helped by cheap checks, looks at every picture
before it is sent.

## Look and feel

- **Odin's ravens.** Huginn and Muninn, thought and memory, who fly out and
  report back, give Ahjo its art and its own dark themes; empty places carry
  the art. Beside them stand the themes people know from their editors, light
  and dark, and every page reads in each.
- **Colour has fixed meanings**: one for action and focus, one for done, one
  for failed, one for what needs the operator, one for what is running. A
  colour never carries two of them, and colour is never decoration.
- **Emphasis is shaped, not striped.** Selection and emphasis lift the thing
  itself (its border, its ground); a thin coloured bar at an edge is not used.
- **Type has a hierarchy**: headers, group labels, numbers worth reading at a
  glance and comfortable body text, all from tokens.
- **Reads everywhere**: on a laptop and a wide desktop, at any zoom; text that
  matters is never cut off.

## The surfaces

**Swarm**

- **Sessions**: start an agent in the right folder in seconds; see and reach
  every session; arrange them as the operator likes and keep the arrangement.
- **Files**: read and change what agents wrote, beside the session that wrote
  it, at least as well as a good editor.
- **Harness**: what shapes an agent started in a folder: instructions and
  where they come from, skills, hooks, settings and tools.
- **Agents**: each reusable agent, where it is used, and how it has done.
- **Issues**: what is open across projects and how it moves over time.
- **Missions**: a mission understood at a glance (its steps, who does them,
  its gates and loops), its notes, and what has changed in it; a run seen on
  the same board, with a way to talk to its agents.
- **Scheduled**: what will run when, and how the last run went.
- **Host**: is this machine all right now.
- **Settings**: every setting, said plainly, changed in place.

**Desk**: an immersive review. The thing being asked about fills the screen;
options are made things to compare, each carrying the asking agent's reason
where it applies; his answer is his own words and marks.

**Brain**: a really good editor and note app over what we know, where a quick
note lands without ceremony and agents file it, and where exploring the
whole is a pleasure.

## When in doubt

Ask what the surface is for and whether this serves it. A half-finished
feature or a number nobody acts on makes it worse: remove it and say so.
