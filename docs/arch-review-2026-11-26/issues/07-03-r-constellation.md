**What:** Move the Constellation read into core as a read model that returns the answer plus warnings (owner ruling, 2026-11-26).

Line numbers below are as the files read at the time of writing; earlier issues move lines in `crates/cli/src/cli.rs` and `crates/cli/src/main.rs`. Find each item by name.

Today only MCP asks for one Constellation: `Core::call` (`crates/cli/src/main.rs` 79-84) calls `cli::constellation_answer` (`crates/cli/src/cli.rs` 2238-2255), which loads the sky model, resolves the repo name, and serializes the matching `Constellation` (`crates/core/src/sky/model.rs`, struct at line 44). A repo that is not there is Error code `NotFound` with the text `switchboard: no such repo: <repo>`. There is no CLI command and no Tauri command for a Constellation (`crates/core/src/faces.rs` lists it for MCP only).

No Tauri command is added. The window draws a Constellation from the `sky` answer it already holds (`ConstellationView` in `app/src/sky/ConstellationView.tsx`, used by `app/src/views/Sky.tsx`), so its warnings are the Sky view's line from {{#r-sky}}. Nothing under `app/` changes in this issue.

Build this:
1. `crates/core/src/read/constellation.rs` (declare it in `crates/core/src/read/mod.rs`):
   - `pub struct ConstellationAnswer { pub constellation: Option<Constellation>, pub no_snapshot: bool, pub warnings: Vec<Warning> }`.
   - `pub fn load(data: &Path, ledger: Option<&[PathBuf]>, now: UnixSeconds, repo: &str) -> Result<ConstellationAnswer, CoreError>`.
   - It calls `read::sky::load` from {{#r-sky}} and passes its warnings through. So a fleet job ledger that does not read is a warning here too, and the Constellation is returned without its jobs in flight (owner ruling, 2026-11-26: that is a warning in every face). No existing test pins a failure of the MCP `constellation` tool on such a ledger, so no test is edited for it. When that answer's `model` is `None`, it returns `constellation: None, no_snapshot: true`.
   - Otherwise it resolves `repo` with `switchboard_core::guarded::resolve_repo` from {{#w-core}} (`resolve_repo(repo, known) -> Result<Option<&str>, CoreError>`; `known` is the model's repo names). `Ok(None)` (unknown repo) gives `constellation: None, no_snapshot: false`. A bare name that matches two owners is that function's `CoreError::Refused` (`<repo> matches <a> and <b>; name it as <owner>/<repo>`), returned as the error.
   - No new `CoreError` variant: "not there" is the `None`, and the face gives it Error code `NotFound`.
2. `constellation_answer` in cli.rs is deleted. `Core::call` finds the ledger files as the sky command does after {{#r-sky}} and calls the read model. The face keeps the usage error `constellation: repo is required`, the `NO_SNAPSHOT` refusal (when `no_snapshot`), the wording `switchboard: no such repo: <repo>` with Error code `NotFound`, and the two-owners refusal, whose text is `guarded::resolve_repo`'s, rendered as `Core::call` renders that refusal after {{#w-cli}}.
3. MCP reply: the first content item is the serialized `Constellation` exactly as today; warnings ride in the second content item added by {{#r-waiting}}.
4. Code map: one entry line for `crates/core/src/read/constellation.rs` under `### sky-model`, in the map's entry form: `` `<path>` · present · <one sentence> ``, under 400 bytes and with no `#<number>` (the structural code map tests, with the guard {{#m-guard}} turned on, fail a line that breaks this and fail a source file that has no line). The new file starts with a `//!` header that says what it is for.

**Where:** `crates/core/src/read/constellation.rs` (new), `crates/core/src/read/mod.rs`, `crates/cli/src/cli.rs`, `crates/cli/src/main.rs`, new test `crates/core/tests/read_constellation.rs`, `docs/code-map.md`. Code map sections: `### sky-model`, `### mcp`.

**Size:** Easy (about 130 changed lines in 6 files; Rust only).

**Blocked by:** {{#r-sky}} (calls its read model and uses its ledger file list), {{#r-waiting}} (the warning type and the MCP warnings item). Owner ruling (2026-11-26): the seven read-model issues land one at a time in the order waiting, sky, constellation, star, reports, budget, errors. The guarded-writes issues, `guarded::resolve_repo` among them, have landed before the first of the seven.

**Done when:**
1. `crates/cli/src/cli.rs` has no `constellation_answer`; `Core::call` uses `switchboard_core::read::constellation::load`.
2. `crates/core/tests/read_constellation.rs` has four tests showing: a bare repo name and `owner/name` both find the Constellation; an unknown repo gives `constellation` `None` with `no_snapshot` false; no snapshot gives `no_snapshot` true; a corrupt snapshot file of another repo gives the asked Constellation plus a warning whose `file` is that file's path.
3. These pass unedited: `crates/cli/tests/sky_github_cli.rs` (`star_and_the_mcp_constellation_agree_on_the_job_in_flight`, `the_job_in_flight_is_found_through_pipeline_repo_in_the_settings`); `crates/cli/tests/mcp_read.rs` (all); the in-file test `unknown_tool_repo_and_star_are_errors` in `crates/cli/src/main.rs`.
4. `docs/code-map.md` has the entry line for `crates/core/src/read/constellation.rs` in the form step 4 gives, and `crates/core/tests/code_map_structure.rs` passes.
5. `tools/ci.sh rust` passes; its output is in the PR body.

## Qwen brief

**Expectation:** After this change `grep -n constellation_answer crates/cli/src/cli.rs crates/cli/src/main.rs` prints nothing. `crates/core/src/read/constellation.rs` exists with the function `load` and the struct `ConstellationAnswer`. `cargo test -p switchboard-core --test read_constellation` passes its four tests. The two named tests in `crates/cli/tests/sky_github_cli.rs` and all of `crates/cli/tests/mcp_read.rs` pass with no edit to those files. `docs/code-map.md` has one new line under `### sky-model` that starts with `` `crates/core/src/read/constellation.rs` · present · ``, is under 400 bytes and holds no `#<number>`. `tools/ci.sh rust` exits 0. No file under `app/` is touched, and `crates/core/src/error.rs` is not touched.

**Code map snippet** (the start of two entries of `docs/code-map.md` as they read on main before the map revision, which lands before this issue and rewrites both in the entry form above; read the sections as they are when you start):

```
### sky-model
`crates/core/src/sky/model.rs` · SKY-1 to SKY-9 · present. The SkyModel the `sky` command answers: the snapshot store and the priority copy in the app data folder, with the jobs in flight from the fleet ledger (`load_with_ledger`), assembled from the sky modules; no snapshot is `GitHub: no snapshot yet`. [...]

### mcp
`crates/cli/src/mcp.rs` · MCP-1 to MCP-3 present. JSON-RPC over stdio; six read tools; role read tools (`role_read_tools`: the satellite's `errors`, #375, the CLI's `errors --json`); role write tools (the project-manager's `merge`, `request_archive`, `request_restore`, `drop_request`; the budget-manager's pool tools); neither inside a job worktree. Bad lines (non-UTF-8, batch, non-object) get a JSON-RPC error; tool arguments are checked against each tool's input schema.
```
