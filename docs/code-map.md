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
  src/{settings,fleet,fleet_errors,abilities,joblog,rules,platform,clock,error,lock}.rs
  src/github/                 snapshot, transport, poll, queue, merge
  src/sky/                    features, layout, drift, chain, codemap, priority, labels, jobs
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
`crates/core/src/settings.rs` · CORE-5 · present. The one settings file: pipeline repo clone, NextCloud backups folder, vault agents folder. `Settings::missing()` names what is not set; `load` gives defaults for a missing file. Read by every module that needs a path. Face: a Settings view (`app/src/views/Settings.tsx`, present).

### core-fleet
`crates/core/src/fleet.rs` · CORE-1, CORE-2, AUTO-1 · present. `FleetBridge` runs `<python> <root>/tools/fleet/fleet.py <cmd> --json` (root from settings, `FLEET_ROOT` overrides) and parses it; a non-zero exit or bad JSON is a `FleetError` (command, exit code, stderr), recorded in `FleetErrorLog`, never a guessed value. Tests use `tests/fixtures/fleet-stub/` (`FLEET_STUB_EXIT`, `FLEET_STUB_BADJSON`, answers in `answers/<cmd>.json`). Used by abilities, joblog, budget, github::queue, autopilot, it. `clean_child_env(cmd)` (with the pure `clean_ld_library_path(ld, appdir)`) takes the packaged app's `PYTHONHOME`, `PYTHONPATH`, `PYTHONDONTWRITEBYTECODE` and its `APPDIR` entries in `LD_LIBRARY_PATH` out of every fleet child's environment, the bridge's and `FleetStarter::start`'s.

