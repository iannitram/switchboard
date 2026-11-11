# Switchboard

Switchboard is the front door that sends each ask to the right service and model, and the Hyperion page that shows the work. Constellation is its main view: the roadmap and its issues drawn as a star map.

## Constellation

**Sky**:
The single map of every project. A project's place in the priority order sets how far it sits from the centre and how big it is.
_Avoid_: galaxy, universe, map (alone)

**Constellation**:
One project on the sky. Its brightness is its progress to 1.0.
_Avoid_: project node, system

**Feature**:
One item on a project's roadmap feature list. On the map it is a cluster of stars.
_Avoid_: Must, epic, milestone

**Star**:
One GitHub issue. It belongs to exactly one feature.
_Avoid_: node, task, ticket

**Dark star**:
A feature that has no issues yet. It shows as a dim placeholder, not as a star.
_Avoid_: empty feature, ghost

**Orphan star**:
An issue that belongs to no feature, or to more than one.
_Avoid_: stray, unlabelled issue

**Drift**:
Any gap between a project's roadmap and its issues: orphan stars, dark stars, or a roadmap whose `Updated:` date is more than 14 days old. The PM clears it every week.
_Avoid_: staleness, rot

**Reuse line**:
A dashed line between two constellations: one project uses an asset another one built. It stays on the map when the other project is Parked or archived, drawn as a warning.
_Avoid_: dependency, link, connection

## Machines

**Relay**:
The part of Switchboard on each machine that runs fleet jobs there for any other machine's Switchboard. It runs only fleet jobs, never a raw command.
_Avoid_: agent, daemon, satellite (a satellite is a pipeline role)

**Wake ping**:
A one-line nudge, sent over Tailscale, telling a relay to pull now. It carries no command; the work itself is always pulled from fleet-state.
_Avoid_: trigger, push, command

**Loadout**:
What a machine can do right now, read in code and published every minute: CLIs and whether they're signed in, models loaded, toolchains installed (such as C++ or Xcode), busy or idle, and recent errors. A machine whose loadout is silent for 3 minutes drops out of routing.
_Avoid_: capabilities, inventory, status

**Fleet error**:
A machine or tooling problem that has nothing to do with any project's code: disk full, a CLI signed out, a relay that has gone silent, a model that isn't loaded.
_Avoid_: IT error, infra bug

**IT ticket**:
A GitHub issue in the pipeline repo (private), labelled `it`, opened automatically for a fleet error. There is one ticket per error signature. A routed agent works it on a machine other than the one that failed.
_Avoid_: bug, incident, alert

**Coding error**:
A fault in a project's own code, such as Z13-Helper's tests failing. It's filed as a bug in that project's repo, never as an IT ticket.
_Avoid_: IT ticket, fleet error

**Adopted loop**:
A satellite loop whose run ended while it still ran (the window closed, a headless run killed). It stays on disk, and the next satellite tick on that machine, from any face, takes it over and watches it to its end instead of starting another copy.
_Avoid_: orphan, zombie

**Repo backup**:
The one latest copy of a repo, enough to rebuild it if GitHub loses it: all of its git history plus its issues, PRs, comments, labels, milestones, releases and wiki. Each new backup replaces the last one; no older versions are kept.
_Avoid_: snapshot (that is the cached GitHub read), archive, mirror

## Local only (after 1.0)

**Local only**:
An app-wide mode in which every chat and job, Eleanor included, goes to local models and no paid pool is used. A single star can also be sent local while the mode is off.
_Avoid_: offline mode, Qwen mode

**Readiness**:
The learned chance that a local model can finish a star without choking. It's predicted from the framework around the star and trained on past local attempts. It complements Easy/Medium/Hard, which measure the work, not the scaffolding.
_Avoid_: size, difficulty, Qwen-fit

**Prep loop**:
A scripted background loop that feeds a project to a local model piece by piece. The model writes code-map snippets, briefs and handoffs until enough stars are ready to run unattended.
_Avoid_: indexing, warm-up

## Routing

**Pool**:
One paid or local source of model time, with its own budget and pace (Claude, Claude API, ChatGPT, Gemini, DeepSeek, Qwen).
_Avoid_: provider, account, service (when budget is meant)

**Pace line**:
On a pool's bar in the budget panel: how much of the pool should be used by this point in its cycle. Usage above it is over pace; usage below it is under pace.
_Avoid_: target, burn line

**Request kind**:
A named type of ask in the abilities index, such as quick-lookup or web-research, that has a ranked list of models for that type.
_Avoid_: intent, category, task type

**Packet**:
What travels with a chat when it moves to another model: one summary of the whole thread so far (rewritten at each handoff, never stacked, and reused until new messages arrive), the last 6 turns word for word, and every file link gathered so far. It never carries the full transcript.
_Avoid_: context dump, transcript

**Sticky chat**:
A chat stays on its model until a signal moves it: I say "ask Claude" or name an agent, I give a thumbs down, or the model replies with the handoff marker. When its pool is braked, the chat moves to a pool with budget and says so, and moves back when the brake lifts.
_Avoid_: pinned chat, session lock

**Handoff marker**:
A fixed line, `[[handoff: <reason>]]`, that a routed model writes when an ask is beyond it. It's the model's only way to pass a chat on.
_Avoid_: refusal, give-up

**Claude handoff**:
A big ask that needs tools. It goes to Claude through an "Open in Claude" button that starts a session seeded with the packet. I press the button; Switchboard never starts the session itself.
_Avoid_: escalation (that is moving up the tier ladder)
