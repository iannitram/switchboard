What: Add the React face's structural test of `docs/code-map.md` and remove the nine tests under `app/src/views` that assert literal code map text. After this, rewording a code map line no longer breaks a face test; an `app/src/` path in the map that is not on disk, or an entry with no valid status, does. This issue is independent of {{#t-core}} and {{#t-tauri}}: the three can land in any order, because none of them changes the map.

Owner ruling (2026-11-26), quoted: "every test that pins code map wording, locked tests included, is replaced by structural tests. One structural test per tree (core, CLI, Tauri, React face)." And: "Only the assertions that read the real `docs/code-map.md` text go; every other test in those files stays untouched." All nine files below carry a locked header ("the builder never edits this file"); this ruling is the permission to edit them, for these tests and their one import only.

Where: nine files in `app/src/views/`, new `app/src/code-map.test.ts`. `docs/code-map.md` is NOT changed. (code map: the whole file is the subject; `### sky-face` and `### sky-layers` hold the face's two entry lines.)

Size: Medium (about 130 changed lines: 59 deleted, about 70 added; 10 files; TypeScript, vitest, Node). This is over the usual limit of 6 files. It is not split further: the owner ruling is one issue per tree. It is reviewed as an oversize change. Not Easy: it edits nine locked test files and adds a new locked test.

Blocked by: nothing. The code map issues that name this one, {{#m-guard}} among them, are blocked by it.

Who writes the locked test: owner ruling (2026-11-26), the builder of this issue writes `app/src/code-map.test.ts` and makes the edits to the nine locked files named in section 1. No other role writes them first. Section 2 is the whole definition of the new tests: the builder adds no rule, drops none and loosens none. The reviewer checks the new test code against the two reading steps (the map, the files on disk), rules R2-R6 and the two numbered tests line by line, and checks the removals against the table in section 1.

## 1. Remove these tests and the `codeMap` import

In each file remove the whole `test(...)` call named here with one blank line beside it, and the line `import codeMap from "../../../docs/code-map.md?raw";`. Leave line 1 (`/// <reference types="vite/client" />`) and everything else as it is. Line numbers are on main at 19e0c65.

| File in `app/src/views/` | Test name | Lines | Import line |
|---|---|---|---|
| `Budget.test.tsx` | "the code map marks the panel present" | 145-147 | 5 |
| `Loadout.test.tsx` | "the code map marks IT-1's timer present" | 237-243 | 7 |
| `PoolEditor.test.tsx` | "the code map marks the editor present" | 154-156 | 5 |
| `Reports.test.tsx` | "the code map marks the view present" | 211-213 | 6 |
| `ReportsFilter.test.tsx` | "the code map marks the filter bar present" | 168-170 | 6 |
| `Settings.test.tsx` | "the code map shows core-settings present" | 148-154 | 7 |
| `SkyRefresh.test.tsx` | "the code map has github-poll present, with live.rs" | 166-174 | 7 |
| `Waiting.test.tsx` | "the code map marks the view present" | 135-137 | 6 |
| `WorkTree.test.tsx` | "the code map marks the work tree present" | 70-72 | 4 |

In every file `codeMap` is used only inside the removed test, so the import must go: `npm run build` runs `tsc --noEmit` with `noUnusedLocals` over the test files. In `Loadout.test.tsx` the removed test sits between two others; the test after it ("gemini is the Antigravity CLI: a missing agy is not found, never not signed in") stays. In the other eight files the removed test is the last one in the file.

## 2. New file `app/src/code-map.test.ts` (locked)

First lines: `/// <reference types="vite/client" />`, then `// Locked tests (R28): the structure of docs/code-map.md for the React face. Written under the owner ruling of 2026-11-26; a builder edits this file only where an issue names the edit under an owner ruling.`

How it reads the map and the disk, with no new dependency (`@types/node` is not installed, and `tsc --noEmit` type-checks test files, so do not import `node:fs`):
- The map: `import codeMap from "../../docs/code-map.md?raw";`, with `\r\n` replaced by `\n`.
- The files on disk: `Object.keys(import.meta.glob("./**/*"))`. The keys are paths relative to `app/src/`, such as `./views/Budget.tsx`; nothing is loaded. A map path `app/src/<rest>` that does not end in `/` is on disk when `./<rest>` is a key. A map path that ends in `/` is on disk when some key starts with `./<rest>`; this covers the bare span `app/src/` (line 119), where `<rest>` is empty.
- `.css` files under `app/src` are source files: the code map issues give each one a map line, and the every-file check in {{#m-guard}} counts them. This test treats them like any other file. The glob pattern `./**/*` has no extension filter, so `app/src/theme.css`, `app/src/sky/roadmap.css` and `app/src/sky3d/sky3d.css` are keys, and a map path that names one is checked on disk the same way as a `.ts` or `.tsx` path. The test has no list of extensions and no special case for `.css`. No `.css` path is in the map today.

Reading rules (the same in every tree's test):
- R2. A line that starts with three backticks opens or closes a fenced block. Fence lines and the lines inside a fenced block are skipped by every rule below.
- R3. A span is the text between the 1st and 2nd backtick of a line, the 3rd and 4th, and so on.
- R4. A tree span is a span that starts with the tree prefix `app/src/` (this does not match `app/src-tauri/`).
- R5. An entry line is a line that starts with a backtick and whose first span is a tree span.
- R6. The line status is the first whole word `present` or `planned` on the line (case-sensitive; a whole word has no ASCII letter, digit or underscore directly before or after it).

Tests (with these names):
1. "every face path in the code map is on disk": every tree span names a path that is on disk as defined above. A span on a line whose line status (R6) is `planned` is not checked. The test fails if it finds no tree span at all. The failure message lists every missing path with its line number. Today: 15 spans on 9 lines (87, 119, 169, 172, 184, 195, 207, 235, 384), all on disk: `app/src/views/Settings.tsx`, `app/src/`, `app/src/views/Sky.tsx`, `app/src/sky3d/Sky3D.tsx`, `app/src/sky/`, `app/src/sky/layers.ts`, `app/src/views/Waiting.tsx`, `app/src/views/Reports.tsx`, `app/src/views/Budget.tsx`, `app/src/views/Chat.tsx`, `app/src/App.tsx`, `app/src/core.ts`, `app/src/ErrorBanner.tsx`, `app/src/brocrew/` (twice).
2. "every face entry has a valid status": split each entry line on the separator (space, U+00B7 MIDDLE DOT, space); at least one field after the first begins with `present` or `planned`, and the character after that word, if any, is not an ASCII letter, digit or underscore. The test fails if there is no entry line. Today: 2 entry lines (169 under `### sky-face`, 172 under `### sky-layers`), both pass.

What these rules accept, so the code map issues that follow do not trip them:
- A `planned` entry may name a path that is not on disk yet. Test 1 skips every span on a line whose line status is `planned`.
- New `###` headings, and renamed or removed ones. No rule reads a heading, a section name or the order of lines.
- Reworded entry lines. No rule reads the sentence of an entry, only its paths and its status.
- A map line for a `.css` file, as said above.

The machine-read blocks (`modules:`, `reuses:`, `Updated:`) are checked by the core tree's test in {{#t-core}}, not here.

## Not in this issue
- The check that every source file has a map line, the 400-byte line limit and the `#<number>` ban: owner ruling (2026-11-26), {{#m-guard}} turns them on, after every map section is rewritten. This issue checks only what holds on main today.
- `app/src/brocrew.test.ts` and its `?raw` imports: they read source files, not the code map, and stay.
- Any change to `app/package.json`, `app/tsconfig.json` or `app/vite.config.ts`.

Done when:
1. The nine tests in the table and the nine `codeMap` import lines are gone; `git diff` of those nine files shows only those deletions.
2. `grep -rn "code-map.md" app/src` names only `app/src/code-map.test.ts`.
3. `app/src/code-map.test.ts` has the two tests named above and the locked header, and both pass against the map as it is on main. The PR body lists the two reading steps, R2-R6 and the two tests, each with the line range of the test code that carries it, so the reviewer can check them line by line.
4. Each new test is shown to bite: the PR body gives a one-line change to `docs/code-map.md` in the working tree that makes it fail (an `app/src/` path that is not on disk; `· done.` in place of `· present.` on line 172) and the failing test output for each. It also shows two accepted cases hold: with a new line `` `app/src/not_yet.ts` · planned · x `` added to the map, test 1 still passes, and with a new line `` `app/src/theme.css` · present · x `` it still passes. Each change is reverted and not committed.
5. `git diff --stat` shows no change to `docs/code-map.md`, `crates/`, `app/src-tauri/`, `app/package.json` or `app/package-lock.json`.
6. `tools/ci.sh face` passes (`npm ci`, `npm run build` with `tsc --noEmit`, `npm test`); its output is in the PR body.
