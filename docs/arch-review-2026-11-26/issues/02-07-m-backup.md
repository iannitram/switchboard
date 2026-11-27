What: Rewrite the `backup` section of `docs/code-map.md` so `backup.rs`, `backup_run.rs` and `renumber.rs` each have one short entry line, remove the ten bullet lines, and move the behaviour detail that is cut into those files' header doc comments. One of the code map revision issues (owner ruling, 2026-11-26); comments and docs only.

Where: `docs/code-map.md` lines 220-232 (code map: `### backup`; 4,034 bytes), and the `//!` headers of `crates/core/src/backup.rs`, `crates/core/src/backup_run.rs` and `crates/core/src/renumber.rs`.

Size: Medium (about 120 changed lines in 4 files; Rust and Node toolchains for the proof, no code change). The file count is small, but `backup.rs` is 2,956 lines and the builder must check each cut fact against it before deciding whether to add it to the header.

Blocked by: {{#m-budget}}, {{#t-core}} (`crates/core/tests/backup_run.rs:409-422` reads the `### backup` section and wants a `- BAK-W ...: present.` bullet).

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, each blocked by the one before it: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the seventh: the six before it have landed and {{#m-frontdoor}} is next.

Line numbers in this issue are those of the map on main at commit 19e0c65. The six earlier map issues have rewritten the lines above this section, so the numbers have moved; they leave the section itself as it is. Find it by its `### backup` heading; it ends at the blank line before `### frontdoor`.

## Rules for every map line in this issue (owner ruling, 2026-11-26)

- **Entry format.** Each source file has exactly one line, and it starts with the file's backticked repo-relative path: `` `path` · status · one sentence ``. Status is `present` or `planned`. The sentence says what the file owns, its Must ids, and the few public names a caller uses.
- **Limits.** An entry line is at most 400 bytes (UTF-8, newline not counted; `·` is 2 bytes) and holds no `#` followed by digits.
- **No bullets.** The `- ...: present.` bullets are removed; the names they list go into the file's one sentence only where a caller uses them.
- **Where cut text goes.** Behaviour detail goes into the `//!` header at the top of the file that holds the code, unless that header or the doc comment on the named item already says it. Fix history (issue numbers, dated notes) is dropped; `CHANGELOG.md` has it. In source files only comments change.
- **Leave alone.** The `reuses:` block and the `modules:` block (lines 9-26), the `### backup` heading, and the form of the first `Updated:` line. Set its date to the day of the PR.
- No earlier map issue writes a line for these three paths.

## The lines

| Map line | Bytes | What it holds | Becomes / goes to |
|---|---|---|---|
| 221 | 850, two issue numbers | the `backup.rs` entry: what a backup holds, the overwrite guard, restore, the swap and `settle`, which writes use `lock::write_whole` and `lock::rename_retrying` | the one entry for `crates/core/src/backup.rs`. The swap, `settle` and the write order go to the `backup.rs` header. |
| 222 | 232 | BAK-2 overwrite guard (`refuse_reason`) and the two refusals before any read | `backup.rs` header |
| 224-228 | 5 bullets | BAK-1 parts: `write_json`, `bundle`, `write_release_list`, `fetch_release_files`, `copy_wiki`, and when each fails the backup | `backup.rs` header |
| 229 | 536 | `backup_repo`, `backup_all`: one backup of a repo at a time, the staging folder, the manifest's `bundle_hash` and `resolved_repo` | `backup.rs` header |
| 230-231 | 2 lines | BAK-W: `backup_run` (`folder`, `repos`, `record`, `failed_row`, `run_one`, `run_all`), a failed repo is a row with outcome `failed` | a new entry for `crates/core/src/backup_run.rs`; detail to its header (4 lines today) |
| 232 | 1,584, two issue numbers | BAK-3 restore: `restore_history`, `restore_issues`, `restore_over_live`, `restore_held_over_live`, `restore_to_new`; the manifest check before any write; what is and is not created again; closing and renumbering only what the restore made (`Made`); `left_out`. Also names `crates/core/src/renumber.rs` (`renumber`, `renumber_for`, `Refs`). | restore detail to the `backup.rs` header; a new entry for `crates/core/src/renumber.rs` (7 header lines today, which already state its rules) |

Lines over 400 bytes today: 221, 229, 232. Lines with issue numbers: 221, 232.

Source files in this area with no line of their own today: `crates/core/src/backup_run.rs` (named only as a word in line 230) and `crates/core/src/renumber.rs` (named only inside line 232). After this issue the section is the heading and three entry lines.

The `backup.rs` header is 4 lines at 19e0c65. {{#m-core}} may have added a comment there about `Marks`, `Made` and `restore_to_new` (detail from the old `core-rules` bullet); keep it and do not say the same thing twice. Keep what is added in the order the file runs: taking a backup (BAK-1), the overwrite guard and the swap (BAK-2), restore (BAK-3). Do not split or move any code; this issue changes comments only.

Not this issue's: the confirm step that calls the restores (`crates/core/src/confirm.rs`) got its line in {{#m-core}}.

Done when:
1. `crates/core/src/backup.rs`, `crates/core/src/backup_run.rs` and `crates/core/src/renumber.rs` each start exactly one line of `docs/code-map.md`, in the entry format.
2. Each of these three commands prints nothing (entry lines over 400 bytes, entry lines with an issue number, bullets left in the section):
   ```
   sed -n '/^### backup$/,/^### frontdoor$/p' docs/code-map.md | LC_ALL=C awk '/^`/ && length($0) > 400'
   sed -n '/^### backup$/,/^### frontdoor$/p' docs/code-map.md | grep -E '^`.*#[0-9]'
   sed -n '/^### backup$/,/^### frontdoor$/p' docs/code-map.md | grep -E '^ *- '
   ```
3. Every function, type or constant an entry line names exists in the file that line names.
4. The PR body lists each fact cut from lines 221-232 with where it is now (the file and "already there" or "added") or "dropped as history".
5. `git diff --stat` shows only `docs/code-map.md` and the three Rust files, and every Rust hunk is a `//!` or `///` comment. Lines 9-26 of the map are unchanged.
6. The first `Updated:` line has the PR's date.
7. `tools/ci.sh rust` and `tools/ci.sh face` pass; their output is in the PR body.
