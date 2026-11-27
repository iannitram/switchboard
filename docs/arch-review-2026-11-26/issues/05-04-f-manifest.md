What: The Repo backup's `manifest.json` is read with `state_file::load` and its final write in `build` goes through `state_file::save`. Owner ruling (2026-11-26): the manifest uses `load`/`save` only; it takes no lock today and takes none after this (whether it needs one is {{#x-lock-gaps}}, not this issue).

Where: `crates/core/src/backup.rs`: `read_manifest` (964-974) and the last two statements of `build` before `Ok(manifest)` (884-885). Line numbers are from main at 19e0c65; find by function name if they moved. (code map: section `### backup`)

Size: Easy (about 15 changed lines in 1 file; Rust)

Blocked by: {{#f-module}}

Independent of open issue #503.

Possible bug, reported here and not changed silently: `backup.rs:885` writes the final manifest with a plain `std::fs::write(&path, pretty(&manifest)?)`. That is not a whole write: a run that dies mid-write leaves half a `manifest.json` in the staging folder, over the whole one `write_json` put there (line 866). The staging folder (`.<name>.new-<run>`) is renamed into place only after `build` returns, so no case is known where the half file reaches the live backup; that is why it is a possible bug and not a known one. This issue replaces that line with `state_file::save`, which writes a temp file and renames it. The PR body says so in one line under a heading "Behaviour change".

Done when:
1. `read_manifest` keeps its signature (`Result<Option<Manifest>, CoreError>`); its body is `crate::state_file::load::<Option<Manifest>>(&folder.join("manifest.json"))`.
2. In `build`, lines 884-885 become `crate::state_file::save(&staging.join("manifest.json"), &manifest)?;`. No `std::fs::write(&path, pretty(&manifest)` is left in the file.
3. Untouched: `write_json` (its `manifest.json` at line 85 is one of six files written as `<name>.new` and renamed together, and stays that way, with no trailing newline), `write_release_list`, `pretty`, `io_error`, `manifest_in` (the only caller of `read_manifest`), and `last_backup_now` (it reads the last backup's manifest with its own code and keeps doing so).
4. Stays the same: no lock; a folder with no `manifest.json` gives `None`; a damaged manifest is `CoreError::Parse("<path>: <e>")`; an unreadable one is `CoreError::Io("<path>: <e>")`.
5. Changes on purpose: the final `manifest.json` of a backup ends in one newline (owner ruling: the module always writes a trailing newline) and is written through a temp file; a `manifest.json` that holds only the word `null` reads as no manifest, so `manifest_in` still refuses it, now with `CoreError::Io("<folder>: no manifest.json, not a backup")` where today it is a `CoreError::Parse` (Switchboard never writes `null`; the owner ruling that keeps `null` a parse error names only `StateStore` and `SnapshotStore`); a manifest that does not serialize gives `CoreError::Parse("<path>: <e>")` (today the text has no path).
6. No test changes, and these stay green: `crates/core/tests/backup_repo.rs` (`a_backup_writes_one_folder_with_a_verified_bundle_the_json_the_wiki_and_the_manifest`, `a_second_backup_replaces_the_last`), `crates/core/tests/backup_safety.rs` (`a_damaged_manifest_in_the_last_backup_refuses_the_new_one`, `the_manifest_holds_the_bundles_hash`), `crates/core/tests/backup_json.rs`, `crates/core/tests/restore_history.rs`.
7. `tools/ci.sh rust` passes; its output is in the PR body.

## Qwen brief
Expectation: Two edits in `crates/core/src/backup.rs`, nothing else.
- `read_manifest(folder)`: the whole body becomes `crate::state_file::load::<Option<Manifest>>(&folder.join("manifest.json"))`. Expected: a folder without the file gives `Ok(None)`; a file cut in half gives `Err(CoreError::Parse(..))` whose text contains `manifest.json`.
- In `build`: replace `let path = staging.join("manifest.json"); std::fs::write(&path, pretty(&manifest)?).map_err(|e| io_error(&path, e))?;` with `crate::state_file::save(&staging.join("manifest.json"), &manifest)?;`. Expected: the backup folder holds the same file names as before (no temp file left), and `manifest.json` ends in `}\n`.
- Do not touch `write_json`. Do not edit any file under `crates/core/tests`, and do not edit the `mod tests` in `backup.rs`.

Code map snippet (as on main at 19e0c65, cut short; the map revision may have reworded it):
```
`crates/core/src/backup.rs` · BAK-1, BAK-2, BAK-3 · present. A git bundle of every ref plus JSON of issues, PRs, comments, labels, milestones, releases (under 1 GB) and the wiki, with a manifest, in `<NextCloud backups>/github/<repo>/`; ...

// crates/core/src/state_file.rs
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError>;
```
