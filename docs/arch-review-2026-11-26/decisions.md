# Settled decisions for cards 1-4 (owner rulings, 2026-11-26)

Repo: switchboard (GitHub), main at 19e0c65. Audit evidence: `audit.json` beside this file.
These are settled. An issue must not reopen them. Where an issue needs to say who decided, write "owner ruling (2026-11-26)".

## General
- Q1 Everything lands as GitHub issues built through the pipeline.
- Q2 All of it lands before 1.0 (the three follow-ups excepted, see below).
- Q9 Order: (a) swap the map-pinning tests for structural ones; (b) revise the code map, with SPEC/ROADMAP revised in parallel; (c) state-file module; (d) guarded writes, then read models.
- Q28 Every issue carries the label `feature:core`.
- Issue size limit for one issue: aim under 300 changed lines and 6 files. Where that cannot hold (test swap, map sections) the issue says so plainly and says it is reviewed as an oversize change.

## Card 1: code map (`docs/code-map.md`)
- Q3 Owner ruling: every test that pins code map wording, locked tests included, is replaced by structural tests. One structural test per tree (core, CLI, Tauri, React face).
- Q20 The swap is three issues, one per tree: `crates/core/tests` (33 files: 25 locked + 8 without a locked header: auto_after_poll_review, auto_driver, auto_merge_pass, auto_roadmap_pass, auto_weekly_run, fd_pick_for_kind, bug_fix, unit_job), `app/src-tauri/src` (it_tickets_tests.rs, loadout_tests.rs), `app/src/views` (9 `.test.tsx`: Budget, Loadout, PoolEditor, Reports, ReportsFilter, Settings, SkyRefresh, Waiting, WorkTree). Locked core files: abilities_index, backup_run, budget_claude, budget_edit, budget_refresh, budget_rows, fd_chat_route, fd_implicit_log, fd_rating_log, fd_send_held_job, fleet_error_log, it_loadout_command, job_log, platform_seams, reports_build, reports_filter, reports_load, reports_moves, rules_guard, sky_codemap, sky_model, unit_proof, waiting_load, waiting_local, waiting_snapshot. Only the assertions that read the real `docs/code-map.md` text go; every other test in those files stays untouched.
- Assumed default (confirmed): the structural tests land first checking only what holds today (each listed path exists; each status word is valid). The "every source file is listed" check and the size guard turn on in the last map issue.
- Q4 Behaviour detail cut from the map moves to each source file's header doc comment (`//!` in Rust, a top comment in TS). Fix history is dropped (CHANGELOG has it).
- Q16 Every non-test source file in all four trees (crates/core/src, crates/cli/src, app/src-tauri/src, app/src) gets its own map line.
- Q19 Format of a file's entry: one line that starts with the backticked path: `` `path` · status · one sentence ``. Machine-read parts that must keep working unchanged: the `modules:` block (`- <node>: <path> (<status>)`, runner nodes only, read by `crates/core/src/sky/codemap.rs` `modules`/`check_modules`), the `reuses:` block (`- <repo>: <what>`), the first `Updated:` line, and "first line starting with the backticked path" read by `crates/core/src/unit/job.rs` `code_map_lines`. `specs/roadmap-run.json:21` describes the modules line wrongly and is corrected.
- Q10 Guard in the structural test: an entry line over 400 bytes fails; an entry line containing `#<number>` fails.
- Q29 The revision is one issue per map section group (about a dozen), each rewriting that section's lines and those files' header comments in the same PR, then one last small issue that turns on the guard and the every-file check. Line 7 of the map ("remain planned") contradicts lines 299-303 and is fixed in the runner section's issue.

