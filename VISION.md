# Parvi

> Parvi makes tmux sessions great.

*Parvi* is Finnish for a flock: many machines moving as one.

## What it is

Parvi makes tmux sessions great: launch an agent in the right place in seconds,
see what all of them are doing, see what shapes them, see what work is open,
and chain them into missions.

The agents are command-line programs in tmux, and they stay there: tmux is
what lets a session be reached from many places and lets agents read each
other. Each tab has one purpose and is designed to serve it as far as it can
(`DESIGN.md`).

## You and your agent, side by side

A great app for the person and a great CLI for their agent, over the same
things. You work in the app. An agent on the same machine works through `parvi`
and can do what you can.

- **Every capability has a command.** A purely visual gesture needs none.
- **The agent composes; Parvi stays simple.** Parvi offers commands and clear
  configuration, not setup journeys or policy engines.
- **One configuration.** Every setting lives in one place an agent can edit, and
  also appears in Settings.
- **No permissions inside Parvi.** An agent has the access it already has on the
  machine.
- **Easy to change.** Each area is self-contained and plugs in at one place. A
  new idea becomes a request to an agent.

## What "finished" means

The operator works in Parvi alone and the two tools it replaces, CHROTE and
Archon, are switched off. The bar is capability and journey: the operator can do
what they did before, with the best experience and the simplest solution we can
build. Nothing stays compatible with the old tools.

- **Sessions.** The launcher, sessions, tabs and windows, Send to Session,
  leader chords, readable sizing, agent events.
- **Files and Work.** What agents wrote and what is open, beside the sessions.
- **Agents.** What shapes an agent started in a folder (instructions and where
  they come from, skills, hooks, settings, tools), edited in place.
- **Scheduled and Host.** Scheduled prompts, services, health, settings,
  install and upgrade that never stop a session.
- **Missions.** Missions scale what one operator gets done. Define a graph of
  work for several agents, set up what it needs, set it off and come back to
  the result. A mission rarely waits for a person: when the operator is needed,
  one mission ends with something to look at and the next starts from it.

## After that

Windows terminals, missions across devices, moving files between devices, an
agent arranging the operator's view, ChatGPT through MCP, and the phone. The data model is fleet-shaped
from the start (every resource is a node and a local id) so these add on
without rework.

## Not Parvi

Resident agents and compute admission (`heavy` and `gpu-lock` own it).

The brain and the decision desk are their own apps, each with one purpose:
exploring what we know, and answering what agents ask. They share Parvi's
look and are not opened from it. Parvi's Brain tab goes when the brain app
replaces it.
