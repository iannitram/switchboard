# Code map

Updated: 2026-11-08

What each module owns, the Musts it carries (`SPEC.md`), what it talks to, and its status. An issue that fills a module moves its status from **planned** to **present** in the same PR (RUBRIC). Words are the glossary's (`GLOSSARY.md`).

Status today: the skeleton. Every module below exists with its types and signatures; bodies are `todo!("<Must id>")`. Present: the workspace, the clock, the fleet stub fixture, the empty window and the CI.

reuses:
- brocrew: the chat parts, the Waiting on you item, the work report entry and the fixed-report shape (Apache-2.0; our fork is archived, read and copied from only)

(The sky reads this block, SKY-6. Ideas taken from projects that are not ours, such as wayfinder-maps' look, are credited in the README, not here.)

## Layout

```
Cargo.toml                    one Rust workspace (core, cli, app/src-tauri)
crates/core/                  switchboard-core: every rule, read and write
  src/{settings,fleet,abilities,joblog,rules,platform,clock,error}.rs
  src/github/                 snapshot, transport, poll, queue, merge
  src/sky/                    features, layout, drift, chain, codemap, priority, labels
  src/{waiting,reports,budget,backup,it}.rs
  src/frontdoor/              router, labeller, adapters, packet, sticky, thread
  src/autopilot/              review, driver, instructions, classify, weekly
  tests/                      one test file per issue; fixtures/ (fleet stub, recorded answers)
crates/cli/                   the `switchboard` binary: cli.rs (CLI-1..3), mcp.rs (MCP-1..3)
app/                          the React face (Vite): src/App.tsx, src/views/, src/core.ts, src/brocrew/
app/src-tauri/                the Tauri shell: thin commands into the core
tools/                        repo checks (the notice check, SHELL-2)
.github/workflows/ci.yml      Linux, Mac and Windows
```

How they interlink:

```
 GUI (app/) --invoke--> app/src-tauri/commands.rs --+
 CLI (crates/cli/cli.rs) ---------------------------+--> switchboard-core --> rules::guard (every write)
 MCP (crates/cli/mcp.rs) ---------------------------+         |
                                                              +--> fleet::FleetBridge --> pipeline repo tools/fleet/fleet.py --json
                                                              +--> github::poll --> GraphQL / REST (transport) --> snapshot store
                                                              +--> github::queue --> fleet ghq (creates)     github::merge --> merge_pr.py
                                                              +--> frontdoor::adapters --> llama-swap, Gemini, DeepSeek, Claude API, ChatGPT
```

The views read only the snapshot and the fleet bridge. No face writes to GitHub or runs a shell command; every write passes `rules::guard`.

## Platforms (R04)

1.0 ships one AppImage for Linux (SteamOS on the Strix Halo computer). Mac and Windows build and test the core and the CLI in CI from day one, and every OS difference sits in `crates/core/src/platform.rs`:

| Seam | Linux (1.0) | Mac (`platforms`, After 1.0) | Windows (`platforms`, After 1.0) |
|---|---|---|---|
| Python launcher | `python3 -I` | `python3 -I` | `py -3 -I` |
| Dev root | `~/dev` | `~/dev` | the code drive, `D:\dev` |
| NextCloud folder | `~/Nextcloud` until U7 | `~/Nextcloud` | `%USERPROFILE%\Nextcloud` |
| Tokens | `gh auth token`; keychain later | Keychain | Credential Manager |
| Loadout probes | the fixed Linux list (IT-1) | empty until `platforms` | empty until `platforms` |
| Bundle | AppImage (WebKit bundled) | dmg | nsis |

The face is plain React and has no OS code. Paths are always built with `Path::join`, never with `/`.

## Modules

### core-settings
`crates/core/src/settings.rs` · CORE-5 · planned. The one settings file: pipeline repo clone, NextCloud backups folder, vault agents folder. `Settings::missing()` names what is not set; `load` gives defaults for a missing file. Read by every module that needs a path. Face: a Settings view (`app/src/views/Settings.tsx`, planned).

### core-fleet
`crates/core/src/fleet.rs` · CORE-1, CORE-2, AUTO-1 · present. `FleetBridge` runs `<python> <root>/tools/fleet/fleet.py <cmd> --json` (root from settings, `FLEET_ROOT` overrides) and parses it; a non-zero exit or bad JSON is a `FleetError` (command, exit code, stderr), recorded in `FleetErrorLog`, never a guessed value. Tests use `tests/fixtures/fleet-stub/` (`FLEET_STUB_EXIT`, `FLEET_STUB_BADJSON`, answers in `answers/<cmd>.json`). Used by abilities, joblog, budget, github::queue, autopilot, it.

### core-abilities
`crates/core/src/abilities.rs` · the abilities index · present. Reads `fleet actions --json`: every action and request kind (`"request": true`) with `needs` and `first`. Read by the router (FD-1, FD-3) and FD-12.

### core-joblog
`crates/core/src/joblog.rs` · CORE-4 · present. One `JobRow` shape for ledger attempts and local chat rows; a missing field is `None` and shows as "not recorded". U9's asked fields (`checks`, `escalation_step`, `review_fix_loc`) are in the shape and read as "not recorded" until the pipeline batch writes them. Read by reports, the router (FD-4) and budget.

### core-rules
`crates/core/src/rules.rs` · CORE-3 (and the worktree test for CLI-3, MCP-3) · present. `guard(caller, action)` for merge, repo delete and restore over a live repo; the same `Verdict` and text whichever face asks. Called by github::merge, backup restore, cli, mcp and the Tauri commands.

### core-platform
`crates/core/src/platform.rs` · the R04 seams · present. See Platforms above.

### core-clock, core-error
`crates/core/src/clock.rs`, `error.rs` · present. The injected clock (`Clock`, `SystemClock`, `FixedClock`) and `CoreError`, the one error faces see.

### github-snapshot
`crates/core/src/github/snapshot.rs` · GH-1 · present. `RepoSnapshot` (issues, PRs with verdicts and CI on head, milestones, labels, ROADMAP.md, the code map) and `SnapshotStore`: one JSON file per repo under the app's data folder, with its age.

### github-transport
`crates/core/src/github/transport.rs` · present. `GitHubTransport`: GraphQL and REST GET with `If-None-Match`; `Recorded` replays answers in tests. Token from `platform::TokenStore`.

### github-poll
`crates/core/src/github/poll.rs` · GH-1, GH-2, GH-3 · planned. The 10-minute poll (`Poller::tick`), the Refresh button (`refresh_all`), the ETag skip (`unchanged`), refresh-on-action (`after_write`).

### github-queue
`crates/core/src/github/queue.rs` · GH-4 · present. `send(fleet, creates)`: one `fleet ghq add-batch` then `fleet ghq run`; a stop on a rate limit leaves items queued, never a failure.

### github-merge
`crates/core/src/github/merge.rs` · GH-5 · present. `eligibility(pr, carried_over)` (PASS on head or carried by patch-id, CI green on head) and `merge` through `merge_pr.py` (dry run, then push) after `rules::guard`. Never `gh pr merge` or the merge API.

### sky-features
`crates/core/src/sky/features.rs` · SKY-2, SKY-3, SKY-5 · present. Parses ROADMAP.md's `Updated:` and `features:` block (format in `ROADMAP.md`).

### sky-layout
`crates/core/src/sky/layout.rs` · SKY-1, SKY-2 · present. Rank to radius, size and angle; brightness from the `1.0` milestone and `release: 1.0` dark stars.

### sky-drift
`crates/core/src/sky/drift.rs` · SKY-5 · present. Orphan stars, `release: 1.0` dark stars, a roadmap more than 14 days old.

### sky-chain
`crates/core/src/sky/chain.rs` · SKY-3, SKY-7 · present. `Blocked by #N` edges and the longest open chain.

### sky-codemap
`crates/core/src/sky/codemap.rs` · SKY-6 · present. This file's `reuses:` block; a reuse of a Parked or archived repo is a warning.

### sky-priority
`crates/core/src/sky/priority.rs` · SKY-9 · present. The priority copy in fleet-state (U4's default), its age, stale after 7 days.

### sky-labels
`crates/core/src/sky/labels.rs` · SKY-10 · present. The `feature:` label for a design-pass issue and an automatic bug.

### sky-face
`app/src/views/Sky.tsx` (+ `app/src/sky/`) · SKY-1, SKY-3, SKY-4, SKY-5, SKY-6, SKY-7, SKY-8 · planned. SVG: the sky, the zoomed constellation (clusters, stars, blocked-by lines, milestone rings), star states, the drift list, reuse lines, the chain highlight, the star panel. Paints what the core worked out.

### waiting
`crates/core/src/waiting.rs`, `app/src/views/Waiting.tsx` · WAIT-1, WAIT-2, WAIT-3 · planned. Items from the snapshot, CalDAV (U7), handoffs, guarded actions, IT tickets, refused backups, the stale priority copy and pool edits (U5); merged newest first; carried between reads. The item view is ported from BroCrew.

- WAIT-1 CalDAV items (`caldav_items`, `from_caldav`): present.
- WAIT-2 tick a CalDAV task (`tick_caldav`): present.
- WAIT-3 carry between reads (`carry`): present.
- WAIT-1 local items and the merge (`LocalItems`, `merge`): present.

### reports
`crates/core/src/reports.rs`, `app/src/views/Reports.tsx` · REP-1, REP-2, REP-3 · planned. The five-part fixed report from a done card or a thread, its work tree of moves, filters. The report entry is ported from BroCrew.

- REP-1 reports (`from_card`, `from_thread`, `newest_first`, `parse_time`, `project_of`): present.
- REP-2 moves (`moves`): present.
- REP-2 work tree (`WorkTree`): present.
- REP-1 view (`ReportEntry`, `ReportList`, `Reports`): present.

### budget
`crates/core/src/budget.rs`, `app/src/views/Budget.tsx` · BUD-1 to BUD-4 · planned. Rows from `fleet budget --json`, edits to `config.json` with a ledger row (U5: landed through review), refresh on a timer, Claude's pool as reading plus a labelled estimate from local session logs.

- BUD-1 rows (`rows_from_json`, `read`): present.
- BUD-4 Claude pool (`claude_pool`, `session_log_paths`, `parse_utc`): present.
- BUD-3 refresh (`refresh`, `RefreshTimer`, `budget_rows`, `budget_refresh`): present.
- BUD-1 panel (`BudgetPanel`, `Budget`): present.
- BUD-2 save (`save_edit`, `parse_reset`, `budget_save_edit`): present.

### backup
`crates/core/src/backup.rs` · BAK-1, BAK-2, BAK-3 · planned. A git bundle of every ref plus JSON of issues, PRs, comments, labels, milestones, releases (under 1 GB) and the wiki, with a manifest, in `<NextCloud backups>/github/<repo>/`; the overwrite guard; restore to a new private repo (issues through the create queue), over a live repo only after `rules::guard`.

### frontdoor
`crates/core/src/frontdoor/` · FD-1 to FD-13 · planned. `router` (the four steps, the `first` list against budget, the learned pick), `labeller` (rules, then a small model, U8), `adapters` (one per service; none for the Claude subscription), `packet` (summary, last 6 turns, links; the summary check), `sticky` (what moves a chat; implicit signals), `thread` (one JSON Lines file per thread). Face: `app/src/views/Chat.tsx` with BroCrew's composer and messages ported; the "Open in Claude" button (FD-8).

### autopilot
`crates/core/src/autopilot/` · AUTO-1 to AUTO-9 · planned. `may_spend` (budget allow), `review` (R25's ladder as a state machine), `driver` (script loops, Qwen plus one), `instructions` (untracked instruction files; drift of copies), `classify` (by the card, through `fleet errors classify`), `weekly` (Sunday 6am, ROADMAP changes).
- AUTO-9 classify (`classify`, `from_json`): present.

### it
`crates/core/src/it.rs` · IT-1 to IT-4, FD-12 · planned. The loadout from the platform's probe list, unmet `needs`, IT tickets through `fleet errors file`, `it-triage` jobs with `never_on` (waits for U9), ladder-out bugs with the star's `feature:` label.
- IT-3 triage job (`triage_job`): present.

### cli
`crates/cli/src/main.rs`, `cli.rs` · CLI-1, CLI-2, CLI-3 · planned. Text or `--json`; the same numbers as the GUI; role-flagged writes through `rules::guard`.

### mcp
`crates/cli/src/mcp.rs` · MCP-1 and MCP-2 present; MCP-3 planned. JSON-RPC over stdio; six read tools; role write tools; none inside a job worktree.
- MCP-3 no write tools in a job worktree: present.

### shell
`app/src-tauri/`, `app/src/App.tsx`, `app/src/core.ts`, `app/src/ErrorBanner.tsx` · SHELL-1, SHELL-2, SHELL-3 · present (empty window, wired to the core: `views`, `settings_missing`, `fleet_errors`; the error banner present), the AppImage planned. `tools/check-notices.py` (present) lists every file under `app/src/brocrew/` and fails on one without its Apache-2.0 notice. `app/src/brocrew/` holds BroCrew's lifted chat helpers (present): `composer-notice.ts`, `message-links.ts`, `message-paths.ts` and `ipc-types.ts`, all listed in `NOTICE`.
