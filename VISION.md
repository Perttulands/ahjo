# Ahjo Swarm

> Ahjo Swarm makes tmux sessions great.

This file says what Ahjo Swarm is for and when it is finished. Read it before
you decide what Swarm should do.

Ahjo is three apps built on one app kit: Swarm, Brain and Desk. Swarm was
called Parvi, which is Finnish for a swarm: many machines moving as one.

## What it is

With Swarm you can:

- launch an agent in the right place in seconds;
- see what every agent is doing;
- see what shapes each agent;
- see what work is open;
- chain agents into missions.

The agents are command-line programs that run in tmux, and they stay in tmux.
tmux lets you reach one session from many places, and it lets agents read each
other's sessions.

Each tab has one purpose and serves that purpose as well as it can. `DESIGN.md`
says how.

## A few pieces, many shapes

Build Ahjo from a few general pieces that combine into many shapes. Do not add
a feature that does only one job. This is why Claude Code works so well: it is
built around bash, one general tool that an agent combines in endless ways.

## You and your agent, side by side

Swarm is an app for you and a command line for your agent, and both work on
the same things. You work in the app. An agent on the same machine works
through the `ahjo` command and can do everything you can.

- **Every capability has a command.** Anything you can do in the app, an agent
  can do with an `ahjo` command. Only a purely visual gesture needs no command.
- **Agents do the big jobs; Swarm gives them simple tools.** An agent does a
  job by running a few `ahjo` commands in a row. Do not add setup screens or
  rule systems for one kind of job.
- **One configuration.** Every setting lives in one place that an agent can
  edit. The Settings page shows the same settings.
- **No permissions inside Swarm.** An agent has the access it already has on
  the machine. Swarm adds no permission checks of its own.
- **Easy to change.** Each area stands on its own and plugs in at one place.
  So a new idea becomes a request to an agent.

## What "finished" means

Swarm is finished when both of these are true:

1. The operator works in Swarm alone.
2. CHROTE and Archon, the two tools Swarm replaces, are switched off.

Judge Swarm by what the operator can do and how it goes for them. They can do
everything they did before, with the best experience and the simplest solution
we can build. Swarm keeps no compatibility with the old tools.

Swarm must cover these areas:

- **Sessions.** The launcher; sessions, tabs and windows; Send to Session;
  leader chords; sizes that stay readable; agent events.
- **Files and Work.** What agents wrote and what work is open, shown beside
  the sessions.
- **Agents.** What shapes an agent started in a folder: its instructions and
  where they come from, skills, hooks, settings and tools. You edit each of
  these where you see it.
- **Scheduled and Host.** Scheduled prompts, services, health, settings, and
  install and upgrade. None of these ever stops a session.
- **Missions.** A mission lets one operator get much more done. You define a
  graph of work for several agents, set up what it needs, start it, and come
  back to the result. A mission rarely waits for a person. When the operator is
  needed, one mission ends with something for them to look at, and the next
  mission starts from there.

## After that

These come after Swarm is finished:

- Windows terminals;
- missions across devices;
- moving files between devices;
- an agent arranging the operator's view;
- ChatGPT through MCP;
- the phone.

The data model is built for a fleet of machines from the start. Every resource
is named by its node and a local id. So these can be added without rebuilding
what exists.

## Not Swarm

Two things are not Swarm's job:

- **Resident agents**: agents that live on the machine and keep running on
  their own.
- **Compute admission**: deciding when a heavy job may run. `heavy` and
  `gpu-lock` own it.

The brain and the decision desk are separate apps, each with one purpose:

- **Ahjo Brain** is for exploring what we know.
- **Ahjo Desk** is the shared place where an agent publishes something, such
  as a picture, a video, a rendered page or a question. The operator answers
  freely, in his own words and by marking what he sees.

Both apps look like Swarm, but you do not open them from Swarm.

Swarm has no Brain tab. Swarm's agents reach the brain through `ahjo brain`.
In Swarm, Alt+L opens a Brain panel at the window's edge, beside the
terminals. In that panel you can search the brain, read a page and send the
page to a session.
