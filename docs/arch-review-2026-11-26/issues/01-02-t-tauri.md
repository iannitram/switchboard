What: Add the Tauri tree's structural test of `docs/code-map.md` and remove the two test functions under `app/src-tauri/src` that assert literal code map lines. After this, rewording the `### it` section of the code map no longer breaks a Tauri test; a Tauri path in the map that is not on disk, or an entry with no valid status, does. This issue is independent of {{#t-core}} and {{#t-react}}: the three can land in any order, because none of them changes the map.

Owner ruling (2026-11-26), quoted: "every test that pins code map wording, locked tests included, is replaced by structural tests. One structural test per tree (core, CLI, Tauri, React face)." And: "Only the assertions that read the real `docs/code-map.md` text go; every other test in those files stays untouched." Both files below carry a locked header ("The builder never edits this file (R28)"); this ruling is the permission to edit them, for these two functions only.

Where: `app/src-tauri/src/it_tickets_tests.rs`, `app/src-tauri/src/loadout_tests.rs`, new `app/src-tauri/src/code_map_tests.rs`, `app/src-tauri/src/commands.rs` (three new `mod` lines only). `docs/code-map.md` is NOT changed. (code map: the whole file is the subject; `### shell` holds the Tauri tree's one entry line, `### it` the two lines pinned today.)

Size: Medium (about 110 changed lines: 28 deleted, about 85 added; 4 files; Rust with the Tauri toolchain; Linux, with the system libraries the CI `app` job installs). Not Easy although it is small: it edits two locked test files and adds a new locked test.

Blocked by: nothing. The code map issues that name this one, {{#m-guard}} among them, are blocked by it.

Who writes the locked test: owner ruling (2026-11-26), the builder of this issue writes `app/src-tauri/src/code_map_tests.rs` and makes the two edits to the locked files named in section 1. No other role writes them first. Section 2 is the whole definition of the new tests: the builder adds no rule, drops none and loosens none. The reviewer checks the new test code against reading rules R1-R6 and the two numbered tests line by line, and checks the removals against the table in section 1.

## 1. Remove these test functions (the whole function with its `#[test]` line and the blank line before it; nothing else in the file)

Line numbers are on main at 19e0c65.

| File | Function | Lines | What it pins |
|---|---|---|---|
| `app/src-tauri/src/it_tickets_tests.rs` | `the_code_map_marks_it_present` | 255-267 (end of file) | the whole map line that starts "- IT-2 tickets for the app's fleet errors" and ends ": present." (map line 354) |
| `app/src-tauri/src/loadout_tests.rs` | `the_code_map_marks_it_present` | 165-177 (end of file) | the whole map line that starts "- IT-1 loadout every minute" and ends ": present." (map line 352) |

Both functions use only `include_str!`, so no import or helper becomes unused.

## 2. New file `app/src-tauri/src/code_map_tests.rs` (locked)

First lines, in the style of the files beside it: `// Locked tests (R28): the structure of docs/code-map.md for the Tauri tree. Written under the owner ruling of 2026-11-26; a builder edits this file only where an issue names the edit under an owner ruling.` and a line saying `commands.rs` takes it in. Take it in from `commands.rs` directly after the `loadout_tests` lines (lines 785-787):

```rust
#[cfg(test)]
#[path = "code_map_tests.rs"]
mod code_map_tests;
```

The file is self-contained and uses std only (no `use super::*`, no new dependency).

Reading rules (the same in every tree's test):
- R1. Read `docs/code-map.md` from disk under `Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")` (the repo root) and replace `\r\n` with `\n`.
- R2. A line that starts with three backticks opens or closes a fenced block. Fence lines and the lines inside a fenced block are skipped by every rule below.
- R3. A span is the text between the 1st and 2nd backtick of a line, the 3rd and 4th, and so on.
- R4. A tree span is a span that starts with the tree prefix `app/src-tauri/`.
- R5. An entry line is a line that starts with a backtick and whose first span is a tree span.
- R6. The line status is the first whole word `present` or `planned` on the line (case-sensitive; a whole word has no ASCII letter, digit or underscore directly before or after it).

Tests (one function each, with these names):
1. `every_tauri_path_in_the_code_map_is_on_disk`: every tree span names a path that exists under the repo root; a span ending in `/` must be a directory. A span on a line whose line status (R6) is `planned` is not checked. The test fails if it finds no tree span at all. The failure message lists every missing path with its line number. Today: 5 spans (`app/src-tauri/src` on line 7, `app/src-tauri/src/main.rs` on lines 119, 131 and 277, `app/src-tauri/` on line 384), all on disk.
2. `every_tauri_entry_has_a_valid_status`: split each entry line on the separator (space, U+00B7 MIDDLE DOT, space); at least one field after the first begins with `present` or `planned`, and the character after that word, if any, is not an ASCII letter, digit or underscore. The test fails if there is no entry line. Today: 1 entry line (384, under `### shell`), whose last field begins "present (empty window".

What these rules accept, so the code map issues that follow do not trip them:
- A `planned` entry may name a path that is not on disk yet. Test 1 skips every span on a line whose line status is `planned`.
- New `###` headings, and renamed or removed ones. No rule reads a heading, a section name or the order of lines.
- Reworded entry lines. No rule reads the sentence of an entry, only its paths and its status.

The machine-read blocks (`modules:`, `reuses:`, `Updated:`) are checked by the core tree's test in {{#t-core}}, not here.

## Not in this issue
- The check that every source file has a map line, the 400-byte line limit and the `#<number>` ban: owner ruling (2026-11-26), {{#m-guard}} turns them on, after every map section is rewritten. This issue checks only what holds on main today.
- Any other test in the two files, and any change to `commands.rs` beyond the three `mod` lines.

Done when:
1. The two functions in the table are gone; `git diff` of `it_tickets_tests.rs` and `loadout_tests.rs` shows only those deletions.
2. `grep -rn "code-map.md" app/src-tauri/src` names only `code_map_tests.rs`.
3. `app/src-tauri/src/code_map_tests.rs` has the two tests named above and the locked header, is taken in by `commands.rs`, and both tests pass against the map as it is on main. The PR body lists R1-R6 and the two tests, each with the line range of the test code that carries it, so the reviewer can check them line by line.
4. Each new test is shown to bite: the PR body gives a one-line change to `docs/code-map.md` in the working tree that makes it fail (an `app/src-tauri/` path that is not on disk; `· done (` in place of `· present (` on line 384) and the failing test output for each. It also shows the `planned` rule holds: with a new line `` `app/src-tauri/src/not_yet.rs` · planned · x `` added to the map, test 1 still passes. Each change is reverted and not committed.
5. `git diff --stat` shows no change to `docs/code-map.md`, `crates/` or `app/src/`.
6. `tools/ci.sh app` passes (clippy with `-D warnings` and `cargo test -p switchboard-app`), and `cargo fmt --all -- --check` passes (the first step of `tools/ci.sh rust`; it covers the app crate, a workspace member). Both outputs are in the PR body.
