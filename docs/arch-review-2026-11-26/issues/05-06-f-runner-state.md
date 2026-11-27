What: `StateStore` (one run state per Spec and Unit, one JSON file each) reads and writes through `crate::state_file`. It takes no lock today and takes none after this (owner ruling, 2026-11-26: each store's locking stays as it is; whether the stores that take no lock need one is {{#x-lock-gaps}}, not this issue). Owner ruling (2026-11-26): a run-state file that holds only the JSON word `null` stays unreadable (a parse error), as it is today, and the store checks that itself.

Where: `crates/core/src/runner/state.rs`: `StateStore::load` (101-111), `StateStore::save` (113-119), the import `use crate::lock::write_whole;` (line 9), one new private type beside `StateStore`. Line numbers are from main at 19e0c65; find by function name if they moved. `docs/code-map.md` (code map: section `### runner`, the `runner/state.rs` line).

Size: Easy (about 55 changed lines in 2 files, more than half of it new tests; Rust)

Blocked by: {{#f-module}}

Independent of open issue #503.

Done when:
1. `runner/state.rs` has one new private type, `struct Saved(Option<RunState>)`, with `#[derive(Default)]` (so no file is `Saved(None)`) and a hand-written `Deserialize` that parses a `RunState` and wraps it in `Some`. This is how the store keeps the `null` rule: `state_file::load` gives the default only when there is no file; any file that exists must parse as a whole `RunState`, and `null` does not, so it is a `CoreError::Parse` naming the file, as today. `Option<RunState>` is not used as the type `load` reads, because it would read `null` as "no file".
2. `StateStore::load(spec, unit)` keeps its signature (`Result<Option<RunState>, CoreError>`); its body is `crate::state_file::load::<Saved>(&self.file(spec, unit)).map(|saved| saved.0)`.
3. `StateStore::save(state)` keeps its signature; its body is `crate::state_file::save(&self.file(&state.spec, &state.unit), state)` (it writes the `RunState` itself, so the file's shape does not change). The `write_whole` import is gone.
4. `StateStore::file` and `RunState` are untouched. The doc comment on `load` keeps its sentence "a run is never restarted from a guess".
5. Stays the same: no lock; no file is `None` (the Unit has not started); a file that does not parse is `CoreError::Parse` naming the file, and a file holding only `null` is one of those; `save` makes the folder and writes whole.
6. Changes on purpose: a run-state file now ends in one newline (owner ruling: the module always writes a trailing newline); a serialize failure reads `<path>: <e>` instead of `run state: <e>`; when the folder cannot be made, the `CoreError::Io` text starts with the run-state file's path instead of the folder's. The PR body lists these three and nothing else as changed.
7. A new `#[cfg(test)] mod tests` in `runner/state.rs` (the file has none today, and no test in the repo covers a `null` run-state file today) proves: `load` before any `save` is `Ok(None)`; after `save`, `load` gives `Some` of the same `RunState` and the file's text ends in `}\n`; a file holding `{ nope` is `CoreError::Parse`; a file holding `null` is `CoreError::Parse` whose text contains the file's path. The last one is the test that proves the `null` rule.
8. If the `runner/state.rs` code map line names `lock::write_whole`, it names `state_file` instead; the line stays under 400 bytes with no `#<number>`. A line that does not name it is left alone.
9. No file under `crates/core/tests` changes, and `crates/core/tests/runner_engine.rs` (among them `success_moves_on_and_a_restart_continues_from_the_saved_state`), `crates/core/tests/runner_waiting.rs`, `crates/core/tests/roadmap_run_live.rs` and `crates/core/tests/bug_loop_live.rs` stay green.
10. `tools/ci.sh rust` and `tools/ci.sh lint` pass; their output is in the PR body.

## Qwen brief
Expectation: In `crates/core/src/runner/state.rs`:
- Add this private type above `impl StateStore` (`Deserialize` is already imported in the file):
```rust
/// What `state_file::load` reads for this store. No file is `Saved(None)`: the unit has not
/// started. A file must hold a whole `RunState`; one holding only `null` is a parse error.
#[derive(Default)]
struct Saved(Option<RunState>);

impl<'de> Deserialize<'de> for Saved {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        RunState::deserialize(from).map(|state| Saved(Some(state)))
    }
}
```
- `load`: `crate::state_file::load::<Saved>(&self.file(spec, unit)).map(|saved| saved.0)`. Do not use `Option<RunState>` as the type `state_file::load` reads.
- `save`: `crate::state_file::save(&self.file(&state.spec, &state.unit), state)`.
- Remove `use crate::lock::write_whole;`.
- Add at the end of the file a test module with one test that uses a fresh folder under `std::env::temp_dir()` named with `crate::lock::run_tag()`: `StateStore::new(&dir).load("s", "u")` is `Ok(None)`; save `RunState::new("s", "u", "start")`; `load("s", "u")` gives `Some` of it; the one file in the folder ends in `}\n`; overwrite that file with `{ nope` and `load("s", "u")` is `Err(CoreError::Parse(_))`; overwrite it with `null` and `load("s", "u")` is `Err(CoreError::Parse(text))` where `text` contains the file's name (`s--u.json`).
- Expected results: a missing file gives `Ok(None)`; a saved state reads back equal; `null` in the file is an error, never `Ok(None)`.
- In `docs/code-map.md`, only if the `runner/state.rs` line says `lock::write_whole`, say `state_file` there.
- Do not edit any file under `crates/core/tests`.

Code map snippet (as on main at 19e0c65, cut short; the map revision may have reworded it):
```
`crates/core/src/runner/state.rs` · runner-engine (#396) · present. `RunState` (state, failures per step, pending retry, halt, last result and note) and `StateStore`, one JSON file per spec and unit written whole (`lock::write_whole`), so a restart resumes at the same step. ...

// crates/core/src/state_file.rs
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError>;
```