### core-fleet-errors
`crates/core/src/fleet_errors.rs` · CORE-2, IT-2 · present (#375). The fleet errors the app records, kept in `<data>/fleet-errors.jsonl`: one `FleetErrorRecord` per line (command, exit code, stderr, reason as `{"kind": ..}`, undo note, and the shown `message`), appended whole under `lock::FileLock`, kept to the last `FLEET_ERROR_LOG_CAP` (50) lines. `record` returns an error, never panics; `load` reads newest first, skips and counts a line that does not read (`Recorded::skipped`); no file is no errors. The app's `fleet_errors` list and the CLI's `errors` both read through `load`. Failures in the CLI or MCP themselves are not recorded.

### core-abilities
`crates/core/src/abilities.rs` · the abilities index · present. Reads `fleet actions --json`: every action and request kind (`"request": true`) with `needs` and `first`, and per kind the model for each service (`models`, `model(kind, service)`): the `ok.<service>.model` rows, plus `ok.qwen: true` and a top-level `claudeapi: true` filled from `tools/fleet/config.json` (`FleetModels`: this machine's `qwen_model`, `ok.claude`'s tier through `claudeapi_models`; `load_with`). Read by the router (FD-1, FD-3) and FD-12.

### core-joblog
`crates/core/src/joblog.rs` · CORE-4 · present. One `JobRow` shape for ledger attempts and local chat rows; a missing field is `None` and shows as "not recorded". U9's asked fields (`checks`, `escalation_step`, `review_fix_loc`) are in the shape and read as "not recorded" until the pipeline batch writes them. Read by reports, the router (FD-4) and budget.

### core-rules
`crates/core/src/rules.rs` · CORE-3 (and the worktree test for CLI-3, MCP-3) · present. `guard(caller, action)` for merge, repo delete and restore over a live repo; the same `Verdict` and text whichever face asks. `guard_pool_edit(caller, pool)` for a budget pool edit: the app, or the budget-manager role from the CLI or MCP, never inside a job worktree. Called by github::merge, backup restore, cli, mcp and the Tauri commands.

- CORE-3, BAK-3 confirm in the app (`crates/core/src/confirm.rs`: `guarded_request`, `confirm_with`, `confirm`, `Done`; the `confirm_guarded` command; the confirm step in `Waiting.tsx`): present. A delete or restore request the CLI left in Waiting on you is read back from its item, goes through `guard` as the app with my confirm, and runs only when the repo's `owner/name` was typed letter for letter (the view enables Confirm on a match, the core checks the typed name again); a refusal changes nothing. A restore runs `backup::restore_over_live` from the repo's backup folder; one that held comments or closes back keeps its item with `HELD_ACTION`, and its next confirm runs `backup::restore_held_over_live` (`Pass::Held` with the record of what the restore made, kept beside the item in `<data>/waiting/held-restores.json` by `HeldRecords`: no ref push, no issue queued). A delete request archives the repo (`crates/core/src/github/archive.rs`: `archive_repo`, one `PATCH {"archived": true}`); nothing is ever deleted (#347). Not built: a restore to a new private repo from the app, turning a request down, MCP delete and restore tools.
- `guard_front_door(caller, what)` (#384): an ask, a send as job or a rating from the CLI or MCP needs the project-manager role and is refused inside a job worktree; the app passes: present.

### core-platform
`crates/core/src/platform.rs` · the R04 seams · present. See Platforms above.

### core-clock, core-error
`crates/core/src/clock.rs`, `error.rs` · present. The injected clock (`Clock`, `SystemClock`, `FixedClock`) and `CoreError`, the one error faces see.

### core-lock
`crates/core/src/lock.rs` · BUD-2, CORE-2 · present. One writer at a time for a file the core reads, changes and writes back, across threads and processes on every OS, no extra crate (#416): `FileLock::take(target, wait)` makes `<target>.switchboard-lock` with `create_new`, waits up to `wait`, takes over a lock older than `STALE_LOCK` (10 min, a dead run's), and removes it on drop. `run_tag` and `temp_path` give each run its own temp file names, as the backup's `run_tag` does (#363).

### github-snapshot
`crates/core/src/github/snapshot.rs` · GH-1 · present. `RepoSnapshot` (issues, PRs with verdicts and CI on head, milestones, labels, ROADMAP.md, the code map) and `SnapshotStore`: one JSON file per repo under the app's data folder, with its age.

### github-transport
`crates/core/src/github/transport.rs` · present. `GitHubTransport`: GraphQL, REST GET with `If-None-Match`, and REST PATCH (`rest_patch`: closes, body rewrites and the archive, the writes that are not creates; a transport that does not implement it refuses); `Recorded` replays answers in tests and logs each PATCH. Token from `platform::TokenStore`.

### github-poll
`crates/core/src/github/poll.rs`, `crates/core/src/github/live.rs` · GH-1, GH-2, GH-3 · present. The 10-minute poll (`Poller::tick`), the Refresh button (`refresh_all`), the ETag skip (`unchanged`), refresh-on-action (`after_write`). In the app (`live.rs`, #240): the repos it reads (the priority copy's order, then the rest of the snapshot store, by name); a thread in `app/src-tauri/src/main.rs` that ticks every minute and sends `snapshot`; the `refresh_all` command the Sky's Refresh button calls.

### github-queue
`crates/core/src/github/queue.rs` · GH-4 · present. `send(fleet, creates)`: one `fleet ghq add-batch` then `fleet ghq run`; a stop on a rate limit leaves items queued, never a failure. `send_keyed` also answers each create's queue key and whether the batch added it; `records(fleet, keys)` reads those items from the queue's own record (`queue.json` in the folder `fleet ghq status` names).

### github-merge
`crates/core/src/github/merge.rs` · GH-5 · present. `eligibility(pr, carried_over)` (PASS on head or carried by patch-id, CI green on head) and `merge` through `merge_pr.py` (dry run, then push) after `rules::guard`. Never `gh pr merge` or the merge API.
- AUTO-4 merge passed PRs (`ready_to_merge`, `merge_passed`): present.
- GH-5 carry-over by patch-id (`carried_over_by_patch_id`: the PR's own diff from main has the same `git patch-id --verbatim` at the PASS sha and the head; used by the merge pass and `switchboard merge`): present.

### sky-features
`crates/core/src/sky/features.rs` · SKY-2, SKY-3, SKY-5 · present. Parses ROADMAP.md's `Updated:` and `features:` block (format in `ROADMAP.md`; `release: 1.0`, `after-1.0` or `after-2.0`).

### sky-layout
`crates/core/src/sky/layout.rs` · SKY-1, SKY-2 · present. Rank to radius, size and angle; brightness from the `1.0` milestone and `release: 1.0` dark stars (`after-1.0` and `after-2.0` are not counted).

### sky-drift
`crates/core/src/sky/drift.rs` · SKY-5 · present. Orphan stars, `release: 1.0` dark stars (`after-1.0` and `after-2.0` are not drift), a roadmap more than 14 days old.

### sky-chain
`crates/core/src/sky/chain.rs` · SKY-3, SKY-7 · present. `Blocked by #N` edges and the longest open chain.

### sky-codemap
`crates/core/src/sky/codemap.rs` · SKY-6 · present. This file's `reuses:` block; a reuse of a Parked or archived repo is a warning.

### sky-priority
`crates/core/src/sky/priority.rs` · SKY-9 · present. The priority copy in fleet-state (U4's default), its age, stale after 7 days.

### sky-labels
`crates/core/src/sky/labels.rs` · SKY-10 · present. The `feature:` label for a design-pass issue and an automatic bug.

### sky-jobs
`crates/core/src/sky/jobs.rs` · SKY-4, SKY-8 · present. The fleet jobs in flight from the fleet ledger's events (a `start` with no later `end` from the same machine), the job `star` names.

### sky-model
`crates/core/src/sky/model.rs` · SKY-1 to SKY-9 · present. The SkyModel the `sky` command answers: the snapshot store and the priority copy in the app data folder, with the jobs in flight from the fleet ledger (`load_with_ledger`), assembled from the sky modules; no snapshot is `GitHub: no snapshot yet`.

### sky-face
`app/src/views/Sky.tsx` (+ `app/src/sky/`) · SKY-1, SKY-3, SKY-4, SKY-5, SKY-6, SKY-7, SKY-8 · present. SVG: the sky, the zoomed constellation (clusters, stars, blocked-by lines, milestone rings), star states, the drift list, reuse lines, the chain highlight, the star panel. Paints what the core worked out.

### waiting
`crates/core/src/waiting.rs`, `app/src/views/Waiting.tsx` · WAIT-1, WAIT-2, WAIT-3 · planned. Items from the snapshot, CalDAV (U7), handoffs, guarded actions, IT tickets, refused backups, the stale priority copy and pool edits (U5); merged newest first; carried between reads. The item view is ported from BroCrew.

- WAIT-1 snapshot items (`from_snapshots`): present.
- WAIT-1 CalDAV items (`caldav_items`, `from_caldav`): present.
- WAIT-1 view (`WaitingItem`, `WaitingList`, `Waiting`): present.
- WAIT-2 tick a CalDAV task (`tick_caldav`): present.
- WAIT-3 carry between reads (`carry`): present.
- WAIT-1 local items and the merge (`LocalItems`, `merge`): present.
- WAIT-W command (`assemble`, `load`, `waiting`): present.

### reports
`crates/core/src/reports.rs`, `app/src/views/Reports.tsx` · REP-1, REP-2, REP-3 · planned. The five-part fixed report from a done card or a thread, its work tree of moves, filters. The report entry is ported from BroCrew.

- REP-1 reports (`from_card`, `from_thread`, `newest_first`, `parse_time`, `project_of`): present.
- REP-2 moves (`moves`): present.
- REP-2 work tree (`WorkTree`): present.
- REP-1 view (`ReportEntry`, `ReportList`, `Reports`): present.
- REP-3 filter (`filter`, `Filter`): present.
- REP-3 filter bar (`FilterBar`, `Reports`): present.
- REP-W command (`state_dir_from`, `read_done_cards`, `assemble`, `load`, `reports`): present.
- REP follow-up, chat threads in reports (`reports::thread_rows`, read from the thread store in the app data folder; the CLI and MCP pass them too, #348): present. A done card or ledger file that is not UTF-8 is skipped, not fatal (#349).

### budget
`crates/core/src/budget.rs`, `app/src/views/Budget.tsx` · BUD-1 to BUD-4 · planned. Rows from `fleet budget --json`, edits to `config.json` with a ledger row (U5: landed through review), refresh on a timer, Claude's pool as reading plus a labelled estimate from local session logs.

- BUD-1 rows (`rows_from_json`, `read`): present.
- BUD-4 Claude pool (`claude_pool`, `session_log_paths`, `parse_utc`): present.
- BUD-3 refresh (`refresh`, `RefreshTimer`, `budget_rows`, `budget_refresh`): present.
- BUD-1 panel (`BudgetPanel`, `Budget`): present.
- BUD-2 save (`save_edit`, `parse_reset`, `budget_save_edit`): present.
- BUD-2 save for the CLI and MCP (`save_pool_edit`: file, ledger row in `<data>/pool-edits.jsonl` and Waiting item, all or nothing; `edit_summary`, the reply text naming each new value and the old one): present.
- BUD-2 edits at the same time (#416): the save holds `lock::FileLock` on `config.json`, then on the ledger, for the read, the writes, the Waiting item and any undo; a second edit waits up to `EDIT_LOCK_WAIT` (10 s), then is refused with what to do. Temp files are named per run (`lock::temp_path`). A cap is at least `MIN_CAP_USD` (a cent): present.
- BUD-2 editor (`PoolEditor`): present.

### backup
`crates/core/src/backup.rs` · BAK-1, BAK-2, BAK-3 · planned. A git bundle of every ref plus JSON of issues, PRs, comments, labels, milestones, releases (under 1 GB) and the wiki, with a manifest, in `<NextCloud backups>/github/<repo>/`; the overwrite guard; restore to a new private repo (issues through the create queue), over a live repo only after `rules::guard`.
- BAK-2 overwrite guard (`refuse_reason`): present. `backup_repo` also refuses, before any read, when the last backup's manifest is damaged or missing beside other files, and when the last backup has a wiki and the new one has none.

- BAK-1 JSON of issues, PRs, comments, labels, milestones (`write_json`): present.
- BAK-1 git bundle (`bundle`): present.
- BAK-1 releases.json (`write_release_list`, `RELEASE_FILE_LIMIT`): present.
- BAK-1 release files under 1 GB (`fetch_release_files`, `write_releases`; `FileSource`, `HttpFiles` over the network; part of every `backup_repo`): present. A file that does not come with the size the release gives fails the backup.
- BAK-1 wiki mirror (`copy_wiki`): present. Only a clone that says the repo is not there means no wiki; any other failed clone fails the backup.
- BAK-1 back up one repo or all (`backup_repo`, `backup_all`, `BackupOutcome`, `RepoBackup`; the Sky's back up buttons): present. One backup of a repo at a time in a process, each in a staging folder of its own (`.<name>.new-<run>`); `backup_all` tries every repo and gives each its own outcome or error; the manifest holds the bundle's hash (`bundle_hash`) and, when GitHub answered the name asked for with the items of another (a repo renamed or moved before the backup), that name (`resolved_repo`), which the outcome line also says.
- BAK-W back up from the app (`backup_run`: `folder`, `repos`, `record`, `failed_row`, `run_one`, `run_all`; commands `backup_repo`, `backup_all`): present.
  A repo whose backup failed in a "Back up all" is a row with outcome `failed`.
- BAK-3 restore (`restore_history`, `restore_issues`, `restore_over_live`, `restore_held_over_live`, `Pass`, `RestoreReport`, `IssuesReport`; `crates/core/src/renumber.rs`: `renumber`): present in core. The app calls `restore_over_live`, then `restore_held_over_live` for what was held back, after my confirm (`confirm.rs`, #347); no face calls the restore to a new private repo yet. The backup is checked against its manifest before any write (repo name against the folder, and the manifest's `repo` or `resolved_repo` against the API addresses in the JSON, ref count, bundle hash, label and milestone counts); labels and milestones keep colour, description and due date; a comment goes to the number its issue has in the repo now; whatever the repo already has is not created again; issues and milestones that were closed in the backup are closed once they are there (over a live repo only those the restore made: `Made`, the issues and milestones its batch added to the create queue, each by the number the queue's own record says it made, never one found by title; anything else is left as it is and named), and a plain `#N` in a restored body or comment gets the number its issue has now (#365; through `rest_patch`, counted in `closed`, `milestones_closed`, `renumbered`, and in `state_waiting` while they wait); what is not restored is listed in `left_out` (PRs and their comments, the wiki, releases, a label that is there with another colour).

### frontdoor
`crates/core/src/frontdoor/` · FD-1 to FD-13 · planned. `router` (the four steps, the `first` list against budget, the learned pick), `labeller` (rules, then a small model, U8), `adapters` (one per service; none for the Claude subscription), `packet` (summary, last 6 turns, links; the summary check), `sticky` (what moves a chat; implicit signals), `thread` (one JSON Lines file per thread). Face: `app/src/views/Chat.tsx` with BroCrew's composer and messages ported; the "Open in Claude" button (FD-8).
- FD-6 packet (`build`): present.
- FD-7 summary check (`missing_from_summary`, `checked_summary`): present.
- FD-9 thumbs rating log (`thumbs_from_name`, `rating_entry`, `log_rating`, `chat_rate`): present.
- FD-2 rules labeller (`by_rules`, `label`): present.
- FD-2 small labeller (`LlamaLabeller`): present.
- FD-13 thread store: present.
- FD-11 adapter interface (`split_handoff`, `log_row`): present.
- FD-11 Qwen adapter (`QwenAdapter`): present.
- FD-11 DeepSeek adapter (`DeepSeekAdapter`, `post_json_headers`): present.
- FD-11 ChatGPT adapter (`ChatGptAdapter`): present.
- FD-11 Gemini adapter (`GeminiAdapter`): present.
- FD-11 Claude API adapter (`ClaudeApiAdapter`): present.
- FD-9 rating and implicit rows (`rating_row`, `implicit_row`): present.
- FD-9 implicit signal log (`implicit_from_name`, `log_implicit`, `chat_implicit`): present.
- FD-9 rating checks (`log_rating` refuses an unknown thread or turn, a question, or a service and model that did not answer; a second rating of a turn replaces the first through `ThreadStore::upsert`; `log_implicit` refuses an unknown thread; `ThreadStore::turns`, `is_turn`): present.
- FD-10 separate move field (`with_move`, `thumbs_down_counts`): present.
- FD-8 Open in Claude (`as_prompt`): present.
- FD-4 learned pick (`sample_size`, `learned_pick`): present.
- FD-1 chat_route command (`ChatRoute`, `FleetGate`, `route_message`, `estimate_complexity`, `chat_route`): present.
- FD-11 answer a routed turn (`answer_route`, `AnswerContext`, `ChatReply`, `qwen_base_url`, `key_var`; `chat_route` returns `ChatReply`: the route plus `text`, `service`, `model`, `handoff`, `handoff_id` when answered): present. Keys come from one environment variable per paid service: `DEEPSEEK_API_KEY` (deepseek), `OPENAI_API_KEY` (chatgpt), `GEMINI_API_KEY` (gemini), `ANTHROPIC_API_KEY` (claudeapi); qwen needs none and takes its URL from `machines.<machine>.qwen_server_url` in the fleet config. A missing key is the turn's error (`no key for <service>: set <VAR>`), never another service. A chat turn's thread row carries `role` and `text`.
- FD-3 queued ask (`answer_route` stores a wait's ask marked `queued` and the reply's `queued` says why; `queued_asks`; the Chat view shows "Queued: <reason>"): present.
- FD-5 sticky signals (`signals`): present.
- FD-5, FD-6, FD-7, FD-10 sticky chat and move (#378: `route_in_thread` keeps an open thread on its model, step `sticky chat`, until `move_signals` finds a signal or its pool is braked; `answer_route` sends a move the packet and the turns since, the outgoing model's summary checked by `checked_summary`, stores the `summary` and `move` rows, marks a moving thumbs down `moved`, and a Claude handoff's reply carries the `packet` for Open in Claude): present.
- FD-3 pick for a kind (`pick_for_kind`): present.
- FD-3 Qwen fallback (#377: `chat::qwen_fallback_on`, `Fallback`, `answer::answer_fallback`; `ask_on` is the one place it is applied, so the window, CLI and MCP share it; `Seams.qwen_up` is the probe; the reply's `fallback` says why, the answer row is marked `fallback`, and `sticky::open_chat` skips such rows so one fallback does not move a sticky chat; the Chat view shows "Qwen answered: <reason>"): present.
- FD-5 work-tree line of a move (#378: the move row carries `moved_from` and `moved_why` (`answer::move_why`), `reports::moves` reads one line per move row; the reply's `moved_why`; `Chat.tsx` `moveLine`): present.
- FD-6, FD-8 send as job from a chat (#378: `answer::send_thread_as_job` sends the packet plus the ask and stores a move to `fleet-job`; `ask::send_as_job` takes the thread, `job_why`; `chat_send_as_job` takes `handoff_id`; CLI `ask_as_job --thread`, MCP `thread`): present.
- FD-7 open questions (#378: `answer::open_questions`, a queued ask not asked again; listed in the summary ask and checked by `packet::checked_summary`): present.
- FD-9 implicit callers (#378, `Chat.tsx`: `opened-in-claude`, `re-asked`, `copied-answer`, `abandoned` through the `implicit` prop, default `chat_implicit`): present.
- FD-13 thread store lock (#419: `ThreadStore::locked` holds a `FileLock` beside the thread file over `append_all`, `upsert` and `change`, so two processes lose no turn): present.
- FD-1 router (`route`): present.
- FD-1 Chat composer (`PromptComposer`): present.
- FD-1 Chat messages (`UserMessage`, `AssistantMessage`): present.
- FD-9 thumbs (Chat view): present.
- Headless front door (#384, `ask.rs`: `ask`, `ask_on`, `Seams`, `Asked`, `known_thread`, `ask_headless`, `send_as_job`; `rating_log::rate_turn`): present. `ask` is the route (`route_in_thread_on`) and the answer (`answer_route`) in one call, the one `chat_route` makes; a dry run stops after the route, so nothing is sent, stored or spent. `ask_headless` refuses a thread id that is not stored. `send_as_job` is `chat_send_as_job`'s call. `rate_turn` reads the service, model and kind from the stored turn and goes through `log_rating`.

### autopilot
`crates/core/src/autopilot/` · AUTO-1 to AUTO-9 · planned. `may_spend` (budget allow), `review` (R25's ladder as a state machine), `driver` (script loops, Qwen plus one), `instructions` (untracked instruction files; drift of copies), `classify` (by the card, through `fleet errors classify`), `weekly` (Sunday 6am, ROADMAP changes).
- AUTO-1 may_spend: present.
- AUTO-9 classify (`classify`, `from_json`): present.
- AUTO-6 loop driver (`may_start`, `next_command`): present.
- AUTO-6 satellite loop (`tick`, `ChildRunner`, `classify_failed`; the app's satellite thread, switched on by `satellite.json`): present.
- AUTO-5 weekly due check (`weekly_due`): present.
- AUTO-5 weekly run (`weekly_run`, `drift_pass` in `passes.rs`; the drift clear and the backup after Sunday 6am, from the app's poll thread): present.
- AUTO-5 local lane health (`FleetStarter::may_spend` asks `qwen_server_up` for the lane `qwen`: an HTTP GET with `http_qwen_probe` on `machines.<machine>.qwen_server_url`, read by `FleetModels::read_qwen_server_url`; no URL or no answer means the next lane): present.
- AUTO-5 failed-job clean-up (`pm_cleanup_target`, `remove_pm_worktree` in `passes.rs`; `FleetStarter::finished` removes the worktree and branch of a Project Manager job that ended `infra` with nothing pushed): present.
- AUTO-7 place: present.
- AUTO-3 review (`step`, `first_rung`): present.
- AUTO-3 review job (`review_job`): present.
- AUTO-3 review start (`after_poll`, `review_pass` in `passes.rs`; the app's poll thread starts it): present.
- AUTO-4 merge pass (`merge_pass` in `passes.rs`; the app's poll thread runs it after `after_poll`): present.
- AUTO-4 refused merges (`MergePass`, `RefusedMerge`, `refused_merge_item`, `record_refused_merges`: every passed PR is tried, and each refusal is a Waiting item): present.
- AUTO-8 drifted copies (`drifted`): present.
- AUTO-2 roadmap_changed: present.
- AUTO-2 roadmap pass (`roadmap_pass`, `pm_job` in `passes.rs`; run by `after_poll`): present.

### it
`crates/core/src/it.rs` · IT-1 to IT-4, FD-12 · planned. The loadout from the platform's probe list, unmet `needs`, IT tickets through `fleet errors file`, `it-triage` jobs with `never_on` (waits for U9), ladder-out bugs with the star's `feature:` label. `open_tickets(snaps)` lists the open `it`-labelled issues in the stored snapshots, for `errors` (#375).
- IT-1 loadout read (`read_loadout`): present. `SystemProbeRunner` kills a probe after `PROBE_TIMEOUT` (5 s, `run_probe_within`); it then has no exit code (#339).
- IT-1 loadout every minute (`read_and_store`, `latest_loadout`, `current_loadout`, the `loadout` event): present.
- IT-2 file error tickets (`file_error_tickets`): present.
- IT-2 tickets for the app's fleet errors (`file_tickets`, `record_fleet_error`): present.
- IT-3 triage job (`triage_job`): present.
- FD-12 unmet needs (`unmet`, `held_job`): present.
- FD-12 send a held request (`this_machine`, `send_held_job`, `chat_send_as_job`, `ChatHeld`): present.
- IT-4 ladder-out bug (`ladder_out_bug`): present.
- IT-W loadout command (`SystemProbeRunner`, `read_this_machine`, `loadout`): present.

### cli
`crates/cli/src/main.rs`, `cli.rs` · CLI-1, CLI-2, CLI-3 · CLI-1, CLI-2 and CLI-3 present. Text or `--json`; the same numbers as the GUI; role-flagged writes through `rules::guard`. Arguments are read with `args_os` (a non-UTF-8 word is exit 2, `utf8_args`); `--help`/`--version` answer anywhere on the line; extra words after a command are exit 2; stdout goes through `emit` (a closed pipe is a quiet exit 0, any other write error one line and exit 1, never a panic); text output escapes control characters in titles and other outside text (`terminal_safe`). A bare repo name resolves to its one `owner/name` (`resolve_repo`); one that matches two owners is an error naming both. `main.rs` routes `mcp` (`mcp_call`): `--role` before or after it, `--help`, and exit 2 for any other flag or word.
- CLI-1 parse arguments, budget, sky, star, waiting, reports, loadout: present. `sky` and `star` read the stored snapshot in the app data folder (`SWITCHBOARD_DATA`, else the desktop app's) and never call GitHub; `star` reads its running job from the job log when a pipeline checkout is known. The pipeline checkout is `SWITCHBOARD_PIPELINE`, else `pipeline_repo` in `settings.json` (`pipeline_dir`). `budget` text shows points over or under pace, the reading's age and the stale mark (BUD-1); `reports` text shows all five REP-1 parts under each head line.
- CLI-2 `refresh`, `backup <repo>`, `backup --all`: present. `backup` reads `settings.json` from the app config folder (`SWITCHBOARD_CONFIG`, else `SWITCHBOARD_DATA`, else the desktop app's; `config_dir`). The Refresh and Back up buttons from the CLI, one line per repo (`<repo>: refreshed | unchanged`, `<repo>: backed up to <path>`); a failed or refused repo goes to stderr and the exit is 1 after all were tried; `backup` alone exits 2.
- `errors` (#375, read only): the fleet errors the app kept (`fleet_errors::load`), newest first, then the open IT tickets (`it::open_tickets`); `--json` is `{fleet_errors, skipped_lines, it_tickets}` (`errors_answer`), and damaged lines are named on stderr: present.
- CLI-3 delete and restore refused to Waiting on you, `--role` merge through the guard and GH-5's merge (`CliWrites`), none inside a job worktree: present.
- `--role budget-manager set_pool_cap <pool> <cap>` and `pool_edit <pool> [--plan] [--cap] [--reset] [--cost]` (`pool_command`, `take_pool_flags`) through `rules::guard_pool_edit` and `budget::save_pool_edit`, the same path as MCP's tools; `budget --refresh` re-reads the pools first (`budget::refresh`); `settings` prints the settings file, read only (`settings_command`): present. `refresh` and `backup` write stdout through `emit_to` too.
- Front door (#384, `cli_frontdoor.rs`): `ask "<text>" [--dry-run] [--thread <id>]` prints the route and its reason, then the thread and the answer; `ask_as_job <kind> "<text>" --unmet <need>[,<need>] [--thread <id>]`; `rate <thread> <turn> <up|down>`. Each calls the core function the window calls (`frontdoor::ask`). `--role project-manager` through `rules::guard_front_door`; `ask --dry-run` needs no role. A bad line is exit 2, a refusal exit 1; `--json` is the reply, the job or the rating row: present.

### mcp
`crates/cli/src/mcp.rs` · MCP-1 to MCP-3 present. JSON-RPC over stdio; six read tools; role read tools (`role_read_tools`: the satellite's `errors`, #375, the CLI's `errors --json`); role write tools; neither inside a job worktree. Bad lines (non-UTF-8, batch, non-object) get a JSON-RPC error; tool arguments are checked against each tool's input schema.
- MCP-2 write tools reach the real core (`CoreWrites`: the CLI's merge, BUD-2's save, local Waiting items): present. budget-manager has `set_pool_cap`, `pool_edit` and `budget_refresh`; a pool edit's reply states what was set. `reports` takes the CLI's `project`, `outcome` and `pool` filters (`cli::reports_answer`).
- MCP-3 no write tools in a job worktree: present.
- Front door (#384, `mcp_frontdoor.rs`): project-manager also has `ask` (`message`, `thread`, `dry_run`), `ask_as_job` (`kind`, `request`, `unmet`, `thread`) and `rate` (`thread`, `turn`, `rating`), each through `rules::guard_front_door` and `WriteCore::front_door` to the CLI's core calls: present.

### shell
`app/src-tauri/`, `app/src/App.tsx`, `app/src/core.ts`, `app/src/ErrorBanner.tsx` · SHELL-1, SHELL-2, SHELL-3 · present (empty window, wired to the core: `views`, `settings_missing`, `fleet_errors` (newest first, read from the errors kept in the app data folder, #375: `keep_errors_in`, `fleet_error_rows`); the error banner present); the AppImage present: `tools/build-appimage.sh` builds it inside the craft-build container). `tools/check-notices.py` (present) lists every file under `app/src/brocrew/` and fails on one without its Apache-2.0 notice. `app/src/brocrew/` holds BroCrew's lifted chat helpers (present): `composer-notice.ts`, `message-links.ts`, `message-paths.ts` and `ipc-types.ts`, all listed in `NOTICE`.
