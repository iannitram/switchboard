What: Rewrite the `budget` section of `docs/code-map.md` so `crates/core/src/budget.rs` and `app/src/views/Budget.tsx` each have one short entry line, and remove the ten bullets. One of the code map revision issues (owner ruling, 2026-11-26); comments and docs only.

Where: `docs/code-map.md` lines 206-218 (code map: `### budget`; 1,741 bytes), and the `//!` header of `crates/core/src/budget.rs`.

Size: Easy (about 25 changed lines in 2 files; Rust and Node toolchains for the proof, no code change).

Blocked by: {{#m-bug-waiting-reports}}, {{#t-core}}, {{#t-react}} (`app/src/views/Budget.test.tsx:146` and `PoolEditor.test.tsx:155` each expect a bullet word for word, as do `crates/core/tests/budget_claude.rs`, `budget_edit.rs`, `budget_refresh.rs` and `budget_rows.rs`).

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, each blocked by the one before it: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the sixth: the five before it have landed and {{#m-backup}} is next.

Line numbers in this issue are those of the map on main at commit 19e0c65. The five earlier map issues have rewritten the lines above this section, so the map's numbers have moved; they leave the section itself and `crates/core/src/budget.rs` as they are. The block to replace starts at the heading `### budget` and ends at the blank line before `### backup`.

What is there (line numbers as at 19e0c65):
- One path-led line, 207 (283 bytes), which names two files.
- Ten bullets, lines 209-218. None is over 400 bytes. Lines 211 and 216 hold issue numbers. Lines 210 and 211 are two bullets for the same thing (BUD-4 Claude pool).
- `app/src/views/Budget.tsx` has no line of its own. There is no `PoolEditor.tsx`: `PoolEditor` and `BudgetPanel` are in `Budget.tsx`.
- Nearly all the bullets' detail is already in `budget.rs`: the save order and undo, the two locks and the wait are on `save_edit` (lines 189-197) and `EDIT_LOCK_WAIT` (222-224), the all-or-nothing save on `save_pool_edit` (207-210), the reply text on `edit_summary` (229-231), the folder order on `claude_config_dirs` (770-774), the ccusage credit above it (lines 721-722). Only the list of faces is missing; the brief adds where to find it.
- One thing in the map is stale and is dropped, not moved: line 216 says temp files are named with `lock::temp_path`. `budget.rs` does not call `temp_path`; it writes through `lock::write_whole` (lines 476-479).

Rules (owner ruling, 2026-11-26):
- Entry format: one line per source file, starting with the backticked repo-relative path: `` `path` · status · one sentence ``. At most 400 bytes (UTF-8; `·` is 2 bytes) and no `#` followed by digits.
- No bullets under a revised section. Fix history (issue numbers) is dropped; `CHANGELOG.md` has it.
- Do not change the `reuses:` and `modules:` blocks (lines 9-26) or the `### budget` heading. Set the first `Updated:` line to the day of the PR.
- No earlier map issue writes a line for these two paths. {{#m-shell}} later lists the face files still without a line and finds `Budget.tsx` done here.

Done when:
1. The `budget` section of `docs/code-map.md` is the three lines under "Expectation" below, and nothing else in the map changes except the `Updated:` date.
2. Each of these prints 1:
   ```
   grep -c '^`crates/core/src/budget.rs`' docs/code-map.md
   grep -c '^`app/src/views/Budget.tsx`' docs/code-map.md
   ```
3. Each of these prints nothing:
   ```
   sed -n '/^### budget$/,/^### backup$/p' docs/code-map.md | grep '^- '
   sed -n '/^### budget$/,/^### backup$/p' docs/code-map.md | LC_ALL=C awk '/^`/ && length($0) > 400'
   sed -n '/^### budget$/,/^### backup$/p' docs/code-map.md | grep -E '^`.*#[0-9]'
   ```
4. The `//!` header of `crates/core/src/budget.rs` has the two lines from the brief; `git diff` shows no other change in that file and no change in `Budget.tsx`.
5. `tools/ci.sh rust` and `tools/ci.sh face` pass; their output is in the PR body.

## Qwen brief

Expectation: In `docs/code-map.md`, replace the `budget` section (the `### budget` heading, the path-led line, the blank line and the ten bullets; find it by the heading, because earlier map issues have moved the line numbers) with exactly these three lines; the blank line before `### backup` stays:

```
### budget
`crates/core/src/budget.rs` · present · BUD-1 to BUD-4: pool rows from `fleet budget --json` (`read`, `refresh`, `RefreshTimer`), pool edits to `config.json` with a ledger row (`save_edit`, `save_pool_edit`, `edit_summary`), and Claude's pool as a reading plus a labelled estimate from local session logs (`read_with_claude`, `refresh_with_claude`, `BudgetSnapshot`).
`app/src/views/Budget.tsx` · present · BUD-1, BUD-2, BUD-4: the Budget view (`Budget`), its panel of pool rows against the pace line with Claude's estimate (`BudgetPanel`) and the pool editor (`PoolEditor`).
```

Then add these two lines at the end of the `//!` header of `crates/core/src/budget.rs` (after line 3, before the blank line):

```
//! The faces that call this module are the `budget_rows`, `budget_refresh` and `pool_edit` rows
//! of `faces::REGISTRY` (crates/core/src/faces.rs).
```

Set line 3 of the map to `Updated: <today's date, YYYY-MM-DD>`. Change nothing else. Run `tools/ci.sh rust` and `tools/ci.sh face`; both must pass.

Code map snippet (the lines being replaced; the earlier map issues do not change them):
```
### budget
`crates/core/src/budget.rs`, `app/src/views/Budget.tsx` · BUD-1 to BUD-4 · present. Rows from `fleet budget --json`, edits to `config.json` with a ledger row (U5: landed through review), refresh on a timer, Claude's pool as reading plus a labelled estimate from local session logs.

- BUD-1 rows (`rows_from_json`, `read`): present.
- BUD-4 Claude pool (`claude_pool`, `session_log_paths`, `parse_utc`): present.
- BUD-4 Claude pool (`claude_pool`, `claude_for_rows`, `read_with_claude`, `refresh_with_claude`, `BudgetSnapshot`, `session_log_paths`, `parse_utc`): present; wired into `budget_rows`, `budget_refresh`, `Budget.tsx`, `switchboard budget`, and MCP `budget` (#502).
- BUD-3 refresh (`refresh`, `RefreshTimer`, `budget_rows`, `budget_refresh`): present.
- BUD-1 panel (`BudgetPanel`, `Budget`): present.
- BUD-2 save (`save_edit`, `parse_reset`, `budget_save_edit`): present.
- BUD-2 save for the CLI and MCP (`save_pool_edit`: file, ledger row in `<data>/pool-edits.jsonl` and Waiting item, all or nothing; `edit_summary`, the reply text naming each new value and the old one): present.
- BUD-2 edits at the same time (#416): the save holds `lock::FileLock` on `config.json`, then on the ledger, for the read, the writes, the Waiting item and any undo; a second edit waits up to `EDIT_LOCK_WAIT` (10 s), then is refused with what to do. Temp files are named per run (`lock::temp_path`). A cap is at least `MIN_CAP_USD` (a cent): present.
- BUD-2 editor (`PoolEditor`): present.
- BUD-4 log folders (`claude_config_dirs`, `claude_projects_dirs`: the `CLAUDE_CONFIG_DIR` list, then `$XDG_CONFIG_HOME/claude`, `~/.claude`; adapted from ccusage, credited in `NOTICE`; the caller passes the environment values): present.
```
Signatures the lines must fit (they exist today): `budget.rs`: `pub fn read`, `pub fn refresh`, `pub struct RefreshTimer`, `pub fn save_edit`, `pub fn save_pool_edit`, `pub fn edit_summary`, `pub struct BudgetSnapshot`, `pub fn read_with_claude`, `pub fn refresh_with_claude`. `Budget.tsx`: `export function PoolEditor`, `export function BudgetPanel`, `export function Budget`. `faces.rs`: `REGISTRY` has rows with the ids `budget_rows`, `budget_refresh` and `pool_edit` (lines 146, 160 and 159 at 19e0c65; {{#m-core}} may have moved them by adding header comment lines, so find the rows by id).
