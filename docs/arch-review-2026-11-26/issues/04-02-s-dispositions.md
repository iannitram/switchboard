What: Delete the "Review dispositions" section of `SPEC.md` and put what only it says where it belongs; tell the prior art once, in `PEERS.md`; tell the platform plan once, in the code map (owner ruling, 2026-11-26).

Line numbers are from main at 19e0c65; find each place by its heading or opening words if they moved. Part 1 removes 56 lines from `SPEC.md`, so every `SPEC.md` line number after 316 shifts.

Part 1: Review dispositions (`SPEC.md` lines 317 to 372, 33 rows in four tables)

The disposition of every row is ruled. Do not re-sort them.

- 24 rows restate a Must, an Unknown or a Won't: delete them with the section.
- 2 rows are history (1.1 and 1.4): they go to `CHANGELOG.md`.
- 7 rows are after-1.0 ideas found nowhere else (4.1, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8): they become a short list under the after-1.0 table.
- 4 clauses fold into their Musts (from 1.2, 3.2, 4.9, 4.10).

Edits to `SPEC.md`:

1. Delete the heading `## Review dispositions` and everything under it, up to but not including `## Implementation decisions`.
2. Line 5: end the paragraph at "This file collapses them." and drop ", and rules on every review finding and feature idea (see [Review dispositions](#review-dispositions))".
3. U14 (line 297): remove the words " (the review's 3.1)".
4. The `local-only` row (line 222): replace "plus the six review ideas ruled inside its own Spec (see [dispositions](#review-dispositions))" with "plus the review ideas listed under this table, ruled inside its own Spec". Do not state a count.
5. Directly after the last row of that table (the `dashboard` row, line 227) and before "Moving CC into a repo is its own project", add:

   ```
   Review ideas ruled into the `local-only` Spec:

   - Brief compiler: enforces a size on each brief.
   - Readiness on the sky: stars show their Readiness, which supersedes a colour taken from the old Qwen-fit count.
   - Overnight Qwen shift.
   - Prefix-ordered Qwen queue.
   - Replay bench.
   - Ghost split stars.
   - Symbol fence.
   ```

6. GH-4 (line 92), third cell: append "The Roadmap files its issues in staggered batches through the queue."
7. AUTO-2 (line 176), third cell: append "The job writes one brief per issue and builders get only that; the brief compiler that enforces a size is in Local only (`local-only`)."
8. FD-9 (line 163), Must cell: change "which FD-4 does not count in 1.0" to "which FD-4 does not count until I rule on their weight after 1.0".
9. SKY-7 (line 107), third cell: append "Pricing the chain in Qwen-hours and paid points is After 1.0, with Readiness."

