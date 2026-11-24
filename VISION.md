# Ahjo Swarm

> Ahjo Swarm makes tmux sessions great.

Ahjo is a family of three apps built on one app kit: Swarm, Brain and Desk.
This file is Swarm's vision. Swarm used to be called Parvi, which is Finnish
for a swarm: many machines moving as one.

## What it is

Swarm makes tmux sessions great. With it you can:

- launch an agent in the right place in seconds;
- see what every agent is doing;
- see what shapes each agent;
- see what work is open;
- chain agents into missions.

The agents are command-line programs running in tmux, and they stay in tmux.
tmux is what lets you reach one session from many places, and what lets agents
read each other.

Each tab has one purpose, and is designed to serve that purpose as well as it
can (`DESIGN.md`).

## A few pieces, many shapes

Ahjo is built from a few general pieces that combine into many shapes. It does
not get features that do only one job. This is the reason Claude Code works so
well: it is built around bash, one general tool that an agent combines in
endless ways. Ahjo follows the same idea.

## You and your agent, side by side

Swarm is a great app for you and a great command line for your agent, and both
work on the same things. You work in the app. An agent on the same machine
works through the `ahjo` command and can do everything you can.

- **Every capability has a command.** Anything you can do in the app, an agent
  can do with an `ahjo` command. Only a purely visual gesture needs none.
- **Agents do the big jobs; Swarm gives them simple tools.** Every job is a few
  `ahjo` commands an agent runs in a row. Swarm does not get setup screens or
  rule systems for one kind of job.
- **One configuration.** Every setting lives in one place that an agent can
  edit, and it also shows on the Settings page.
- **No permissions inside Swarm.** An agent has the access it already has on
  the machine. Swarm adds no permission checks of its own.
- **Easy to change.** Each area stands on its own and plugs in at one place. So
  a new idea becomes a request to an agent.

## What "finished" means

Swarm is finished when the operator works in Swarm alone, and the two tools it
replaces, CHROTE and Archon, are switched off. The bar is what the operator can
do and how it goes for them: they can do everything they did before, with the
best experience and the simplest solution we can build. Swarm keeps no
compatibility with the old tools.

- **Sessions.** The launcher; sessions, tabs and windows; Send to Session;
  leader chords; sizes that stay readable; agent events.
- **Files and Work.** What agents wrote and what work is open, shown beside the
  sessions.
- **Agents.** What shapes an agent started in a folder: its instructions and
  where they come from, skills, hooks, settings and tools. You edit them where
  you see them.
- **Scheduled and Host.** Scheduled prompts, services, health, settings, and
  install and upgrade. None of these ever stops a session.
- **Missions.** Missions let one operator get much more done. You define a
  graph of work for several agents, set up what it needs, start it, and come
  back to the result. A mission rarely waits for a person. When the operator is
  needed, one mission ends with something for them to look at, and the next
  mission starts from there.

## After that

Later, Swarm adds Windows terminals, missions across devices, moving files
between devices, an agent arranging the operator's view, ChatGPT through MCP,
and the phone. The data model is built for a fleet of machines from the start:
every resource is named by its node and a local id. So these can be added
without rebuilding what exists.

## Not Swarm

Two things are not Swarm's job:

- resident agents (agents that live on the machine and keep running on their
  own);
- compute admission (deciding when a heavy job may run). `heavy` and
  `gpu-lock` own that.

The brain and the decision desk are their own apps, Ahjo Brain and Ahjo Desk.
Each has one purpose:

- **Ahjo Brain** is for exploring what we know.
- **Ahjo Desk** is the shared place where an agent publishes something (a
  picture, a video, a rendered page, a question) and the operator answers
  freely: in his own words, and by marking what he sees.

They look like Swarm, but you do not open them from Swarm. Swarm has no Brain
tab. Swarm's agents reach the brain through `ahjo brain`. In Swarm, Alt+L opens
a Brain panel at the window's edge, beside the terminals. There you can search
the brain, read a page and send it to a session.
