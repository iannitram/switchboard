What: `AutopilotState` (`<data>/autopilot.json`, what the autopilot passes remember between polls) reads and writes through `crate::state_file`. Its `load` and `save` take no lock today and take none after this (owner ruling, 2026-11-26: each store's locking stays as it is; whether the stores that take no lock need one is {{#x-lock-gaps}}, not this issue).

Where: `crates/core/src/autopilot/passes.rs`: `AutopilotState::load` (50-59) and `AutopilotState::save` (64-71). Line numbers are from main at 19e0c65; find by function name if they moved. `docs/code-map.md` only under Done-when item 7. (code map: section `### autopilot`)

Size: Easy (about 30 changed lines in 1 or 2 files, half of it one new test; Rust)

Blocked by: {{#f-module}}

Independent of open issue #503.

Done when:
1. `AutopilotState::load(data_dir)` keeps its signature; its body is `crate::state_file::load(&data_dir.join(STATE_FILE))`.
2. `AutopilotState::save(&self, data_dir)` keeps its signature; its body is `crate::state_file::save(&data_dir.join(STATE_FILE), self)`.
3. `STATE_FILE`, the struct and its `#[serde(default)]`, and every caller (`passes.rs` lines 478, 493, 553, 570; `autopilot/run.rs` line 820) are untouched. The other `serde_json::to_string_pretty` in `passes.rs` (line 911) is not part of this store and stays.
4. Stays the same: no lock; a missing file is `AutopilotState::default()`; a file that does not parse is `CoreError::Parse` naming the file; `save` makes the data folder and writes whole, so a read never sees half a file.
5. Changes on purpose: `autopilot.json` now ends in one newline (owner ruling: the module always writes a trailing newline); when the folder cannot be made, the `CoreError::Io` text starts with the file's path instead of the folder's; a state that does not serialize gives `CoreError::Parse("<path>: <e>")` (today the text has no path).
6. A new `#[cfg(test)] mod tests` in `passes.rs` (the file has none today) saves a state and asserts the file's text ends in `}\n` and `load` gives it back.
7. If the code map line for `crates/core/src/autopilot/passes.rs` names `lock::write_whole`, it names `state_file` instead; the line stays under 400 bytes with no `#<number>`. (On main at 19e0c65 the file has no line of its own; the map revision, which lands before this, gives it one.) A line that does not name `lock::write_whole` is left alone.
8. No file under `crates/core/tests` or `crates/cli/tests` changes, and these stay green: `crates/core/tests/state_files_431.rs` `autopilot_state_is_never_read_half_written`, `crates/core/tests/auto_after_poll_review.rs` (its `the_state_file_round_trips_and_a_missing_one_is_the_default` asserts the default on a missing file, the round trip and the parse error), `crates/core/tests/auto_roadmap_pass.rs`, `crates/core/tests/auto_weekly_run.rs`, `crates/core/tests/auto_headless_run.rs`.
9. `tools/ci.sh rust` and `tools/ci.sh lint` pass; their output is in the PR body.

## Qwen brief
Expectation: In `crates/core/src/autopilot/passes.rs`:
- `load`: `crate::state_file::load(&data_dir.join(STATE_FILE))`.
- `save`: `crate::state_file::save(&data_dir.join(STATE_FILE), self)`. Reword its doc comment so it names `state_file::save` where it names `lock::write_whole`.
- Add at the end of the file a test module with one test: in a fresh folder under `std::env::temp_dir()` named with `crate::lock::run_tag()`, `AutopilotState::load(&dir)` is the default; save `AutopilotState { reviews_started: vec!["owner/demo#1".into()], ..Default::default() }`; the text of `dir.join(STATE_FILE)` ends in `}\n`; `load` gives the saved state back; the folder holds only `autopilot.json`.
- In `docs/code-map.md`, only if there is a line for `autopilot/passes.rs` that says `lock::write_whole`, say `state_file` there.
- Do not edit any file under `crates/core/tests` or `crates/cli/tests`.

Code map snippet (`autopilot/passes.rs` has no line of its own on main at 19e0c65; the passes are listed under the `autopilot/run.rs` entry, cut short here; the map revision gives the file its own line):
```
`crates/core/src/autopilot/run.rs` · AUTO-2 to AUTO-9, #385 · present. The passes behind every face: the window's threads (`app/src-tauri/src/main.rs`), `switchboard autopilot` and the MCP's autopilot tools call the same code. ...
- AUTO-3 review start (`after_poll`, `review_pass` in `passes.rs`; the app's poll thread starts it): present.

// crates/core/src/state_file.rs
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError>;
```