No Must changes its id. No other Must cell changes. {{#s-musts}} lands first; it changes neither these four rows nor the number of lines in `SPEC.md`, so the line numbers above hold after it.

Two clauses of the deleted table are stated by no Must: finding 2.6 (the core applies the FD-4 rule itself and keys chat rows on request kind, service and model only) and finding 2.10a (the instruction source changes only through a reviewed PR). It is decided that they go to `CHANGELOG.md` (the second line below) and are folded into no Must.

Edits to `CHANGELOG.md`: add two lines at the top of the list, dated the day the PR is opened, in the file's format:

- `- <date> Review dispositions removed from SPEC.md: 24 rows restated a Must and are deleted, 7 after-1.0 ideas are listed under the after-1.0 table, and 4 clauses are folded into GH-4, AUTO-2, FD-9 and SKY-7. History kept from the table: Tauri was proven to run on the Strix Halo computer on 2026-11-07 at 3:55pm (`shell`; Game Mode is U1), and the reviewed pipeline batch lands first, with Switchboard reading its fields (CORE-4).`
- `- <date> Two clauses of the deleted table that no Must states, kept here: the core applies the FD-4 rule itself, not `capability.py`'s proposals, and keys chat rows on request kind, service and model only (finding 2.6; routing by the FD-4 pick is still planned, {{#x-fd4-learned-pick}}); the instruction source changes only through a reviewed PR (finding 2.10a).`

The `{{#x-fd4-learned-pick}}` in that line is the number of the FD-4 follow-up issue; write it as `#<number>` in `CHANGELOG.md`.

Edit to `ROADMAP.md` line 33: "this follows `SPEC.md` 2.10b" becomes "this follows `SPEC.md` (SKY-2, SKY-3 and SKY-5)". The `features:` and `nodes:` blocks are not touched.

Part 2: prior art told once, in `PEERS.md`

1. `PEERS.md`: the twelve `peer:` lines (15 to 26) and the `upstream:` line stay byte for byte. Rename the heading on line 28 from "### Notes moved from SPEC.md `## Prior art`" to "### What we take from each". Keep the status paragraph (line 30). Under it, add the table now in `ROADMAP.md` lines 108 to 115 with three columns, `Project | What we take | Where it lands` (the license column goes; the `peer:` lines carry the license). Correct one row while moving it: for ocuclaw/obsidian-wayfinder, "What we take" is "Code adopted with credit (`NOTICE`): the dependency layers and in-column order (`app/src/sky/layers.ts`). Ideas: the takeable frontier". The `## Imported code` table is not touched.
2. `ROADMAP.md`, section "## Open source brought in" (lines 102 to 117): keep the heading, replace lines 104 to 115 (the intro sentence and the table) with "What we take from each peer is in `PEERS.md`." Keep the sentence about Rust crates (line 117) as it is.
3. `PIPELINE.md` line 6: "(the list is in SPEC.md, Prior art)" becomes "(the list is in PEERS.md)". Keep the spacing before the bracket.
4. `SPEC.md` "## Prior art" (lines 399 to 401) already only points at `PEERS.md`: leave it. `README.md` "Credits" stays as it is; `tools/check-notices.py` reads it.

Part 3: platform plan told once, in `docs/code-map.md`

The one home of the platform plan is `docs/code-map.md` (decided). {{#m-core}} rewrites the top of the code map around this section and leaves the section itself (heading, first sentence, table) as it is. Which of the two issues lands first is not fixed and neither blocks the other: in either order, find the section by its heading "## Platforms (R04)", make only the two edits of item 1, and leave every other line of the code map as you find it. If {{#m-core}} lands after this issue, it keeps these two edits because it does not change the section.

1. `docs/code-map.md`, section "## Platforms (R04)" (lines 69 to 82 on main at 19e0c65) is the one full telling. In its first sentence, change "(SteamOS on the Strix Halo computer)" to "(SteamOS on the Strix Halo computer, Desktop Mode; Game Mode is U1)". After the table, add: "The `platforms` feature (After 1.0) fills in the Mac and Windows seams and adds the dmg and nsis bundles." Set the file's first `Updated:` line to the day of the PR (leave it if main already has that day or a later one). Change nothing else in the code map.
2. `SPEC.md` "### Platform plan (R04)" (lines 263 to 265): keep the heading (`crates/core/src/platform.rs` cites it). Replace the paragraph with "1.0 ships one AppImage for Linux. The Mac and Windows seams, and what the `platforms` region fills in, are in [docs/code-map.md](docs/code-map.md), Platforms."
3. `ROADMAP.md` "## Platforms (R04)" (lines 119 to 121): keep the heading. Replace the paragraph with "The platform plan is in `docs/code-map.md`, Platforms. An issue that is specific to one OS carries a `platform:` label."
4. `README.md` line 11 becomes "- `ROADMAP.md`: the feature list and the build stages; `docs/code-map.md`: the modules and the platform plan."
5. Re-date `ROADMAP.md`'s `Updated:` line (line 3).

Where: `SPEC.md`, `ROADMAP.md`, `PEERS.md`, `PIPELINE.md`, `README.md`, `CHANGELOG.md`, `docs/code-map.md` (section "Platforms (R04)"). No source file changes.

Size: Medium (about 130 changed lines, 56 of them the deleted section; documents only). Seven files, one over the six-file aim: this issue is reviewed as an oversize change.

Blocked by: {{#s-musts}}

Both edit the `frontdoor` and `autopilot` tables of `SPEC.md`. No code map issue blocks this one; see Part 3.

Done when:
1. `SPEC.md` has no heading "Review dispositions", no link to `#review-dispositions`, and no finding number of the review (the text "the review's 3.1" is gone).
2. `SPEC.md` has the seven-line idea list directly after the after-1.0 regions table, and the `local-only` row points at it without a count.
3. GH-4, AUTO-2, FD-9 and SKY-7 each carry their folded clause as written above; `SPEC.md` still has 68 Must rows with the same ids in the same order.
4. `CHANGELOG.md` has the two new lines at the top of its list.
5. `PEERS.md` has the three-column table under "### What we take from each", its `peer:` and `upstream:` lines are unchanged in `git diff`, and the obsidian-wayfinder row names `app/src/sky/layers.ts`.
6. `ROADMAP.md` has no table of peer projects, no "2.10b", and its `features:` and `nodes:` blocks are unchanged in `git diff`.
7. `PIPELINE.md` line 6 names `PEERS.md` and no longer names "Prior art".
8. The list of seams (Python launcher, dev root, NextCloud folder, tokens, loadout probes, bundle) appears in `docs/code-map.md` only; `SPEC.md` and `ROADMAP.md` each keep their platform heading with the pointer sentence. In `git diff` for `docs/code-map.md`, the only changed lines are the first `Updated:` line, the first sentence under "## Platforms (R04)" and the one sentence added after its table.
9. `tools/ci.sh rust` and `tools/ci.sh lint` pass (three core tests read the real `ROADMAP.md`, and the notice check reads `README.md`); their output is in the PR body.