## Card 2: SPEC, ROADMAP, CHANGELOG, GLOSSARY
- Q30 Four issues: (1) SPEC Musts shortened, PR body shows each changed Must before/after; (2) "Review dispositions" (SPEC.md:317-372) deleted and folded, prior art told once (PEERS.md is its home), platform plan told once; (3) ROADMAP "Current state" cut to one sentence with no issue ledger (ROADMAP.md:52-62), `features:`/`nodes:` untouched, `Updated:` re-dated, plus the missing CHANGELOG lines since 2026-11-18; (4) GLOSSARY gains a Runner section.
- Q13 The SPEC rewrite is strictly meaning-preserving. Q17 Every Must keeps its id; no split, no renumber. Cells over 800 chars: CORE-3 (line 77), BAK-3 (147), FD-3 (157), FD-5 (159), AUTO-3 (177). Mechanism detail moves to file doc comments.
- Q5 CHANGELOG is the only home for dated rulings and issue numbers; Musts state the current rule with no issue numbers.
- Q18 Dispositions: 24 of 33 restate a Must (delete), 2 are history (1.1, 1.4 -> CHANGELOG), 7 are after-1.0 ideas found nowhere else (4.1, 4.3-4.8 -> a short list under the `local-only` row, SPEC.md:222), 4 stray clauses fold into their Musts (1.2 staggered batches, 3.2 brief compiler to Local only, 4.9 "until I rule on their weight after 1.0", 4.10 pricing in Qwen-hours after 1.0).
- Q11 GLOSSARY Runner section defines Node, Unit, Spec, Run. "node" stays under Avoid only as a word for a Star.
- Q28 FD-4 learned pick (`crates/core/src/frontdoor/router.rs` `learned_pick`) is marked present but nothing calls it: SPEC and map mark it planned; one follow-up issue wires it.

## Card 3: state-file module
- Q12 New module `crates/core/src/state_file.rs` beside `lock.rs`. GLOSSARY gains "State file": a JSON file Switchboard owns and rewrites whole under a lock.
- Q21 Interface: exactly three operations: `load` (missing file gives the default), `save` (whole write through a temp file), `change` (lock, read, change, write only if it changed). Each store keeps its own type.
- Q22 The module always writes a trailing newline.
- Q23 Each store's locking stays exactly as it is today. Locked today: LocalItems (waiting.rs), RunnerItems (runner/waiting.rs), HeldRecords (confirm.rs), Marks (backup.rs). Not locked: Settings, StateStore (runner/state.rs), SnapshotStore (github/snapshot.rs), AutopilotState (autopilot/passes.rs), backup manifest. A follow-up issue asks whether the unlocked stores need a lock.
- Q7 Rollout: first issue = the module + LocalItems + RunnerItems; then one small issue per remaining store: HeldRecords, Marks, backup manifest, Settings, StateStore, SnapshotStore, AutopilotState (7 more, 8 in all).
- Q24 Manifest uses `load`/`save`; the plain `std::fs::write` at backup.rs:885 is reported in that issue as a possible bug, not silently changed. Marks keep delete-when-empty in backup.rs. The bug ledger (JSONL) is out.
- Q15 Follow-up issue: the line-file logs (job log, rating log, implicit log, thread store, bug ledger).
- Open issue #503 (write through craft-atomic, needs an owner answer) touches `lock::write_whole`; the state-file module calls `lock::write_whole` and `FileLock`, so #503 stays independent. Mention it, do not block on it.
- `SPEC.md:81` ("State files two processes write") is revised only by the SPEC issue, meaning-preserving.

## Card 4: guarded writes and read models into core
- Q8 Writes first, then read models. Q14 The new core modules return `CoreError` with its Error code; faces only render.
- Q31 Guarded writes = three issues: (1) the core module with repo resolution; (2) the CLI moved onto it; (3) MCP moved onto it (the drift fix lands here; `trait Writes` in cli.rs:89-128 and `trait WriteCore` in mcp.rs:40-86 are deleted or shrunk to what is truly face-only). Read models = seven issues, one per view, in this order: waiting, sky, constellation, star, reports, budget, errors.
- Q25 Where CLI and MCP drifted, the CLI rule wins: only the asked repo's snapshot must be readable (a corrupt snapshot of another repo does not refuse); matches sorted and deduped; a merge sent to Waiting on you is built with `guarded_item` and added with `add_new` (links to the PR; asking twice keeps the first item). The MCP test `crates/cli/tests/mcp_write_core.rs:388` that pins the hand-built item changes under the owner ruling.
- Q26 A face keeps only its wording and its exit or reply shape: the CLI's exit 1 after filing a request, the CLI/MCP NO_SNAPSHOT refusal, the `delete` alias, "from the CLI"/"from MCP" text, the `drop:`/`drop_request:` prefixes. Tests pinning face wording stay green. MCP inferring NotFound by matching text (mcp.rs:276) goes away.
- Q27 One read model per view in core returns the answer plus warnings; all three faces (Tauri commands, CLI, MCP) use it. The CLI prints warnings to stderr; MCP and the window carry them in the answer.
- Q32 The window shows the warnings as one quiet line at the top of the affected view, naming the file, as Reports already shows its warnings; no dialog.

