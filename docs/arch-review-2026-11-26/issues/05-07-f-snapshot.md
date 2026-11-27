What: `SnapshotStore` (the cached GitHub read, one JSON file per repo under `<data>/snapshot/`) reads and writes each file through `crate::state_file`. It takes no lock today and takes none after this (owner ruling, 2026-11-26: each store's locking stays as it is; whether the stores that take no lock need one is {{#x-lock-gaps}}, not this issue). Owner ruling (2026-11-26): a snapshot file that holds only the JSON word `null` stays unreadable (a parse error), as it is today, and the store checks that itself.

Where: `crates/core/src/github/snapshot.rs`: `SnapshotStore::save` (98-105), the free function `read_one` (173-182), one new private type beside `read_one`. Line numbers are from main at 19e0c65; find by function name if they moved. `docs/code-map.md` (code map: section `### github-snapshot`).

Size: Easy (about 60 changed lines in 2 files, more than half of it new tests; Rust)

Blocked by: {{#f-module}}

Independent of open issue #503.

Done when:
1. `SnapshotStore::save(snap)` keeps its signature; its body is `crate::state_file::save(&self.path_for(&snap.repo), snap)` (it writes the `RepoSnapshot` itself, so the file's shape does not change). Its doc comment names `state_file::save` where it names `lock::write_whole`.
2. `snapshot.rs` has one new private type, `struct Saved(Option<RepoSnapshot>)`, with `#[derive(Default)]` (so no file is `Saved(None)`) and a hand-written `Deserialize` that parses a `RepoSnapshot` and wraps it in `Some`. This is how the store keeps the `null` rule: `state_file::load` gives the default only when there is no file; any file that exists must parse as a whole `RepoSnapshot`, and `null` does not, so it is a `CoreError::Parse` naming the file, as today. `Option<RepoSnapshot>` is not used as the type `load` reads, because it would read `null` as "no file".
3. `read_one(path)` keeps its signature (`Result<Option<RepoSnapshot>, CoreError>`); its body is `crate::state_file::load::<Saved>(path).map(|saved| saved.0)`.
4. `load`, `load_all`, `load_each`, `path_for`, `Unreadable`, `repo_of`, `age_minutes` are untouched. `load_each` still lists the folder itself and still names each file that does not read, a file holding only `null` among them.
5. Stays the same: no lock; no file is `None`; a file that does not parse is `CoreError::Parse` naming the file, and a file holding only `null` is one of those; `save` makes the snapshot folder and writes through a temp file of its own, so two saves of one repo at once each land whole and leave no temp file.
6. Changes on purpose: a snapshot file now ends in one newline (owner ruling: the module always writes a trailing newline); when the folder cannot be made, the `CoreError::Io` text starts with the snapshot file's path instead of the folder's; a snapshot that does not serialize gives `CoreError::Parse("<path>: <e>")` (today the text has no path). The PR body lists these three and nothing else as changed.
7. A new `#[cfg(test)] mod tests` in `snapshot.rs` (the file has none today, and no test in the repo covers a `null` snapshot file today) holds two tests. The first saves one `RepoSnapshot` and asserts the file's text ends in `}\n` and `load` gives it back. The second proves the `null` rule: with `null` as the whole text of `path_for("owner/demo")`, `load("owner/demo")` is `CoreError::Parse` whose text contains the file's path, and `load_each` gives no snapshot and one `Unreadable` whose `repo` is `owner/demo`.
8. If the `github/snapshot.rs` code map line names `lock::write_whole`, it names `state_file` instead; the line stays under 400 bytes with no `#<number>`. A line that does not name it is left alone.
9. No file under `crates/core/tests` changes, and these stay green: `crates/core/tests/gh_snapshot_store.rs` (among them `save_creates_the_snapshot_folder_and_load_reads_it_back`, `load_is_none_when_nothing_was_saved`, `save_replaces_the_old_snapshot_and_leaves_no_temp_file`, `load_of_a_file_that_is_not_json_is_a_parse_error`), `crates/core/tests/gh_snapshot_store_review.rs`, `crates/core/tests/gh_snapshot_unreadable.rs`, and `crates/core/tests/state_files_431.rs` `saves_of_one_repo_s_snapshot_at_once_each_land_whole`.
10. `tools/ci.sh rust` and `tools/ci.sh lint` pass; their output is in the PR body.

## Qwen brief
Expectation: In `crates/core/src/github/snapshot.rs`:
- Add this private type above `read_one` (`Deserialize` is already imported in the file):
```rust
/// What `state_file::load` reads for this store. No file is `Saved(None)`: no snapshot yet.
/// A file must hold a whole `RepoSnapshot`; one holding only `null` is a parse error.
#[derive(Default)]
struct Saved(Option<RepoSnapshot>);

impl<'de> Deserialize<'de> for Saved {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        RepoSnapshot::deserialize(from).map(|snap| Saved(Some(snap)))
    }
}
```
- `save`: `crate::state_file::save(&self.path_for(&snap.repo), snap)`. Reword its doc comment so it names `state_file::save` where it names `lock::write_whole`.
- `read_one`: `crate::state_file::load::<Saved>(path).map(|saved| saved.0)`. Do not use `Option<RepoSnapshot>` as the type `state_file::load` reads.
- Add at the end of the file a test module with two tests, each in a fresh folder under `std::env::temp_dir()` named with `crate::lock::run_tag()`, with `SnapshotStore::new(&dir)`.
  - Test 1: save `RepoSnapshot { repo: "owner/demo".into(), ..Default::default() }` (`RepoSnapshot` derives `Default`), read `store.path_for("owner/demo")` as text, assert it ends in `}\n`, and assert `store.load("owner/demo")` gives `Some` of the same snapshot.
  - Test 2: make the folder `dir.join("snapshot")`, write the text `null` to `store.path_for("owner/demo")`; `store.load("owner/demo")` is `Err(CoreError::Parse(text))` where `text` contains `owner__demo.json`; `store.load_each()` gives an empty list of snapshots and one `Unreadable` whose `repo` is `owner/demo`.
- Expected results: a missing file gives `Ok(None)`; a saved snapshot reads back equal; `null` in the file is an error and is named by `load_each`, never skipped.
- In `docs/code-map.md`, only if the `github/snapshot.rs` line says `lock::write_whole`, say `state_file` there.
- Do not edit any file under `crates/core/tests`.

Code map snippet (as on main at 19e0c65; the map revision may have reworded it):
```
`crates/core/src/github/snapshot.rs` · GH-1 · present. `RepoSnapshot` (issues, PRs with verdicts and CI on head, milestones, labels, ROADMAP.md, the code map) and `SnapshotStore`: one JSON file per repo under the app's data folder, with its age, saved whole through a temp file of each save's own (`lock::write_whole`, #431).

// crates/core/src/state_file.rs
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, CoreError>;
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError>;
```
