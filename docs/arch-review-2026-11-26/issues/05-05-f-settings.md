What: `settings::load` and `settings::save` call `crate::state_file` and keep no read or write code of their own. Settings take no lock today and take none after this (owner ruling, 2026-11-26: each store's locking stays as it is; whether the stores that take no lock need one is {{#x-lock-gaps}}, not this issue).

Where: `crates/core/src/settings.rs`: `load` (49-56), `save` (78-86), and the test inside the file (93-108). Line numbers are from main at 19e0c65; find by function name if they moved. (code map: section `### core-settings`)

Size: Easy (about 15 changed lines in 1 file; Rust)

Blocked by: {{#f-module}}

Independent of open issue #503.

Done when:
1. `load(path)` keeps its signature; its body is `crate::state_file::load(path)`.
2. `save(path, settings)` keeps its signature; its body is `crate::state_file::save(path, settings)`.
3. `settings::temp_path` and `SAVE_SEQ` stay as they are (`crates/core/tests/settings_probe.rs` pins `temp_path`).
4. Stays the same: no lock; a missing file is `Settings::default()`; a file that does not parse is `CoreError::Parse` naming the full path; unknown keys are ignored; `save` makes the folder; a failed save leaves the old file and no temp file.
5. Changes on purpose: the settings file now ends in one newline (owner ruling: the module always writes a trailing newline). When the folder cannot be made, the `CoreError::Io` text now starts with the settings file's path instead of its folder's path. Settings that do not serialize give `CoreError::Parse("<path>: <e>")` (today the text has no path).
6. The test inside `settings.rs` (`save_round_trips_and_leaves_no_temp_file`) also reads the file's text and asserts it ends in `}\n`.
7. No file under `crates/core/tests` changes, and `crates/core/tests/settings_file.rs` and `crates/core/tests/settings_probe.rs` stay green (among them `a_missing_file_loads_as_the_default`, `a_file_that_does_not_parse_is_a_parse_error_naming_the_file`, `save_under_a_file_is_an_io_error`, `a_failed_rename_removes_the_temp_file_on_every_system`, `saves_at_once_from_many_threads_all_succeed`).
8. `tools/ci.sh rust` and `tools/ci.sh app` pass (the Tauri face calls `settings::load` and `settings::save` in `app/src-tauri/src/commands.rs`); their output is in the PR body.

## Qwen brief
Expectation: Edit only `crates/core/src/settings.rs`.
- `pub fn load(path: &Path) -> Result<Settings, CoreError> { crate::state_file::load(path) }`
- `pub fn save(path: &Path, settings: &Settings) -> Result<(), CoreError> { crate::state_file::save(path, settings) }`
- Keep `temp_path`, `SAVE_SEQ` and their imports. Reword the doc comments on `save` and `temp_path` so they name `state_file::save` where they name `lock::write_whole`.
- In the test `save_round_trips_and_leaves_no_temp_file`, after `save`, add: read the file to a string and `assert!(text.ends_with("}\n"))`.
- Expected results: loading a path that does not exist gives `Settings::default()`; saving then loading gives the same `Settings`; the folder holds only `settings.json` afterwards; the file's last two characters are `}` and a newline.
- Do not edit any file under `crates/core/tests`.

Code map snippet (as on main at 19e0c65, cut short; the map revision may have reworded it):
```
`crates/core/src/settings.rs` · CORE-5 · present. The one settings file: pipeline repo clone, NextCloud backups folder, vault agents folder. `Settings::missing()` names what is not set; `load` gives defaults for a missing file. ...

// crates/core/src/state_file.rs
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError>;
```
