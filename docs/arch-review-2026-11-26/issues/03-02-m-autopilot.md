What: Rewrite the `### autopilot` section of `docs/code-map.md` to one short line per source file, and move the behaviour detail cut from the map into each file's `//!` header comment. Today the section is a directory line, two file paragraphs and 18 bullets; 7 of the 9 files have no line of their own.

Where: `docs/code-map.md` (code map: section `### autopilot`) and the 9 files of `crates/core/src/autopilot/`.

Map lines today (main at 19e0c65; sizes in bytes, line ends not counted): the heading is line 274; the section has 21 non-blank lines after it, 4685 bytes. Line 275 is the directory line (334 bytes), 277 is `run.rs` (1936 bytes), 278 is `loops.rs` (495 bytes), 279 to 296 are 18 bullets. Over 400 bytes: 277 and 278. Lines 277, 278 and 287 carry issue numbers, 7 numbers in all.

Source files that get a line and whose header comment receives the detail (all 9 have a `//!` header). The bullet numbers say where each bullet's names are defined (checked on main at 19e0c65):
- `crates/core/src/autopilot/run.rs` (has line 277; its 1936 bytes of detail go to its header)
- `crates/core/src/autopilot/loops.rs` (has line 278)
- `crates/core/src/autopilot/mod.rs`: no line yet (bullet 279, `may_spend`)
- `crates/core/src/autopilot/classify.rs`: no line yet (280)
- `crates/core/src/autopilot/driver.rs`: no line yet (281, 282)
- `crates/core/src/autopilot/weekly.rs`: no line yet (283, 295)
- `crates/core/src/autopilot/passes.rs`: no line yet (284, 285, 286, 287, 291, 292, 293, 296)
- `crates/core/src/autopilot/instructions.rs`: no line yet (288, 294)
- `crates/core/src/autopilot/review.rs`: no line yet (289, 290)

Rules for the rewrite (owner ruling, 2026-11-26; none of this is open):
1. Each of the 9 files gets exactly one line that starts with its backticked repo-relative path, in the form `` `path` · status · one sentence `` (the separator is space, U+00B7 MIDDLE DOT, space). Status is `present` or `planned`; all 9 are `present`. The sentence says what the file owns, its Must ids (AUTO-1 to AUTO-9) and the few public names a caller uses. The line is at most 400 bytes and holds no `#<number>`.
2. The directory line 275 and all 18 bullets go. For each sentence cut: if the file's header comment or an item's doc comment already says it, drop it; if not, add it to the `//!` header of the file it describes. Fix history (which issue changed what) is dropped; CHANGELOG has it.
3. Two names are defined outside this directory: `rules::guard_autopilot` (`crates/core/src/rules.rs:144`) and `FleetModels::read_qwen_server_url` (`crates/core/src/abilities.rs:77`). Detail about them stays a short mention in the autopilot file that uses them; do not edit those two files. `PM_RETRY_WAIT` is defined here (`crates/core/src/autopilot/passes.rs:1021`); its detail goes to the header of `passes.rs`.
4. A backticked path in a new line that starts with `crates/core/`, `crates/cli/`, `app/src-tauri/` or `app/src/` must be on disk; the structural code map tests check it.
5. Unchanged: the `### autopilot` heading, the `reuses:` block, the `modules:` block, the Layout block. Set the first `Updated:` line to the day of the PR (leave it if main already has that day or later).
6. Source files change in comments only. No code changes.

Size: Medium (about 130 changed lines; Rust toolchain for the checks; any platform). It touches 10 files (the map and 9 header comments), over the usual 6, because the owner ruling puts a section's lines and its files' header comments in one PR; it is reviewed as an oversize change. Every change is a comment or a doc line.

Blocked by: {{#m-frontdoor}}, {{#t-core}}

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, in this order: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is built after {{#m-frontdoor}}. The line numbers in this issue are those of the map on main at 19e0c65; the earlier map issues move them, so find the section by its `### autopilot` heading and each line by the words it starts with.

{{#s-musts}} is not in this order and does not block this issue. It adds to the headers of `review.rs` and `passes.rs`. If it has landed, add to each header as it stands and do not repeat a sentence it already has.

Five tests under `crates/core/tests` read the map for this section today (`auto_after_poll_review.rs`, `auto_driver.rs`, `auto_merge_pass.rs`, `auto_roadmap_pass.rs`, `auto_weekly_run.rs`); {{#t-core}} removes those assertions.

Done when:
1. This prints 9 (one line per file; the directory line is gone):
   ```
   grep -c '^`crates/core/src/autopilot/' docs/code-map.md
   ```
2. No such line is over 400 bytes; this prints nothing:
   ```
   LC_ALL=C awk '/^`crates\/core\/src\/autopilot\// && length($0) > 400' docs/code-map.md
   ```
3. No such line holds an issue number; this prints nothing:
   ```
   grep -n '^`crates/core/src/autopilot/.*#[0-9]' docs/code-map.md
   ```
4. Between the `### autopilot` heading and the `### runner` heading the map holds only those 9 lines and blank lines.
5. `git diff main -- docs/code-map.md` shows no change inside the `reuses:` and `modules:` blocks.
6. The PR body lists, for lines 277 and 278 and each bullet that held more than names, where its detail went (header comment of which file, already there, or dropped as history).
7. `git diff main --stat` shows only `docs/code-map.md` and files under `crates/core/src/autopilot/`, and no non-comment line changed in a `.rs` file.
8. `tools/ci.sh rust` passes (rustfmt over the edited headers, and the structural code map tests).
