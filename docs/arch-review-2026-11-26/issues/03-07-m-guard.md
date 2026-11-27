What: Turn on the last three checks in the structural code map tests, now that every section of `docs/code-map.md` is revised: an entry line over 400 bytes fails, an entry line containing `#<number>` fails, and every non-test source file in the four trees must have its own entry line. Until now the structural tests checked only that each listed path exists and each status word is valid.

Where: the four structural code map tests added by {{#t-core}}, {{#t-tauri}} and {{#t-react}}, one per tree: `crates/core/tests/code_map_structure.rs` (core), `crates/cli/tests/code_map_structure.rs` (CLI), `app/src-tauri/src/code_map_tests.rs` (Tauri), `app/src/code-map.test.ts` (React face). `docs/code-map.md` only if a check finds a miss (code map: the whole Modules part).

Owner ruling (2026-11-26): these three checks go into the structural tests in this last map issue. All four test files are locked tests; their headers say the builder never edits them. The owner ruling is the permission: the builder of this issue edits those four files, for the additions below and nothing else. The reviewer checks each edited test against the rules in this issue line by line. No other test file changes.

Definitions the checks use:
- The map text is `docs/code-map.md` read as each test file already reads it, with `\r\n` replaced by `\n`. A line that starts with three backticks opens or closes a fenced block; fence lines and the lines inside a fenced block are skipped by the two guard checks.
- An entry line is a line of the map that starts with a backticked path under `crates/` or `app/` (it starts with `` `crates/ `` or `` `app/ ``). Lines of the `modules:` and `reuses:` blocks start with `- ` and are not entry lines.
- A source file is a `.rs`, `.ts`, `.tsx` or `.css` file under `crates/core/src`, `crates/cli/src`, `app/src-tauri/src` or `app/src`, at any depth. Stylesheets count: the three `.css` files under `app/src` each need an entry line. Test files are left out: `*_tests.rs`, `*.test.ts`, `*.test.tsx`. Every other kind of file (fonts, `LICENSE`, `README.md`, `OFL.txt`) is left out.
- A file is listed when some line of the map starts with `` `<its repo-relative path>` `` (the backticked path, closing backtick included), the same rule `crates/core/src/unit/job.rs` `code_map_lines` uses. Paths are compared with `/` on every platform.

What to add. Each check is a function over its inputs (the map text; for the every-file check also the list of the tree's source file paths) that returns the list of failures, so one test can run it on the real map and tree and another on a small inline text. The test names are fixed:
1. Core test file, test `no_code_map_entry_line_is_over_400_bytes`: over every entry line of the map, in all four trees, fail when the line is longer than 400 bytes (`str::len`, bytes not characters; the line end is not counted), naming the line's path and its size. Test `the_400_byte_guard_bites` runs the same function on inline text (Done-when 1).
2. Core test file, test `no_code_map_entry_line_holds_an_issue_number`: over every entry line of the map, in all four trees, fail when the line contains `#` directly followed by an ASCII digit, naming the path. Test `the_issue_number_guard_bites` runs the same function on inline text (Done-when 2).
3. One test in each of the four files, for its own tree: find the tree's source files and fail naming every one that is not listed. Names: `every_core_source_file_has_a_code_map_line`, `every_cli_source_file_has_a_code_map_line`, `every_tauri_source_file_has_a_code_map_line`, and in the React face's test "every face source file has a code map line". The three Rust tests walk the tree on disk with std only (no new dependency), from the repo root each file already finds. The React face's test keeps the way it already finds files, `Object.keys(import.meta.glob("./**/*"))`, and does not import `node:fs`; a key `./<rest>` is the path `app/src/<rest>`. Beside each, one test runs the same function on inline text and an inline list of files (Done-when 3): `the_every_file_check_bites` in each Rust file, "the every-file check bites" in the React face's test.
4. Each failure message says what to do: shorten the line and move the detail to the file's header comment; remove the issue number (CHANGELOG holds history); add a line `` `path` · status · one sentence ``.

What the checks must allow:
- The every-file check goes from the disk to the map only. It never fails because the map has an entry line for a path that is not on disk. An entry whose status is `planned` may name a path not yet on disk (`crates/core/src/workloop/` is the one such line); the on-disk tests from {{#t-core}}, {{#t-tauri}} and {{#t-react}} already skip the paths on such a line, and those tests are not changed here.
- An entry line for a directory (a path ending in `/`) does not list the files under it; every file still needs its own line.

Unchanged in the four files: the header lines, the reading rules, and every test and helper already there. The CLI test's status rule was tightened to the field rule by {{#m-cli-mcp}}; it is not touched here.

Numbers on main at 19e0c65, before the section issues: 64 entry lines, 37 over 400 bytes, 43 with an issue number; source files per tree: `crates/core/src` 82 (55 listed), `crates/cli/src` 6 (3 listed), `app/src-tauri/src` 2 (0 listed; 9 `*_tests.rs` files left out), `app/src` 59, of them 3 stylesheets (2 listed; 44 test files left out). After the thirteen map issues before this one, all four trees are fully listed and no entry line fails; this issue proves it.

Keep working unchanged: the `modules:` block and its reader `crates/core/src/sky/codemap.rs` `modules` / `check_modules`, the `reuses:` block, the first `Updated:` line, and `code_map_lines`. No change under `crates/core/src`, `crates/cli/src`, `app/src-tauri/src` (other than `code_map_tests.rs`) or `app/src` (other than `code-map.test.ts`).

Size: Medium (about 200 changed lines in 4 test files, all added; Rust and Node toolchains; `tools/ci.sh app` needs Linux with the WebKitGTK system libraries that CI's app job installs).

Blocked by: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#t-core}}, {{#t-tauri}}, {{#t-react}}

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, in this order: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the last: it is built after {{#m-shell}}, when all thirteen other map issues and the three structural test issues have landed.

Done when:
1. `the_400_byte_guard_bites` shows, on a small inline map text and not by editing `docs/code-map.md`, that an entry line of 401 bytes is reported and one of 400 bytes is not, and that a 401-byte line that is not an entry line (a `- ` line, a line inside a fenced block) is not reported.
2. `the_issue_number_guard_bites` shows that an inline entry line containing `#123` is reported and one containing `#` with no digit after it (`#N`) is not.
3. In each of the four test files the every-file check is shown to bite on an inline map text and an inline list of files: a source file with no line is reported by its path; a `.css` file with no line is reported (React face); a test file (`x_tests.rs`, `x.test.tsx`) with no line is not; a file whose path only appears later in another file's line, not at the start of a line, is reported; an inline `planned` entry line whose path is not in the list of files reports nothing.
4. On the real `docs/code-map.md` and the real trees all the tests pass. If a file is found unlisted or a line fails the guard, the PR adds or shortens that line under the same rules as the section issues and says so in its body.
5. Both of these print nothing:
   ```
   LC_ALL=C awk '/^`(crates|app)\// && length($0) > 400' docs/code-map.md
   grep -nE '^`(crates|app)/.*#[0-9]' docs/code-map.md
   ```
6. `git diff main --stat` shows only the four test files and, where item 4 applies, `docs/code-map.md`. `git diff main` of the four test files shows added lines only: no test or helper that was there is changed or removed.
7. `tools/ci.sh rust` passes (core and CLI trees), `tools/ci.sh app` passes (Tauri tree), `tools/ci.sh face` passes (React face); the PR body carries all three outputs.
