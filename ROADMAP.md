# Roadmap: Switchboard

Updated: 2026-11-11

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

**The live loops (added 2026-11-09).** By 2026-11-09 most of stages 1 to 7 are merged, but several of the core's parts are tested functions that nothing runs yet: the 10-minute GitHub poll and the Refresh button (GH-1), the autopilot's review start, merge pass, PM job and Sunday run (AUTO-2 to AUTO-6), the loadout timer (IT-1), automatic IT tickets (IT-2), the CLI and MCP write paths (CLI-3, MCP-2) and `switchboard loadout` (CLI-1). Each gets one wiring issue. The poll comes first: the autopilot issues that act on a fresh snapshot are `Blocked by` it, and they share one `autopilot::after_poll` hook that the review start adds. They run beside the open stage 3 to 7 issues and before the design pass. By 2026-11-11 the wiring issues (#240 to #251) are closed.

**The bug-fix run (added 2026-11-11).** On 2026-11-10 a bug hunt on the installed build filed 51 issues (#334 to #375, #377 to #385). Eleven PRs are merged and 45 of those issues are closed. What the product can now do:
- #386: the MCP server survives a bad line, answers every line that is not a request, and checks tool arguments against their schemas.
- #387: a fleet call has a time limit, and its error carries the exit code and stderr; a pool edit is all or nothing; a cap is checked.
- #388: chat threads show in `reports`; a damaged file is skipped and named; each loadout probe has a time limit.
- #389: the CLI never panics on its output or its arguments; `mcp` takes its flags; every command answers `--help`; extra words and a short name that matches two repos are refused; text output escapes control characters.
- #390: `sky` handles a very long blocked-by chain and a damaged snapshot, shows jobs in flight and the priority copy's age; a stale copy is a Waiting on you item; `star` says when a review is on an older commit; `merge` refuses a closed PR.
- #391: two backups of one repo no longer collide; releases are backed up; `backup --all` goes past a failed repo; a restore checks the backup first and creates only what the live repo lacks.
- #414: pool edits (`set_pool_cap`, `pool_edit`) and a forced budget re-read from the CLI and MCP, MCP `reports` filters, and a read-only `switchboard settings`.
- #415 and #417: a guarded delete or a restore over a live repo is confirmed in the app by typing the repo's `owner/name`; the delete archives the repo; a restore brings back closed issues and milestones and rewrites `#N` to the new numbers.
- #418: the app saves each fleet error to disk; `switchboard errors` and the MCP `errors` tool list them with the open IT tickets; pool edits from several processes take a lock.
- #393: a failed satellite loop or Project Manager job is recorded as a fleet error (the job's slot is freed at once; a failed loop's lane waits before it is tried again); the merge pass goes on past a refusal and merges a PASS carried over by patch-id; ratings are checked; an ask with every pool braked is queued in its thread; an open chat stays on its model, and a move hands over a summary packet.

In work (open): #384 the front door from the CLI and MCP; #385 the autopilot from the CLI and MCP; the leftovers of #347, #365, #377 and #378; #419 (a rating and a stored turn at the same moment can lose the turn). The #378 leftovers start after #384 merges.

**Rulings, 2026-11-10.** #384 and #385: build both the CLI and the MCP path now. #347: a guarded delete is an archive, never a delete; the confirm is typing the repo's `owner/name`; a second confirm on a restore that held comments back only queues the held comments and pushes no ref again. #373: settings are changed in the app only; `switchboard settings` reads them. #375: the app saves fleet errors to disk and the CLI and MCP list them; failures in the CLI or MCP themselves are not recorded. #365: a restore brings back closed state too. #374: CLI `budget --refresh` has no role (a re-read, like `refresh`); MCP keeps it behind budget-manager.

**Rulings, 2026-11-11.** #377: a queued ask stays manual and is never sent again on its own; an ask whose picked pool is braked or over budget falls back to local Qwen when the Qwen lane is up, and is queued only when Qwen is down too. #378: build all four leftovers (FD-5's work-tree line, "Send as job" with the packet, FD-7's open questions in the summary, the face's `chat_implicit` callers). #347: build all four leftovers (turning a request down, CORE-3 worded "archive", a restore to a new private repo from the face, MCP tools that file a delete or restore request while the typed confirm stays with me). #365: a label already in the live repo keeps its live colour and is named in the report; renumbering also rewrites full links and `owner/name#N`, for the restored repo only. Accepted as built: a cap of at most 100000 dollars and at least one cent; lock waits of 10 s for a pool edit and 2 s for saving an error, stale after 10 min; the MCP `errors` tool for the satellite role only.

After 1.0 the dark stars above get their own Specs and issues (the after-2.0 stars wait for 2.0); `platforms` turns the Mac and Windows seams on.

## The roadmap graph

Draft format, first used by `runner`. One line per node: `- <id>: <title> (feature: <id>, type: build|research|prototype|grilling|task, size: easy|medium|hard, kind: <k>, after: <ids separated by spaces, or none>)`. A node is one unit of work; `after` lists the nodes that must be done first. Each node's detail is `roadmap/nodes/<id>.json`. An issue is made from a node, never the other way round. `unspecified:` holds what is known to be missing: `- <id>: <gist> (waits on: <ids>)`. The workflows that run this graph are in `specs/`; the `roadmap-graph` node settles the format.

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

`docs/code-map.md` holds the modules, what each owns, how they interlink and each one's status. The skeleton is committed: one Rust workspace (`crates/core`, `crates/cli`, `app/src-tauri`) whose modules carry their types and signatures with `todo!("<Must id>")` bodies, the React face (`app/`) with five empty views, a stub `fleet.py` for core tests (`crates/core/tests/fixtures/fleet-stub/`), and CI on Linux, Mac and Windows. Issues fill the stubs; each moves its module from planned to present.

Build and test: `cargo test` (the core and the CLI), `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`; the face with `npm ci && npm run build && npm test` in `app/`; the Tauri app and the AppImage with `cargo build -p switchboard-app` / `npm run tauri build` in `app/`. Rust and Tauri are compiled only inside the build container on the Strix Halo computer, never on the host.

## Open source brought in

From the Prior art list in `SPEC.md` (awaiting my approval, R41). Code comes in only where the license allows it, with credit; otherwise ideas only.

| Project | License | What we take | Where it lands |
|---|---|---|---|
| BroCrew (our archived fork, upstream `brocrew/brocrew`) | Apache-2.0 | Code: the plain TypeScript chat parts (message paths, links, receipts, composer notices) lifted as is; the Vue composer, messages, Waiting on you item and work report entry ported to React (U2) | `app/src/brocrew/`, each file with its notice (SHELL-2) |
| rengwu/wayfinder-maps | MIT | The look of the sky; the renderer is ours | `sky` face, credited in the README |
| ocuclaw/obsidian-wayfinder | MIT | Ideas: dependency layers and the takeable frontier | `sky` (SKY-3, SKY-7) |
| ccusage/ccusage | MIT | Ideas: reading Claude Code's local usage logs | BUD-4's estimate |
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
