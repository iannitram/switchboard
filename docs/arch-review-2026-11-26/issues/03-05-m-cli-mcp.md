What: Rewrite the `### cli` and `### mcp` sections of `docs/code-map.md` to one short line per source file of `crates/cli/src`, and move the behaviour detail cut from the map into each file's `//!` header comment. Today three of the six files have no line of their own, two files share one line, and two paragraphs start with a command instead of a path. In the same PR, tighten the status rule of the CLI tree's structural code map test to the field rule the other three trees already use.

Where: `docs/code-map.md` (code map: sections `### cli` and `### mcp`), the 6 files of `crates/cli/src/`, and the locked test `crates/cli/tests/code_map_structure.rs` (one test function, see "The CLI structural test" below).

Map lines today (main at 19e0c65; sizes in bytes, line ends not counted):
- `### cli`, heading on line 362: 9 non-blank lines after it, 3895 bytes. Lines 363 and 365 are command paragraphs (`switchboard spec validate`, `switchboard roadmap lint`) that start with a backtick but not a path; 367 is the line for `crates/cli/src/main.rs` and `cli.rs` together (797 bytes); 368 to 373 are 6 bullets. Over 400 bytes: 367 (797), 368 (557), 369 (439), 372 (441), 373 (565). Lines 363, 365, 370 and 373 carry issue numbers.
- `### mcp`, heading on line 375: 5 non-blank lines after it, 1956 bytes. 376 is `mcp.rs` (475 bytes), 378 is `autopilot.rs` (815 bytes), 379 to 381 are 3 bullets. Over 400: 376 and 378. Lines 376, 378 and 381 carry issue numbers.

Source files that get a line and whose header comment receives the detail (all 6 have a `//!` header):
- `crates/cli/src/main.rs` (368 lines; shares line 367 today)
- `crates/cli/src/cli.rs` (2740 lines): no line of its own yet; receives 363, 365, the `cli.rs` part of 367, and 368 to 372
- `crates/cli/src/cli_frontdoor.rs` (339 lines): no line yet; receives 373
- `crates/cli/src/mcp.rs` (800 lines; line 376); receives 379, 380
- `crates/cli/src/mcp_frontdoor.rs` (170 lines): no line yet; receives 381
- `crates/cli/src/autopilot.rs` (476 lines; line 378)

