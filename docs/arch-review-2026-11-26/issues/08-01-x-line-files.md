What: Four files in the core are written one JSON record per line, each with its own append, rewrite and read code, and two more kinds of line file are only read. This issue asks for an owner ruling: do the written ones move onto one line-file module beside `crates/core/src/state_file.rs`, or stay as they are. It follows the state-file module ({{#f-module}}; owner ruling, 2026-11-26), which left the line files out on purpose. If the ruling is one module, this issue builds the module and moves the thread store onto it.

Where: (line numbers are from main at 19e0c65; find by function name if they moved)
- `crates/core/src/frontdoor/thread.rs`, `crates/core/src/bug/intake.rs`, `crates/core/src/fleet_errors.rs`, `crates/core/src/budget.rs`, `crates/core/src/joblog.rs` (the facts below)
- if one module: `crates/core/src/line_file.rs` (new), `crates/core/src/lib.rs`, `crates/core/src/frontdoor/thread.rs`, `crates/core/tests/line_file.rs` (new), `docs/code-map.md`
- if left: `crates/core/src/state_file.rs` (its `//!` header only)
- code map: the entry lines of `crates/core/src/frontdoor/thread.rs`, `crates/core/src/lock.rs` and `crates/core/src/state_file.rs`. The map revision ({{#m-core}} to {{#m-guard}}) has landed by then, so every source file has one entry line of the form `` `path` · status · one sentence ``; find a line by its path.

Size: Medium (if the ruling is one module: about 190 changed lines in 5 files: the module about 60, its test about 80, the thread store moved onto it about 40, `lib.rs` 1, the code map about 5; Rust). Moving all four stores in one PR would be over the limit of 6 files, so the other three stores each get their own issue. If the ruling is to leave them, the change is one sentence in the `//!` header of `crates/core/src/state_file.rs`.

Blocked by: {{#f-module}} (so the two modules can be made alike; it lands after the whole map revision, so the code map is in its one-line-per-file form), and an owner ruling on the question below.

## The question

1. **One module.** A line-file module with three operations: read the lines, append lines, rewrite the whole file. Each store keeps its own row type and its own rules.
2. **Leave them.** The stores differ enough that a shared module would have a wide interface and little depth. The header of `state_file.rs` says so in one sentence and this closes.

If 1, three rules also need a yes or a correction:

- (a) **Who takes the lock.** Proposed: the store, as today; the module's operations take no lock. The thread store reads and then writes under one hold of its lock (`append_if`, `upsert`, `change`), and the pool edit ledger holds its lock across the whole save, so a module that took the lock inside each operation would not fit either.
- (b) **The read rule.** Today there are four (see the tables). Proposed: the module reads the file as text and gives every line with its 1-based number, blank lines included; a missing file gives no lines; each store parses the lines and keeps its own rule for a blank or bad line. The fleet error record splits on bytes so a line that is not UTF-8 is still one line; under this rule it keeps its own read.
- (c) **The word.** Proposed: GLOSSARY gains "Line file" beside "State file" when the first follow-on store moves, not in this issue.

## Facts (checked at 19e0c65)

The owner ruling named five logs: job log, rating log, implicit log, thread store, bug ledger. Two of them are not files of their own, the job log is only read, and two line files the ruling did not name are written.

**Stores the core writes:**

| Store | File | Write functions | Read functions | Lock | A line that does not parse |
|---|---|---|---|---|---|
| Thread store (`frontdoor/thread.rs`, 221 lines) | `<data>/threads/<handoff_id>.jsonl`, rows are `JobRow` | `append_unlocked` (67-84, one append write), `rewrite_unlocked` (88-99, `lock::write_whole`); public `append` (102), `append_all` (108), `append_if` (115), `upsert` (134), `change` (151) | `read` (182-198), `turns` (177), `exists` (171), `list` (201) | `locked` (59-64): `FileLock::take`, `THREAD_LOCK_WAIT` 10 s (line 14) | `CoreError::Parse` naming the file and the line number; a blank line is an error too |
| Bug ledger (`bug/intake.rs`, 417 lines) | `bugs.jsonl` in fleet-state, path given by the caller, rows are `Bug` | `edit_ledger` (332-346): read all, change, then always rewrite whole with `lock::write_whole`; used by `link_duplicate` (349) and `record` (364). No append. | `read_bugs` (319-329) | `FileLock::take` with a bare 10 s at line 336 | `CoreError::Parse("bugs.jsonl: <e>")` with no path and no line number; blank lines skipped |
| Fleet error record (`fleet_errors.rs`, 185 lines) | `<data>/fleet-errors.jsonl`, rows are `FleetErrorRecord` | `record` (102-125): append, mend a line cut short (`ends_mid_line`, 128), `sync_data`, then `trim` (141-155) keeps the last `FLEET_ERROR_LOG_CAP` (50, `fleet.rs:463`) lines with `lock::write_whole` | `load` (168-185), `lines` (158-163, split on bytes) | `FileLock::take`, `RECORD_LOCK_WAIT` 2 s (line 25) | Skipped and counted in `Recorded.skipped` |
| Pool edit ledger (`budget.rs`, 869 lines) | `<data>/pool-edits.jsonl` (`POOL_EDITS_LOG`, line 227), rows are JSON values | `append_line` (452-474), which returns `LedgerUndo` (396-418) to cut the line off again with `set_len` (or delete the file it made) | None: no function reads this file back | `FileLock::take(edits_log, EDIT_LOCK_WAIT)` 10 s (line 224), taken by `save_edit_then` at line 358 and held until the pool edit is saved or taken back | Not read |

**Line files the core only reads (written by other tools):**

| File | Read functions | A line that does not parse |
|---|---|---|
| Job log: `<pipeline>/logs/jobs.jsonl` and `<state>/ledger/*.jsonl`, written by the fleet tools | `joblog::parse_jsonl` (214-222), `ledger_files` (225-246), `read_ledger` (250-266), `read_all` (179-210) | Skipped with no count; bytes that are not UTF-8 spoil only their line; an event equal to one already kept is dropped |
| Claude session logs, `*.jsonl` under the Claude projects folders | `budget::session_log_paths` (684-717), the line loop in `budget::claude_pool` (657-673), `counted_usage` (627-635) | Skipped |

**Not files of their own:** the rating log (`frontdoor/rating_log.rs`: `log_rating` calls `ThreadStore::upsert` at line 97) and the implicit log (`frontdoor/implicit_log.rs`: `log_implicit` calls `ThreadStore::append` at line 45) write rows into the thread's file. `joblog.rs` writes nothing and takes no lock.

**Who calls the writers:**
- Thread store: `frontdoor/answer.rs` (lines 556, 569, 591, 634, 727, 847), `rating_log.rs:97`, `implicit_log.rs:45`; reached from the window, the CLI and MCP.
- Fleet error record: `app/src-tauri/src/commands.rs:325` and `crates/cli/src/autopilot.rs:236`.
- Pool edit ledger: `budget::save_pool_edit` (line 211), called from the window (`app/src-tauri/src/commands.rs:470`), the CLI (`crates/cli/src/cli.rs:1374`) and MCP (`crates/cli/src/mcp.rs:120`).
- Bug ledger: no file under `crates/core/src`, `crates/cli/src` or `app/src-tauri/src` calls `record`, `link_duplicate` or `read_bugs` from outside `intake.rs`; only `crates/core/tests/bug_intake.rs` does.

**What they share:**
- One JSON object per line, each line ended with a newline; a missing file reads as empty.
- Append: serialize, add the newline, open with create and append, write once. Written three times: `thread.rs:67-84`, `fleet_errors.rs:107-122`, `budget.rs:452-474`.
- Rewrite whole: join the lines and hand the text to `lock::write_whole`. Written three times: `thread.rs:88-99`, `intake.rs:339-344`, `fleet_errors.rs:148-154`.
- All four written stores take `FileLock` on the file itself; the waits are 10 s, 10 s, 2 s and 10 s, each its own constant or literal.
- The shared part is about 12 to 20 lines per store, roughly 60 lines in all.

**What differs on purpose:** the fleet error record caps the file, mends a cut line, syncs to disk and reads bytes; the pool edit ledger must be able to take its line back and keeps its lock for the whole save; the thread store replaces a row by id and reads before it writes under one hold; the four read rules above.

**Tests today:** `crates/core/tests/fd_thread.rs` (6 tests), `fd_thread_lock.rs` (4), `fleet_errors_store.rs` (9), `bug_intake.rs` (12), `job_log.rs` (13), `budget_edit_lock.rs` (8).

Related, not blocking: open issue #503 asks whether `lock::write_whole` should write through craft-atomic. A line-file module would call `lock::write_whole`, so neither waits for the other.

Done when:

1. The owner ruling (choice 1 or 2, and for 1 the rules (a), (b) and (c)) is written as a comment on this issue.
2. If one module, with rules (a) and (b) as proposed (if the ruling corrects a rule, the ruling comment says what replaces the matching clause below):
   1. `crates/core/src/line_file.rs` exists beside `lock.rs` and `state_file.rs`, declared in `crates/core/src/lib.rs`, with exactly three public operations and no lock taken inside any of them: read (every line as text with its 1-based number, blank lines included; a missing file gives none; any other read error is `CoreError::Io("<path>: <e>")`), append (the given lines, each ended with a newline, in one write to a file opened with create and append), rewrite (the given lines as the whole file through `lock::write_whole`).
   2. `ThreadStore` in `frontdoor/thread.rs` uses the module inside `append_unlocked`, `rewrite_unlocked` and `read`. `locked`, the public functions, `THREAD_LOCK_WAIT` and every error text are unchanged: a line that does not parse (a blank one included) is still `CoreError::Parse("<path>: line <n>: <e>")`.
   3. A new test file `crates/core/tests/line_file.rs` shows: a missing file reads as no lines; an appended line reads back with number 1; a blank line is given with its number; two threads that each take `FileLock` on the file and append 50 lines leave 100 whole lines; a rewrite leaves the given lines and no other file in the folder.
   4. No existing file under `crates/core/tests` changes, and `fd_thread.rs`, `fd_thread_lock.rs`, `fd_rating_log.rs` and `fd_implicit_log.rs` pass.
   5. In the same PR the code map gains one entry line for `crates/core/src/line_file.rs`, in the form `` `path` · status · one sentence `` with status `present`, and the `crates/core/src/frontdoor/thread.rs` line names the module. Both lines stay under 400 bytes with no `#<number>`, and the map's first `Updated:` line is re-dated. `line_file.rs` starts with a `//!` header that states the three operations and that the store, not the module, takes the lock.
   6. Three follow-on issues are filed and linked from this issue: the bug ledger, the fleet error record, the pool edit ledger. Each says what stays the store's own (from "What differs on purpose" above).
   7. `tools/ci.sh rust` and `tools/ci.sh lint` pass and their output is in the PR body.
3. If left as they are: the `//!` header of `crates/core/src/state_file.rs` gains one sentence saying the line files keep their own read and write code and why (owner ruling, 2026-11-26: behaviour detail lives in a file's header comment, not in the code map; the sentence has no issue number). No code map line changes. `tools/ci.sh rust` passes with its output in the PR body, and this issue closes.
