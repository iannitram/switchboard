What: Rewrite the nine core `sky-*` sections of `docs/code-map.md` (`sky-features` to `sky-model`) so each of the ten files under `crates/core/src/sky/` has one short entry line, and move the one fact the map alone holds into a doc comment. One of the code map revision issues (owner ruling, 2026-11-26); comments and docs only. The face's sky sections (`sky-face`, `sky-layers`) are {{#m-sky-face}}.

Where: `docs/code-map.md` lines 141-166 (code map: `### sky-features`, `### sky-layout`, `### sky-drift`, `### sky-chain`, `### sky-codemap`, `### sky-priority`, `### sky-labels`, `### sky-jobs`, `### sky-model`; 3,155 bytes), and one doc comment in `crates/core/src/sky/model.rs`.

Size: Easy (about 35 changed lines in 2 files; Rust and Node toolchains for the proof, no code change).

Blocked by: {{#m-github}}, {{#t-core}} (`crates/core/tests/sky_model.rs:759-769` reads the line under `### sky-model` and wants the old `· present.` wording).

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, each blocked by the one before it: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the third: {{#m-core}} and {{#m-github}} have landed before it and {{#m-sky-face}} is next.

Line numbers in this issue are those of the map on main at commit 19e0c65. The two earlier map issues have rewritten the lines above this block, so the numbers have moved; they leave the block itself as it is. The block to replace starts at the heading `### sky-features` (it now follows the `### github-archive` section) and ends at the blank line before `### sky-face`.

What is there (line numbers as at 19e0c65):
- Nine path-led lines: 142, 145, 148, 151, 154, 157, 160, 163, 166. Three are over 400 bytes: 142 (582), 154 (555), 166 (943). Lines 142, 154 and 166 hold issue numbers.
- No bullets.
- File with no line of its own: `crates/core/src/sky/mod.rs`.
- Nearly all the detail the long lines carry is already in the source: the stage-table rules are on `parse_stages` (`features.rs:118-121`), the code map check rules on `check_modules` (`codemap.rs:113-117`) and in the `codemap.rs` header (lines 16-18), the in-flight rule on `in_flight` (`jobs.rs:10-14`), and the stages and node-layer rules on the fields and functions of `model.rs` (lines 53-60, 101-102, 129-130, 213-215). One fact is only in the map: how a stage's `done`, `total` and `complete` are counted. `pub struct SkyStage` (`model.rs:79`) has no doc comment; the brief adds one.

Rules (owner ruling, 2026-11-26):
- Entry format: one line per source file, starting with the backticked repo-relative path: `` `path` · status · one sentence ``. At most 400 bytes (UTF-8; `·` is 2 bytes) and no `#` followed by digits.
- Fix history (issue numbers) and "Tests: ..." tails are dropped; `CHANGELOG.md` has the history.
- Do not change the `reuses:` and `modules:` blocks (lines 9-26; line 18 lists `sky/codemap.rs` under the node `unit-proof` and stays) or any existing `###` heading. Set the first `Updated:` line to the day of the PR.
- The node specs `roadmap/nodes/unit-proof.json` and `roadmap/nodes/sky-roadmap-graph.json` name `crates/core/src/sky/codemap.rs` and `crates/core/src/sky/model.rs` as code map entries, and `code_map_lines` in `crates/core/src/unit/job.rs` finds them by a line that starts with the backticked path. Both keep such a line.
- The new heading `### sky` is part of this issue; the structural code map tests from {{#t-core}} do not read headings.
- No earlier map issue writes a line for these ten paths.

Done when:
1. The block from `### sky-features` to the `sky-model` entry line is replaced by the text under "Expectation" below, and nothing else in the map changes except the `Updated:` date.
2. This prints 10:
   ```
   grep -c '^`crates/core/src/sky/' docs/code-map.md
   ```
3. Each of these prints nothing:
   ```
   LC_ALL=C awk '/^`crates\/core\/src\/sky/ && length($0) > 400' docs/code-map.md
   grep -nE '^`crates/core/src/sky.*#[0-9]' docs/code-map.md
   ```
4. `git diff --stat` shows only `docs/code-map.md` and `crates/core/src/sky/model.rs`, and the only change in `model.rs` is the three `///` lines from the brief.
5. `tools/ci.sh rust` and `tools/ci.sh face` pass; their output is in the PR body.

## Qwen brief

Expectation: Replace the block from the heading `### sky-features` to the `sky-model` entry line of `docs/code-map.md` (find it by the headings; earlier map issues have moved the line numbers) with exactly this. Keep one blank line between sections, as now, and the blank line before `### sky-face`.

```
### sky
`crates/core/src/sky/mod.rs` · present · Constellation: declares the nine sky modules; everything the Sky view draws is worked out here as pure functions.

### sky-features
`crates/core/src/sky/features.rs` · present · SKY-2, SKY-3, SKY-5: reads ROADMAP.md's `Updated:` date and `features:` block (`parse`) and its Development stages table (`parse_stages`).

### sky-layout
`crates/core/src/sky/layout.rs` · present · SKY-1, SKY-2: where each constellation sits and how big (`place`), and how bright (`brightness`).

### sky-drift
`crates/core/src/sky/drift.rs` · present · SKY-5: drift (`find`): orphan stars, `release: 1.0` dark stars and a roadmap more than `STALE_DAYS` (14) days old.

### sky-chain
`crates/core/src/sky/chain.rs` · present · SKY-3, SKY-7: the blocked-by lines between stars (`edges`) and the longest open chain of them (`longest`).

### sky-codemap
`crates/core/src/sky/codemap.rs` · present · SKY-6 and the code map check: reads this file's `reuses:` block (`reuses`, `is_warning`) and `modules:` block (`modules`); `check_modules` judges a change's files against the `modules:` lines of its node.

### sky-priority
`crates/core/src/sky/priority.rs` · present · SKY-9: the priority copy in fleet-state (`parse`), its age (`age_days`, `age_line`) and whether it is stale after `STALE_DAYS` (7) days (`is_stale`).

### sky-labels
`crates/core/src/sky/labels.rs` · present · SKY-10: the `feature:` label for a design-pass issue (`design_pass_label`) and for an automatic bug (`bug_label`).

### sky-jobs
`crates/core/src/sky/jobs.rs` · present · SKY-4, SKY-8: the fleet jobs in flight, read from the fleet ledger's events (`in_flight`).

### sky-model
`crates/core/src/sky/model.rs` · present · SKY-1 to SKY-9: the `SkyModel` the `sky` command answers, assembled from the snapshot store, the priority copy and the jobs in flight (`build`, `load`, `load_with_ledger`).
```

Then, in `crates/core/src/sky/model.rs`, add these three lines directly above the `#[derive(Debug, Clone, PartialEq, Serialize)]` line that precedes `pub struct SkyStage` (line 78), and change nothing else in the file:

```
/// One development stage of the ROADMAP's stages table, with its issue counts. An issue on
/// features in two stages counts once, in the first. `complete` is a stage that has at least
/// one issue and every one of them closed.
```

Set line 3 of the map to `Updated: <today's date, YYYY-MM-DD>`. Run `tools/ci.sh rust` and `tools/ci.sh face`; both must pass.

Code map snippet (the lines being replaced; the earlier map issues do not change them):
```
### sky-features
`crates/core/src/sky/features.rs` · SKY-2, SKY-3, SKY-5 · present. Reads ROADMAP.md's `Updated:` and `features:` block (`parse`; `release: 1.0`, `after-1.0` or `after-2.0`) and the `Development stages` table (`parse_stages`, #392). Stages are in numeric order; the leading bold phrase supplies a stage name, and backtick-delimited ids in the Features cell supply membership. A feature repeated in later stages belongs to the first stage; prose such as "all with a view" assigns no features. Missing tables return `None`. Reader and model checks: `crates/core/tests/sky_stages.rs`.

### sky-layout
`crates/core/src/sky/layout.rs` · SKY-1, SKY-2 · present. Rank to radius, size and angle; brightness from the `1.0` milestone and `release: 1.0` dark stars (`after-1.0` and `after-2.0` are not counted).

### sky-drift
`crates/core/src/sky/drift.rs` · SKY-5 · present. Orphan stars, `release: 1.0` dark stars (`after-1.0` and `after-2.0` are not drift), a roadmap more than 14 days old.

### sky-chain
`crates/core/src/sky/chain.rs` · SKY-3, SKY-7 · present. `Blocked by #N` edges and the longest open chain.

### sky-codemap
`crates/core/src/sky/codemap.rs` · SKY-6, unit-proof (#405) · present. This file's `reuses:` block; a reuse of a Parked or archived repo is a warning. `modules` reads the `modules:` block; `check_modules(node, touched, base, head)` is the code map check: a file the change adds needs a `modules:` line of its node that covers it (a path ending in `/` covers its directory); a covered file's line must read `present`; and the change must touch docs/code-map.md with its `Updated:` date not older than the base's. Tests: `crates/core/tests/unit_proof.rs`.

### sky-priority
`crates/core/src/sky/priority.rs` · SKY-9 · present. The priority copy in fleet-state (U4's default), its age, stale after 7 days.

### sky-labels
`crates/core/src/sky/labels.rs` · SKY-10 · present. The `feature:` label for a design-pass issue and an automatic bug.

### sky-jobs
`crates/core/src/sky/jobs.rs` · SKY-4, SKY-8 · present. The fleet jobs in flight from the fleet ledger's events (a `start` with no later `end` from the same machine), the job `star` names.

### sky-model
`crates/core/src/sky/model.rs` · SKY-1 to SKY-9 · present. The SkyModel the `sky` command answers: the snapshot store and the priority copy in the app data folder, with the jobs in flight from the fleet ledger (`load_with_ledger`), assembled from the sky modules; no snapshot is `GitHub: no snapshot yet`. Development stages carry done / total unique issues and completion (a nonempty stage with every issue closed); the optional `stages` field is omitted when there is no stages table, preserving older sky answers. The `nodes:` block (#413, `roadmap::graph::parse`) is layered by longest path (layer 0 has no `after`): `Constellation.nodeLayers` is the layer count and `Star.nodeLayer` the layer of the node its `node:<id>` label names, both omitted unless the block is usable. A cycle, an `after` naming no node, a repeated id or a line that does not parse gives no layers and never fails the sky. Tests: `crates/core/tests/sky_nodes.rs`.
```
Signatures the lines must fit (they exist today): `features.rs`: `pub fn parse`, `pub fn parse_stages`. `layout.rs`: `pub fn place`, `pub fn brightness`. `drift.rs`: `pub const STALE_DAYS: i64 = 14`, `pub fn find`. `chain.rs`: `pub fn edges`, `pub fn longest`. `codemap.rs`: `pub fn reuses`, `pub fn modules`, `pub fn check_modules`, `pub fn is_warning`. `priority.rs`: `pub const STALE_DAYS: i64 = 7`, `pub fn parse`, `age_days`, `age_line`, `is_stale`. `labels.rs`: `pub fn design_pass_label`, `pub fn bug_label`. `jobs.rs`: `pub fn in_flight`. `model.rs`: `pub struct SkyModel`, `pub struct SkyStage` (fields `done`, `total`, `complete`), `pub fn build`, `load`, `load_with_ledger`. `mod.rs` declares `chain`, `codemap`, `drift`, `features`, `jobs`, `labels`, `layout`, `model`, `priority`.
