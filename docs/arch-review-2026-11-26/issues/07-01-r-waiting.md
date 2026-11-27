**What:** Move the Waiting on you read into core as one read model that returns the answer plus warnings, and make the Tauri command, the CLI and MCP all call it (owner ruling, 2026-11-26). This is the first of seven read-model issues, so it also adds the three shared pieces the other six reuse: the warning type with the snapshots helper, the way an MCP reply carries warnings, and the window's quiet warning line.

Line numbers below are as the files read at the time of writing. The guarded-writes issues ({{#w-core}}, {{#w-cli}}, {{#w-mcp}}) land first and move lines in `crates/cli/src/cli.rs`, `crates/cli/src/main.rs` and `crates/cli/src/mcp.rs`; find each item by name.

Today the three faces do not agree:
- CLI: `waiting_items` (`crates/cli/src/cli.rs` 2003-2019) calls `SnapshotStore::load_each`, prints each unreadable snapshot file to stderr through `warn_unreadable` (1778-1785), loads `LocalItems`, refuses with `NO_SNAPSHOT` (const, line 72) when there are no snapshots and no local items, then merges `waiting::assemble` with `waiting::priority_items`.
- MCP: `Core::call` (`crates/cli/src/main.rs` 94-97) calls `cli::waiting_items` and truncates to `limit`. The warning goes to the server's stderr; the caller never sees it.
- Window: the `waiting` command (`app/src-tauri/src/commands.rs` 431-438) calls `switchboard_core::waiting::load`, which reads through `load_all` (`crates/core/src/waiting.rs` 461-474). An unreadable snapshot file is skipped and nothing says so.

Build this:
1. New module folder `crates/core/src/read/`; add `pub mod read;` to `crates/core/src/lib.rs`.
   - `crates/core/src/read/mod.rs`:
     - `pub struct Warning { pub file: String, pub reason: String }` (Serialize, Deserialize, Clone, Debug, PartialEq, Eq).
     - `impl From<&github::snapshot::Unreadable> for Warning` (`crates/core/src/github/snapshot.rs` 155-161): `file` is `path` as text; `reason` is `error` whole. `error` already names the file (`parse: <path>: <why>` or `file: <path>: <why>`), and the CLI prints `reason` as it prints `error` today.
     - `pub fn snapshots(data: &Path) -> Result<(Vec<RepoSnapshot>, Vec<Warning>), CoreError>`, built on `SnapshotStore::load_each` (same file, 121-150).
   - `crates/core/src/read/waiting.rs`:
     - `pub struct WaitingAnswer { pub items: Vec<Item>, pub warnings: Vec<Warning>, #[serde(skip)] pub nothing_read: bool }` (Serialize). `nothing_read` is true when no snapshot was read and there are no local items. It serializes as `{"items": [...], "warnings": [...]}`.
     - `pub fn load(data: &Path, now: UnixSeconds) -> Result<WaitingAnswer, CoreError>`: what `waiting_items` does today, minus the printing and minus the refusal. The items are always built, also when `nothing_read` is true.
2. CLI: `waiting_items` shrinks to a call to `read::waiting::load`. The CLI keeps `data_or_fail` (1676-1688); one stderr line per warning in today's exact words, `switchboard waiting: <reason>; skipped, run switchboard refresh`, with `reason` passed through `terminal_safe` and printed before any refusal; the `NO_SNAPSHOT` refusal (exit 1), now decided from `nothing_read`; `waiting_text` (2023-2039); and the `--json` print, which stays the bare list of items. `snapshots` (1763-1775) and `warn_unreadable` stay for the commands not yet moved.
3. MCP: the answer carries the warnings. The first content item of the reply stays exactly what `waiting --json` prints (the rule in the doc comment at main.rs 51-52), truncated to `limit`. When there are warnings the reply gains a second text content item whose text is the JSON `{"warnings": [{"file": ..., "reason": ...}]}`; with no warnings the reply has one content item, as today. To carry them:
   - `trait ReadCore` (`crates/cli/src/mcp.rs` 35-37) gains a provided method `fn call_with_warnings(&self, tool: &str, args: &Value) -> Result<(Value, Vec<Warning>), ErrorBody>` whose default calls `call` and returns no warnings. The nine test files that implement `ReadCore` need no edit.
   - `read_call` (mcp.rs 578-588) calls `call_with_warnings` and adds the second content item.
   - `Core` (the `impl mcp::ReadCore for Core` at main.rs line 70) overrides it for `waiting`; `McpCore` (`impl ReadCore for McpCore` in `crates/cli/src/autopilot.rs`, line 455) overrides it so its `other => self.reads.call(other, args)` arm passes the warnings through.
   - The MCP reply still refuses with `NO_SNAPSHOT` as today.
4. Window: the `waiting` Tauri command returns `WaitingAnswer` from `switchboard_core::read::waiting::load`. `app/src/views/Waiting.tsx` (the `call` at line 114) reads the answer through one function that accepts the object or a bare list, as `answerOf` does in `app/src/views/Reports.tsx` (32-35). The bare list must keep working: `app/src/views/Waiting.test.tsx` is a locked test (R28) and mocks `waiting` as a bare array. The view still calls `waiting` once per load.
5. New file `app/src/WarningLine.tsx`: given a list of `{file, reason}` it renders no element at all when the list is empty (the locked `app/src/App.wired.test.tsx` asserts there is no `status` role on a quiet window). Otherwise it renders one line, `role="status"`, no dialog: `Left out: <file>`, several files joined with `, ` on the one line, each file's `reason` in its `title`. `Waiting.tsx` shows it at the top of the view. Reports already shows its warnings at the top of its view (`Reports.tsx` 124-143); this is the same place for the other views.
6. `waiting::load` and `waiting::load_at` stay in core: the locked `crates/core/tests/waiting_load.rs` calls `load`.
7. One assertion of that locked file pins the old wiring, and the builder of this issue edits it (owner ruling, 2026-11-26: the builder edits the locked test an issue names; here that is this one assertion). In the test `the_shell_registers_the_waiting_command`, lines 270-273 read today:

   ```rust
       assert!(
           commands.contains("switchboard_core::waiting::load("),
           "commands.rs: the waiting command must call `switchboard_core::waiting::load(`"
       );
   ```

   The rules of the edit, exactly:
   - In those four lines the text `switchboard_core::waiting::load(` becomes `switchboard_core::read::waiting::load(`, once in the `contains` argument and once in the message. After the edit they read:

     ```rust
         assert!(
             commands.contains("switchboard_core::read::waiting::load("),
             "commands.rs: the waiting command must call `switchboard_core::read::waiting::load(`"
         );
     ```
   - No other line of the file changes: not the header (lines 1-2), not the `use` lines, not the other three assertions of that test (`commands::waiting,` in the handler list, `pub fn waiting(`, `FaceError::new("waiting"`), not any other test. `git diff` of the file shows two changed lines.
   - So `commands.rs` holds the call written out in full as `switchboard_core::read::waiting::load(`, not shortened through a `use`.
   - The PR body quotes the four lines before and after and names the owner ruling (2026-11-26) as the reason a locked file changed. The reviewer checks the edited file against these rules line by line.
8. Code map: one entry line each for `crates/core/src/read/mod.rs` and `crates/core/src/read/waiting.rs` under `### waiting`, and for `app/src/WarningLine.tsx` under `### shell`, in the map's entry form: `` `<path>` · present · <one sentence> ``, under 400 bytes and with no `#<number>` (the structural code map tests, with the guard {{#m-guard}} turned on, fail a line that breaks this and fail a source file that has no line). Correct any line of `### waiting`, `### cli`, `### mcp` and `### shell` that this change makes untrue, within the same limits. Each new source file starts with a header comment (`//!` in Rust, a top comment in the `.tsx`) that says what it is for; behaviour detail goes there, not in the map.

**Where:** `crates/core/src/read/mod.rs` (new), `crates/core/src/read/waiting.rs` (new), `crates/core/src/lib.rs`, `crates/cli/src/cli.rs`, `crates/cli/src/main.rs`, `crates/cli/src/mcp.rs`, `crates/cli/src/autopilot.rs`, `app/src-tauri/src/commands.rs`, `app/src/views/Waiting.tsx`, `app/src/WarningLine.tsx` (new), `crates/core/tests/waiting_load.rs` (the one assertion), new tests `crates/core/tests/read_waiting.rs`, `crates/cli/tests/mcp_warnings.rs`, `app/src/views/WaitingWarning.test.tsx`, and `docs/code-map.md`. Code map sections: `### waiting`, `### cli`, `### mcp`, `### shell`.

**Size:** Medium (about 400 changed lines in 15 files, tests included; Rust and React). This is over the 300-line, 6-file aim because it carries the shared pieces for all seven read-model issues; it is reviewed as an oversize change.

**Blocked by:** {{#w-mcp}} (the last guarded-writes issue; owner ruling, 2026-11-26: the guarded writes land first, then the read models).

**Done when:**
1. `crates/core/src/read/mod.rs` and `crates/core/src/read/waiting.rs` exist with the items above, and `waiting_items` in `crates/cli/src/cli.rs` no longer calls `load_each`, `LocalItems::new`, `waiting::merge` or `waiting::priority_items`.
2. `crates/core/tests/read_waiting.rs` shows: one good and one corrupt snapshot file give the good repo's items plus one warning whose `file` is the corrupt file's path and whose `reason` contains that path; no snapshot and no local items gives `nothing_read` true; a stale priority copy alone gives its item and `nothing_read` true; the answer serializes with exactly the keys `items` and `warnings`.
3. These pass unedited: `crates/cli/tests/waiting_reports_cli.rs` (all), `crates/cli/tests/sky_github_cli.rs` (`waiting_lists_a_stale_priority_copy`, `the_views_show_the_good_repo_and_name_the_bad_file`), `crates/cli/tests/cli_edges.rs` (`star_and_waiting_print_no_raw_control_characters`), `crates/cli/tests/mcp_read.rs` (all), and the nine test files under `crates/cli/tests/` that implement `ReadCore`.
4. `crates/cli/tests/mcp_warnings.rs` shows the MCP `waiting` reply with a corrupt snapshot file: the first content item equals what `waiting --json` prints, the second parses as `{"warnings": [...]}` and names the corrupt file; with no corrupt file the reply has exactly one content item.
5. The Tauri command `waiting` returns `{items, warnings}`, and `commands.rs` contains `switchboard_core::read::waiting::load(`. `git diff` of `crates/core/tests/waiting_load.rs` shows exactly the two changed lines step 7 spells out, the PR body quotes the assertion before and after, and the reviewer has checked the file against step 7's rules line by line.
6. `app/src/views/WaitingWarning.test.tsx` (new, not locked) shows the line at the top of the Waiting view naming the file when the answer has a warning, and no element with role `status` when it has none. `app/src/views/Waiting.test.tsx`, `app/src/views/WaitingConfirm.test.tsx` and `app/src/App.wired.test.tsx` pass unedited.
7. `docs/code-map.md` has one entry line for each of the three new files in the form step 8 gives, each new file has its header comment, and the structural code map tests of the core tree (`crates/core/tests/code_map_structure.rs`) and of the React tree pass.
8. `tools/ci.sh rust`, `tools/ci.sh app` and `tools/ci.sh face` pass; their output is in the PR body.
