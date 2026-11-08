# Roadmap: Switchboard

Updated: 2026-11-08

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

## The features block

One line per feature: `- <id>: <title> (release: 1.0|after-1.0)`. An id is lowercase letters, digits and hyphens and never changes once issues carry it; it is the `feature:<id>` label. The block ends at the first line that does not start with `- `. The release marker is what SKY-2 and SKY-5 read: a `release: after-1.0` feature is drawn as a dark star but neither dims the constellation nor counts as drift. `Updated:` is the date this file was last brought in line with the code and the issues; re-date it on every edit. (The pipeline's block format is not ruled yet; this follows `SPEC.md` 2.10b and the pipeline's roadmap map. If the ruling changes the format, this block and `crates/core/src/sky/features.rs` change together.)

The regions, their Musts and their checks are in `SPEC.md`. Milestones: `1.0` holds every issue of a `release: 1.0` feature; `After 1.0` holds the dark stars once they get issues. Every issue carries exactly one `feature:<id>` label.

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

After 1.0 the dark stars above get their own Specs and issues; `platforms` turns the Mac and Windows seams on.

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