## Follow-ups (three issues, labelled `after-1.0` unless stated)
1. Line-file logs: do job log, rating log, implicit log, thread store and bug ledger want one module (Q15).
2. Lock gaps: do Settings, StateStore, SnapshotStore, AutopilotState, backup manifest need the file lock (Q23).
3. FD-4: wire `learned_pick` into routing, or rule it out (Q28). Timing not ruled: no `after-1.0` label; add `needs-ian`.

## Rules for the issue text
- Template (`.github/ISSUE_TEMPLATE/issue.md`): Title `<id or area>: <what it adds or fixes>`; body fields **What:**, **Where:** (files, plus the code map section), **Size:** Easy | Medium | Hard (about N changed lines; toolchain), **Blocked by:**, **Done when:** (numbered, checkable). Easy issues add a `## Qwen brief` with the expectation and the pasted code map snippet.
- Proof is `tools/ci.sh <part>`; name the part.
- No private details: no person's name, e-mail address, home path or machine name. Paths are repo-relative. Say "owner ruling", never a name.
- Use GLOSSARY.md words for the domain (Waiting on you, Star, Feature, Fleet error, Error code...). For architecture use exactly: module, interface, implementation, depth, deep, shallow, seam, adapter, leverage, locality. Never: component, service, API, boundary, layer, wrapper.
- Plain, direct sentences. No issue may ask the builder to decide something settled above.
- Cross-references to sibling issues not yet filed are written as `{{#key}}` using the sibling's key; they are replaced with real numbers at filing.
- Labels allowed: feature:core (always), one of size:easy|size:medium|size:hard, one of kind:data-shape|kind:pure-fn|kind:state|kind:ui|kind:wiring|kind:rename|kind:docs, and where ruled: after-1.0, needs-ian.

## Later rulings (owner, 2026-11-26, after the drafts)
- Q33 An unreadable fleet job ledger is a warning in every face: the Sky or Star is shown without its job and the warning names the ledger file. The CLI and MCP no longer fail on it.
- Q34 A guarded write with the app data or config folder unset answers with the Error code `setting_missing` (from CoreError), not `refused`. The PR lists it as a deliberate change.
- Q35 StateStore (runner/state.rs) and SnapshotStore (github/snapshot.rs) keep today's rule: a file holding only `null` is unreadable (a parse error), not "no file". Each of those two stores checks this itself; the module stays three operations.
- Q36 The builder of an issue writes or edits the locked tests that issue names, under the owner ruling (2026-11-26). The issue spells out the test's rules exactly and says the reviewer checks the test against those rules line by line. This covers: the new structural code map tests (t-core, t-tauri, t-react), their edits in m-guard (and in the cli/mcp map issue if it tightens the CLI status rule), `crates/core/tests/waiting_load.rs` lines 270-273 in r-waiting, and `crates/cli/tests/mcp_write_core.rs` in w-mcp.
- Defaults taken (not separately ruled; state them plainly, do not mark them as open): `.css` files under app/src count as source files and each gets a map line; the every-file check in m-guard uses the same rule. A map entry whose status is `planned` may name a path not yet on disk (for example `crates/core/src/workloop/`); the structural tests and the every-file check allow that. The platform plan's one home is docs/code-map.md. Disposition clauses 2.6 and 2.10a go to CHANGELOG lines. `specs/roadmap-run.json:21` is corrected in exactly one issue: m-core. The CLI structural test's status rule is tightened to the field rule in m-cli-mcp.

## Final key table (use exactly these in {{#key}} references)
t-core, t-tauri, t-react,
m-core, m-github, m-sky-core, m-sky-face, m-bug-waiting-reports, m-budget, m-backup, m-frontdoor, m-autopilot, m-runner, m-it, m-cli-mcp, m-shell, m-guard,
s-musts, s-dispositions, s-roadmap-changelog, s-glossary-runner,
f-module, f-held, f-marks, f-manifest, f-settings, f-runner-state, f-snapshot, f-autopilot,
w-core, w-cli, w-mcp,
r-waiting, r-sky, r-constellation, r-star, r-reports, r-budget, r-errors,
x-line-files, x-lock-gaps, x-fd4-learned-pick
(There is no key `m-sky`: it was split into m-sky-core and m-sky-face. m-guard is blocked by all sixteen other m- issues.)
Landing order for the map issues, because all of them edit docs/code-map.md: they are built one at a time in the order of the key table; each m- issue is blocked by the m- issue before it in the table (m-core first), as well as by its t- issues.
