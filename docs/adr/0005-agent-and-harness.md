# Agent and harness

Role, character and agent were three words for one idea, and "agents" also named
the area that set up the agent programs. An agent is now a named, reusable
setup, and a harness is the program it runs in. A measured naming test showed
that a misleading name makes agents misread the data.

## Decision

An **agent** is a named, reusable setup: instructions, harness, model, effort,
and notes on how it has done. It is one file, `missions/agents/<id>.toml`, made
and improved with `ahjo agent list|show|create|edit|delete`, and seats name an
agent. A **harness** is the program an agent runs in (Claude Code, Codex) and
what shapes it in a folder: instruction files, skills, hooks and settings. That
is the harness area (`areas/harness`, `ahjo harness`, the Harness tab).
`ARCHITECTURE.md` (Words) holds both words.

## Why

The naming review of 2026-11-20 found three words for one idea:

- A role held only the instructions.
- A character was a saved setup that named a role.
- An "agent" was a worker whose session took its name.

The design in ahjo-biq records the idea as one good setup that is made, used and
improved. The review also measured what a misleading name costs. Shown a slot as
`agentId`/`agent`, none of five agents could say which field sets its session,
and two would have overwritten its role. With `roleId`/`agentName`, all five
were right. The program-setup area took the word harness, so that agent could
mean the reusable setup alone.

## What it replaced

On 2026-11-11 the Agents page moved into `areas/agents` (c9f638b5,
parvi-pnu.5), holding the setup of the agent programs and the roles library.
62ecad94 (11-14) made Agents the agent-setup page, with roles in a section. On
2026-11-20:

- 49be0c7d (ahjo-biq.17) renamed the setup area, its CLI noun and its MCP path
  to harness.
- 2cde8fd9 (ahjo-biq.18) merged the saved roles and characters into agent
  files. The upgrade wrote the seven built-in roles as starter agents.
- 14c37993 (ahjo-biq.19) staffed seats with agents.

48cd8f1f (11-22, ahjo-biq.34) put the Words table and the naming rules on
record. Retired: role and character, for an agent; agents, for the harness area.
