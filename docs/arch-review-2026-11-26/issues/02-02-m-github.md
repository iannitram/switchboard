What: Rewrite the five `github-*` sections of `docs/code-map.md` so each of the eight files under `crates/core/src/github/` has one short entry line, and move the two facts the map alone holds into header doc comments. One of the code map revision issues (owner ruling, 2026-11-26); comments and docs only.

Where: `docs/code-map.md` lines 124-139 (code map: `### github-snapshot`, `### github-transport`, `### github-poll`, `### github-queue`, `### github-merge`; 2,214 bytes), and the header comments of `crates/core/src/github/live.rs` and `crates/core/src/github/merge.rs`.

Size: Easy (about 40 changed lines in 3 files; Rust and Node toolchains for the proof, no code change).

Blocked by: {{#m-core}}, {{#t-core}}, {{#t-react}} (`app/src/views/SkyRefresh.test.tsx:166-174` reads the `### github-poll` section and wants the old `· present.` wording).

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, each blocked by the one before it: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the second: {{#m-core}} has landed before it and {{#m-sky-core}} is next.

Line numbers in this issue are those of the map on main at commit 19e0c65. {{#m-core}} has rewritten the lines above this block, so the numbers have moved; it leaves the block itself as it is. The block to replace starts at the heading `### github-snapshot` and ends at the blank line before `### sky-features`.

What is there (line numbers as at 19e0c65):
- Five path-led lines: 125, 128, 131, 134, 137. One is over 400 bytes: line 131 (503 bytes), which also names two files (`poll.rs` and `live.rs`). Lines 125 and 131 hold issue numbers.
- Two bullets under `github-merge`: lines 138 and 139.
- Files with no line of their own: `crates/core/src/github/live.rs` (named only inside line 131), `crates/core/src/github/archive.rs` (at 19e0c65 named only in line 104, under `core-rules`, a bullet that {{#m-core}} removed), `crates/core/src/github/mod.rs` (not named at all).

Rules (owner ruling, 2026-11-26):
- Entry format: one line per source file, starting with the backticked repo-relative path: `` `path` · status · one sentence ``. At most 400 bytes (UTF-8; `·` is 2 bytes) and no `#` followed by digits.
- No bullets under a revised section. Fix history (issue numbers) is dropped; `CHANGELOG.md` has it.
- Behaviour detail cut from the map goes to the file's `//!` header comment unless the header or the doc comment on the named item already says it. For these files the doc comments already hold all of it except the two sentences in the brief.
- Do not change the `reuses:` and `modules:` blocks (lines 9-26) or any existing `###` heading. Set the first `Updated:` line to the day of the PR.
- The new headings `### github` and `### github-archive` are part of this issue; the structural code map tests from {{#t-core}} do not read headings.
- {{#m-core}} writes no line for these eight paths. It may have added a comment about `archive_repo` to the header of `crates/core/src/github/archive.rs`; leave that comment as it is.

Done when:
1. The block from `### github-snapshot` to the last line of the `github-merge` section is replaced by the text under "Expectation" below, and nothing else in the map changes except the `Updated:` date.
2. This prints 8:
   ```
   grep -c '^`crates/core/src/github/' docs/code-map.md
   ```
3. Each of these prints nothing:
   ```
   LC_ALL=C awk '/^`crates\/core\/src\/github/ && length($0) > 400' docs/code-map.md
   grep -nE '^`crates/core/src/github.*#[0-9]' docs/code-map.md
   sed -n '/^### github$/,/^### sky/p' docs/code-map.md | grep '^- '
   ```
4. `live.rs` and `merge.rs` have the header lines from the brief; `git diff --stat` shows only `docs/code-map.md` and those two files, and no change in a Rust file outside `//!` lines.
5. `tools/ci.sh rust` and `tools/ci.sh face` pass; their output is in the PR body.

## Qwen brief

Expectation: Replace the block from the heading `### github-snapshot` to the end of the `github-merge` section of `docs/code-map.md` (find it by the headings; an earlier map issue has moved the line numbers) with exactly this. Keep one blank line between sections, as now, and the blank line before `### sky-features`.

```
### github
`crates/core/src/github/mod.rs` · present · GitHub reads and writes: declares the seven github modules and re-exports the snapshot types.

### github-snapshot
`crates/core/src/github/snapshot.rs` · present · GH-1: `RepoSnapshot` (issues, PRs, milestones, labels, ROADMAP.md, the code map) and `SnapshotStore`, one JSON file per repo in the app data folder, with `age_minutes`.

### github-transport
`crates/core/src/github/transport.rs` · present · `GitHubTransport`: GraphQL, REST GET with `If-None-Match` and REST PATCH (`rest_patch`); `HttpTransport` takes its token from `platform::TokenStore`, `Recorded` replays answers in tests.

### github-poll
`crates/core/src/github/poll.rs` · present · GH-1, GH-2, GH-3: the 10-minute poll (`Poller::tick`), the Refresh button (`Poller::refresh_all`), the ETag skip (`unchanged`) and refresh-on-action (`Poller::after_write`).
`crates/core/src/github/live.rs` · present · GH-1, GH-3: the app's poll over the snapshot store in the app data folder: `repos` (which repos to read), `tick`, `refresh_all` and `after_write`.

### github-queue
`crates/core/src/github/queue.rs` · present · GH-4: every create goes through `fleet ghq`: `send`, `send_keyed` (each create's queue key) and `records` (the queue's own record of those items).

### github-merge
`crates/core/src/github/merge.rs` · present · GH-5, AUTO-4: `eligibility`, `merge` through `merge_pr.py` after `rules::guard`, `ready_to_merge`, `merge_passed` and `carried_over_by_patch_id`.

### github-archive
`crates/core/src/github/archive.rs` · present · CORE-3: `archive_repo` archives a repo on GitHub with one REST PATCH; nothing is ever deleted.
```

Then add these comment lines and nothing else:

At the end of the `//!` header of `crates/core/src/github/live.rs` (after line 2, before the blank line):
```
//! The poll thread is in app/src-tauri/src/main.rs: it calls `tick` every `TICK_EVERY_SECS` and
//! sends `SNAPSHOT_EVENT`. The Sky's Refresh button calls the Tauri command `refresh_all`.
```

At the end of the `//!` header of `crates/core/src/github/merge.rs` (after line 3, before the blank line):
```
//! `carried_over_by_patch_id` is called by the autopilot merge pass and by `switchboard merge`.
```

Set line 3 of the map to `Updated: <today's date, YYYY-MM-DD>`. Run `tools/ci.sh rust` and `tools/ci.sh face`; both must pass.

Code map snippet (the lines being replaced; the earlier map issue does not change them):
```
### github-snapshot
`crates/core/src/github/snapshot.rs` · GH-1 · present. `RepoSnapshot` (issues, PRs with verdicts and CI on head, milestones, labels, ROADMAP.md, the code map) and `SnapshotStore`: one JSON file per repo under the app's data folder, with its age, saved whole through a temp file of each save's own (`lock::write_whole`, #431).

### github-transport
`crates/core/src/github/transport.rs` · present. `GitHubTransport`: GraphQL, REST GET with `If-None-Match`, and REST PATCH (`rest_patch`: closes, body rewrites and the archive, the writes that are not creates; a transport that does not implement it refuses); `Recorded` replays answers in tests and logs each PATCH. Token from `platform::TokenStore`.

### github-poll
`crates/core/src/github/poll.rs`, `crates/core/src/github/live.rs` · GH-1, GH-2, GH-3 · present. The 10-minute poll (`Poller::tick`), the Refresh button (`refresh_all`), the ETag skip (`unchanged`), refresh-on-action (`after_write`). In the app (`live.rs`, #240): the repos it reads (the priority copy's order, then the rest of the snapshot store, by name); a thread in `app/src-tauri/src/main.rs` that ticks every minute and sends `snapshot`; the `refresh_all` command the Sky's Refresh button calls.

### github-queue
`crates/core/src/github/queue.rs` · GH-4 · present. `send(fleet, creates)`: one `fleet ghq add-batch` then `fleet ghq run`; a stop on a rate limit leaves items queued, never a failure. `send_keyed` also answers each create's queue key and whether the batch added it; `records(fleet, keys)` reads those items from the queue's own record (`queue.json` in the folder `fleet ghq status` names).

### github-merge
`crates/core/src/github/merge.rs` · GH-5 · present. `eligibility(pr, carried_over)` (PASS on head or carried by patch-id, CI green on head) and `merge` through `merge_pr.py` (dry run, then push) after `rules::guard`. Never `gh pr merge` or the merge API.
- AUTO-4 merge passed PRs (`ready_to_merge`, `merge_passed`): present.
- GH-5 carry-over by patch-id (`carried_over_by_patch_id`: the PR's own diff from main has the same `git patch-id --verbatim` at the PASS sha and the head; used by the merge pass and `switchboard merge`): present.
```
Signatures the lines must fit (they exist today): `mod.rs`: `pub mod archive`, `live`, `merge`, `poll`, `queue`, `snapshot`, `transport`. `snapshot.rs`: `pub struct RepoSnapshot`, `pub struct SnapshotStore`, `pub fn age_minutes`. `transport.rs`: `pub trait GitHubTransport` with `fn rest_patch`, `pub struct Recorded`, `pub struct HttpTransport`. `poll.rs`: `pub fn unchanged`, `pub struct Poller` with `tick`, `refresh_all`, `after_write`. `live.rs`: `pub fn repos`, `tick`, `refresh_all`, `after_write`, `pub const SNAPSHOT_EVENT`, `pub const TICK_EVERY_SECS`. `queue.rs`: `pub fn send`, `send_keyed`, `records`. `merge.rs`: `pub fn eligibility`, `merge`, `ready_to_merge`, `merge_passed`, `carried_over_by_patch_id`. `archive.rs`: `pub fn archive_repo`.
