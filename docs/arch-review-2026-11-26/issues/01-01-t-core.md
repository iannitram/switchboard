What: Add one structural test of `docs/code-map.md` for the core tree and one for the CLI tree, and remove the 32 test functions under `crates/core/tests` that assert on the real code map text. One more file, `unit_job.rs`, stops reading the real map through its helper. After this, rewording a code map line no longer breaks a core test; a path that is not on disk, a bad status word or a broken machine-read block does. This issue is independent of {{#t-tauri}} and {{#t-react}}: the three can land in any order, because none of them changes the map.

Owner ruling (2026-11-26), quoted: "every test that pins code map wording, locked tests included, is replaced by structural tests. One structural test per tree (core, CLI, Tauri, React face)." And: "Only the assertions that read the real `docs/code-map.md` text go; every other test in those files stays untouched." 25 of the 33 files below carry a locked-tests header (marked L in the table); this ruling is the permission to edit them, for these functions only.

The CLI tree: `crates/cli` has no test that reads the code map today (no file under `crates/cli/src` or `crates/cli/tests` names `docs/code-map.md`). Its structural test is added here, because `tools/ci.sh rust` runs both crates.

Where: `crates/core/tests/` (33 files edited, 1 new), `crates/cli/tests/` (1 new). `docs/code-map.md` is NOT changed by this issue, and no file under `crates/core/src` or `crates/cli/src` is changed. (code map: the whole file is the subject; `### sky-codemap` describes the machine-read blocks.)

Size: Medium (about 600 changed lines: about 385 deleted, about 215 added; 35 files; Rust). This is over the usual limit of 300 lines and 6 files. It is not split further: the owner ruling is one issue per tree. It is reviewed as an oversize change. Not Easy: it edits locked test files and adds new locked tests.

Blocked by: nothing. The code map issues ({{#m-core}} to {{#m-guard}}) are blocked by this one.

Who writes the locked tests: owner ruling (2026-11-26), the builder of this issue writes `crates/core/tests/code_map_structure.rs` and `crates/cli/tests/code_map_structure.rs` and makes the edits to the locked files named in section 1. No other role writes them first. Sections 3 and 4 are the whole definition of the new tests: the builder adds no rule, drops none and loosens none. The reviewer checks the new test code against reading rules R1-R6 and the eight numbered tests line by line, and checks the removals against the table in section 1.

## 1. Remove these test functions (the whole function with its `#[test]` line and one blank line beside it; nothing else in the file)

Line numbers are on main at 19e0c65. L = the file has a locked-tests header.

| File in `crates/core/tests/` | Function | Lines |
|---|---|---|
| `abilities_index.rs` (L) | `the_code_map_shows_core_abilities_present` | 178-187 |
| `auto_after_poll_review.rs` | `the_code_map_names_the_review_start_present` | 435-444 |
| `auto_driver.rs` | `the_code_map_names_the_satellite_loop_present` | 349-358 |
| `auto_merge_pass.rs` | `the_code_map_names_the_merge_pass_present` | 539-548 |
| `auto_roadmap_pass.rs` | `the_code_map_names_the_roadmap_pass_present` | 287-296 |
| `auto_weekly_run.rs` | `the_code_map_names_the_weekly_run_present` | 397-406 |
| `backup_run.rs` (L) | `the_code_map_lists_bak_w_present` | 407-422 |
| `budget_claude.rs` (L) | `the_code_map_marks_the_claude_pool_present` | 208-217 |
| `budget_edit.rs` (L) | `the_code_map_marks_the_save_present` | 388-392 |
| `budget_refresh.rs` (L) | `the_code_map_marks_the_refresh_present` | 139-145 |
| `budget_rows.rs` (L) | `the_code_map_marks_the_rows_present` | 201-205 |
| `bug_fix.rs` | `the_code_map_covers_the_bug_fix_files` | 418-436 |
| `fd_chat_route.rs` (L) | `the_code_map_marks_the_command_present` | 607-624 |
| `fd_implicit_log.rs` (L) | `the_code_map_marks_the_command_present` | 295-309 |
| `fd_pick_for_kind.rs` | `the_code_map_shows_fd_3_present` | 554-566 |
| `fd_rating_log.rs` (L) | `the_code_map_marks_the_command_present` | 452-468 |
| `fd_send_held_job.rs` (L) | `the_code_map_marks_it_present` | 303-321 |
| `fleet_error_log.rs` (L) | `the_code_map_shows_core_fleet_present` | 70-79 |
| `it_loadout_command.rs` (L) | `the_code_map_lists_the_loadout_command_present` | 244-258 |
| `job_log.rs` (L) | `the_code_map_shows_core_joblog_present` | 252-261 |
| `platform_seams.rs` (L) | `the_code_map_shows_core_platform_present` | 65-74 |
| `reports_build.rs` (L) | `the_code_map_marks_the_reports_present` | 324-330 |
| `reports_filter.rs` (L) | `the_code_map_marks_the_filter_present` | 135-139 |
| `reports_load.rs` (L) | `the_code_map_marks_the_command_present` | 491-497 |
| `reports_moves.rs` (L) | `the_code_map_marks_the_moves_present` | 134-138 |
| `rules_guard.rs` (L) | `the_code_map_shows_core_rules_present` | 205-214 |
| `sky_codemap.rs` (L) | `the_repo_code_map_names_brocrew` | 68-73 |
| `sky_model.rs` (L) | `the_code_map_lists_sky_model_present` | 759-769 |
| `unit_proof.rs` (L) | `the_repo_code_map_has_both_unit_proof_files_present` | 229-243 |
| `waiting_load.rs` (L) | `the_code_map_lists_the_waiting_command_present` | 280-287 |
| `waiting_local.rs` (L) | `the_code_map_marks_the_local_items_and_the_merge_present` | 414-419 |
| `waiting_snapshot.rs` (L) | `the_code_map_marks_the_snapshot_items_present` | 325-330 |

That is 32 functions in 32 files, 335 lines plus one blank line each.

What the removals leave unused (clippy runs with `-D warnings`, so these go in the same PR and nothing else does):
- `auto_roadmap_pass.rs` line 6 and `auto_weekly_run.rs` line 7: `use std::path::{Path, PathBuf};` becomes `use std::path::PathBuf;`.
- `reports_build.rs` (helper `fn manifest`, lines 12-14, and `use std::path::PathBuf;`, line 5), `reports_filter.rs` (`fn manifest`, lines 9-11, and the `use`, line 5), `reports_moves.rs` (`fn manifest`, lines 9-11, and the `use`, line 5): the helper and that `use` line are used only by the removed test; remove both.
- In every other file the imports and helpers the removed function used are still used by other tests.

## 2. `unit_job.rs`: stop reading the real map

`crates/core/tests/unit_job.rs` (no locked header) removes no test. Its helper `code_map()` (lines 23-25) reads the real `docs/code-map.md`. It is called by the helper `built()` (line 39) and by three tests: `the_brief_has_what_expectations_and_code_map_lines_and_nothing_from_outside_the_node` (lines 101 and 107), `a_unit_with_no_claim_is_refused` (line 119) and `a_claim_for_another_repository_or_issue_is_refused` (line 128). Change only the body of `code_map()` so it returns this fixed text:

```
`crates/core/src/unit/job.rs` · present · The fleet job and brief of a claimed unit.
`crates/core/src/unit/claim.rs` · present · The claim file lock.
```

(two lines, each ending in a newline; the separator is space, U+00B7 MIDDLE DOT, space). The fixture must not contain the text `unit-claim` or `ROADMAP`, because the brief test asserts the brief has neither. No test body in the file changes.

## 3. New file `crates/core/tests/code_map_structure.rs` (locked)

Header: `//! Locked tests (R28): the structure of docs/code-map.md for the core tree. Written under the owner ruling of 2026-11-26; a builder edits this file only where an issue names the edit under an owner ruling.` The file is self-contained: std plus `switchboard_core::sky::codemap::{modules, reuses}`; no new dependency; no shared helper file.

Reading rules (the same in every tree's test):
- R1. Read `docs/code-map.md` from disk under `Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")` (the repo root) and replace `\r\n` with `\n`.
- R2. A line that starts with three backticks opens or closes a fenced block. Fence lines and the lines inside a fenced block are skipped by every rule below.
- R3. A span is the text between the 1st and 2nd backtick of a line, the 3rd and 4th, and so on.
- R4. A tree span is a span that starts with the tree prefix. Core: `crates/core/`.
- R5. An entry line is a line that starts with a backtick and whose first span is a tree span.
- R6. The line status is the first whole word `present` or `planned` on the line (case-sensitive; a whole word has no ASCII letter, digit or underscore directly before or after it).

Tests (one function each, with these names):
1. `the_code_map_backticks_pair_up`: every line outside a fenced block has an even number of backticks. The failure message names the line number.
2. `every_core_path_in_the_code_map_is_on_disk`: every tree span names a path that exists under the repo root; a span ending in `/` must be a directory. A span on a line whose line status (R6) is `planned` is not checked. The test fails if it finds no tree span at all. The failure message lists every missing path with its line number. Today: 82 core spans on 62 lines, one not checked (`crates/core/src/workloop/`, line 338, planned), the other 81 on disk.
3. `every_core_entry_has_a_valid_status`: split each entry line on the separator (space, U+00B7 MIDDLE DOT, space); at least one field after the first begins with `present` or `planned`, and the character after that word, if any, is not an ASCII letter, digit or underscore. The test fails if there is no entry line. Today: 58 core entry lines, all pass.
4. `the_modules_block_parses_and_its_paths_are_on_disk`: find the first line that equals `modules:` once trailing whitespace is trimmed, and take the raw lines after it while they start with `- `. `modules(text)` gives the same number of rows (not zero; 14 today). For every row: `node` is not empty and holds only `a-z`, `0-9` and `-`; `path` is not empty; `status` is exactly `present` or `planned`; `format!("- {node}: {path} ({status})")` equals the raw line; a `present` row's path exists on disk (a path ending in `/` must be a directory).
5. `the_reuses_block_parses`: the same for `reuses:`: `reuses(text)` gives one row per raw line (not zero; 1 today); `repo` is not empty and has no whitespace; `what` is not empty; `format!("- {repo}: {what}")` equals the raw line.
6. `the_updated_line_is_a_date`: the first line that starts with `Updated:` exists, and the rest of it, trimmed, is `YYYY-MM-DD` (10 characters; digits, with `-` as the 5th and 8th character; month 01-12; day 01-31).

What these rules accept, so the code map issues that follow do not trip them:
- A `planned` entry may name a path that is not on disk yet (`crates/core/src/workloop/` today). Test 2 skips every span on a line whose line status is `planned`, and test 4 checks the disk for `present` rows of the `modules:` block only.
- New `###` headings, and renamed or removed ones. No rule reads a heading, a section name or the order of lines.
- Reworded entry lines. No rule reads the sentence of an entry, only its paths and its status.

## 4. New file `crates/cli/tests/code_map_structure.rs` (locked)

Header: `//! Locked tests (R28): the structure of docs/code-map.md for the CLI tree. Written under the owner ruling of 2026-11-26; a builder edits this file only where an issue names the edit under an owner ruling.` Same reading rules R1-R6, tree prefix `crates/cli/`. std only. Two tests:
1. `every_cli_path_in_the_code_map_is_on_disk`: as core test 2. Today: 6 CLI spans (lines 7, 116, 119, 367, 376, 378), all on disk.
2. `every_cli_entry_has_a_status`: every entry line has a line status (R6); the test fails if there is no entry line. Today: 3 CLI entry lines (367, 376, 378), all pass. The stricter field rule of core test 3 does not hold for the CLI tree today: lines 367 and 376 write the status inside a sentence ("CLI-1, CLI-2 and CLI-3 present.", "MCP-1 to MCP-3 present."). This issue does not edit those lines and does not apply the field rule to the CLI tree. {{#m-cli-mcp}} rewrites those lines and, in the same PR, tightens `every_cli_entry_has_a_status` to the field rule of core test 3. The same accepted cases as in section 3 hold here: a `planned` entry may name a path not on disk, and no rule reads a heading.

## Not in this issue
- The check that every source file has a map line, the 400-byte line limit and the `#<number>` ban: owner ruling (2026-11-26), {{#m-guard}} turns them on, after every map section is rewritten. This issue checks only what holds on main today.
- The field rule for the CLI tree's status: {{#m-cli-mcp}} tightens it (section 4).
- A check that every `code_map_entries` path of a node spec file has a map line: it does not hold today (`roadmap/nodes/sky-roadmap-graph.json` names `app/src/sky/RoadmapView.tsx`, which has no entry line).
- Tests that name `docs/code-map.md` only as a string and never read the file stay as they are: `auto_weekly_run.rs` line 126, `gh_parse_repo.rs` line 84, `unit_land.rs` lines 55 and 74, and the fixture rows in `unit_proof.rs` (lines 155, 166, 176, 209).

Done when:
1. The 32 functions in the table are gone, and `git diff` of those 32 files shows only those deletions plus the two `use` edits and the three `manifest` helpers (with their `use` lines) named in section 1. No other test function in those files is added, removed or changed.
2. `git diff` of `crates/core/tests/unit_job.rs` shows a change only inside the body of `code_map()`.
3. `grep -rlE 'include_str!\("\.\./\.\./\.\./docs/code-map\.md"\)|join\("(\.\./\.\./)?docs/code-map\.md"\)' crates/core/tests crates/cli/tests` lists no file other than the two new `code_map_structure.rs` files (on main at 19e0c65 it lists the 33 files of sections 1 and 2).
4. `crates/core/tests/code_map_structure.rs` has the six tests named in section 3 and `crates/cli/tests/code_map_structure.rs` the two named in section 4, each file with its locked header, and all eight pass against the map as it is on main. The PR body lists R1-R6 and the eight tests, each with the line range of the test code that carries it, so the reviewer can check them line by line.
5. The new tests are shown to bite: for core tests 2, 3 and 4 the PR body gives a one-line change to `docs/code-map.md` in the working tree (a `crates/core/` path that is not on disk; `· done.` in place of `· present.` on an entry line; a `modules:` line ending in `(done)`) and the failing test output for each. It also shows the `planned` rule holds: with a new line `` `crates/core/src/not_yet/` · planned · x `` added to the map, core test 2 still passes. Each change is reverted and not committed.
6. `git diff --stat` shows no change to `docs/code-map.md` and none under `crates/core/src`, `crates/cli/src` or `app/`.
7. `tools/ci.sh rust` passes (fmt, clippy with `-D warnings`, `cargo test`); its output is in the PR body.
