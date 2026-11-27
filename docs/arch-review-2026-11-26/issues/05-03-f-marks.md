What: `Marks` (the new repos Switchboard made for a restore and has not finished, `<data>/waiting/new-repo-restores.json`) reads and changes its file through `crate::state_file`. Owner ruling (2026-11-26): the rule "an empty record deletes the file" stays, and stays in `backup.rs`.

Where: `crates/core/src/backup.rs`: `Marks::load` (1522-1530), `Marks::save` (1537-1552), `Marks::change` (1556-1569). Line numbers are from main at 19e0c65; find by function name if they moved. (code map: section `### backup`)

Size: Easy (about 45 changed lines in 1 file; Rust)

Blocked by: {{#f-module}}

Independent of open issue #503.

Done when:
1. `Marks::load` keeps its signature; its body is `crate::state_file::load(&self.path)`.
2. `Marks::change` keeps its signature (`impl FnOnce(&mut BTreeMap<String, Mark>)`, returns `Result<(), CoreError>`). Its body is one `crate::state_file::change(&self.path, MARKS_LOCK_WAIT, None, ...)`. `None` keeps today's lock, which is taken with `FileLock::take` and no holder text.
3. Inside that closure, in this order: clone the map; run the caller's change; if the map equals the clone return `Ok(((), false))`; if the map is now empty, remove the file there (a missing file is not an error; any other failure is `CoreError::Io("<path>: <e>")`) and return `Ok(((), false))`; otherwise return `Ok(((), true))`. The removal therefore still happens under the lock.
4. `Marks::save` is removed; only `Marks::change` called it.
5. Stays the same: the lock and its 10 s wait (`MARKS_LOCK_WAIT`); no file gives an empty map; no change writes nothing; an emptied record deletes the file; the file ends in one newline (it does today); `get`, `begin`, `finished`, `clear` are untouched. The imports at the top of `backup.rs` stay (`lock` and `FileLock` are still used elsewhere in the file).
6. No test changes, and these stay green: in `crates/core/tests/restore_history.rs`: `a_retry_into_the_repo_switchboard_made_finishes_the_restore_and_clears_the_record` (asserts the file is gone), `a_retry_runs_only_the_steps_that_did_not_finish`, `a_record_finishes_only_the_private_repo_with_the_id_it_made`, `a_retry_finds_the_record_whatever_the_case_of_the_name`; inside `backup.rs`: `restores_at_once_keep_every_record_and_step`.
7. `tools/ci.sh rust` passes; its output is in the PR body.

## Qwen brief
Expectation: Only the three `Marks` functions named above change, all in `crates/core/src/backup.rs`. `load` becomes `crate::state_file::load(&self.path)`, `save` is deleted, and `Marks::change` reads:
```rust
fn change(&self, change: impl FnOnce(&mut BTreeMap<String, Mark>)) -> Result<(), CoreError> {
    crate::state_file::change(
        &self.path,
        MARKS_LOCK_WAIT,
        None,
        |all: &mut BTreeMap<String, Mark>| {
            let before = all.clone();
            change(all);
            if *all == before {
                return Ok(((), false));
            }
            if all.is_empty() {
                // An empty record deletes the file, still under the lock.
                return match std::fs::remove_file(&self.path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        Err(CoreError::Io(format!("{}: {e}", self.path.display())))
                    }
                    _ => Ok(((), false)),
                };
            }
            Ok(((), true))
        },
    )
}
```
Expected results: after `begin` then `clear` for the same repo, `new-repo-restores.json` does not exist; after `begin` for two repos and `clear` for one, the file holds one record and ends in a newline; `finished` with a step already recorded writes nothing. Do not edit any file under `crates/core/tests`, and do not edit the `mod tests` in `backup.rs`.

Code map snippet (as on main at 19e0c65, cut short; the map revision may have reworded it):
```
`crates/core/src/backup.rs` · BAK-1, BAK-2, BAK-3 · present. A git bundle of every ref plus JSON of issues, PRs, comments, labels, milestones, releases (under 1 GB) and the wiki, with a manifest, in `<NextCloud backups>/github/<repo>/`; ...

// crates/core/src/state_file.rs
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;
pub fn change<T, R>(path: &Path, wait: Duration, holder: Option<&str>,
    change: impl FnOnce(&mut T) -> Result<(R, bool), CoreError>) -> Result<R, CoreError>
where T: DeserializeOwned + Default + Serialize;
```
