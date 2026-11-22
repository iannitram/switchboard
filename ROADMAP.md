# Roadmap: Switchboard

Updated: 2026-11-21

features:
- core: The core and the fleet bridge, with the abilities index (release: 1.0)
- shell: App shell and packaging (release: 1.0)
- github: GitHub reads and writes (release: 1.0)
- budget: The budget panel (release: 1.0)
- sky: Constellation (release: 1.0)
- reports: Reports (release: 1.0)
- waiting: Waiting on you (release: 1.0)
- cli: The switchboard CLI (release: 1.0)
- mcp: switchboard mcp (release: 1.0)
- backup: Repo backup (release: 1.0)
- it: Loadout, fleet errors and IT tickets on this machine (release: 1.0)
- frontdoor: The front door chat (release: 1.0)
- autopilot: Project Manager, Budget Manager and satellite as code (release: 1.0)
- relay: A headless relay per machine, woken by a wake ping (release: after-1.0)
- loadouts: Every machine's loadout in routing (release: after-1.0)
- webhooks: One GitHub App with webhooks and a shared snapshot (release: after-1.0)
- chat-sync: Chats synced through fleet-state (release: after-1.0)
- local-only: Local only: Readiness, the prep loop and the work loop (release: after-1.0)
- platforms: Windows and Mac builds (release: after-1.0)
- fleet-rust: Porting fleet to Rust (release: after-1.0)
- dashboard: Retiring the fleet dashboard, only with my yes again (release: after-1.0)
- plugins: Plugins: add views, commands and MCP tools to Switchboard without changing the core (release: after-1.0)
- runner: The step runner: Switchboard runs a project's roadmap and its bug fixes from spec files (release: after-1.0)
- gamification: A gamification plugin (release: after-2.0)

## The features block

One line per feature: `- <id>: <title> (release: 1.0|after-1.0|after-2.0)`. An id is lowercase letters, digits and hyphens and never changes once issues carry it; it is the `feature:<id>` label. The block ends at the first line that does not start with `- `. The release marker is what SKY-2 and SKY-5 read: a `release: after-1.0` feature is drawn as a dark star on the outer ring but neither dims the constellation nor counts as drift. A `release: after-2.0` feature is drawn as a dark star on the outermost ring and, like after-1.0, is never counted and never drift. `Updated:` is the date this file was last brought in line with the code and the issues; re-date it on every edit. (The pipeline's block format is not ruled yet; this follows `SPEC.md` 2.10b and the pipeline's roadmap map. If the ruling changes the format, this block and `crates/core/src/sky/features.rs` change together.)

The regions, their Musts and their checks are in `SPEC.md`. Milestones: `1.0` holds every issue of a `release: 1.0` feature; `After 1.0` and `After 2.0` hold their dark stars once they get issues. Every issue carries exactly one `feature:<id>` label.

## Development stages

Each stage's first issues are `Blocked by` the stage before (written on the issues). Inside a stage, issues run in parallel unless an issue says otherwise.

| # | Stage | Features | Waypoint (SPEC.md) |
|---|---|---|---|
| 1 | **Core first.** Settings, the platform seams, the fleet bridge and its error record, the abilities index, the job log reader, the rules guard. The faces wait on this. | `core` | - |
| 2 | **The empty window**, once the core's settings are in: the Tauri window with five empty views, the error banner, the notice check, the AppImage. | `shell` | WP1 Shell up (with `switchboard budget`) |
| 3 | **GitHub.** The snapshot store, the transport, the GraphQL poll, the ETag skip, refresh-on-action, the create queue, the merge gate. Every view after it reads the snapshot. | `github` | - |
| 4 | **The read views**, in parallel: budget, sky, reports, waiting, with the CLI and the six MCP read tools beside them (the same core calls). | `budget`, `sky`, `reports`, `waiting`, `cli`, `mcp` | WP2 Read-only Switchboard |
| 5 | **Backup**, after `github`. | `backup` | WP3 Backups |
| 6 | **The loadout** (IT-1), then **the front door**, after `budget` (the router needs `fleet budget allow`) and the loadout. | `it` (IT-1), `frontdoor` | WP4 Front door |
| 7 | **Autopilot and the rest of IT**, after `github` and `frontdoor` (routed jobs use the router). | `autopilot`, `it` | WP5 Autopilot, WP6 Errors |
| 8 | **The design pass** (R04) on the running build, the last stage before 1.0: one shared design system across our apps; its findings are `design-pass` issues, each with the `feature:` label of the region whose view it changes (SKY-10). | all with a view | WP7 1.0 |

