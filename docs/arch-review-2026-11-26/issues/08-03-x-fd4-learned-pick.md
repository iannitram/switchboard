What: `router::learned_pick` holds the FD-4 rule, but nothing outside tests calls it, so ratings never change a pick. This issue asks for an owner ruling (wire it before 1.0, wire it after 1.0, or cut it) and then does what the ruling says. The timing is not ruled yet.

Where: (line numbers are from main at 19e0c65; find by function name if they moved)
- if wired: `crates/core/src/frontdoor/router.rs`, `crates/core/src/frontdoor/ask.rs`, `crates/core/tests/fd_learned_route.rs` (new), `SPEC.md` (the FD-4 row), `docs/code-map.md`
- if cut: the files named in item 4 of "Done when" and under "Text that says FD-4 works today" below
- code map: the entry line of `crates/core/src/frontdoor/router.rs`, as {{#m-frontdoor}} left it (one line per file, `` `path` · status · one sentence ``); find it by its path

Size: Medium (if wired: about 260 changed lines in 5 files: `router.rs` about 50, `ask.rs` about 30, the new test about 170, the SPEC row and the code map about 5; Rust). If the ruling is to cut it, the change is about 380 lines, nearly all deletions, in 11 files, and is reviewed as an oversize change.

Blocked by: {{#s-musts}}, {{#m-frontdoor}}, and an owner ruling on the question below. Until this lands, the Spec and the code map mark the FD-4 learned pick planned (owner ruling, 2026-11-26): {{#s-musts}} appends "Planned: the rule is written and tested, and routing does not use it yet." to FD-4's "Done looks like" cell, and {{#m-frontdoor}} says so on the `router.rs` code map line and in that file's `//!` header. This issue takes those marks away again, so both land first.

## The question

Pick one:

1. **Wire it before 1.0.** A Request kind's pick follows its ratings, as the FD-4 acceptance line says.
2. **Wire it after 1.0.** FD-4 stays `planned`; this issue gets the `after-1.0` label and waits.
3. **Cut it.** The rule and its tests are deleted and FD-4 leaves the spec. One of those tests is in `crates/core/tests/fd_rating_log.rs`, a locked test file, so a ruling of 3 also says the builder deletes that one test and its import under the ruling, and the reviewer checks that nothing else in the file changed.

If the answer is 1 or 2, four rules also need a yes or a correction:

- (a) A learned pick whose Pool is braked or over budget is skipped and the `first` walk of FD-3 stands (a braked Pool is never called). Proposed: yes.
- (b) A learned pick is taken even when its Pool is not in the kind's `first` list, but only when the abilities index still gives that kind and Pool the same model the ratings were for (`AbilitiesIndex::model(kind, pool)` equals the learned model); otherwise the `first` walk stands. Proposed: yes.
- (c) A learned pick that names the Claude subscription is never called; it is the Claude handoff, as in FD-3. Proposed: yes.
- (d) An ask whose ledger cannot be read is still routed, with the chat rows alone; a routing hint never fails an ask. Proposed: yes.

## Facts (checked at 19e0c65)

**The Must (`SPEC.md:158`):** "WHEN a request kind has N or more rated rows for a model, THE CORE SHALL take the cheapest model with at least 80% passes over its last N rated rows; N starts at 6 and rises by 1 for every 20 rows the kind has logged, to a cap of 30; below N rows the index's guess stands." Acceptance: "The pick changes as ratings come in." Check: "Router test over fixture job logs at 5, 6, 26 and 600 rows." The check is met today; the acceptance line is not.

**The rule as built (`crates/core/src/frontdoor/router.rs`):**
- `pub fn sample_size(rows_logged: usize) -> usize` at lines 112-114.
- `pub fn learned_pick(kind: &str, rows: &[JobRow]) -> Option<(String, String)>` at lines 127-180. It returns `(lane, model)`; the lane is the Pool's name. N counts every row of the kind (ledger or chat, rated or not). Only chat rows (`RowSource::Chat`) that are rated and not implicit count toward a model. Rows with `fallback` set are left out. It reads no file.

**Who calls it:** no file under `crates/core/src`, `crates/cli/src` or `app/src-tauri/src`. Only tests: `crates/core/tests/fd_learned_pick.rs` (17 tests, 241 lines), `crates/core/tests/fd_qwen_fallback.rs:555` and `:565`, `crates/core/tests/fd_rating_log.rs:273`. No test shows a route changing because of ratings.

**Where routing would call it:**
- `pub fn pick_for_kind(index: &AbilitiesIndex, kind: &str, budget: &dyn BudgetGate) -> Pick` at `router.rs:64-99`. Its doc comment says it reads no clock and no file. It takes no rows.
- `pick_for_kind` is called in two places outside tests, both in `router.rs`: line 259 (`explicit_ask`) and line 339 (the labeller step of `route`). The trigger step and the budget rule step have no Request kind, so FD-4 does not apply there.
- `pub fn route(message, index, budget, complexity, small) -> Route` at `router.rs:322-348` takes no rows.
- `router::route` is called in three places outside tests: `ask.rs:70`, `chat.rs:470`, `chat.rs:607`. Only the first is on a path a face uses: the window (`app/src-tauri/src/commands.rs:834`, `ask::ask`), the CLI (`crates/cli/src/cli_frontdoor.rs:241`) and MCP (`crates/cli/src/mcp_frontdoor.rs:137`, both `ask::ask_headless`) all end in `ask::ask_on` (`ask.rs:61`), whose router closure at `ask.rs:69-71` is passed to `chat::route_in_thread_on` (`chat.rs:506`) or, for a dry run, to `chat::route_in_thread_gated` (`chat.rs:528`). `chat.rs:470` is inside `chat::route_message` (`chat.rs:457`), which only tests call and which takes no data folder; `chat.rs:607` is inside `chat::route_in_thread` (`chat.rs:590`), which nothing calls.
- The closure type is `impl Fn(&str, &AbilitiesIndex, &dyn BudgetGate, Complexity) -> Route`, so the closure in `ask_on` can carry the rows and no signature in `chat.rs` needs to change.
- A Sticky chat does not ask the router while it stays on its model or moves back after a brake lifts (`chat.rs:528-585`: the router is asked only at `:542`, a new chat, and `:582`, a chat a signal or a brake moves), so a learned pick changes new chats and moved chats only.

**Where the rows come from:**
- Chat rows: `reports::thread_rows(data)` (`crates/core/src/reports.rs:488`) reads every thread through `ThreadStore::list` and `ThreadStore::read`; a thread that cannot be read is skipped.
- Ledger rows: `joblog::read_ledger(&joblog::ledger_files(pipeline_root, state_dir))` (`crates/core/src/joblog.rs:250`, `:225`). The pipeline root is `fleet.root()`; the state folder is `reports::state_dir(root, env)` (`reports.rs:376`), where `env` is the value of `FLEET_STATE_DIR` (`reports::FLEET_STATE_DIR_ENV`); when that is not set it runs `git worktree list` once.
- `joblog::read_all(ledger_events, chat_rows)` (`joblog.rs:179`) joins them and marks the chat rows `RowSource::Chat`, which `learned_pick` needs.
- `ask_on` already has `fleet`, `data_dir` and `seams.env` (`ask::Seams`, `ask.rs:37`), which is all the read needs.
- Cost of wiring: per ask, one read of every thread file and of the ledger, and one `git worktree list` when `FLEET_STATE_DIR` is not set.

**Tests that call the two functions today:** `pick_for_kind(` is called on 11 lines of `crates/core/tests/fd_pick_for_kind.rs`; `router::route` is called 4 times in `fd_route.rs`, 3 times in `fd_chat_route.rs` and 3 times in `fd_labeller_small.rs`. None of these change if the two signatures stay.

**Text that names FD-4 or the learned pick** (as on main; {{#s-musts}}, {{#s-dispositions}} and {{#m-frontdoor}} reword some of it and move the lines, so find each by its words)**:** `docs/code-map.md:253` ("FD-4 learned pick (`sample_size`, `learned_pick`): present."), `:235` (router: "the learned pick"), `:99` (job log "Read by reports, the router (FD-4) and budget"); `SPEC.md:157` (FD-3: "thumbs on a fallback answer never change learned routing (FD-4)"), `:163` (FD-9: implicit rows "which FD-4 does not count in 1.0"), `:295` (U11); doc comments at `crates/core/src/frontdoor/router.rs:1`, `rating_log.rs:70`, `sticky.rs:62` and `crates/core/src/joblog.rs:42`.

Done when:

1. The owner ruling (choice 1, 2 or 3, and rules (a) to (d) for 1 or 2) is written as a comment on this issue.
2. If the ruling is 2: the `after-1.0` label is on this issue and nothing else changes until it is taken up; item 3 then applies.
3. If wired, with rules (a) to (d) as proposed (if the ruling corrects a rule, the ruling comment says what replaces the matching clause below):
   1. `router.rs` gains `pub fn pick_for_kind_learned(index: &AbilitiesIndex, kind: &str, budget: &dyn BudgetGate, rows: &[JobRow]) -> Pick` and `pub fn route_with_rows(message, index, budget, complexity, small, rows: &[JobRow]) -> Route`. `pick_for_kind` and `route` keep their signatures and results (`route` is `route_with_rows` with no rows). In `route_with_rows` both the explicit-ask step and the labeller step pick through `pick_for_kind_learned`. The router still reads no file and no clock.
   2. `pick_for_kind_learned` gives exactly what `pick_for_kind` gives when `kind` is not a Request kind, when `learned_pick(kind, rows)` is `None`, when `budget` does not allow the learned Pool, or when `index.model(kind, pool)` is not the learned model. Otherwise it gives the learned Pool and model; for the Pool `claude` it gives the Claude handoff, never a call.
   3. `ask::ask_on` loads the rows once per ask, before it routes, for a real ask and a dry run alike: `joblog::read_all` over the ledger events (read as under "Where the rows come from", with `FLEET_STATE_DIR` taken through `seams.env`) and every row of `reports::thread_rows(data_dir)`. A ledger that cannot be read gives no ledger events and the ask goes on. Its router closure calls `route_with_rows`. `ask::Seams` is unchanged. `chat::route_message` and `chat::route_in_thread` are left as they are.
   4. A new test file `crates/core/tests/fd_learned_route.rs` shows, through `route_with_rows`: with 5 rated passing rows for a cheaper model the route is the `first` list's pick; with 6 it is that model; with that model's Pool braked it is the `first` list's pick again; 6 rows marked `fallback` change nothing; 6 rows for a model the index does not give that kind and Pool change nothing; a learned pick on the Pool `claude` is a Claude handoff. And through `ask::ask_on` with `dry_run` true: a data folder whose threads hold 6 rated passing rows for a cheaper model gives a reply whose route is that model.
   5. In the same PR: the "Planned:" sentence that {{#s-musts}} appended to FD-4's "Done looks like" cell in `SPEC.md` is removed and the Must's own words are not changed; the `crates/core/src/frontdoor/router.rs` entry line of the code map and that file's `//!` header no longer say the learned pick is planned, and say routing uses it (the entry line stays under 400 bytes with no `#<number>`); the map's first `Updated:` line is re-dated.
   6. No existing test file changes. If an existing test cannot pass unchanged, the job stops and says which on this issue.
   7. `tools/ci.sh rust` and `tools/ci.sh lint` pass and their output is in the PR body.
4. If cut:
   1. `sample_size` and `learned_pick` are deleted from `router.rs` (lines 110-180), with `crates/core/tests/fd_learned_pick.rs`, the test `thumbs_on_fallback_answers_never_teach_routing` in `fd_qwen_fallback.rs` (line 540), the test `rating_rows_are_rated_chat_rows_the_learned_pick_counts` in `fd_rating_log.rs` (line 252), and the two `learned_pick` imports (`fd_qwen_fallback.rs:23`, `fd_rating_log.rs:13`).
   2. The FD-4 row leaves `SPEC.md` (its id is not reused and no other Must is renumbered), and the FD-4 mentions in the FD-3 row, the FD-9 row and U11 go. The code map and the four doc comments listed above no longer name FD-4 or the learned pick. `CHANGELOG.md` gets one dated line with the ruling; the CHANGELOG is the only place that still names FD-4.
   3. `grep -rn "FD-4\|learned_pick\|sample_size" SPEC.md docs crates app/src-tauri/src` finds nothing.
   4. `tools/ci.sh rust` and `tools/ci.sh lint` pass and their output is in the PR body.
