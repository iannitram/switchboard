What: Add one deep module, `crates/core/src/state_file.rs`, beside `lock.rs`, that owns how a State file is read, written whole and changed under its lock, and move the first two stores (`LocalItems`, `RunnerItems`) onto it. GLOSSARY gains the term "State file" and the code map gains the module's line. Owner ruling (2026-11-26): the interface is exactly three operations, each store keeps its own type, and each store's locking stays exactly as it is today.

Where: (line numbers are from main at 19e0c65; find by function name if they moved)
- `crates/core/src/state_file.rs` (new)
- `crates/core/src/lib.rs` (add `pub mod state_file;` after `pub mod sky;`, line 36)
- `crates/core/src/waiting.rs`: `LocalItems::load` (286-294), `LocalItems::change` (349-361), `LocalItems::write` (365-370)
- `crates/core/src/runner/waiting.rs`: the import `use crate::lock::FileLock;` (line 16), `RunnerItems::load` (63-71), `RunnerItems::change` (201-219)
- `GLOSSARY.md` (new entry at the end of `## Machines`, after "Repo backup")
- `docs/code-map.md` (code map: a new line right after the `crates/core/src/lock.rs` line; on main at 19e0c65 that line is under `### core-lock`; find it by its path, since the map revision that lands before this may have renamed the section)

Size: Medium (about 230 changed lines in 6 files: about 90 of module, about 80 of its tests, about 45 in the two stores, the rest docs; Rust)