**Current state (2026-11-21).** Stages 1 to 7, the live-loop wiring issues (#240 to #251), the 2026-11-10/11 bug-fix run (#334 to #433), and the core/CLI architecture hardening (#489 command registry, #490 face parity, #491 stable error codes, #493 CI gate + `arch_lint.py`, #497 Tauri/CLI registry rows, #501 `claude_projects_dirs`) are closed and merged. On the `runner` feature, 16 of the 19 nodes are implemented (`runner-spec` #395 through `unit-land` #406, `bug-intake` #408 through `bug-sweep` #410, and `sky-roadmap-graph` #413).

Open for **1.0**:
- `#492` (`shell`, in-progress on `sat/492-shell-scenes`): named scenes for the face, rendered in vitest and to PNG for the design pass.
- `#502` (`budget`): BUD-W wiring Claude's reading and labelled estimate into the Budget view, CLI, and MCP (unblocked now that `#501` is merged).
- Stage 8 (`design-pass`): follows `#492`.

Open **After 1.0 / Runner / Follow-ups**:
- Runner live wiring & scoring: `#407` (`roadmap-run-live`), `#411` (`bug-loop-live`), `#412` (`run-scoring`).
- Runner/unit-claim & bug-fix follow-ups: `#482` (`unit-claim` follow-ups; items 2 and 3 split into `#506` and `#507`), `#506` (short label window), `#507` (reap retry half-mirrored claim), `#486` (`files_allowed` glob rules).
- Core, Budget, and Sky after-1.0: `#503` (`craft-atomic` reuse, `needs-ian`), `#494` (`budget` after-1.0, `needs-split`), `#302`, `#333`, `#376` (`sky` after-1.0).

After 1.0 the dark stars above get their own Specs and issues (the after-2.0 stars wait for 2.0); `platforms` turns the Mac and Windows seams on.

## The roadmap graph

The format, settled by `roadmap-graph`. One line per node: `- <id>: <title> (feature: <id>, type: build|research|prototype|grilling|task, size: easy|medium|hard, kind: <k>, after: <ids separated by spaces, or none>)`; `kind` is optional. A node is one unit of work; `after` lists the nodes that must be done first. Each node's detail is `roadmap/nodes/<id>.json`. An issue is made from a node, never the other way round. `unspecified:` holds what is known to be missing: `- <id>: <gist> (waits on: <ids>)`. `switchboard roadmap lint <clone>` checks the graph and names each problem by id: ids are unique and lowercase, every `after` and `waits on` id is a node, no dependency cycle, every node has its spec file, a build node has `verify`, `files_allowed`, `test_files` and `code_map_entries`, and each node's feature is in the features block. A feature with no node is a warning. The workflows that run this graph are in `specs/`.

nodes:
- runner-spec: Spec files load and validate (feature: runner, type: build, size: medium, kind: data-shape, after: none)
- runner-engine: The step engine: run one step and move the state (feature: runner, type: build, size: hard, kind: state, after: runner-spec)
- runner-waiting: Owner steps raise one Waiting on you item (feature: runner, type: build, size: medium, kind: state, after: runner-engine)
- roadmap-graph: Read the roadmap graph and lint it (feature: runner, type: build, size: medium, kind: data-shape, after: runner-spec)
- roadmap-units: Turn nodes into issues and link them in order (feature: runner, type: build, size: medium, kind: wiring, after: roadmap-graph)
- roadmap-drift: Compute drift between the roadmap and the issues (feature: runner, type: build, size: easy, kind: pure-fn, after: roadmap-graph)
- fleet-state-write: Can Switchboard write fleet-state files safely? (feature: runner, type: research, size: easy, after: none)
- unit-frontier: List ready work and choose the next unit (feature: runner, type: build, size: medium, kind: pure-fn, after: roadmap-units)
- unit-claim: Claim a unit with a lock two workers cannot share (feature: runner, type: build, size: hard, kind: state, after: unit-frontier fleet-state-write)
- unit-job: Write the build job from the node file (feature: runner, type: build, size: medium, kind: data-shape, after: unit-claim)
- unit-proof: Prove a test is red and the code map matches the change (feature: runner, type: build, size: medium, kind: pure-fn, after: roadmap-graph)
- unit-land: Open the pull request and record the result (feature: runner, type: build, size: medium, kind: wiring, after: unit-job unit-proof)
- roadmap-run-live: roadmap-run builds one project end to end (feature: runner, type: build, size: hard, kind: wiring, after: runner-waiting roadmap-drift unit-land)
- bug-intake: Collect a bug report into one tracked bug (feature: runner, type: build, size: hard, kind: data-shape, after: runner-engine fleet-state-write)
- bug-fix: Reproduce, fix and record one bug (feature: runner, type: build, size: hard, kind: state, after: bug-intake unit-claim unit-proof)
- bug-sweep: Check sister apps for the same bug (feature: runner, type: build, size: medium, kind: wiring, after: bug-fix)
- bug-loop-live: bug-loop fixes one reported bug end to end (feature: runner, type: build, size: hard, kind: wiring, after: runner-waiting bug-sweep unit-land)
- run-scoring: Score a sample of runs and propose changes to the specs (feature: runner, type: build, size: hard, kind: state, after: roadmap-run-live)
- sky-roadmap-graph: The Roadmap lens draws the node graph (feature: runner, type: build, size: medium, kind: ui, after: roadmap-graph)

unspecified:
- approval-lines: Who writes the dated approval line into SPEC.md and ROADMAP.md after you answer (waits on: runner-waiting)
- picked-lane: Whether `fleet run` honours a lane `fleet pick` already chose, or picks again (waits on: unit-job)
- fleet-pick-exits: `fleet pick` answers with exits 3, 4 and 5; the runner reads only 0 and 75 today (waits on: runner-engine)

## Code map and skeleton

`docs/code-map.md` holds the modules, what each owns, how they interlink and each one's status. All 1.0 core modules (`crates/core`), CLI/MCP commands (`crates/cli`), Tauri commands (`app/src-tauri`), and React views (`app/`) are present with no `todo!` stubs remaining, alongside a stub `fleet.py` for core tests (`crates/core/tests/fixtures/fleet-stub/`) and CI on Linux, Mac and Windows.

Build and test: `tools/ci.sh` runs the Rust format, lint and tests, the face checks, Python tests, notice check and architecture lints; `tools/ci.sh rust|app|face|lint` runs one part. The Tauri app and the AppImage build with `cargo build -p switchboard-app` / `npm run tauri build` in `app/`. Rust and Tauri are compiled only inside the build container on the Strix Halo computer, never on the host.

## Open source brought in

From the Prior art list in `SPEC.md` (awaiting my approval, R41). Code comes in only where the license allows it, with credit; otherwise ideas only.

| Project | License | What we take | Where it lands |
|---|---|---|---|
| BroCrew (our archived fork, upstream `brocrew/brocrew`) | Apache-2.0 | Code: the plain TypeScript chat parts (message paths, links, receipts, composer notices) lifted as is; the Vue composer, messages, Waiting on you item and work report entry ported to React (U2) | `app/src/brocrew/`, each file with its notice (SHELL-2) |
| rengwu/wayfinder-maps | MIT | The look of the sky; the renderer is ours | `sky` face, credited in the README |
| ocuclaw/obsidian-wayfinder | MIT | Ideas: dependency layers and the takeable frontier | `sky` (SKY-3, SKY-7) |
| ccusage/ccusage | MIT | Ideas: reading Claude Code's local usage logs; code adopted with credit (`NOTICE`): the list of Claude configuration folders | BUD-4's estimate |
| josegonzalez/python-github-backup | MIT | Ideas: what a full repo backup covers | `backup` (BAK-1) |
| musistudio/claude-code-router, BerriAI/litellm, lm-sys/RouteLLM | MIT, MIT, Apache-2.0 | Ideas: rule routing, fallbacks and cost tracking, cost-quality trade | `frontdoor` router and adapters |
| BloopAI/vibe-kanban, mvschwarz/openrig | Apache-2.0 | Ideas: a board of agent work per worktree; one map of agent roles | `reports`, `autopilot` |
| winfunc/opcode, smtg-ai/claude-squad | AGPL-3.0 | Ideas only | - |

The Rust crates the issues may add: `tauri` 2 (the shell), `serde`/`serde_json` (already in), an HTTP client for the transport and the adapters, a clock-free cron check (AUTO-5 is written by hand against the injected clock). Each new crate is named in its PR.

## Platforms (R04)

1.0 ships one AppImage for Linux (SteamOS on the Strix Halo computer, Desktop Mode; Game Mode is U1). Mac and Windows are kept honest from day one: CI builds and tests the core and the CLI on all three, and every OS difference sits in `crates/core/src/platform.rs` (the Python launcher, the dev root, the NextCloud folder, the keychain, the loadout probe list, the bundle). The `platforms` feature (After 1.0) fills in the Mac and Windows seams and adds the dmg and nsis bundles. An issue that is specific to one OS carries a `platform:` label.

## Unknowns that hold issues

| Unknown | Holds | Until then |
|---|---|---|
| U2 BroCrew lift or port | the ported React components | lift plain TypeScript, port Vue components |
| U4 priority source | SKY-9 | the copy in fleet-state |
| U5 who lands pool edits | BUD-2 | local edit plus a Waiting on you item |
| U6 per-CLI instruction files | AUTO-7 | instructions in the prompt for a CLI with no untracked option |
| U7 NextCloud and CalDAV on this machine | BAK-1, WAIT-2 (needs my yes for a stored credential) | the rest of 1.0 goes on |
| U8 the labeller model | FD-2's small model | rules only |
| U9 the pipeline batch's extra fields, `never_on`, `it-triage` | CORE-4's extra fields, IT-3 | "not recorded"; no `it-triage` job |
| U13 the vault agent scripts | FD-1's trigger words | Harmon and the Therapist built in |
