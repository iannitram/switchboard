What: Rewrite the top of `docs/code-map.md` (header, Layout) and its eleven `core-*` sections so every source file has one short entry line, and move the behaviour detail that is cut into each file's header doc comment. This is one of the code map revision issues (owner ruling, 2026-11-26); it changes comments and docs only.

Where: `docs/code-map.md` lines 1-123 (code map: the header, `## Layout`, `## Platforms (R04)`, and `### core-settings` to `### core-lock`), the header comments of the files listed below, and `specs/roadmap-run.json` line 21.

Size: Medium (about 250 changed lines; Rust and Node toolchains for the proof, no code change). It touches up to 19 files: the map, one spec file, up to 11 headers under `crates/core/src/` and up to 6 comment-only edits where line 104's detail lands. That is over the 6-file aim. It cannot be avoided for a section group, so it is reviewed as an oversize change.

Blocked by: {{#t-core}}, {{#t-react}} (`app/src/views/Settings.test.tsx:148-154` reads the line under `### core-settings`; `crates/core/tests/abilities_index.rs`, `fleet_error_log.rs`, `job_log.rs`, `platform_seams.rs` and `rules_guard.rs` each want `· present.` in the line under a `core-*` heading).

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, each blocked by the one before it: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the first: no map issue lands before it, and {{#m-github}} is next.

Line numbers in this issue are those of the map on main at commit 19e0c65. No map issue has changed the map before this one; if another change has moved a line, find each section by its `###` heading.

## Rules for every map line in this issue (owner ruling, 2026-11-26)

- **Entry format.** Each source file has exactly one line, and it starts with the file's backticked repo-relative path: `` `path` · status · one sentence ``. Status is `present` or `planned`. The sentence says what the file owns, its Must ids, and the few public names a caller uses.
- **Limits.** An entry line is at most 400 bytes (UTF-8, newline not counted; `·` is 2 bytes) and holds no `#` followed by digits.
- **One path per line.** A line that names two files is split. The `- ...` bullets under a section are removed. "Tested by ..." tails are removed (Layout already says one test file per issue).
- **Where cut text goes.** Behaviour detail goes into the header doc comment of the file that holds the code (`//!` at the top of a Rust file, a `//` comment at the top of a TypeScript file), unless that header or the doc comment on the named item already says it. Fix history (issue numbers, dated notes, what it used to do) is dropped; `CHANGELOG.md` has it. In source files only comments change.
- **Leave alone.** The `reuses:` block and the `modules:` block (lines 9-26, read by `reuses`, `modules` and `check_modules` in `crates/core/src/sky/codemap.rs`), every existing `###` heading, and the form of the first `Updated:` line. Set its date to the day of the PR.
- `code_map_lines` in `crates/core/src/unit/job.rs` reads the first line that starts with a backticked path, so a path must start its own line.
- This issue writes the lines for its own 15 paths only. The later map issues write the rest; {{#m-shell}} lists the face files still without a line and finds `app/src/views/Settings.tsx` already done here.

## Header, Layout, Platforms (lines 1-83)

| Lines | What to do |
|---|---|
| 5 | Reword so it states the entry format above in one sentence. Keep the planned -> present rule. |
| 7 (776 bytes) | Do not touch. {{#m-runner}} fixes this line. |
| 9-26 | No change. |
| 28 | Drop `(#405)`. |
| 35 | Add `lib` to the file list (`crates/core/src/lib.rs` exists and is not named). |
| 39, 42, 43, 44 | Replace the prose with the file names, as lines 36, 37, 40 and 41 do. bug: `intake, fix, sweep`. runner: `spec, engine, state, waiting, roadmap_run, bug_loop, score` (line 42 omits the last three today). roadmap: `graph, units, drift`. unit: `proof, frontier, claim, job, land` (line 44 is 372 bytes today). |
| 58-60 | Fix the three paths in the diagram: `app/src-tauri/src/commands.rs`, `crates/cli/src/cli.rs`, `crates/cli/src/mcp.rs` (each lacks `src/` today). Shorten the dashes so the `+` at the end of each of the three lines stays in one column. |
| 69-83 | No change. This section is the one home of the platform plan; {{#s-dispositions}} edits its first sentence and adds one sentence after its table, and is not blocked by this issue; if it has landed, keep both edits. |

Also correct `specs/roadmap-run.json` line 21 (the `code_map_entry` record): its `lives_in` text says the line is `- <path glob>: <status> (node: <id>)`. The real form, read by `modules` in `crates/core/src/sky/codemap.rs`, is `- <node>: <path> (<status>)`. Change only that quoted form inside `lives_in`; leave the record's `fields` as they are. This is the one issue that corrects that line; no other map issue touches `specs/roadmap-run.json`.

## core-* sections (lines 86-123, 9,296 bytes)

Entry lines over 400 bytes today: 90 (785), 93 (644), 96 (520), 102 (582), 104 (2,567), 116 (426), 122 (1,678). Lines with issue numbers: 93, 102, 104, 105, 113, 116, 119, 122.

| Map line | Section | Becomes the entry for | Cut detail goes to the header of |
|---|---|---|---|
| 87 (332 B) | core-settings | `crates/core/src/settings.rs`, plus a new line for `app/src/views/Settings.tsx` (named inside line 87 today) | `settings.rs` (1 header line today) |
| 90 | core-fleet | `crates/core/src/fleet.rs` | `fleet.rs` (4 header lines today): the stub variables, `clean_child_env`, `clean_ld_library_path`. Two names in line 90 are easy to misplace: `FleetErrorLog` is in `fleet.rs`, and `FleetStarter` is in `crates/core/src/autopilot/passes.rs`, not in `fleet.rs`. |
| 93 | core-fleet-errors | `crates/core/src/fleet_errors.rs` | `fleet_errors.rs` (9 header lines today). `FLEET_ERROR_LOG_CAP` is defined in `fleet.rs` and used here. |
| 96 | core-abilities | `crates/core/src/abilities.rs` | `abilities.rs` (12 header lines today) |
| 99 (359 B) | core-joblog | `crates/core/src/joblog.rs` | reformat only |
| 102 | core-rules | `crates/core/src/rules.rs` | `rules.rs` (2 header lines today) |
| 104 (bullet) | core-rules | a new line for `crates/core/src/confirm.rs` (no line of its own today) | `confirm.rs` first (14 header lines today). A fact about another file goes to that file's header, comment only: `crates/core/src/github/archive.rs` (`archive_repo`), `crates/core/src/backup.rs` (`Marks`, `Made`, `restore_to_new`), `crates/core/src/waiting.rs` (`shown`), `crates/cli/src/cli.rs` (`confirm_command`), `crates/cli/src/mcp.rs` (`guarded_request`), `app/src/views/Waiting.tsx` (`shownItem`). Those files' own map lines are written by later map issues: `archive.rs` in {{#m-github}}, `backup.rs` in {{#m-backup}}, `waiting.rs` and `Waiting.tsx` in {{#m-bug-waiting-reports}}, `cli.rs` and `mcp.rs` in {{#m-cli-mcp}}. Do not write them here. |
| 105 (bullet) | core-rules | folded into the `rules.rs` line (`guard_front_door`) | `rules.rs` |
| 108 | core-platform | `crates/core/src/platform.rs` | reformat only |
| 111, 113 | core-clock, core-error, core-error-code | one line for `crates/core/src/clock.rs`, one for `crates/core/src/error.rs` (line 111 names both files; line 113 is a second line about `error.rs`) | `error.rs` (2 header lines today) |
| 116 | core-faces | `crates/core/src/faces.rs` | `faces.rs` (4 header lines today) |
| 119 (382 B) | core-parity | `crates/core/src/parity.rs` | `parity.rs` has no header comment today: add one |
| 122 | core-lock | `crates/core/src/lock.rs` | `lock.rs` (17 header lines today): the lock file fields, takeover rules and retry counts |
| new | add `### core-lib` after the `core-lock` section | `crates/core/src/lib.rs` (no map line today) | `lib.rs` lines 4-6 say the crate is a skeleton of `todo!` stubs, which is no longer true (no `todo!` is left in `crates/core/src`): rewrite those three comment lines. Leave lines 8-9 (the `#![allow(...)]` and its comment) as they are. |

Source files in this area with no line of their own today: `crates/core/src/confirm.rs`, `crates/core/src/lib.rs`, `app/src/views/Settings.tsx`. After this issue the area has 15 entry lines.

Done when:
1. Each of these 15 paths starts exactly one line of `docs/code-map.md`, in the entry format: under `crates/core/src/`, `settings.rs`, `fleet.rs`, `fleet_errors.rs`, `abilities.rs`, `joblog.rs`, `rules.rs`, `confirm.rs`, `platform.rs`, `clock.rs`, `error.rs`, `faces.rs`, `parity.rs`, `lock.rs`, `lib.rs`; and `app/src/views/Settings.tsx`.
2. Each of these three commands prints nothing (entry lines over 400 bytes, entry lines with an issue number, bullets left in the core sections):
   ```
   sed -n '/^### core-settings/,/^### github/p' docs/code-map.md | LC_ALL=C awk '/^`/ && length($0) > 400'
   sed -n '/^### core-settings/,/^### github/p' docs/code-map.md | grep -E '^`.*#[0-9]'
   sed -n '/^### core-settings/,/^### github/p' docs/code-map.md | grep '^- '
   ```
3. Every function, type or constant an entry line names exists in the file that line names.
4. Layout lines 35, 39, 42, 43, 44 and diagram lines 58-60 read as the table above says; line 7 and lines 9-26 are unchanged in the diff.
5. `specs/roadmap-run.json` line 21 gives the modules line as `- <node>: <path> (<status>)`, `grep -n 'path glob' specs/roadmap-run.json` prints nothing, and the file still parses as JSON.
6. `crates/core/src/parity.rs` starts with a `//!` header, and `crates/core/src/lib.rs` lines 4-6 no longer say the crate is a skeleton.
7. The PR body lists each fact cut from the map with where it is now: the file and "already there" or "added", or "dropped as history".
8. `git diff --stat` shows only `docs/code-map.md`, `specs/roadmap-run.json` and source files, and every source hunk is a comment.
9. The first `Updated:` line has the PR's date.
10. `tools/ci.sh rust` and `tools/ci.sh face` pass; their output is in the PR body.
