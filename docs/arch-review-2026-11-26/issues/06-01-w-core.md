**What:** Add one core module, `crates/core/src/guarded.rs`, that holds the guarded-write sequence the CLI and MCP each carry a copy of today: resolve the repo, ask `rules::guard`, then merge, or file the request in Waiting on you. It returns `CoreError`; the faces only render. This issue adds the module and its tests and changes no face: {{#w-cli}} and {{#w-mcp}} move the faces onto it. Owner ruling (2026-11-26): the core module with repo resolution comes first, and where the two faces drifted the CLI rule wins.

**Where:**
- New: `crates/core/src/guarded.rs`, `crates/core/tests/guarded_writes.rs`.
- Changed: `crates/core/src/lib.rs` (add `pub mod guarded;` to the module list, lines 11-38), `docs/code-map.md` (section `### core-rules`).
- Read, not changed: `crates/core/src/rules.rs` (`guard`, lines 74-99; `GuardedAction`, `Caller`, `Verdict`), `crates/core/src/waiting.rs` (`guarded_item`, line 185; `LocalItems::add_new`, line 313), `crates/core/src/confirm.rs` (`request_text_for`, line 96; `drop_requests_for`, line 400; `confirm_into`, line 579; `Done`), `crates/core/src/github/snapshot.rs` (`SnapshotStore::load_each`, line 121; `Unreadable`, line 155), `crates/core/src/error.rs` (`CoreError`, lines 135-149).
- The copies this module replaces, for reference only (do not edit them here): `crates/cli/src/cli.rs` `trait Writes` (89-128), `repo_known` (132-136), `resolve_repo` (141-166), `CliWrites::full_repo` (289-297), `readable_snapshots` (375-387), `write_command` (946-1054), `drop_command` (1063-1116); `crates/cli/src/mcp.rs` `trait WriteCore` (40-86), `CoreWrites::full_repo` (159-186), `guarded_request` (701-735), `guarded_merge` (739-764).

**Size:** Hard (about 480 changed lines in 4 files: about 220 in the module, about 250 of tests; Rust toolchain). It is over the 300-line aim because the module and the tests that pin its rules land together; it is reviewed as an oversize change.

**Blocked by:** {{#f-module}} (owner ruling, 2026-11-26: the state-file module lands before the guarded writes; `guarded::file` writes through `LocalItems::add_new`, which {{#f-module}} moves onto that module).

**The interface (settled, do not redesign):**

1. The seam, one trait the faces' adapters and the test fakes implement:

   ```rust
   pub trait GuardedWrites {
       /// GH-5's merge of `repo#pr`. `repo` is as the caller typed it; the adapter resolves it.
       fn merge(&self, repo: &str, pr: u64) -> Result<(), CoreError>;
       /// Add one Waiting on you item; asking twice keeps the first.
       fn file(&self, item: Item) -> Result<(), CoreError>;
       /// The one `owner/name` that `repo` names among the repos Switchboard knows; `Ok(None)` when none.
       fn full_repo(&self, repo: &str) -> Result<Option<String>, CoreError>;
       /// One line for each request dropped. Default: `Err(CoreError::Refused("no Waiting on you list here"))`.
       fn drop_requests(&self, action: &GuardedAction, caller: &Caller) -> Result<Vec<String>, CoreError> { .. }
       /// The plain lines of what was done. Default: the same refusal as `drop_requests`.
       fn confirm_restore_new(&self, typed: &str, into: &str, caller: &Caller) -> Result<Vec<String>, CoreError> { .. }
   }
   ```

2. Repo resolution, pure:
   - `pub fn repo_known(repo: &str, known: &[String]) -> bool`: the rule of `cli.rs` `repo_known`.
   - `pub fn resolve_repo<'a>(repo: &str, known: impl IntoIterator<Item = &'a str>) -> Result<Option<&'a str>, CoreError>`: the rule of `cli.rs` `resolve_repo`, matches sorted and deduped. A bare name that matches more than one owner is `CoreError::Refused("{repo} matches {a} and {b}; name it as <owner>/{repo}")`, the text the CLI prints today.

3. Repo resolution over the app data folder (the CLI rule):
   - `pub fn snapshots_for(store: &SnapshotStore, repo: &str) -> Result<Vec<RepoSnapshot>, CoreError>`: the rule of `cli.rs` `readable_snapshots`. Only a snapshot file that does not read and could be `repo` (by `repo_known` on its `Unreadable::repo`) refuses, as `CoreError::Refused("{file.error}; run switchboard refresh")`. An unreadable snapshot of another repo does not refuse. An error from `load_each` passes through unchanged. No terminal escaping here: that is the CLI's.
   - `pub fn full_repo(data_dir: &Path, repo: &str) -> Result<Option<String>, CoreError>`: `snapshots_for`, then `resolve_repo`.

4. The Waiting on you write and the CLI's one confirm, over the app data folder:
   - `pub fn file(data_dir: &Path, item: Item) -> Result<(), CoreError>`: `LocalItems::new(data_dir).add_new(item)`.
   - `pub fn confirm_restore_new(settings: &Settings, data_dir: &Path, typed: &str, into: &str, caller: &Caller) -> Result<Done, CoreError>`: `confirm::request_text_for` for `GuardedAction::RestoreOverLive { repo: typed }`; no such request is `CoreError::Refused("restore over {typed}: no such request is waiting on you; nothing was changed")`; else `confirm::confirm_into(settings, data_dir, &text, typed, Some(into), caller)`.

5. The folder names an adapter puts in `CoreError::MissingPath` when a folder is not known, so no face matches text: `pub const DATA_FOLDER: &str = "the app data folder";` and `pub const CONFIG_FOLDER: &str = "the app config folder";`. Owner ruling (2026-11-26): a guarded write with the app data folder or the app config folder unset answers with the Error code `setting_missing`, not `refused`. `ErrorBody::from(&CoreError)` in `crates/core/src/error.rs` already gives that code for `MissingPath`, with the folder name as the `reason`; this module adds no code of its own. {{#w-cli}} and {{#w-mcp}} list the changed Error code in their PRs.

6. The sequence, in two calls a face makes in this order:
   - `pub fn request_text(action: &GuardedAction) -> String`: `merge {repo}#{pr}`, `archive {repo}`, `restore over {repo}` (the words `rules::guard` uses).
   - `pub fn resolve(action: GuardedAction, as_written: bool, writes: &dyn GuardedWrites) -> Result<Option<GuardedAction>, CoreError>`. A `Merge` comes back unchanged and `full_repo` is not called (the merge resolves the repo itself). For an archive or a restore: `full_repo` gives `Some(full)`, the action comes back naming `full`; `None` with `as_written` true and a `/` in the repo, the action comes back as typed (a drop of a repo Switchboard no longer knows); any other `None` is `Ok(None)` (the face says "no such repo" with the Error code `not_found`); an error passes through. No new `CoreError` variant is added.
   - `pub enum Asked { Merged, Waiting { refusal: String, filed: Result<(), CoreError> } }`
   - `pub fn ask(caller: &Caller, action: &GuardedAction, from: &str, now: UnixSeconds, writes: &dyn GuardedWrites) -> Result<Asked, CoreError>`, on `rules::guard(caller, action)`:
     - `Refuse(text)`: `Err(CoreError::Refused(text))`; nothing is called on `writes`.
     - `Allow` and a `Merge`: `writes.merge(repo, pr)`, then `Ok(Asked::Merged)` or the merge's error.
     - `Allow` and anything else: `Err(CoreError::Refused("{request_text}: needs a confirm in the app"))`; nothing runs (the guard allows only a merge from the CLI or MCP today).
     - `ToWaiting(text)`: `writes.file(waiting::guarded_item(action, "I was asked from {from} to {request_text}", now))`, then `Ok(Asked::Waiting { refusal: text, filed })`. A filing error is carried in `filed`, not returned, because the CLI prints it and then still refuses with `text`. This holds for every action, a merge included, so a merge sent to Waiting on you links to its PR and asking twice keeps the first item.
   - `from` is the face's own words (`the CLI`, `MCP`); the face passes them.

**Done when:**
1. `crates/core/src/guarded.rs` exists with exactly the items above, each with a doc comment, under a `//!` header comment that says what the module does, and `crates/core/src/lib.rs` declares `pub mod guarded;`.
2. `crates/core/tests/guarded_writes.rs` has tests, written against a fake `GuardedWrites` and a temp app data folder, that fail if any of these stops holding:
   - `resolve_repo`: a whole `owner/name` match; a bare name with one owner; a bare name with two owners gives the `matches ... and ...; name it as <owner>/...` refusal with the matches sorted; a name listed twice counts once; an unknown `owner/name` and an unknown bare name give `Ok(None)`.
   - `full_repo`: an unreadable snapshot of another repo does not refuse; an unreadable snapshot that could be the asked repo (by `owner/name`, and by bare name) refuses with a text ending `; run switchboard refresh`.
   - `resolve`: a `Merge` never calls `full_repo`; an archive by bare name comes back with the full `owner/name`; an unknown repo is `Ok(None)`; with `as_written` an unknown `owner/name` comes back as typed and an unknown bare name is `Ok(None)`.
   - `ask`: inside a job worktree nothing is called on the fake and the error text passes `rules::is_job_worktree_refusal`; a merge with the project-manager role calls `merge` once; a merge without the role calls nothing; an archive and a restore from `Face::Cli` and from `Face::Mcp` file one item whose `Source::GuardedAction` text is `I was asked from {from} to archive o/r` (or `restore over o/r`) and whose link is `https://github.com/o/r`; a filing error comes back inside `Asked::Waiting { filed: Err(..) }` beside the guard's text.
   - `file`: the same item filed twice leaves one item; `guarded_item` for a `Merge` filed through it links to `https://github.com/o/r/pull/7`.
   - `confirm_restore_new`: no waiting request gives the `no such request is waiting on you; nothing was changed` refusal.
   - The trait defaults of `drop_requests` and `confirm_restore_new` refuse.
   - `ErrorBody::from(&CoreError::MissingPath(guarded::DATA_FOLDER))` has the code `ErrorCode::SettingMissing` and the reason `the app data folder`; the same with `guarded::CONFIG_FOLDER` and `the app config folder`.
3. No file under `crates/cli` or `app` changes.
4. `docs/code-map.md`, section `### core-rules`, has one entry line for the new file in the map's form: `` `crates/core/src/guarded.rs` · present · one sentence ``. The line is under 400 bytes and holds no `#<number>`; the behaviour detail is in the file's `//!` header comment, not in the map. The structural code map test fails without this line, because every source file must be listed.
5. `tools/ci.sh rust` passes, and its output is in the PR body.