Blocked by: {{#m-guard}} (owner ruling, 2026-11-26: the code map revision lands before the state-file module. {{#m-guard}} is the last issue of that revision and turns on the check that every source file has a map line, so this PR must add the line for `state_file.rs` or that check fails.)

Independent of open issue #503 (write through craft-atomic): #503 touches `lock::write_whole`; this module only calls `lock::write_whole` and `FileLock`, so neither waits for the other.

Not in this issue: the seven other stores (one issue each: {{#f-held}}, {{#f-marks}}, {{#f-manifest}}, {{#f-settings}}, {{#f-runner-state}}, {{#f-snapshot}}, {{#f-autopilot}}); whether the stores that take no lock today need one ({{#x-lock-gaps}}); the line files, which are the job log, the rating log, the implicit log, the thread store and the bug ledger ({{#x-line-files}}).

## The interface

The error type is `crate::CoreError`, the one `lock.rs` returns today: `lock::write_whole` returns `Result<(), CoreError>` and builds `CoreError::Io("<path>: <e>")`; `FileLock::take` and `FileLock::take_for` return `Result<FileLock, CoreError>` (a `CoreError::Io` that names the file and what holds the lock). No new error type.

```rust
//! crates/core/src/state_file.rs
use crate::CoreError;
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;
use std::time::Duration;

/// Read the State file at `path`.
/// - No file: `T::default()`. Nothing is created.
/// - A file that cannot be read: `CoreError::Io("<path>: <e>")`.
/// - A file that does not parse: `CoreError::Parse("<path>: <e>")`.
/// Takes no lock.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;

/// Write `value` to `path` whole: make the parent folder, serialize as pretty JSON
/// (`serde_json::to_string_pretty`), add one trailing `\n`, write through
/// `lock::write_whole` (a temp file of this write's own, then a rename).
/// A folder that cannot be made is `CoreError::Io("<path>: <e>")`; a value that does
/// not serialize is `CoreError::Parse("<path>: <e>")`. `<path>` is always the State
/// file's path. Takes no lock.
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError>;

/// Change the State file at `path` under its lock, in this order:
/// 1. make the parent folder (a failure is `CoreError::Io("<path>: <e>")`);
/// 2. take the lock: `FileLock::take_for(path, wait, text)` when `holder` is
///    `Some(text)`, `FileLock::take(path, wait)` when it is `None`;
/// 3. `load` the value;
/// 4. run `change` on it; its `bool` says whether it changed anything;
/// 5. `save` only when that `bool` is true;
/// 6. return the closure's `R`. The lock is held from step 2 until return.
/// An `Err` from `change` writes nothing and is returned as it is.
pub fn change<T, R>(
    path: &Path,
    wait: Duration,
    holder: Option<&str>,
    change: impl FnOnce(&mut T) -> Result<(R, bool), CoreError>,
) -> Result<R, CoreError>
where
    T: DeserializeOwned + Default + Serialize;
```

Nothing else is public in the module. The file starts with a `//!` header comment that says what a State file is and states the rules above (missing file gives the default; every write is whole and ends in one newline; `change` writes only when the closure says it changed something).

A store whose "no file" answer is `None` needs a `T` whose default is `None`. A bare `Option<...>` is one, but with it a file that holds only the JSON word `null` reads as "no file". Owner ruling (2026-11-26): for `StateStore` and `SnapshotStore` such a file stays unreadable (a parse error), each of those two stores checks this itself, and the module stays three operations. So `load` gets no flag, no fourth function and no special case for `null`: it parses the file's text as `T` and nothing more. {{#f-runner-state}} and {{#f-snapshot}} each give `load` a private type of their own whose default is "no file" and which parses only a whole value. This issue moves no such store.

## The two stores

`LocalItems` (`crates/core/src/waiting.rs`):
- `load` keeps its signature; its body becomes `crate::state_file::load(&self.path)`.
- `change` keeps its signature (`impl FnOnce(&mut Vec<Item>) -> bool`, returns `Result<bool, CoreError>`); its body becomes one call to `crate::state_file::change(&self.path, self.wait, Some(LOCAL_HOLDER), ...)` whose closure maps the caller's `bool` to `Ok((changed, changed))`.
- The private `write` is removed.
- `add`, `add_new`, `remove`, the `wait` field, `LOCAL_LOCK_WAIT` and `LOCAL_HOLDER` do not change. `remove` keeps its read before the lock (lines 333-337: an item that is not there takes no lock and makes no folder).

`RunnerItems` (`crates/core/src/runner/waiting.rs`):
- `load` keeps its signature; its body becomes `crate::state_file::load(&self.path)`.
- The private `change` keeps its signature; its body becomes one call to `crate::state_file::change(&self.path, LOCAL_LOCK_WAIT, Some(HOLDER), change)`.
- The import `use crate::lock::FileLock;` is removed (nothing else in the file uses it).
- `raise`, `answer`, `list` do not change. In `answer` the run state is still saved inside the closure, under the items lock.

What must stay the same for both: the lock file, its wait (`LOCAL_LOCK_WAIT`, 10 s) and its holder text; no file gives no items; a change that says "nothing changed" writes nothing; the file is pretty JSON ending in one newline (both stores already write it so, so no file's bytes change in this issue); error wording `<path>: <e>`.

## GLOSSARY and code map

GLOSSARY entry, in the GLOSSARY's form. The definition is the owner ruling's, word for word:

```
**State file**:
A JSON file Switchboard owns and rewrites whole under a lock.
_Avoid_: data file, database
```

Code map line, in the map's entry form, under 400 bytes and with no `#<number>`:

```
`crates/core/src/state_file.rs` · present · <one sentence: load (no file gives the default), save (whole write through a temp file, trailing newline), change (lock, read, change, write only if it changed)>
```

Done when:
1. `crates/core/src/state_file.rs` exists with exactly the three public functions above and the `//!` header; `lib.rs` declares it.
2. Tests inside the module (`#[cfg(test)] mod tests`) prove each of these: (a) `load` of a missing file gives the default and creates no file or folder; (b) `load` of text that does not parse is `CoreError::Parse` naming the path (one case reads a file holding `null` as a `Vec<u32>`, which is a parse error: `load` treats `null` like any other text), and `load` of a path that is a folder is `CoreError::Io` naming the path; (c) `save` makes the parent folder, the file's text is pretty JSON ending in exactly one `\n`, no other file is left in the folder, and `load` reads the value back; (d) `change` with a closure that returns `false` leaves an existing file's bytes as they were, and on a missing file creates no State file; (e) `change` with `true` writes and returns the closure's `R`; (f) a closure that returns `Err` writes nothing and that error comes back; (g) while another `FileLock` holds the path, `change` with a short wait returns `CoreError::Io`; (h) after `change` returns, `lock::held(path)` is false.
3. `crates/core/src/waiting.rs` and `crates/core/src/runner/waiting.rs` contain none of the words `to_string_pretty`, `write_whole`, `FileLock`.
4. No file under `crates/core/tests` or `crates/cli/tests` changes, and these stay green: `crates/core/tests/waiting_local.rs` (among them `a_missing_store_loads_empty_and_is_not_created`, `the_file_is_a_pretty_json_array_ending_in_a_newline`, `a_store_that_does_not_parse_is_a_parse_error_naming_the_file`), `crates/core/tests/state_files_431.rs` `a_waiting_change_waits_for_the_lock_another_writer_holds`, the test `waiting_items_written_at_once_lose_nothing_and_bring_nothing_back` inside `waiting.rs`, and all of `crates/core/tests/runner_waiting.rs`.
5. `GLOSSARY.md` has the "State file" entry and `docs/code-map.md` has the module's line with status `present`; the structural code map test for the core tree (from {{#t-core}}, with the checks {{#m-guard}} turned on) passes.
6. `tools/ci.sh rust` and `tools/ci.sh lint` pass; their output is in the PR body.
