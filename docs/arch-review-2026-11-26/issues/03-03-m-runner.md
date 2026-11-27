What: Rewrite the `### runner`, `### roadmap`, `### unit` and `### workloop` sections of `docs/code-map.md` to one short line per source file, and move the behaviour detail cut from the map into each file's `//!` header comment. In the same PR, fix the status paragraph on map line 7, which says three runner nodes "remain planned" while lines 24-26 and 301, 303, 305 say they are present.

Where: `docs/code-map.md` (code map: sections `### runner`, `### roadmap`, `### unit`, `### workloop`, and line 7), the 18 source files below, and `docs/work-loop.md`. `specs/roadmap-run.json` is not edited here: {{#m-core}} corrects its line 21.

Map lines today (main at 19e0c65; sizes in bytes, line ends not counted):
- Line 7, the "Status today" paragraph: 776 bytes, three issue numbers.
- `### runner`, heading on line 298: 8 entry lines (299, 301, 303, 305, 307, 309, 311, 313), 4253 bytes. Over 400 bytes: 303 (401), 307 (515), 309 (1227), 313 (883). All 8 carry an issue number.
- `### roadmap`, heading on line 315: 4 entry lines (316, 318, 320, 322), 2473 bytes. Over 400: 318 (588), 320 (725), 322 (787). All 4 carry an issue number.
- `### unit`, heading on line 324: 6 entry lines (325, 327, 329, 331, 333, 335), 6814 bytes. Over 400: 327 (756), 329 (2913), 331 (1175), 333 (1168), 335 (680). All 6 carry an issue number.
- `### workloop`, heading on line 337: 1 entry line (338, 732 bytes), 6 bullets (340 to 345) and a "Talks to" paragraph (347, 308 bytes), 2109 bytes. No issue numbers.

Source files whose header comment receives the detail (every one already has a `//!` header and already has a map line):
- `crates/core/src/runner/`: `mod.rs`, `spec.rs`, `engine.rs`, `state.rs`, `waiting.rs`, `roadmap_run.rs`, `bug_loop.rs`, `score.rs` (8 files)
- `crates/core/src/roadmap/`: `mod.rs`, `graph.rs`, `units.rs`, `drift.rs` (4 files)
- `crates/core/src/unit/`: `mod.rs`, `frontier.rs`, `claim.rs`, `job.rs`, `land.rs`, `proof.rs` (6 files)

Source files with no map line yet: none in these sections. `crates/core/src/workloop/` does not exist; it keeps one line with status `planned`. Do not create the directory.

Rules for the rewrite (owner ruling, 2026-11-26; none of this is open):
1. Each of the 18 files gets exactly one line that starts with its backticked repo-relative path, in the form `` `path` · status · one sentence `` (the separator is space, U+00B7 MIDDLE DOT, space). Status is `present` or `planned`; all 18 are `present`. The sentence says what the file owns, its node or Must ids, and the few public names a caller uses. The line is at most 400 bytes and holds no `#<number>`.
2. Every other paragraph and bullet in the four sections goes. For each sentence cut: if the file's header comment or an item's doc comment already says it, drop it; if not, add it to the `//!` header of the file it describes. Fix history (which issue changed what) and lists of test cases (the tail of line 329) are dropped; CHANGELOG and the tests hold them.
3. Each file's line must stay the first line in the map that starts with that backticked path. `crates/core/src/unit/job.rs` `code_map_lines` pastes that line into a unit's brief, and the node spec files under `roadmap/nodes/` name all 18 files in `code_map_entries` (`runner/state.rs` is named by `roadmap/nodes/runner-engine.json`).
4. A backticked path in a new line that starts with `crates/core/`, `crates/cli/`, `app/src-tauri/` or `app/src/` must be on disk; the structural code map tests check it. The one exception is the `crates/core/src/workloop/` line: an entry whose status is `planned` may name a path that is not yet on disk, and the structural tests skip the paths on such a line.
5. Unchanged: the four section headings, the `reuses:` block, the `modules:` block, the Layout and Platforms blocks. Set the first `Updated:` line to the day of the PR (leave it if main already has that day or later).
6. Source files change in comments only. No code changes.
7. `### workloop`: line 338 becomes one line starting `` `crates/core/src/workloop/` · planned · `` within 400 bytes. Its sentence does not hold the word `present`. The six bullets (`backlog`, `run`, `packet`, `judge`, `progress`, `handoff`) and the "Talks to" paragraph leave the map; whatever `docs/work-loop.md` does not already say is added there as a short "Proposed modules" list at the end of its section 6.
8. Line 7 (the paragraph that starts `Status today:`) becomes exactly this sentence, with no date, no issue numbers and no list of what landed:
   ```
   Status: `crates/core/src/workloop/` (`local-only`, after 1.0) is **planned**; every other module below is **present**. Each file's line carries its status.
   ```
   The word order is fixed. The core structural test from {{#t-core}} takes a line's status to be the first whole word `present` or `planned` on it, and checks every `crates/core/` path on a line unless that status is `planned`. With `planned` first, the `workloop/` path, which is not on disk, is not checked.
9. Do not edit `specs/roadmap-run.json`. Its line 21 describes the `modules:` line wrongly; that is corrected in exactly one issue, {{#m-core}}.

Size: Hard (about 250 changed lines; Rust toolchain for the checks; any platform). This is an oversize change and is reviewed as one: it touches 20 files (the map, 18 source headers, `docs/work-loop.md`), over the usual 6, because the owner ruling puts a section's lines and its files' header comments in one PR. Every change is a comment or a doc line.

Blocked by: {{#m-autopilot}}, {{#t-core}}

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, in this order: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is built after {{#m-autopilot}}. The line numbers in this issue are those of the map on main at 19e0c65; the earlier map issues move them, so find each section by its `###` heading and each line by the path or words it starts with. {{#m-core}} leaves the status paragraph (line 7) to this issue.

{{#t-core}} also gives `crates/core/tests/unit_job.rs` a fixed map text, so the real line of `job.rs` is free to change.

Done when:
1. These print 8, 4, 6 and 1:
   ```
   grep -c '^`crates/core/src/runner/' docs/code-map.md
   grep -c '^`crates/core/src/roadmap/' docs/code-map.md
   grep -c '^`crates/core/src/unit/' docs/code-map.md
   grep -c '^`crates/core/src/workloop/' docs/code-map.md
   ```
2. No such line is over 400 bytes; this prints nothing:
   ```
   LC_ALL=C awk '/^`crates\/core\/src\/(runner|roadmap|unit|workloop)\// && length($0) > 400' docs/code-map.md
   ```
3. No such line holds an issue number; this prints nothing:
   ```
   grep -nE '^`crates/core/src/(runner|roadmap|unit|workloop)/.*#[0-9]' docs/code-map.md
   ```
4. Between the `### runner` heading and the `### it` heading the map holds only the `### roadmap`, `### unit` and `### workloop` headings, those 19 lines and blank lines.
5. `grep -n 'remain planned' docs/code-map.md` prints nothing, and the status paragraph is the sentence in rule 8, word for word.
6. `git diff main -- docs/code-map.md` shows no change inside the `reuses:` and `modules:` blocks.
7. `git diff main --stat` does not show `specs/roadmap-run.json`.
8. The PR body lists, for each of the 13 oversize entry lines (12 file lines and line 338) and the workloop bullets, where its detail went (header comment of which file, `docs/work-loop.md`, already there, or dropped as history).
9. `git diff main --stat` shows only `docs/code-map.md`, `docs/work-loop.md` and files among the 18 listed, and no non-comment line changed in a `.rs` file.
10. `tools/ci.sh rust` passes (it runs `cargo fmt --check` over the edited headers and the tests, among them the structural code map tests, `unit_job`, `unit_proof`, `unit_land` and `runner_spec`).
