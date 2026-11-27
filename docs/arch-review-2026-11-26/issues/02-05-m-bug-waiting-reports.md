What: Rewrite the `bug`, `waiting` and `reports` sections of `docs/code-map.md` so each of their nine source files has one short entry line, remove the 15 bullets, and move the behaviour detail that is cut into each file's header doc comment. One of the code map revision issues (owner ruling, 2026-11-26); comments and docs only.

Where: `docs/code-map.md` lines 174-204 (code map: `### bug`, `### waiting`, `### reports`; 4,868 bytes), and the header comments of the files listed below.

Size: Medium (about 120 changed lines; Rust and Node toolchains for the proof, no code change). It touches up to 10 files (the map and nine source files), over the 6-file aim. That cannot be avoided for a section group, so it is reviewed as an oversize change.

Blocked by: {{#m-sky-face}}, {{#t-core}}, {{#t-react}} (`Waiting.test.tsx:136`, `Reports.test.tsx:212`, `ReportsFilter.test.tsx:169` and `WorkTree.test.tsx:71` under `app/src/views/` each expect one of these bullets word for word, as do `waiting_load.rs`, `waiting_local.rs`, `waiting_snapshot.rs`, `reports_build.rs`, `reports_filter.rs`, `reports_load.rs` and `reports_moves.rs` under `crates/core/tests/`).

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, each blocked by the one before it: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the fifth: the four before it have landed and {{#m-budget}} is next.

Line numbers in this issue are those of the map on main at commit 19e0c65. The four earlier map issues have rewritten the lines above these three sections, so the numbers have moved; they leave the three sections as they are. Find each section by its `###` heading. Header line counts below are also as at 19e0c65: {{#m-core}} may have added a comment about `shown` to the header of `crates/core/src/waiting.rs` and about `shownItem` to the top of `app/src/views/Waiting.tsx`. Keep what it added and build on it.

## Rules for every map line in this issue (owner ruling, 2026-11-26)

- **Entry format.** Each source file has exactly one line, and it starts with the file's backticked repo-relative path: `` `path` · status · one sentence ``. Status is `present` or `planned`. The sentence says what the file owns, its Must ids, and the few public names a caller uses.
- **Limits.** An entry line is at most 400 bytes (UTF-8, newline not counted; `·` is 2 bytes) and holds no `#` followed by digits.
- **One path per line.** A line that names two files is split. The `- ...: present.` bullets are removed; the names they list go into the file's one sentence only where a caller uses them. "Tests: ..." tails are removed.
- **Where cut text goes.** Behaviour detail goes into the header doc comment of the file that holds the code (`//!` at the top of a Rust file, a `//` comment at the top of a TypeScript file), unless that header or the doc comment on the named item already says it. Fix history (issue numbers, dated notes) is dropped; `CHANGELOG.md` has it. In source files only comments change.
- **Leave alone.** The `reuses:` block and the `modules:` block (lines 9-26; lines 19-21 list `bug/fix.rs`, `bug/mod.rs` and `crates/core/tests/bug_fix.rs` under the node `bug-fix`, and `crates/core/tests/bug_fix.rs:418-435` checks them), every existing `###` heading, and the form of the first `Updated:` line. Set its date to the day of the PR.
- `roadmap/nodes/bug-intake.json`, `bug-fix.json` and `bug-sweep.json` name the four bug files as code map entries, and `code_map_lines` in `crates/core/src/unit/job.rs` finds each by a line that starts with its backticked path. All four keep such a line.
- No earlier map issue writes a line for these nine paths. {{#m-shell}} later lists the face files still without a line and finds the three views done here.

## The lines

| Map line | Bytes | Becomes the entry for | Cut detail goes to the header of |
|---|---|---|---|
| 175 | 213 | `crates/core/src/bug/mod.rs` | reformat; drop the issue number (4 header lines today) |
| 177 | 909 | `crates/core/src/bug/intake.rs` | `intake.rs` (3 header lines today): what each of the seven steps does |
| 179 | 1,011 | `crates/core/src/bug/fix.rs` | `fix.rs` (8 header lines today): the order of `next`, the `job` rules, `attempts` and `park`, `lesson_check`, `signature_check` |
| 181 | 637 | `crates/core/src/bug/sweep.rs` | `sweep.rs` (5 header lines today) |
| 184 | 478 | `crates/core/src/waiting.rs` and a new line for `app/src/views/Waiting.tsx` (line 184 names both) | `waiting.rs` (3 header lines today): the item sources, `LocalItems` and its file and lock |
| 186-192 | 7 bullets | removed | `waiting.rs`, or `Waiting.tsx` for the view names (`WaitingList`, `Waiting`) |
| 195 | 223 | `crates/core/src/reports.rs` and a new line for `app/src/views/Reports.tsx` (line 195 names both) | `reports.rs` (2 header lines today) |
| 197-203 | 7 bullets | removed | `reports.rs`, or `Reports.tsx` for `ReportList`, `FilterBar`, `Reports` |
| 204 | 504, a bullet with five issue numbers | removed | `reports.rs`: `thread_rows`, `thread_rows_checked` and its `ThreadWarning`; a done card or ledger file that is not UTF-8 is skipped |
| new | | `app/src/views/WorkTree.tsx` (named only as a word in line 199) | none |

Lines over 400 bytes today: 177, 179, 181, 184, 204. Lines with issue numbers: 175, 177, 179, 181, 184, 204.

Source files in this area with no line of their own today: `app/src/views/Waiting.tsx`, `app/src/views/Reports.tsx`, `app/src/views/WorkTree.tsx`. None of the three has a top comment at 19e0c65 (`Waiting.tsx` may have one from {{#m-core}}); one is needed only where cut detail lands.

Two names in the bullets are not in these files: `WaitingItem` (line 188) is in `app/src/brocrew/WaitingItem.tsx` and `ReportEntry` (line 200) is in `app/src/brocrew/ReportEntry.tsx`. Do not give them as names of `Waiting.tsx` or `Reports.tsx`.

Not this issue's: the files under `app/src/brocrew/` get their lines in {{#m-shell}}, and `crates/core/src/runner/waiting.rs` gets its line in {{#m-runner}}.

The three notes "the CLI faces are not part of this file" (lines 177, 179, 181) leave the map. If a bug file's header does not already say the file has no face, add that one sentence to it.

Done when:
1. Each of these nine paths starts exactly one line of `docs/code-map.md`, in the entry format: `crates/core/src/bug/mod.rs`, `crates/core/src/bug/intake.rs`, `crates/core/src/bug/fix.rs`, `crates/core/src/bug/sweep.rs`, `crates/core/src/waiting.rs`, `app/src/views/Waiting.tsx`, `crates/core/src/reports.rs`, `app/src/views/Reports.tsx`, `app/src/views/WorkTree.tsx`.
2. Each of these three commands prints nothing (entry lines over 400 bytes, entry lines with an issue number, bullets left in the three sections):
   ```
   sed -n '/^### bug$/,/^### budget$/p' docs/code-map.md | LC_ALL=C awk '/^`/ && length($0) > 400'
   sed -n '/^### bug$/,/^### budget$/p' docs/code-map.md | grep -E '^`.*#[0-9]'
   sed -n '/^### bug$/,/^### budget$/p' docs/code-map.md | grep '^- '
   ```
3. Every function, type or constant an entry line names exists in the file that line names.
4. The PR body lists each fact cut from the map with where it is now (the file and "already there" or "added") or "dropped as history".
5. `git diff --stat` shows only `docs/code-map.md` and the nine source files, and every source hunk is a comment. Lines 9-26 of the map are unchanged.
6. The first `Updated:` line has the PR's date.
7. `tools/ci.sh rust` and `tools/ci.sh face` pass; their output is in the PR body.
