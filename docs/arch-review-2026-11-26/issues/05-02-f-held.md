What: `HeldRecords` (the record of what each held restore made, `<data>/waiting/held-restores.json`) reads and changes its file through `crate::state_file` and keeps no read, lock or write code of its own. Behaviour does not change.

Where: `crates/core/src/confirm.rs`: `HeldRecords::load` (481-489) and `HeldRecords::set` (499-524). Line numbers are from main at 19e0c65; find by function name if they moved. (code map: section `### core-rules`, the entry that names `crates/core/src/confirm.rs`)

Size: Easy (about 30 changed lines in 1 file; Rust)

Blocked by: {{#f-module}}

Independent of open issue #503.

Done when:
1. `HeldRecords::load` keeps its signature; its body is `crate::state_file::load(&self.path)`.
2. `HeldRecords::set` keeps its signature and its first check (lines 501-503: dropping a record that is not there returns before any lock or folder is made). The rest is one `crate::state_file::change(&self.path, self.wait, Some("a change to the held restores"), ...)` whose closure computes `changed` exactly as lines 513-516 do and returns `Ok(((), changed))`.
3. Between `pub struct HeldRecords` and the end of its `impl`, `confirm.rs` has no `to_string_pretty`, `write_whole`, `FileLock` or `create_dir_all`.
4. Stays the same: locked with the same wait (`self.wait`, default `waiting::LOCAL_LOCK_WAIT`) and the same holder text; no file gives an empty map; no change writes nothing; the file ends in one newline (it does today); an emptied map is written as `{}` (the file is not deleted); `get` and the `wait` field are untouched.
5. No test changes, and these stay green: in `crates/core/tests/confirm_guarded.rs`: `a_second_confirm_of_a_restore_that_held_comments_back_only_runs_the_held_pass`, `an_unreadable_record_of_held_restores_stops_the_confirm_before_the_restore`, `dropping_a_held_restore_removes_its_record_too`, `an_unreadable_record_stops_the_drop`; inside `confirm.rs`: `held_restores_recorded_at_once_all_keep_their_record`.
6. `tools/ci.sh rust` passes; its output is in the PR body.

## Qwen brief
Expectation: Only `HeldRecords::load` and `HeldRecords::set` in `crates/core/src/confirm.rs` change. After the change:
- `load`: `crate::state_file::load(&self.path)`.
- `set(text, made)`: first, unchanged, `if made.is_none() && !self.load()?.contains_key(text) { return Ok(()); }`. Then:
  `crate::state_file::change(&self.path, self.wait, Some("a change to the held restores"), |all: &mut BTreeMap<String, Made>| { let changed = match made { Some(made) => all.insert(text.to_string(), made.clone()).as_ref() != Some(made), None => all.remove(text).is_some() }; Ok(((), changed)) })`.
- Expected results: setting the same `Made` twice writes once; dropping the last record leaves a file holding `{}` and a newline; a file holding `{ nope` gives `CoreError::Parse` whose text contains `held-restores.json`.
- Do not edit any file under `crates/core/tests`, and do not edit the `mod tests` in `confirm.rs`.

Code map snippet (as on main at 19e0c65, cut short; the map revision may have reworded it):
```
- CORE-3, BAK-3 confirm in the app (`crates/core/src/confirm.rs`: `guarded_request`, `confirm_with`, `confirm`, `Done`, `drop_request`, `drop_requests_for`, `Dropped`; ...

// crates/core/src/state_file.rs
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError>;
pub fn change<T, R>(path: &Path, wait: Duration, holder: Option<&str>,
    change: impl FnOnce(&mut T) -> Result<(R, bool), CoreError>) -> Result<R, CoreError>
where T: DeserializeOwned + Default + Serialize;
```