Rules for the rewrite (owner ruling, 2026-11-26; none of this is open):
1. Each of the 6 files gets exactly one line that starts with its backticked repo-relative path, in the form `` `path` · status · one sentence `` (the separator is space, U+00B7 MIDDLE DOT, space). Status is `present` or `planned`; all 6 are `present`. The sentence says what the file owns, its Must ids (CLI-1 to CLI-3, MCP-1 to MCP-3) and the commands or tools it carries, by name only. The line is at most 400 bytes and holds no `#<number>`.
2. `main.rs`, `cli.rs` and `cli_frontdoor.rs` sit under `### cli`; `mcp.rs`, `mcp_frontdoor.rs` and `autopilot.rs` sit under `### mcp`, where `autopilot.rs` is today.
3. The two command paragraphs, the joint line 367 and all 9 bullets go. For each sentence cut (exit codes, environment variables such as `SWITCHBOARD_DATA`, `SWITCHBOARD_CONFIG` and `SWITCHBOARD_PIPELINE`, output shapes, flags): if the file's header comment or an item's doc comment already says it, drop it; if not, add it to the `//!` header of the file it describes. Fix history (which issue changed what) is dropped; CHANGELOG has it.
4. Do not describe `trait Writes` (`crates/cli/src/cli.rs:89`) or `trait WriteCore` (`crates/cli/src/mcp.rs:40`) in the new lines or headers beyond the file's purpose: {{#w-core}}, {{#w-cli}} and {{#w-mcp}} move the guarded writes into the core, and those traits shrink or go.
5. A backticked path in a new line that starts with `crates/core/`, `crates/cli/`, `app/src-tauri/` or `app/src/` must be on disk; the structural code map tests check it.
6. Unchanged: both headings, the `reuses:` block, the `modules:` block, the Layout block. Set the first `Updated:` line to the day of the PR (leave it if main already has that day or later).
7. Source files change in comments only. No code changes under `crates/cli/src/`.
8. `crates/core/src/confirm.rs` names `confirm_command` (`cli.rs`) and `guarded_request` (`mcp.rs`); {{#m-core}} may already have added a comment about them to the headers of `cli.rs` and `mcp.rs`. Add to each header as it stands and do not repeat a sentence it already has.

The CLI structural test (owner ruling, 2026-11-26). `crates/cli/tests/code_map_structure.rs` is a locked test added by {{#t-core}}; its header says the builder never edits it. The owner ruling is the permission: the builder of this issue edits that file, for the one change below and nothing else. The reviewer checks the edited test against these rules line by line.
- Why: {{#t-core}} gave the CLI tree a loose status rule (the line holds the whole word `present` or `planned` somewhere), because map lines 367 and 376 write the status inside a sentence ("CLI-1, CLI-2 and CLI-3 present.", "MCP-1 to MCP-3 present."). After the rewrite above, every CLI entry line carries its status as a field, so the CLI tree takes the same field rule as the core, Tauri and React trees.
- The change: the test function `every_cli_entry_has_a_status` is replaced by one named `every_cli_entry_has_a_valid_status`. An entry line is, as in that file today, a line outside a fenced block that starts with a backtick and whose first backticked span starts with `crates/cli/`. The test splits each entry line on the separator (space, U+00B7 MIDDLE DOT, space). The line passes when at least one field after the first begins with `present` or `planned` and the character after that word, if there is one, is not an ASCII letter, digit or underscore. The test fails if the map has no CLI entry line. The failure message lists every failing line with its line number.
- Unchanged in that file: the header lines, the reading rules, the test `every_cli_path_in_the_code_map_is_on_disk` and every helper it uses. A helper used only by the removed function goes with it (clippy runs with `-D warnings`). No other test file changes.

Size: Medium (about 125 changed lines; Rust toolchain for the checks; any platform). It touches 8 files (the map, 6 header comments and one test file), over the usual 6, because the owner ruling puts a section's lines and its files' header comments in one PR; it is reviewed as an oversize change. Outside the one test function, every change is a comment or a doc line.

Blocked by: {{#m-it}}, {{#t-core}}

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, in this order: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is built after {{#m-it}}. The line numbers in this issue are those of the map on main at 19e0c65; the earlier map issues move them, so find each section by its `###` heading and each line by the path or words it starts with.

No test pins the wording of these two sections today (nothing under `crates/cli/` reads the code map on main at 19e0c65). {{#t-core}} adds the CLI tree's structural test, `crates/cli/tests/code_map_structure.rs`, which this issue edits.

Done when:
1. This prints 6, one line for each of the 6 files:
   ```
   grep -c '^`crates/cli/src/' docs/code-map.md
   ```
2. No such line is over 400 bytes; this prints nothing:
   ```
   LC_ALL=C awk '/^`crates\/cli\/src\// && length($0) > 400' docs/code-map.md
   ```
3. No such line holds an issue number and no command paragraph is left; both print nothing:
   ```
   grep -n '^`crates/cli/src/.*#[0-9]' docs/code-map.md
   grep -n '^`switchboard ' docs/code-map.md
   ```
4. Between the `### cli` heading and the `### shell` heading the map holds only the `### mcp` heading, those 6 lines and blank lines.
5. `git diff main -- docs/code-map.md` shows no change inside the `reuses:` and `modules:` blocks.
6. The PR body lists, for each of the 7 oversize lines (367, 368, 369, 372, 373, 376, 378), where its detail went (header comment of which file, already there, or dropped as history).
7. `git diff main --stat` shows only `docs/code-map.md`, files under `crates/cli/src/` and `crates/cli/tests/code_map_structure.rs`, and no non-comment line changed in a file under `crates/cli/src/`.
8. `crates/cli/tests/code_map_structure.rs` has the test `every_cli_entry_has_a_valid_status` with the rule above and no test named `every_cli_entry_has_a_status`; `git diff main` of that file shows no change to its header or to `every_cli_path_in_the_code_map_is_on_disk`. The test passes on the rewritten map.
9. The tightened test is shown to bite: the PR body gives two one-line changes to `docs/code-map.md` in the working tree, each with the failing test output: `· done ·` in place of `· present ·` on the `crates/cli/src/main.rs` line; and that line with its status field taken out and the words `CLI-1 present.` put at the end of its sentence (the form the loose rule let through). Each change is reverted and not committed.
10. `tools/ci.sh rust` passes (rustfmt over the edited headers, the structural code map tests, and the CLI and MCP tests); its output is in the PR body.
