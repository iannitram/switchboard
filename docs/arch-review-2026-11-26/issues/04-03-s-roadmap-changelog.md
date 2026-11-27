What: Cut the "Current state" block of `ROADMAP.md` to one sentence with no issue ledger, and add to `CHANGELOG.md` the lines missing since 2026-11-18 (owner ruling, 2026-11-26). The history moves to the changelog; the roadmap stops repeating it.

Line numbers are from main at 19e0c65; find each place by its opening words if they moved.

1. `ROADMAP.md` lines 52 to 62: the block starts "**Current state (2026-11-21).**" and ends with the bullet that starts "- Core, Budget, and Sky after-1.0:". Replace the whole block (the paragraph, "Open for **1.0**:" with its three bullets, and "Open **After 1.0 / Runner / Follow-ups**:" with its three bullets) with this one line:

   ```
   **Current state.** Stages 1 to 7 are merged; what is left before 1.0 is stage 8 (the design pass) and whatever is still open in the `1.0` milestone on GitHub, and the history is in `CHANGELOG.md`.
   ```

   The next paragraph ("After 1.0 the dark stars above get their own Specs and issues ...") stays as it is.
2. `ROADMAP.md` line 3: re-date `Updated:` to the day the PR is opened.
3. Do not touch the `features:` block, the `nodes:` block, the `unspecified:` block or the "Development stages" table. Tests read them.
4. `CHANGELOG.md`: insert these nine lines, in this order, directly above the line that starts "- 2026-11-18 Claude log folders (BUD-4)". The text is final; copy it as it is.

   ```
   - 2026-11-22 Peers file (#515): the upstream and the peer list are in `PEERS.md` at the repo root; `SPEC.md`'s Prior art section and `AGENTS.md` point at it.
   - 2026-11-21 Run scoring (#412): `runner/score.rs` samples runs by each scorer's `sample_rate`, writes one `ScoreRow` per sampled run and scorer, batches the failed ratings into one proposed spec PR and adds one Waiting on you item.
   - 2026-11-21 Bug loop, live (#411): `runner/bug_loop.rs` runs `specs/bug-loop.json` through the engine and the owner steps (`drive_bug`, `run_end_to_end`).
   - 2026-11-21 Budget view (#502): Claude's reading and its labelled estimate are shown in the Budget view, the CLI and MCP (BUD-4).
   - 2026-11-21 Roadmap run, live (#407): `runner/roadmap_run.rs` runs `specs/roadmap-run.json` (`drive_unit`, `run_frontier`); after 3 failed builds (`MAX_BUILD_ATTEMPTS`) a unit is parked with one Waiting on you item.
   - 2026-11-21 Fix file rules (#486): `bug::fix::glob_match` applies the harness glob rules to the files a fix may change; the `test-not-edited-by-fix` invariant in `specs/bug-loop.json` is reworded.
   - 2026-11-21 Unit claim follow-ups (#482, #506, #507): `unit/claim.rs` reconciles a lost push reply and restores the mirror; the claim record in `specs/roadmap-run.json` gains `token`, and `claimed_at` is an integer.
   - 2026-11-21 Documents (#508): ROADMAP and the code map brought in line with the merged runner nodes; documents only.
   - 2026-11-18 Stable error codes (#491): `error.rs` holds `ErrorCode` and `ErrorBody`; the CLI prints `{"error": ..}` on stdout under `--json`, MCP's read tools return the same `ErrorBody`, and GLOSSARY gains "Error code".
   ```

5. {{#s-dispositions}} lands first. It adds two lines at the top of the list in `CHANGELOG.md` and re-dates `ROADMAP.md`; it changes no line of `ROADMAP.md` in the block of item 1. Leave its two changelog lines where they are; the nine lines go below them, found by the "Claude log folders (BUD-4)" line and not by a line number.

Where: `ROADMAP.md` (lines 3 and 52 to 62), `CHANGELOG.md` (above the "Claude log folders (BUD-4)" line; line 5 on main at 19e0c65, two lines lower after {{#s-dispositions}}). No code map section covers these two documents; the map lines that describe the code reading `ROADMAP.md` are pasted below.

Size: Easy (about 25 changed lines in 2 files; documents only).

Blocked by: {{#s-dispositions}}

Both edit the top of the list in `CHANGELOG.md` and the `Updated:` line of `ROADMAP.md`.

Done when:
1. `ROADMAP.md` has exactly one line that starts "**Current state", it is the sentence above, and it has no `#` followed by a digit.
2. `ROADMAP.md` no longer has the lines "Open for **1.0**:" or "Open **After 1.0 / Runner / Follow-ups**:".
3. `git diff` for `ROADMAP.md` shows changes only on the `Updated:` line and in the replaced block.
4. `CHANGELOG.md` has the nine lines, word for word, directly above the "Claude log folders (BUD-4)" line, and `git diff` for the file shows no other change.
5. `tools/ci.sh rust` passes (`roadmap_graph`, `sky_features` and `sky_stages` read the real `ROADMAP.md`); its output is in the PR body.

## Qwen brief

Expectation: after the change, `grep -c '^\*\*Current state' ROADMAP.md` prints `1`; `grep -c '^Open ' ROADMAP.md` prints `0`; `grep -c '(#412)\|(#411)\|(#502)\|(#407)\|(#486)\|(#482, #506, #507)\|(#508)\|(#491)\|(#515)' CHANGELOG.md` prints `9`; `tools/ci.sh rust` ends with every test passing. Only `ROADMAP.md` and `CHANGELOG.md` appear in `git diff --stat`.

Code map snippet (as on main at 19e0c65; the code map revision, {{#m-core}} and {{#m-sky-core}}, rewords these lines, and this issue does not touch the code map in either order):

```
  src/roadmap/                the roadmap graph (ROADMAP's nodes and unspecified blocks, node spec files), its lint, its drift against the issues, and the units plan (issues and blocked-by links)

### sky-features
`crates/core/src/sky/features.rs` · SKY-2, SKY-3, SKY-5 · present. Reads ROADMAP.md's `Updated:` and `features:` block (`parse`; `release: 1.0`, `after-1.0` or `after-2.0`) and the `Development stages` table (`parse_stages`, #392).
```

These parts of `ROADMAP.md` are read by code, which is why they must not change. The "Current state" block and `CHANGELOG.md` are read by no code.
