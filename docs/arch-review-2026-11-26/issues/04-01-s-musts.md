What: Shorten the long Musts in `SPEC.md` without changing what any of them requires, take every issue number out of the Spec, and mark FD-4's learned pick as planned (owner ruling, 2026-11-26).

Line numbers are from main at 19e0c65; find each place by its Must id or its opening words if they moved.

Rules for the whole change:

- Strictly meaning-preserving. Every Must keeps its id. No Must is split, merged, renumbered or dropped.
- Every rule stays in its cell. Only mechanism detail moves out: file names, flag spellings, exact message text, field names, the order of internal steps. Mechanism detail goes into the `//!` header comment of the source file that implements it, in plain sentences, with no issue number.
- `CHANGELOG.md` is the only home for dated rulings and issue numbers. The rulings behind these cells are already there (lines 20 to 27), so nothing is added to `CHANGELOG.md` by this issue.

1. Cells over 800 characters. Bring each "Must" cell (the second column) under 800 characters:

   | Must | Line | Length now | Header comments that take the mechanism detail |
   |---|---|---|---|
   | CORE-3 | 77 | 1116 | `crates/core/src/rules.rs`, `crates/core/src/confirm.rs` |
   | BAK-3 | 147 | 1373 | `crates/core/src/backup.rs`, `crates/core/src/renumber.rs` |
   | FD-3 | 157 | 2135 | `crates/core/src/frontdoor/router.rs`, `crates/core/src/frontdoor/chat.rs` |
   | FD-5 | 159 | 858 | `crates/core/src/frontdoor/sticky.rs` |
   | AUTO-3 | 177 | 1021 | `crates/core/src/autopilot/review.rs`, `crates/core/src/autopilot/passes.rs` |

   Each of those files already has a `//!` header; add to it, do not replace it. Touch a header only when a clause really leaves the cell; a clause that is already said in the header needs no edit there.

   The code map revision adds to the same headers: {{#m-core}} (`rules.rs`, `confirm.rs`, `lock.rs`), {{#m-backup}} (`backup.rs`, `renumber.rs`), {{#m-frontdoor}} (`router.rs`, `chat.rs`, `sticky.rs`) and {{#m-autopilot}} (`review.rs`, `passes.rs`). Which lands first is not fixed and neither blocks the other. Add to each header as it stands on main when you start, and do not repeat a sentence it already has.

2. Issue numbers. `SPEC.md` has exactly ten `#<digits>` references, one per line: CORE-3 (line 77, `#426`), the paragraph that starts "**State files two processes write (#431).**" (line 81), BAK-3 (147, `#426`), FD-3 (157, `#433`), FD-5 (159, `#433`), FD-6 (160, `#433`), FD-12 (166, `#429`), AUTO-4 (178, `#432`), AUTO-5 (179, `#432`), AUTO-6 (180, `#432`). Remove all ten and reword the sentence around each so it states the current rule. The placeholders `#N` and `owner/name#N` (SKY-3, BAK-3) and `<repo>#<n>`, `<repo>#<pr>` (the `cli` Musts) are not issue numbers and stay.

3. Line 81, the "State files two processes write" paragraph (985 characters, not a table cell). Rewrite it shorter, meaning-preserving, without the issue number. Its mechanism detail goes to the header of `crates/core/src/lock.rs`. This issue is the only one that changes this paragraph.

4. FD-4 (line 158). `crates/core/src/frontdoor/router.rs` has `sample_size` (line 112) and `learned_pick` (line 127), and only tests call them (`crates/core/tests/fd_learned_pick.rs`, `fd_qwen_fallback.rs`, `fd_rating_log.rs`). Leave the FD-4 "Must" cell as it is. Append this sentence to FD-4's "Done looks like" cell: "Planned: the rule is written and tested, and routing does not use it yet." Wiring the pick into routing, or cutting it, is {{#x-fd4-learned-pick}}, not this issue.

   The code map must say the same. {{#m-frontdoor}} rewrites the `### frontdoor` section of `docs/code-map.md` to one line per file, and its `crates/core/src/frontdoor/router.rs` line says the learned pick is planned. Which of the two issues lands first is not fixed and neither blocks the other. Look at the section when you start:
   - It still has the bullet "- FD-4 learned pick (`sample_size`, `learned_pick`): present." (line 253 on main at 19e0c65; {{#m-frontdoor}} has not landed): change that bullet to "- FD-4 learned pick (`sample_size`, `learned_pick`): present, called only by tests; routing by the learned pick: planned." and set the code map's first `Updated:` line to the day of the PR. Change nothing else in the code map.
   - The bullet is gone and a line starts with `` `crates/core/src/frontdoor/router.rs` `` ({{#m-frontdoor}} has landed): check that this line says the FD-4 learned pick is planned. If it does, do not touch `docs/code-map.md`. If it does not, add that statement to the line, keeping its form (`` `path` · status · one sentence ``, status `present`, at most 400 bytes, no issue number), and set the first `Updated:` line to the day of the PR.

5. The PR body shows every changed Must (and the line 81 paragraph) as before and after, and under each one lists every clause that left the cell with the file whose header now holds it. A clause with no listed home fails review.

Where: `SPEC.md` (regions `core`, `backup`, `frontdoor`, `autopilot`); `docs/code-map.md`, section `### frontdoor`; and the header comments of up to ten files under `crates/core/src/` named above (the nine in the table and `lock.rs`). Code map section: `### frontdoor`.

Size: Medium (about 150 changed lines; documents and Rust comments only). The Spec edit is one file, but the header comments can bring the PR to twelve files. That is over the six-file aim, so this issue is reviewed as an oversize change; no code line other than `//!` comments changes.

Blocked by: none

No test reads `SPEC.md` or the FD-4 bullet of the code map, so this issue does not wait for the structural code map tests or for any code map issue.

Done when:
1. In `SPEC.md`, the "Must" cells of CORE-3, BAK-3, FD-3, FD-5 and AUTO-3 are each under 800 characters, and no table cell in the file is over 800 characters.
2. `SPEC.md` has no `#` followed directly by a digit anywhere in the file.
3. `SPEC.md` still has 68 Must rows with the same ids in the same order as main at 19e0c65.
4. The paragraph that starts "**State files two processes write" is shorter than 985 characters and carries no issue number.
5. FD-4's "Done looks like" cell ends with the "Planned:" sentence above, and the code map's frontdoor section says the FD-4 learned pick is planned (on the changed bullet, or on the `router.rs` line when {{#m-frontdoor}} landed first); nowhere does it say the learned pick is simply present.
6. The PR body has a before and after for each changed Must and for the line 81 paragraph, and a list of each moved clause with the file that now holds it.
7. `git diff` shows no change in any `.rs` file outside `//!` comment lines, and no change to `CHANGELOG.md`, `ROADMAP.md` or `GLOSSARY.md`.
8. `tools/ci.sh rust` and `tools/ci.sh lint` pass; their output is in the PR body.
