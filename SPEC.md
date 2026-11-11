# Spec: Switchboard 1.0

**1.0 means:** on the Z13 (the Strix Halo computer) I open one Switchboard app and, without opening a Claude session, I can see every project's sky and its drift, everything waiting on me, each job's report and every pool against its pace line; I can back up any repo; and I can ask the front door anything and have it answered by the cheapest pool in budget that is good enough, with Claude reached only when I press "Open in Claude".

**Approved by me:** not yet. The R02 grilling for this Spec is the 2026-11-07 grill-with-docs rulings and the rulings made after that day's adversarial review (pipeline repo, private: the Switchboard proposal). This file collapses them, and rules on every review finding and feature idea (see [Review dispositions](#review-dispositions)).

A project is Spec'd only when the line above has a date and a link. Change a Must or a Won't after approval and the approval line goes back to "not yet" until I approve again.

## How this Spec is laid out

It is a wayfinder map in the shape of the `/wayfinder` method (the skill isn't installed on this machine, so this file follows its shape by hand):

- **[Goal](#goal):** the destination, fixed before anything else.
- **[Regions](#regions-in-10):** the features of 1.0. Each region has an id, which is its `feature:<id>` label and its entry in the ROADMAP `features:` block. Each Must in a region is something I can do and see, with a check that proves it; each Must becomes at least one issue.
- **[Regions after 1.0](#regions-after-10-dark-stars):** features that get no issues yet. On the sky they are dark stars.
- **[Routes](#routes):** the order the regions are built in, and how data travels between them.
- **[Waypoints](#waypoints):** checkable stops on the way to 1.0.
- **[Unknowns](#unknowns):** the fog: questions I can see coming, each with a type (research, prototype, grilling or task) and the default the build uses until it is answered.
- **[Won'ts](#wonts):** out of scope for 1.0, so nobody builds it by accident.

Words are the glossary's (`GLOSSARY.md`). Decisions already made are in `docs/adr/`.

## Goal

I want to stop spending Claude tokens on management. Switchboard is one front door that takes any message and sends it to the cheapest model and service that can do it within budget, and one app that shows every handoff, everything waiting on me, and each result.

1.0 is **one machine, the Z13**, running every service that machine can reach and talking to GitHub directly. It is:

- a **Tauri** app: a Rust core and a React face, with BroCrew's chat parts lifted (Apache-2.0, credited);
- **one core with three faces**: the GUI, the `switchboard` CLI, and `switchboard mcp` (stdio, read-only by default). Every face goes through the same core and the same rules;
- a core that **wraps the Python `fleet` tools** in the pipeline repo rather than porting them;
- the views **Constellation** (the sky), **Waiting on you** and **Reports**, the **budget panel** with each pool's pace line, and **repo backup**;
- GitHub read in code by a **10-minute GraphQL poll**, plus refresh-on-action and a Refresh button.

## Problem

Today I route by hand inside Claude sessions. Claude spends its quota deciding where work goes, reading GitHub to find out what is open, and writing reports I then have to chase. Each project's roadmap goes unread once its Dev Loop issues are filed: "we never go back and reference either of those unless I force it". What is waiting on me is spread over CalDAV, GitHub labels and chat. Pool budgets live in a terminal command. Nothing backs up the repos except GitHub itself.

## Solution

One app on the Z13. The sky keeps every project's roadmap in front of me and the Project Manager, with drift drawn on it. Waiting on you is one list. Reports have a fixed shape that says what wasn't checked. The budget panel shows each pool against its pace line, and I edit pools in place. The front door answers each ask with the cheapest pool in budget that is good enough, by rules in code, and hands big asks that need tools to Claude only when I press the button. The Project Manager, the Budget Manager and the satellite run as code and routed jobs on this machine, so the management work stops costing Claude tokens.

## User stories

1. As the person running the pipeline, I want one sky of every project, so that I see priority, progress and drift without opening a repo.
2. As the Project Manager, I want drift (orphan stars, dark stars, a stale roadmap) flagged on the sky, so that I can clear it every Sunday.
3. As the person running the pipeline, I want everything waiting on me in one list with one action each, so that nothing stalls on me unseen.
4. As the person running the pipeline, I want a fixed report for every job and handoff that says what wasn't checked, so that I know what to trust.
5. As the Budget Manager, I want each pool's usage against its pace line, editable in place, so that routing follows the real budget.
6. As the person running the pipeline, I want any message answered by the cheapest model that is good enough, so that Claude's quota goes only to work that needs it.
7. As the person running the pipeline, I want a big ask that needs tools to reach Claude only through a button I press, so that I stay in the loop.
8. As the owner of the repos, I want one latest repo backup of each, so that I could rebuild it if GitHub lost it.
9. As an agent in a Claude, Codex, Gemini or opencode session, I want read-only MCP tools for the sky, Waiting on you, Reports and the budget, so that I can answer without scanning GitHub.

## Regions in 1.0

Region ids are the ROADMAP `features:` ids and the `feature:<id>` labels. "The core" is the Rust library every face calls.

### `shell`: app shell and packaging

The Tauri window, the React face's frame, and the AppImage. Tauri on the Z13 was proven on 2026-11-07 at 3:55pm: a minimal Tauri 2 AppImage built in `craft-build`, with WebKit bundled (96 MB), ran on the host in Desktop Mode under XWayland.

| # | Must | Done looks like | Check |
|---|---|---|---|
| SHELL-1 | WHEN I start the Switchboard AppImage on the Z13 in Desktop Mode, THE APP SHALL open one window with Sky, Waiting on you, Reports, Budget and Chat views. | The window opens on the host with nothing installed on the host; empty views say "no data yet". | Build inside `craft-build`; launch on the host; screenshot shows the five views. |
| SHELL-2 | WHEN a file lifted or ported from BroCrew ships, THE APP SHALL keep its Apache-2.0 notice and credit. | Each such file carries the notice; `NOTICE` and the README credit BroCrew. | A license check that lists every lifted file and fails on a missing notice. |
| SHELL-3 | WHEN the core reports an error to the face, THE APP SHALL show it in place with the command that failed and its first lines of output. | A banner on the view, not a blank pane. | Face test with a failing core fixture. |

### `core`: the core and the fleet bridge

One Rust library: settings, the fleet bridge, the job log, the snapshot store, and the rules every face goes through.

| # | Must | Done looks like | Check |
|---|---|---|---|
| CORE-1 | WHEN any face needs fleet data (budget, ledger, pick, route, status, plan), THE CORE SHALL get it by running the pipeline repo's `fleet` tools with `--json` and parsing the result; a fleet call that runs past 120 seconds is killed and is a fleet error (CORE-2). | Values in the app match `fleet.py <cmd> --json` run by hand. | Core tests against a stub `fleet.py` (the `FLEET_ROOT` override) that prints fixture JSON. |
| CORE-2 | WHEN a fleet command fails or prints JSON the core can't parse, THE CORE SHALL record a fleet error with the command, exit code and stderr, and never guess a value. | The view shows the fleet error; the IT region sees it. | Core test with a stub that exits 3 and one that prints bad JSON. |
| CORE-3 | WHEN any face asks for a guarded action, THE CORE SHALL apply the same rule whichever face asked. **Merge:** from the GUI, or from the CLI or MCP with a role flag (CLI-3, MCP-2), and only under GH-5's rule (a passing R25 review on the PR's head or carried over by patch-id, and CI green on that head); never from inside a job worktree (CLI-3, MCP-3). **Repo archive and restore over a live repo:** only after I confirm in the app; a CLI or MCP request for either becomes a Waiting on you item. Switchboard never deletes a repo: the `delete` request archives it on GitHub (read-only), and nothing is removed. A waiting request can be turned down (dropped) from the app, the CLI or MCP: its item leaves Waiting on you and nothing else happens; dropping one that is not waiting is refused. | The same rule and the same refusal text from the GUI, the CLI and MCP. | One rules test per face per guarded action. |
| CORE-4 | WHEN the core reads the job log, THE CORE SHALL read one row per attempt from the fleet ledger and from the local chat rows, with one row shape for both: the fields the ledger already has (`kind`, `size`, `lane` as the service, `model`, `effort`, `tokens`, `cost_usd`, `seconds`, `tools`, `loc_changed`, `files_changed`, `outcome`, `failure_mode`, `failure_class`, `attempt`, `rounds`) and the ones the pipeline batch adds (`rating` and `handoff_id`, ruled; `checks`, `escalation_step` and `review_fix_loc`, which this Spec asks for, see [U9](#unknowns)). A field a row lacks is shown as "not recorded", never guessed. | Reports and the router read the same rows. | Core test over a fixture ledger that carries the batch's fields, and one written before the batch. |
| CORE-5 | WHEN I first open the app, THE APP SHALL ask for its paths (pipeline repo clone, NextCloud backups folder, vault agents folder) and keep them in one settings file. | Settings view; a missing path is named, never guessed. | Face test with an empty settings file. |

### `github`: GitHub reads and writes

The snapshot is the cached GitHub read. 1.0 reads GitHub in code; no agent scans. Agents keep their own reads for now.

| # | Must | Done looks like | Check |
|---|---|---|---|
| GH-1 | WHILE the app is open, THE CORE SHALL read every constellation's repo (issues, labels, milestones, PRs and their review verdicts, ROADMAP.md, the code map) with GraphQL every 10 minutes and keep it as the snapshot. | Each view shows the snapshot's age; the Refresh button reads now. | Core test with recorded GraphQL responses and an injected clock: one read per 10 minutes. |
| GH-2 | WHEN a repo has not changed since the last read, THE CORE SHALL skip its GraphQL read. | Unchanged repos cost no GraphQL points. | Core test: a REST conditional request with the stored ETag answers 304 and no GraphQL query is sent. |
| GH-3 | WHEN the core makes any GitHub write, THE CORE SHALL re-read that repo right after (refresh-on-action). | The sky reflects the write within seconds. | Core test: a write is followed by one read of that repo. |
| GH-4 | WHEN the core creates anything on GitHub (issue, comment, label, milestone, PR), THE CORE SHALL send it through the one throttled create queue the pipeline batch adds, shared with fleet jobs. | A rate limit makes the queue wait; it never fails a job or files a bug. | Core test: 30 queued creates are spaced; a secondary-rate-limit answer re-queues the item. |
| GH-5 | WHEN the core merges a PR, THE CORE SHALL merge it only with `merge_pr.py`, only when its latest `VERDICT: PASS` is on its head or carries over an update from main by patch-id (R25), and only once CI is green on that head (R14); never `gh pr merge` or the merge API. | A PR with an old PASS and a new code commit is refused; one whose only new commit is an update from main with the same patch-id keeps its PASS; a PASS on a head whose CI is red or still running is refused. | Core test with fixture verdicts and check runs on three head shas: a new code commit, an update from main, and a reviewer's fix with CI pending. |

### `sky`: Constellation

The main view. Our sky, not wayfinder-maps' viewer: the look is borrowed (MIT, credited), the renderer is ours.

| # | Must | Done looks like | Check |
|---|---|---|---|
| SKY-1 | WHEN I open the Sky, THE APP SHALL draw every repo in the priority order as a constellation, its distance from the centre and its size set by its rank, with the top 3 large and in the middle. | Moving a project up the priority order moves it inward and makes it larger. | Pure layout test: rank to radius and size; screenshot of a fixture sky. |
| SKY-2 | WHEN I look at a constellation, THE APP SHALL set its brightness by its progress to 1.0: closed stars in the `1.0` milestone over all stars in it, each dark star of a feature marked `release: 1.0` in the `features:` block counting as one open star. A dark star of a feature marked `release: after-1.0` or `release: after-2.0` is drawn but not counted. | A project with unfiled 1.0 features does not look finished; its After 1.0 features don't keep it dim once 1.0 is done. | Layout test over fixture issues and a `features:` block with both kinds. |
| SKY-3 | WHEN I click a constellation, THE APP SHALL zoom into it: each feature a cluster (label `feature:<id>` matching the ROADMAP `features:` block), each star one issue, a line for each `Blocked by #N`, and a ring for the `1.0`, `After 1.0` and `After 2.0` milestones. | The zoomed view of a fixture repo matches its issues. | Face test over a fixture snapshot. |
| SKY-4 | WHEN a star changes state, THE APP SHALL draw it dim while open, lit once merged or closed, and pulsing while a fleet job for it is in flight. | A running `fleet run` makes its star pulse. | Face test with a fixture `jobs/running` entry. |
| SKY-5 | WHEN a project has drift, THE APP SHALL flag it on the constellation and list it: orphan stars (an issue on no feature or on two), dark stars of features marked `release: 1.0` (an After 1.0 or After 2.0 dark star is planned, not drift), and a ROADMAP `Updated:` date more than 14 days old. | A drift count on each constellation and a drift list the PM can clear every week; a project whose only dark stars are After 1.0 shows no drift. | Core test for each of the three kinds, and one with only After 1.0 dark stars. |
| SKY-6 | WHEN a project's code map lists `reuses:`, THE APP SHALL draw a dashed reuse line to each project named, drawn as a warning when that project is Parked or archived. | A fixture reuse of an archived repo shows the warning. | Face test. |
| SKY-7 | WHEN I zoom into a constellation, THE APP SHALL highlight its longest chain of blocked-by lines and say how many stars are on it. | The chain that gates 1.0 stands out. | Pure test over a fixture graph. |
| SKY-8 | WHEN I click a star, THE APP SHALL show its title, labels, size, linked PR, review verdict and fleet job, with a link to GitHub. | One panel per star. | Face test. |
| SKY-9 | WHEN the sky draws, THE APP SHALL read the priority order from its priority copy (see [U4](#unknowns)) and show the copy's age; older than 7 days it becomes a Waiting on you item. | The age is on the sky; a stale copy is not silently trusted. | Core test with an old copy. |
| SKY-10 | WHEN an issue is filed by the design pass (R04) or a bug is filed automatically (IT-4), THE CORE SHALL give it a `feature:` label: a design-pass issue gets the `feature:<id>` of the region whose view it changes (one that spans views gets `feature:shell`, the app frame); an automatic bug gets the `feature:<id>` of the star whose job ran out of ladder. | Neither shows up as an orphan star. | Core test: a fixture design-pass issue and an IT-4 bug each carry one `feature:` label. |

### `waiting`: Waiting on you

One list of everything that needs me, each item with what, why, where it came from, and one action.

| # | Must | Done looks like | Check |
|---|---|---|---|
| WAIT-1 | WHEN anything needs me, THE APP SHALL list it in Waiting on you: my CalDAV tasks, issues and PRs labelled as needing me, PRs whose R25 review posted a FAIL without a fix (a design question or a decision of mine), PRs whose review ladder reached Fable, Claude handoffs not yet opened, guarded actions asked for from the CLI or MCP, IT tickets that need a setting, a delete or a credential, refused backups, and a stale priority copy. | One list, newest first, each item linked to its source. | Core test with one fixture per source. |
| WAIT-2 | WHEN I tick a CalDAV task in Waiting on you, THE APP SHALL mark it done in CalDAV. | The task is done on my phone too. | Core test against a CalDAV fixture server. |
| WAIT-3 | WHEN an item's source clears (a label removed, a PR merged, an IT ticket closed), THE APP SHALL drop the item at the next read. | No stale items. | Core test over two snapshots. |

### `reports`: Reports

A fixed report for every finished job and every chat handoff, in BroCrew's fixed-report shape plus the line our job cards lack.

| # | Must | Done looks like | Check |
|---|---|---|---|
| REP-1 | WHEN a job or a chat handoff ends, THE APP SHALL show a fixed report: outcome first, what changed (PR, files, lines ±), what was checked (the build, test and lint output R40 puts in the PR), what wasn't checked, and cost (pool, tokens, dollars, time). | Every report has all five parts; an empty "what wasn't checked" says "nothing". | Face test over fixture job cards and chat rows. |
| REP-2 | WHEN I open a report, THE APP SHALL show its work tree: each move of the chat or job as a line (from service and model, to service and model, and why). | I can see every handoff of a chat. | Face test with a three-move fixture thread. |
| REP-3 | WHEN I filter Reports by project, outcome or pool, THE APP SHALL show only those reports. | Filters work on fixtures. | Face test. |

### `budget`: the budget panel

| # | Must | Done looks like | Check |
|---|---|---|---|
| BUD-1 | WHEN I open the budget panel, THE APP SHALL show one collapsible row per pool with a bar, its pace line, how far over or under pace it is in points, its verdict, and the age of its reading. | Matches `fleet.py budget --json`. | Core test over a fixture budget. |
| BUD-2 | WHEN I edit a pool in place (plan, cap, reset day and time, cost), THE APP SHALL save it to `config.json` with a ledger row, and pace and routing SHALL use the new values at once. | The bar moves when I save. | Core test: the edit writes the file and a ledger row; the next `budget` read shows the new pace line. |
| BUD-3 | WHILE the app is open, THE CORE SHALL read each pool's quota itself where a reader exists (`fleet budget --refresh` for Gemini and Codex, the DeepSeek balance, the Claude API reading) on a timer. | Readings refresh without a Claude session. | Core test with stub readers. |
| BUD-4 | WHEN the core shows Claude's pool, THE APP SHALL use the newest recorded `get_usage` reading and label its age, and SHALL add an estimate from my own sessions' local usage logs since that reading, marked as an estimate. | Claude's row never shows a guess as a reading. | Core test with a reading and fixture session logs. |

### `backup`: repo backup

| # | Must | Done looks like | Check |
|---|---|---|---|
| BAK-1 | WHEN I press "back up" on a constellation, or "back up all", or the weekly run after the Sunday 6am reset fires, THE CORE SHALL write the repo backup to `Backups/github/<repo>/` in NextCloud: a git bundle of every ref, plus JSON of issues, PRs, comments, labels, milestones, releases (release files under 1 GB) and the wiki, replacing the last one. | One folder per repo, with a manifest of counts and the time. | Core test against a local bare repo and fixture API answers; `git bundle verify` passes. |
| BAK-2 | WHEN a new backup, compared with the one it would replace, has far fewer issues, PRs or labels in all, far fewer open issues or open PRs, far more issues closed that no merged PR closed, or far more label assignments removed from issues ("far" meaning more than 10% of the old backup's count or more than 5), THE CORE SHALL keep the old one and put the change in Waiting on you. | A mass close or relabel by a bad automated pass cannot overwrite the last good copy, even when the totals stay the same; a week of merged PRs closing their issues can. | Core test with fixtures: shrinking totals; the same totals with 8 issues closed by no PR; the same totals with 8 label assignments removed; 20 issues closed by merged PRs (kept as the new backup). |
| BAK-3 | WHEN I restore a repo backup, THE APP SHALL restore it to a new private repo by default (history, labels, milestones, then issues through the create queue), and restore over a live repo only after I confirm in the app. A label the live repo already has keeps its live colour and description: the restore leaves it alone and names it in the report when the colour differs. In a restored body or comment a `#N`, an `owner/name#N` and a full issue or pull-request link (`https://github.com/owner/name/issues/N`, `/pull/N`) that name the restored repo get the number that issue has now; a reference to another repo, in code, or to a number the restore did not map stays as it is. | A restored repo matches its manifest. | Core test against a local bare repo and a fixture GitHub. |

### `frontdoor`: the front door chat

The router is code, not a model. The abilities index is the pipeline repo's `actions.json` with request kinds and `needs`, added by the pipeline batch.

| # | Must | Done looks like | Check |
|---|---|---|---|
| FD-1 | WHEN I send a message in the front door, THE CORE SHALL route it in this order, stopping at the first match: a trigger word (an agent's name) goes to that agent's bound model; an explicit ask ("search the web", "write code", a skill name) goes to the matching `needs` tag; otherwise the labeller names the request kind; still unclear, the budget and complexity rule (small: the cheapest pool in budget; big without tools: Claude API Sonnet; big with tools: a Claude handoff). | The chat says which step picked the model. | Router table test, one row per step. |
| FD-2 | WHEN the labeller runs, THE CORE SHALL use rules first and then a small model in a llama-swap group that never swaps out, and never the 27B job's slot; it labels, it never answers. | Labelling a message takes under a second and never touches a running Qwen job. | Core test of the rules; a timed run with a Qwen job in flight. |
| FD-3 | WHEN the core picks a model for a request kind, THE CORE SHALL take the first service in that kind's `first` list whose pool is under budget (`fleet budget allow`), preferring quotas that reset (Claude, ChatGPT, Gemini) and using paid pools only as overflow; with none left, it queues the ask and says so. The Claude subscription (the `claude` lane, `runnable: false`) is never called by the core: when it is the pick and Claude's pool is under budget by BUD-4's reading, the chat becomes a Claude handoff (FD-8); when it is braked, the kind's fallback is taken, and a kind with none (judgement-planning) waits for budget and the chat says so. | A braked pool is skipped and the chat says why; a judgement-planning ask shows "Open in Claude", or "waiting for Claude's budget". | Router test over fixture budgets, with rows for the Claude subscription under budget, braked with a fallback, and braked with none. |
| FD-4 | WHEN a request kind has N or more rated rows for a model, THE CORE SHALL take the cheapest model with at least 80% passes over its last N rated rows; N starts at 6 and rises by 1 for every 20 rows the kind has logged, to a cap of 30; below N rows the index's guess stands. | The pick changes as ratings come in. | Router test over fixture job logs at 5, 6, 26 and 600 rows. |
| FD-5 | WHILE a chat is open, THE CORE SHALL keep it on its model (a sticky chat) until I say "ask Claude" or name an agent, I give a thumbs down, or the model writes the handoff marker `[[handoff: <reason>]]`; each move is a line in the work tree. | A follow-up question stays on the same model. | Core test of each signal. |
| FD-6 | WHEN a chat moves to another model, THE CORE SHALL send a packet: the rolling summary (rewritten by the outgoing model at each handoff, never stacked), the last 6 turns word for word, and every file link gathered so far; never the full transcript. | The incoming model's prompt holds exactly those three parts. | Core test of the packet builder. |
| FD-7 | WHEN the outgoing model writes the rolling summary, THE CORE SHALL check it names every file link and open question the core has logged for the thread, and ask once more if it doesn't. | A summary from a model that just failed can't silently drop facts. | Core test with a summary missing one file. |
| FD-8 | WHEN an ask is big and needs tools, names Harmon or the Therapist, or its request kind's pick is the Claude subscription (FD-3), THE APP SHALL show an "Open in Claude" button that starts a desktop session seeded with the packet; I press it; the app never starts a session itself. | The packet arrives in the new Claude session. | Face test; a by-hand check of the button. |
| FD-9 | WHEN an answer arrives, THE APP SHALL offer thumbs up or down, logged as that turn's `outcome`; and THE CORE SHALL log implicit signals (pressing "Open in Claude", re-asking, abandoning a chat, copying an answer) as separate rows marked implicit, which FD-4 does not count in 1.0. | Ratings and implicit signals are in the job log, apart. | Core test of the rows. |
| FD-10 | WHEN a thumbs down also moves the chat, THE CORE SHALL log the rating and the move as separate fields. | Later I can see whether thumbs-downs are rating or moving. | Core test. |
| FD-11 | WHEN the core sends a chat turn, THE CORE SHALL call the chosen service through one adapter per service (local Qwen through llama-swap, Gemini, DeepSeek, Claude API, ChatGPT), each logging service, model, tokens and cost. There is no adapter for the Claude subscription: it is reached only through FD-8. | Every pool's chat works through the same interface. | Adapter tests against recorded answers; one live by-hand call per service. |
| FD-12 | WHEN a request kind's `needs` don't match this machine's loadout, THE APP SHALL offer to send it as a normal fleet job another machine pulls. | I can still reach a tool the Z13 lacks. | Core test with a fixture loadout. |
| FD-13 | WHEN a chat changes, THE CORE SHALL store it as a thread with its handoff id in local files, in the format fleet-state will carry later. | Threads survive a restart. | Core round-trip test. |

### `autopilot`: Project Manager, Budget Manager and satellite as code

The management work leaves Claude. The Budget Manager is code; the Project Manager and the satellite run as routed jobs. On 1.0 the Z13 is its own satellite.

| # | Must | Done looks like | Check |
|---|---|---|---|
| AUTO-1 | WHEN anything is about to spend a pool (a chat turn, a job, a review), THE CORE SHALL ask `fleet budget allow` first and obey it. | Nothing starts on a braked pool. | Core test with a stub that answers 75. |
| AUTO-2 | WHEN the snapshot shows a ROADMAP change, THE CORE SHALL start a routed Project Manager job that files or adjusts that project's issues through the create queue, on the local model, one job at a time, falling back to the smallest model with budget and waiting when no pool has budget. | New features get issues without me. | Core test: a changed ROADMAP fixture starts one job. |
| AUTO-3 | WHEN this machine's satellite opens a PR, THE CORE SHALL start its one R25 review at once, with no wait for CI, as R25 is written: a reviewer at the builder's size on another service, given the diff and the issue as its brief; the reviewer fixes any gap inside the issue's scope itself on the PR branch (labelled `fixed-in-review`, `FIX ATTEMPT <n>`, the builder's model recorded failed with `fleet ledger --review-failed`); a fix of about 30 changed lines or fewer, or one to docs, the code map, comments or tests only, posts `VERDICT: PASS`; a larger fix is checked by the next size up, the same fixer gets up to 3 tries, then the next size up takes over with its own 3; Fable only after Opus has failed its 3. A FAIL without a fix (a design question or a decision of mine) and a ladder that reaches Fable go to Waiting on you. A PR another machine's satellite built is reviewed by that satellite; the core reviews it only by the Project Manager's rule, when it has no PASS on its head and no review of it is in flight. | No PR goes back to its builder; no PR is reviewed twice; a PR with a PASS on its head is never reviewed again. | Core test of the review state machine over R25's ladder: a small fix, a larger fix passed on try 2, three failed tries moving up a size, Opus failing 3 into Fable, a FAIL for a decision of mine, and another machine's PR with a PASS (not reviewed). |
| AUTO-4 | WHEN a PR's latest `VERDICT: PASS` is on its head or carried over by patch-id, and CI is green on that head, THE CORE SHALL merge it (GH-5) and close its issue. | Passed PRs don't sit open; a reviewer's fix is merged only after CI has run on it. | Core test with a PASS on a head whose CI is pending, then green. |
| AUTO-5 | WHEN Sunday's 6am reset has passed, THE CORE SHALL start the weekly drift clear and the weekly backup, the drift clear as one Project Manager job per project on the local model, one at a time, falling back to the smallest model with budget and waiting when no pool has budget. | Drift is cleared every week. | Core test with an injected clock. |
| AUTO-6 | WHILE the satellite is on, THE CORE SHALL drive jobs with script loops, one `fleet run --next` per service lane, at most Qwen plus one service job at once (R10), with a model brought in only to read a failure card. | No model sits in a wait loop. | Core test of the loop driver with a stub `fleet run`. |
| AUTO-7 | WHEN a job's worktree is made, THE CORE SHALL give it the instruction file its CLI reads, written from the one instruction source in this repo at the sha the job was claimed at, as an untracked file listed in `.git/info/exclude`, never overwriting a tracked file. | `git status` in the worktree stays clean; `files_allowed` is not tripped. | Core test in a temp repo that tracks its own CLAUDE.md. |
| AUTO-8 | WHEN a hand-kept copy of the instruction source differs from it, THE CORE SHALL flag it as drift. | The drift list names the file. | Core test. |
| AUTO-9 | WHEN a job ends, THE CORE SHALL classify a failure by the done card's `outcome` and `failure_mode`, never by the exit code. | A rate limit or a lock timeout is a fleet error, not a failed build. | Core test over fixture cards. |

### `it`: loadout, fleet errors and IT tickets (this machine)

| # | Must | Done looks like | Check |
|---|---|---|---|
| IT-1 | WHILE the app is open, THE CORE SHALL read this machine's loadout every minute in code from a fixed Linux probe list: CLIs and whether they're signed in, models loaded, toolchains, busy or idle, recent errors. | A loadout panel; the router reads it. | Core test with stub probes. |
| IT-2 | WHEN a fleet error happens on this machine, THE CORE SHALL open an IT ticket in the pipeline repo labelled `it`, through the create queue, one per error signature; a repeat adds to that ticket, never a second one. | One ticket per signature. | Core test: the same error twice makes one ticket. |
| IT-3 | WHEN an IT ticket opens, THE CORE SHALL offer an `it-triage` fleet job that runs only on another machine, which pulls it from fleet-state as today; the job carries the machine exclusion the pipeline batch adds (`never_on: [<this machine>]`, honoured by `fleet run --next` at all hours, unlike `not_on`, which only keeps a job off a machine during its quiet hours); a fix that needs a setting, a delete or a credential goes to Waiting on you. Until that exclusion lands (U9), no `it-triage` job is offered; the ticket waits. | The Z13 never triages its own failure. | Core test: the job carries `never_on` naming this machine; with a stub `fleet run --next` on this machine outside quiet hours, the job is not taken. |
| IT-4 | WHEN a coding error has run out of ladder, THE CORE SHALL file a bug in that project's repo, never an IT ticket, with its star's `feature:` label (SKY-10). | Coding errors land in the project, on the feature they came from. | Core test. |

### `cli`: the `switchboard` CLI

| # | Must | Done looks like | Check |
|---|---|---|---|
| CLI-1 | WHEN I run `switchboard sky`, `waiting`, `reports`, `budget`, `loadout` or `star <repo>#<n>`, THE CLI SHALL print what the GUI shows, as text or with `--json`. | The same numbers as the GUI. | CLI tests over the core fixtures. |
| CLI-2 | WHEN I run `switchboard refresh` or `switchboard backup <repo>` or `switchboard backup --all`, THE CLI SHALL do what the GUI button does, under the same rules. | Same result as the button. | CLI tests. |
| CLI-3 | WHEN the CLI is asked to archive a repo (`switchboard delete <repo>`; nothing is deleted) or restore over a live one, THE CLI SHALL refuse and add a Waiting on you item (CORE-3); `switchboard drop <delete \| restore> <repo>` turns that request down. WHEN it is run with a role flag (`switchboard --role <role> merge <repo>#<pr>`), THE CLI SHALL give that role's write actions under the same CORE-3 rules as MCP-2; without a flag its only writes are CLI-2's; inside a job worktree it gives no role's write actions whatever the flag, as MCP-3. | The refusal names the item; a role-flagged `merge` without a PASS on head, or with CI not green, is refused with the same text as MCP. | Rules test, the same cases as MCP-2 and MCP-3. |

The raw `fleet` CLI stays the documented escape hatch: Switchboard keeps no state that only it can read.

### `mcp`: `switchboard mcp`

| # | Must | Done looks like | Check |
|---|---|---|---|
| MCP-1 | WHEN a client starts `switchboard mcp` over stdio without a role flag, THE CORE SHALL register six read tools only: `sky`, `constellation`, `star`, `waiting`, `reports`, `budget`. | `tools/list` returns six tools, their schemas under about 2,000 tokens together. | JSON-RPC test over stdio. |
| MCP-2 | WHEN a client starts it with a role flag, THE CORE SHALL add that role's write tools, each going through CORE-3. The project-manager role's `request_archive` and `request_restore` only file CORE-3's Waiting on you request and its `drop_request` only turns one down; no MCP tool can confirm one. | A `merge` call without a PASS on head is refused. | JSON-RPC test. |
| MCP-3 | WHEN `switchboard mcp` starts inside a job worktree, THE CORE SHALL register no write tool whatever the flag: only the six read tools. | An agent can't merge its own PR or raise a pool cap, and write tools' schemas cost the job nothing. | JSON-RPC test from a temp worktree with a role flag: `tools/list` returns the six read tools; a `merge` call answers "unknown tool". |

## Regions after 1.0 (dark stars)

These features go in the ROADMAP `features:` block marked `release: after-1.0` (the regions above are marked `release: 1.0`) and get no issues yet, so the sky draws them as dark stars. Being After 1.0, they don't count against the 1.0 brightness (SKY-2) and are not drift (SKY-5).

| Id | Feature | Where it was ruled |
|---|---|---|
| `relay` | Each machine runs its own Switchboard and they share through fleet-state. A headless `switchboard relay`, started at logon, wakes on a wake ping, then pulls, and runs fleet jobs only ([ADR 0001](docs/adr/0001-relay-wake-then-pull.md)). | grill, Machines |
| `loadouts` | Every machine publishes its loadout every minute; one silent for 3 minutes drops out of routing; a kind's `needs` must match the loadout. | grill, Machines |
| `webhooks` | One GitHub App, webhooks through Tailscale Funnel on the RTX 3090 Ti desktop on one secret-checked path, a backfill poll at start and nightly, and a GitHub snapshot shared through fleet-state. | grill, GitHub data |
| `chat-sync` | Chats synced through fleet-state, in the format 1.0 already writes (FD-13). | proposal §7 |
| `local-only` | Local only: Readiness, the prep loop and the work loop, plus the six review ideas ruled inside its own Spec (see [dispositions](#review-dispositions)). | Local only rulings |
| `platforms` | Windows and Mac builds. | proposal §7 |
| `fleet-rust` | Porting `fleet` to Rust. | proposal §7 |
| `plugins` | Plugins that add views, commands and MCP tools without changing the core. | my ruling, 2026-11-09 |
| `dashboard` | Retiring the fleet dashboard, on the Z13 after parity, then everywhere. It needs my yes again before anyone starts (the 2026-11-08 run rule is never to retire it). | grill, Retire / defer |

Moving CC into a repo is its own project, not a Switchboard feature.

## Regions after 2.0 (dark stars)

These features go in the ROADMAP `features:` block marked `release: after-2.0`. They are dark stars like the ones above, but they wait for 2.0: they don't count against the 1.0 brightness (SKY-2) and are not drift (SKY-5).

| Id | Feature | Where it was ruled |
|---|---|---|
| `gamification` | A gamification plugin, built on `plugins`. | my ruling, 2026-11-09 |

## Routes

### Build order

Regions are built in this order; each arrow is a `Blocked by` the Roadmap writes between their first issues.

1. **`core` and `shell`** first: settings, the fleet bridge, the job log reader, the rules (CORE-3), and an empty window.
2. **`github`** next: the snapshot, the create queue and the merge path. Every view after it reads the snapshot.
3. **The read views**, in parallel once 2 lands: **`budget`**, **`sky`**, **`reports`**, **`waiting`**, with **`cli`** and **`mcp`** (read tools) beside them, since they share the same core calls.
4. **`backup`**, after `github`.
5. **`frontdoor`**, after `budget` (the router needs `fleet budget allow`) and `it` IT-1 (the loadout).
6. **`autopilot`** and the rest of **`it`**, after `github` and `frontdoor` (routed jobs use the router).
7. **The design pass** (R04) on the running build: the last stage before 1.0. Its findings are `design-pass` issues, each with the `feature:` label of the region it changes (SKY-10).

The pipeline batch lands in the pipeline repo before any of this, through Harmon's review. Ruled: ledger fields for rating, tokens, effort and handoff id (tokens and effort are already ledger fields, so the new ones are `rating` and `handoff_id`); request kinds with `needs`; failure classes from the job card; the one throttled create queue. This Spec also asks it for: the ledger fields `checks` (each check's name and result), `escalation_step` (the size on the ladder the attempt ran at) and `review_fix_loc` (the lines a reviewer's fix changed, U11), which CORE-4 reads; a job field `never_on` that keeps a job off the named machines at all hours, which IT-3 needs; and an `it-triage` request kind. Those asks are U9. Nothing here edits the pipeline repo.

### How data travels

- **GitHub → snapshot → views.** The GraphQL poll, refresh-on-action and the Refresh button fill the snapshot; the Sky, Waiting on you, Reports and the MCP read tools read only the snapshot.
- **Fleet → core → views.** The core runs `fleet.py … --json` for budget, ledger, pick and job state, and reads fleet-state for jobs in flight.
- **Chat → router → adapter → thread log.** Each turn writes a row in the same shape as a fleet job row.
- **Core → GitHub.** Creates go through the create queue; merges through `merge_pr.py`. No face writes to GitHub directly.
- **Pipes stay as they are.** Code jobs go through the fleet queue, my to-dos through CalDAV tasks, agent-to-agent through NC Talk, chats through the thread log. Switchboard reads them all and joins them by handoff id.

### Platform plan (R04)

1.0 ships one AppImage for Linux (SteamOS on the Z13). The code keeps the Mac and Windows seams in one place each: the Python launcher (`python3` or `py`), the dev root (`~/dev` or the Windows code drive), the NextCloud folder, the OS keychain for tokens, and the loadout probe list per OS. The `platforms` region fills them in.

## Waypoints

| # | Waypoint | I can check it by |
|---|---|---|
| WP1 | Shell up | The AppImage opens on the Z13 and `switchboard budget` prints the same as `fleet.py budget`. |
| WP2 | Read-only Switchboard | The sky, Waiting on you, Reports and the budget panel show live data; the CLI and the six MCP read tools answer. I could use it every day from here. |
| WP3 | Backups | "Back up all" fills `Backups/github/`; a shrinking fixture is refused. |
| WP4 | Front door | A quick lookup is answered by Qwen, a writing ask by Gemini, and a big ask with tools gives me "Open in Claude", with no Claude tokens spent on routing. |
| WP5 | Autopilot | A PR opened in a project is reviewed and merged with nobody asked; the Sunday drift clear runs. |
| WP6 | Errors | A forced fleet error opens one IT ticket, a repeat opens none, and the `it-triage` job waits for another machine. |
| WP7 | 1.0 | The design pass's issues are built, every branch is merged, and the app launches. |

## Unknowns

The fog. Each has a type (how it gets answered) and a default the build uses until then.

| # | Question | Type | Default until answered |
|---|---|---|---|
| U1 | Does the AppImage run in Game Mode? Only Desktop Mode was proven. | prototype | 1.0 is Desktop Mode only. |
| U2 | BroCrew's desktop app is Electron with Vue 3 components, but the face is ruled React. Which of BroCrew's framework-free parts (its message paths, message links, receipts, composer notices) lift unchanged, and which components are rewritten in React? | research | Lift the plain TypeScript as is; port the Vue components (prompt composer, user and assistant messages, Waiting on you, work report entry) to React, keeping the Apache-2.0 notice on each. |
| U3 | Is there a headless reader for Claude's usage? Today it comes only from `get_usage` inside a Claude session. | research | BUD-4: the newest recorded reading plus an estimate from local session logs, both labelled. |
| U4 | Where does the sky's priority order come from? The priority board lives in a claude.ai Artifact database, reachable only through `ArtifactData` in a Claude session. | research, then grilling | A copy of the order in fleet-state, refreshed by any Claude session at the Sunday drift clear and whenever I re-rank; SKY-9 shows its age. After 1.0 I decide whether Switchboard owns the order. |
| U5 | Pool edits (BUD-2) land in the pipeline repo's tracked `config.json`, and only Harmon changes the pipeline repo. Who commits them? | grilling | The edit is saved in the local clone at once (so pace and routing update) and a Waiting on you item asks for it to be landed through Harmon's review. |
| U6 | Which untracked instruction file does each CLI read (Claude Code, Codex, Gemini, opencode), so that AUTO-7 never touches a tracked file? | research | A CLI with no untracked option gets its instructions in its prompt instead. |
| U7 | Where is NextCloud on the Z13 (a synced folder or WebDAV), and how does the app reach CalDAV? A stored credential needs my yes first (R34). | task | BAK-1 and WAIT-2 wait for the path and my yes; the rest of 1.0 does not. |
| U8 | Which small labeller model fits in a never-swapped llama-swap group beside the 27B within the Z13's memory? | prototype | Rules only, with unclear asks going to the budget and complexity rule. |
| U9 | Does the pipeline batch take this Spec's asks beyond the ruled four: the ledger fields `checks`, `escalation_step` and `review_fix_loc`, the `never_on` job field, and an `it-triage` request kind? | task (Harmon's, filed as an issue in the pipeline repo) | CORE-4 shows the missing fields as "not recorded"; IT-3 offers no `it-triage` job until `never_on` and the kind land; IT-2 does not wait. |
| U10 | Should the router give free Qwen a small exploration share, so untried models get rows? It conflicts with "below N rows, the index's guess stands". | grilling | No exploration in 1.0. |
| U11 | Should a pass with a reviewer's fix (R25 lets a reviewer fix up to about 30 lines) count as a pass in FD-4? | grilling | It counts, as ruled ("checks + R25 review count as the rating"); the job log records the fix's size (`review_fix_loc`, U9) so the share can be shown. |
| U12 | RUBRIC says Qwen's context is 32k on both machines; the Z13's config says 131072. | task (Harmon's: RUBRIC is the pipeline repo's, which the Project Manager never works on; filed there as an issue) | Switchboard reads the context size from the running config, never from RUBRIC. |
| U14 | Should a review wait for green checks, and should the ladder stop after 2 rounds and go to Waiting on you (the review's 3.1)? Both change R25, which only my words in the pipeline repo can do. | grilling, then a RULES change in my words | R25 as written (AUTO-3): the review starts when the PR opens, 3 tries per size, then the next size up, Fable after Opus. |
| U13 | Where are the vault's agent scripts on the Z13, for trigger words and their `model:` / `fallback:` lines? Adding those lines is a vault change through Harmon. | task | Trigger words for Harmon and the Therapist are built in (FD-8); every other agent routes by the message. |

## Won'ts

- No relays, wake pings or loadouts from other machines (After 1.0, ADR 0001).
- No webhooks, GitHub App or Funnel.
- No Local only, Readiness, prep loop or work loop.
- No Windows or Mac builds, and no Rust port of `fleet`.
- No chat sync between machines.
- It does not retire the fleet dashboard; it runs beside it.
- It never starts a Claude session itself, and never runs headless `claude -p`.
- The router never calls Claude to decide where something goes.
- No face ever runs a raw shell command; work runs only as fleet jobs through `fleet run`.
- No agent scans of GitHub for Switchboard's data.
- No MCP write tools without a role flag, and none inside a job worktree; never `gh pr merge` or the merge API.
- No older repo backups: the latest only.
- It sits beside Claude sessions; it does not replace them.
- It does not edit the pipeline repo, apart from the pool edits in U5, IT tickets, and the issues for Harmon in U9 and U12.

## Review dispositions

Every finding and feature idea in the 2026-11-07 adversarial review (pipeline repo, private), with where it lands. "Ruled" means a ruling already settled it; this Spec only places it.

### Top 5 issues

| # | Finding | Disposition |
|---|---|---|
| 1.1 | Tauri can't run on the Z13 | **Ruled, closed:** proven on 2026-11-07 at 3:55pm (`shell`). Game Mode is U1. |
| 1.2 | The create limit turns into a bug storm | **In 1.0, ruled:** one throttled create queue (GH-4), failures classed by the job card (AUTO-9). The Roadmap files its issues in staggered batches through the queue. |
| 1.3 | The fan-out can't run in parallel on one Z13 | **In 1.0, ruled:** at most Qwen plus one service job (AUTO-6); script loops, no model drivers (AUTO-6); the labeller off the 27B (FD-2); chat rows go to local files (FD-13), so only fleet jobs take the fleet-state lock, and a lock timeout is a fleet error by its card (AUTO-9). |
| 1.4 | Pipeline repo changes and the ledger schema | **Ruled, closed:** the Harmon-reviewed pipeline batch lands first ([Routes](#build-order)); Switchboard reads its fields (CORE-4). |
| 1.5 | "MCP exposes every action" bypasses the guards | **In 1.0, ruled:** six read tools by default, write tools only with a role flag, never in a job worktree, the merge rule in the core (MCP-1 to MCP-3, CORE-3). |

### Things we hadn't thought of

| # | Finding | Disposition |
|---|---|---|
| 2.1 | The sky's priority order isn't readable from the app | **In 1.0:** a copy of the order with its age shown (SKY-9); the source question is U4. |
| 2.2 | Claude usage can't be read headless | **In 1.0:** BUD-4 labels the reading's age and adds a marked estimate; a headless reader is U3. **The `claude` lane is `runnable: false`, so the reset quota is spent only by my press: in 1.0, as ruled:** the core has no Claude-subscription adapter (FD-11); when a kind's pick is the Claude subscription, the chat becomes a Claude handoff (FD-8), and braked, it takes the kind's fallback or waits for budget (FD-3); no headless `claude -p` (Won'ts). |
| 2.3 | IT triage "on another machine" on a one-machine 1.0 | **In 1.0:** the `it-triage` job is a normal fleet job another machine pulls, as today (IT-3); no relay needed. |
| 2.4 | The rolling summary is written by the model that just failed | **In 1.0:** the outgoing model still writes it (ruled); the core checks it against the thread's logged files and open questions and asks once more (FD-7). The packet keeps its ruled shape. |
| 2.5 | Thumbs-down both rates and moves a chat | **In 1.0, measured:** both stay as ruled; the rating and the move are logged apart (FD-10) so drift in ratings shows. |
| 2.6 | The ranking won't learn the way the design assumes | **In 1.0:** the core applies the ruled rule itself (FD-4) rather than `capability.py`'s proposals, and keys chat rows on request kind, service and model only. Exploration is U10 (no exploration in 1.0). |
| 2.7 | R25 lets a reviewer pass its own fix | **In 1.0, measured:** counts as ruled; the fix's size is logged (U11). |
| 2.8 | Latest-only backup can be overwritten by a bad pass | **Keeping 4 weeks: declined**, because the ruling is latest only. **The overwrite guard: in 1.0** (BAK-2), which keeps the one copy good. |
| 2.9 | Instruction files trip `files_allowed` and collide with tracked files | **In 1.0:** untracked, listed in `.git/info/exclude`, never over a tracked file (AUTO-7); per-CLI file names are U6. |
| 2.10a | The PM job edits the instruction source the next job reads | **In 1.0:** each job gets the source at the sha it was claimed at (AUTO-7); the source changes only through a reviewed PR. |
| 2.10b | Dark stars need a feature list the code can read | **In 1.0:** ROADMAP.md carries a `features:` block, which this Spec's region ids fill, each marked `release: 1.0`, `release: after-1.0` or `release: after-2.0`; SKY-2, SKY-3 and SKY-5 read it, and design-pass issues and automatic bugs get a `feature:` label (SKY-10). |
| 2.10c | RUBRIC's 32k against the config's 131072 | **In 1.0:** read the running config (U12); the RUBRIC fix is Harmon's, filed as an issue in the pipeline repo (the Project Manager may only file there). |
| 2.10d | Keep the raw `fleet` CLI as the escape hatch | **In 1.0:** documented (`cli`); Switchboard keeps no state only it can read. |

### Places we'll lose tokens

| # | Finding | Disposition |
|---|---|---|
| 3.1 | The R25 review ladder | **In 1.0, R25 as written** (AUTO-3), with the diff and the issue as the reviewer's brief, which R25 allows. **Green checks before any review and the 2-round cap: an Unknown (U14)**, because both change R25, a pipeline rule change that needs my words; until then the ladder stands. |
| 3.2 | Opus planners' context | **In 1.0:** the Project Manager job writes one brief per issue and builders get only that (AUTO-2). The brief compiler that enforces a size is ruled into Local only (4.1). |
| 3.3 | Haiku drivers | **Ruled, in 1.0:** script loops (AUTO-6). |
| 3.4 | Misfiled failures climbing the ladder | **Ruled, in 1.0:** classed by the job card (AUTO-9). |
| 3.5 | MCP schemas and instruction files | **Ruled, in 1.0:** six read tools under about 2,000 tokens, none in jobs (MCP-1, MCP-3). |

### Ten feature ideas

| # | Idea | Disposition |
|---|---|---|
| 4.1 | Brief compiler | **After 1.0:** ruled into the Local only feature's own Spec (`local-only`). |
| 4.2 | Labeller off the 27B | **In 1.0** (FD-2); the model is U8. |
| 4.3 | Qwen-fit colouring | **After 1.0, folded into Readiness:** stars show their Readiness on the sky (`local-only`), which supersedes a colour taken from the old fit count. |
| 4.4 | Overnight Qwen shift | **After 1.0:** ruled into Local only. |
| 4.5 | Prefix-ordered Qwen queue | **After 1.0:** ruled into Local only. |
| 4.6 | Replay bench | **After 1.0:** ruled into Local only. |
| 4.7 | Ghost split stars | **After 1.0:** ruled into Local only. |
| 4.8 | Symbol fence | **After 1.0:** ruled into Local only. |
| 4.9 | Implicit ratings | **In 1.0, logged only** (FD-9): the router doesn't count them until I rule on their weight after 1.0. Thumbs stay. |
| 4.10 | Critical-path ring | **In 1.0, the chain only** (SKY-7), drawn as highlighted blocked-by lines because a ring already means a milestone. **Pricing it in Qwen-hours and paid points: After 1.0**, with Readiness. |

## Implementation decisions

- **One Rust workspace:** a core library; the Tauri app, whose commands are thin calls into the core; and one `switchboard` binary for the CLI and `switchboard mcp`. The AppImage bundles both.
- **The core owns every rule.** One guard every write passes through takes the face and the role; faces hold no rules of their own.
- **The fleet bridge** runs `python3 -I <pipeline repo>/tools/fleet/fleet.py <cmd> --json` and parses it. It uses only commands with `--json`; one that lacks it gets `--json` added in a pipeline repo change, never text parsing.
- **GitHub:** GraphQL for reads, a REST conditional request with the stored ETag to skip an unchanged repo (GraphQL has no ETags), writes through the pipeline batch's create queue, merges through `merge_pr.py`. One queue for fleet jobs and Switchboard, because the create limit is per account.
- **Stores:** the snapshot as one JSON file per repo in the app's data folder; threads as one JSON Lines file per thread, each row in the job-log row shape; settings in one file.
- **The face:** React, with views Sky (SVG), Waiting on you, Reports, Budget and Chat. The sky layout (rank to radius and size, brightness, the blocked-by chain) is a pure function the tests drive.
- **Adapters:** one per service behind one chat interface; each returns text, tokens, cost and a handoff marker if present.

## Testing decisions

- A good test drives the core through its public functions with fixtures, not the GUI: recorded GraphQL and REST answers, a stub `fleet.py` reached through `FLEET_ROOT` that prints fixture JSON, an injected clock, and temp git repos for backups and worktrees.
- Every guarded action has one test per face (GUI command, CLI, MCP) proving the same refusal.
- MCP is tested as JSON-RPC over stdio.
- The router is tested as a table: one row per routing step and per budget state.
- The face's views are tested on fixture snapshots; the sky layout is tested as a pure function.
- Rust and Tauri build and test only inside `craft-build`. The AppImage launch is checked on the host by hand or by a scripted screenshot.
- Tests to copy: the pipeline repo's fleet tests for budget and pace fixtures; BroCrew's look scenes for Waiting on you and the work report.

## Further notes

- Decisions this Spec makes that should become ADRs when built: GraphQL plus an ETag check for the poll (GH-2); `it-triage` as a normal fleet job in 1.0 (IT-3); the rolling summary check (FD-7); the backup overwrite guard (BAK-2).
- Agents in Claude, Codex, Gemini and opencode sessions keep their own GitHub reads in 1.0; the MCP read tools are there for them to move to.

## Prior art

**Status: awaiting my approval (R41).** Proposed list of projects doing the same thing, with their licenses. Their code comes in only when the license allows it, with credit; otherwise only their ideas.

peer: rengwu/wayfinder-maps | MIT | the roadmap drawn as a navigable sky; the look behind Constellation
peer: brocrew/brocrew | Apache-2.0 | the work tree, "Waiting on you" and the fixed report; the chat parts lifted (Electron + Vue, ported to React)
peer: mvschwarz/openrig | Apache-2.0 | one file that maps the team of agents and their roles across Claude Code and Codex
peer: ocuclaw/obsidian-wayfinder | MIT | wayfinder maps from GitHub issues drawn as dependency layers, with the takeable frontier highlighted
peer: musistudio/claude-code-router | MIT | routing a coding agent's requests across models and services by rule
peer: lm-sys/RouteLLM | Apache-2.0 | learned routers that trade cost against quality; inactive since 2024, ideas only
peer: BerriAI/litellm | MIT (outside enterprise/) | one gateway to 100+ model APIs with cost tracking, budgets and fallbacks
peer: BloopAI/vibe-kanban | Apache-2.0 | a board of coding-agent work across agents, each in its own worktree
peer: ccusage/ccusage | MIT | reading Claude Code's local usage logs, the estimate behind BUD-4
peer: josegonzalez/python-github-backup | MIT | backing up a repo's issues, PRs, comments, labels, milestones, releases and wiki
peer: winfunc/opcode | AGPL-3.0 | a desktop GUI for Claude Code sessions and agents; AGPL, so ideas only
peer: smtg-ai/claude-squad | AGPL-3.0 | managing several terminal agents (Claude Code, Codex, opencode) at once; AGPL, so ideas only
