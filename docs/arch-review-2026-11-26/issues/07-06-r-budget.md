**What:** Give the Budget view one read model in core that returns the answer plus warnings, used by the Tauri commands, the CLI and MCP (owner ruling, 2026-11-26).

Line numbers below are as the files read at the time of writing; earlier issues move lines in `crates/cli/src/cli.rs` and `crates/cli/src/main.rs`. Find each item by name.

This is the smallest of the seven. The read is already in core: `budget::read_with_claude` (`crates/core/src/budget.rs` 852-859) returns `BudgetSnapshot` (rows and Claude's reading, 546-549), and `budget::refresh_with_claude` (862-869) does the same after a refresh. All three faces call them:
- CLI: `budget_rows` (`crates/cli/src/cli.rs` 1711-1715) and `budget_refreshed_rows` (1718-1722), printed by `budget` (1693-1708) and `budget_text` (1728-1760).
- MCP: `Core::call` (`crates/cli/src/main.rs` 108-109) calls `cli::budget_rows`.
- Window: the `budget_rows` command (`app/src-tauri/src/commands.rs` 397-405) and the `budget_refresh` command (408-416).

Nothing is left out of the Budget read today, so there is no warning to show yet. A Claude session log that cannot be read is an error, not a warning: the locked `crates/core/tests/budget_claude.rs` pins it in `a_log_that_cannot_be_read_is_an_io_error`. Keep that. This issue puts the Budget read behind the same interface as the other six, so MCP stops reaching into cli.rs and the view shows a warning the day the read gains one.

Build this:
1. `crates/core/src/read/budget.rs` (declare it in `crates/core/src/read/mod.rs`):
   - `pub struct BudgetAnswer { #[serde(flatten)] pub snapshot: BudgetSnapshot, pub warnings: Vec<Warning> }` (Serialize; `Warning` from {{#r-waiting}}; the list is empty today). It serializes as `{"rows": [...], "claude": ..., "warnings": []}`.
   - `pub fn load(fleet: &FleetBridge, clock: &dyn Clock) -> Result<BudgetAnswer, CoreError>` over `read_with_claude`, and `pub fn refresh(fleet: &FleetBridge, clock: &dyn Clock) -> Result<BudgetAnswer, CoreError>` over `refresh_with_claude`.
2. CLI: `budget_rows` and `budget_refreshed_rows` call the read model and return `BudgetAnswer`. The CLI keeps `pipeline_or_fail`, `budget_text`, and the `--json` shape unchanged: it prints the answer's `snapshot`, not the answer, so no `warnings` key appears. It prints any warning to stderr as `switchboard budget: <file>: <reason>`.
3. MCP: `Core::call` uses the read model through the same cli.rs function. The first content item is unchanged (the `snapshot`); warnings, when there are any, ride in the second content item added by {{#r-waiting}}.
4. Window: the two Tauri commands keep their names, their `FaceError::new(..)` texts and their `record_fleet_error` call on failure, and return `BudgetAnswer`: the same `rows` and `claude` fields with `warnings` beside them. Two locked tests read source text and stay green as long as the names stay: `crates/core/tests/fd_rating_log.rs` (line 438) needs `pub fn budget_rows(` in commands.rs after `pub fn chat_rate(`, and `crates/core/tests/reports_load.rs` (`main_registers_the_reports_command`, line 445) needs the handler entries in `app/src-tauri/src/main.rs`, which this issue does not edit. `budget_save_edit` (commands.rs 443-460) and the `budget-rows` event (`app/src-tauri/src/main.rs` line 159, whose payload is the rows alone) are not changed.
5. `app/src/views/Budget.tsx`: the type `BudgetData` (line 20) gains an optional `warnings`, `unwrapBudget` (22-26) also returns `warnings` (missing, or a bare list, means none: the locked `app/src/views/Budget.test.tsx` mocks bare rows), and the view shows `WarningLine` at the top. A `budget-rows` event keeps the warnings of the last `budget_rows` answer.
6. Code map: one entry line for `crates/core/src/read/budget.rs` under `### budget`, in the map's entry form: `` `<path>` · present · <one sentence> ``, under 400 bytes and with no `#<number>` (the structural code map tests, with the guard {{#m-guard}} turned on, fail a line that breaks this and fail a source file that has no line). Correct any line of `### budget`, `### cli` and `### shell` that this change makes untrue, within the same limits. The new file starts with a `//!` header that says what it is for.

Not in this issue: the MCP write tool `budget_refresh` (`WriteCore::budget_refresh` in `crates/cli/src/mcp.rs`) calls `budget::refresh` for the rows alone and stays as it is.

**Where:** `crates/core/src/read/budget.rs` (new), `crates/core/src/read/mod.rs`, `crates/cli/src/cli.rs`, `crates/cli/src/main.rs`, `app/src-tauri/src/commands.rs`, `app/src/views/Budget.tsx`, new tests `crates/core/tests/read_budget.rs` and `app/src/views/BudgetWarning.test.tsx`, `docs/code-map.md`. Code map sections: `### budget`, `### cli`, `### mcp`, `### shell`.

**Size:** Medium (about 170 changed lines in 9 files, tests included; Rust and React). Few lines, but two toolchains and three faces put it over the 6-file aim; it is reviewed as an oversize change.

**Blocked by:** {{#r-reports}} (it edits `crates/core/src/read/mod.rs`, `cli.rs`, `Core::call` and `commands.rs` just before this one; no code is shared with it), {{#r-waiting}} (the warning type, the MCP warnings item, `app/src/WarningLine.tsx`). Owner ruling (2026-11-26): the seven read-model issues land one at a time in the order waiting, sky, constellation, star, reports, budget, errors.

**Done when:**
1. `crates/cli/src/cli.rs` and `app/src-tauri/src/commands.rs` no longer call `budget::read_with_claude` or `budget::refresh_with_claude`; they call `switchboard_core::read::budget::load` and `refresh`.
2. `crates/core/tests/read_budget.rs` shows: the answer serializes with exactly the keys `rows`, `claude` and `warnings`, and `warnings` is empty; a session log that cannot be read is still an error from `load`.
3. These pass unedited: `crates/cli/tests/budget_cli.rs` (all); the `budget_refresh_` tests in `crates/cli/tests/headless_cli.rs`; `crates/cli/tests/cli_edges.rs` (`budget_and_reports_read_pipeline_repo_from_settings`); `crates/cli/tests/mcp_read.rs` (all); `crates/core/tests/budget_claude.rs`; `crates/core/tests/fd_rating_log.rs`; `crates/core/tests/reports_load.rs`; `app/src/views/Budget.test.tsx`; `app/src/views/BudgetClaude.test.tsx`.
4. `app/src/views/BudgetWarning.test.tsx` (new, not locked) shows the line at the top of the Budget view naming the file when the answer carries a warning, and no element with role `status` when `warnings` is empty or missing.
5. `docs/code-map.md` has the entry line for `crates/core/src/read/budget.rs` in the form step 6 gives, and `crates/core/tests/code_map_structure.rs` passes.
6. `tools/ci.sh rust`, `tools/ci.sh app` and `tools/ci.sh face` pass; their output is in the PR body.
