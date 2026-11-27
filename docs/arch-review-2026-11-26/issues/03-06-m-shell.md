What: Give every non-test source file under `app/src-tauri/src` and `app/src` its own line in `docs/code-map.md`, and rewrite the `### shell` section's one 725-byte line into them. On main at 19e0c65 neither of the 2 files in `app/src-tauri/src` and only 2 of the 59 files in `app/src` have a line that starts with their path; the map issues before this one write 36 more, and this issue writes the 23 that are left.

Where: `docs/code-map.md` (code map: section `### shell`, and one line under `### it`), plus header comments in `app/src-tauri/src/main.rs`, `app/src-tauri/src/commands.rs` and `app/src/App.tsx` only where detail cut from the map is not already there.

Map lines today (main at 19e0c65; sizes in bytes, line ends not counted): the `### shell` heading is line 383 and the section is one entry line, 384 (725 bytes, one issue number). It starts with the directory `app/src-tauri/` and also names `app/src/App.tsx`, `app/src/core.ts` and `app/src/ErrorBanner.tsx`. The only `app/` files with a line of their own are `app/src/views/Sky.tsx` (line 169) and `app/src/sky/layers.ts` (line 172).

A source file here is a `.rs`, `.ts`, `.tsx` or `.css` file that is not a test (`*_tests.rs`, `*.test.ts`, `*.test.tsx`). Stylesheets count: each of the three `.css` files under `app/src` gets a map line, and the every-file check in {{#m-guard}} uses the same rule. Fonts, `LICENSE`, `README.md` and `OFL.txt` are not source files.

Source files with no map line on main at 19e0c65 (58; `app/src/views/Chat.tsx` is the 59th):
- `app/src-tauri/src/` (2): `main.rs`, `commands.rs`
- `app/src/` (9): `App.tsx`, `ErrorBanner.tsx`, `core.ts`, `dev-core.ts`, `dev-fixtures.ts`, `external-links.ts`, `main.tsx`, `report-bug.ts`, `theme.css`
- `app/src/brocrew/` (9): `AssistantMessage.tsx`, `PromptComposer.tsx`, `ReportEntry.tsx`, `UserMessage.tsx`, `WaitingItem.tsx`, `composer-notice.ts`, `ipc-types.ts`, `message-links.ts`, `message-paths.ts`
- `app/src/views/` (8): `Budget.tsx`, `Empty.tsx`, `Loadout.tsx`, `Reports.tsx`, `Settings.tsx`, `Waiting.tsx`, `WorkTree.tsx`, `index.ts`
- `app/src/sky/` (12): `ConstellationView.tsx`, `DriftList.tsx`, `ReuseLines.tsx`, `RoadmapView.tsx`, `SkyMap.tsx`, `StarPanel.tsx`, `chain.ts`, `geometry.ts`, `model.ts`, `roadmap.css`, `roadmap.ts`, `stars.ts`
- `app/src/sky3d/` (18): `Sky3D.tsx`, `SkyScene.ts`, `anchor.ts`, `camera.ts`, `galaxy.ts`, `labels.ts`, `layout.ts`, `milkyway.ts`, `motion.ts`, `nebula.ts`, `optics.ts`, `physics.ts`, `post.ts`, `random.ts`, `sky3d.css`, `support.ts`, `wayfind.ts`, `wink.ts`

This issue is the catch-all for the two trees. The map issues that land before it write 38 of the 59 `app/src` lines:
- {{#m-core}}: `app/src/views/Settings.tsx`
- {{#m-sky-face}}: `app/src/views/Sky.tsx`, the 13 files of `app/src/sky/` and the 18 files of `app/src/sky3d/` (both stylesheets among them)
- {{#m-bug-waiting-reports}}: `app/src/views/Waiting.tsx`, `Reports.tsx`, `WorkTree.tsx`
- {{#m-budget}}: `app/src/views/Budget.tsx`
- {{#m-frontdoor}}: `app/src/views/Chat.tsx`

That leaves 23 files for this issue: the 2 of `app/src-tauri/src/`, the 9 directly in `app/src/`, the 9 of `app/src/brocrew/`, and `app/src/views/Empty.tsx`, `Loadout.tsx` and `index.ts`. When you start, list the files with no line and write only those. If the loop prints a file outside the 23, write its line too, by the placement in rule 2, and say so in the PR body. Never add a second line for a path that has one.

```
for f in $(find app/src app/src-tauri/src -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.css' \) ! -name '*_tests.rs' ! -name '*.test.ts' ! -name '*.test.tsx' | sort); do grep -q "^\`$f\`" docs/code-map.md || echo "$f"; done
```

Rules (owner ruling, 2026-11-26; none of this is open):
1. Each file gets exactly one line that starts with its backticked repo-relative path, in the form `` `path` · status · one sentence `` (the separator is space, U+00B7 MIDDLE DOT, space). Status is `present`. The sentence says what the file owns, with its Must ids where it has them. Take it from the file's own header comment where there is one. Of the 23 files, 4 have none and need reading; each starts with an import: `app/src/App.tsx`, `app/src/main.tsx`, `app/src/views/Loadout.tsx`, `app/src/views/index.ts`. The line is at most 400 bytes and holds no `#<number>`; several header comments carry issue numbers (`app/src/ErrorBanner.tsx`, `app/src/theme.css`), which do not go into the map line.
2. Placement. Under `### shell`: `app/src-tauri/src/main.rs`, `commands.rs`, `app/src/App.tsx`, `core.ts`, `ErrorBanner.tsx`, `main.tsx`, `dev-core.ts`, `dev-fixtures.ts`, `external-links.ts`, `report-bug.ts`, `theme.css`, `app/src/views/Empty.tsx`, `app/src/views/index.ts` and the nine `app/src/brocrew/` files. `app/src/views/Loadout.tsx` goes under `### it`, after the `crates/core/src/it.rs` line that {{#m-it}} left as the section's only line. A file outside the 23 that the loop prints goes at the end of its view's section: an `app/src/sky/` file under `### sky-face`, an `app/src/sky3d/` file under `### sky-3d` (the heading {{#m-sky-face}} adds), `Waiting.tsx` under `### waiting`, `Reports.tsx` and `WorkTree.tsx` under `### reports`, `Budget.tsx` under `### budget`, `Settings.tsx` under `### core-settings`, `Chat.tsx` under `### frontdoor`. If one of those headings is not in the map, put the file's line under `### shell`.
3. Line 384 goes. Its detail is already in the source: `views`, `settings_missing`, `keep_errors_in` and `fleet_error_rows` have doc comments in `app/src-tauri/src/commands.rs` (lines 134, 147, 312, 334); the AppImage build is described at the top of `tools/build-appimage.sh`; the notice check at the top of `tools/check-notices.py` and in `app/src/brocrew/README.md`. Check each sentence; add to a header comment (of `main.rs`, `commands.rs` or `App.tsx`) only what is truly missing. Fix history is dropped; CHANGELOG has it.
4. A file that gets a new line needs no new header comment. Do not edit the `app/src/brocrew/` files: `tools/check-notices.py` reads the first 15 lines of each for its Apache-2.0 notice.
5. `roadmap/nodes/sky-roadmap-graph.json` names `app/src/sky/RoadmapView.tsx` in `code_map_entries`, and no map line starts with that path on main at 19e0c65, so `crates/core/src/unit/job.rs` `code_map_lines` refuses that node's brief. {{#m-sky-face}} writes that line; check that it is there, and write it under `### sky-face` if it is not.
6. A backticked path in a new line that starts with `crates/core/`, `crates/cli/`, `app/src-tauri/` or `app/src/` must be on disk; the structural code map tests check it.
7. Unchanged: every heading, the `reuses:` block, the `modules:` block, the Layout block. Set the first `Updated:` line to the day of the PR (leave it if main already has that day or later).
8. No code changes.

Size: Medium (about 50 changed lines, nearly all new one-line entries in `docs/code-map.md`; at most 4 files changed; Rust and Node toolchains for the checks; `tools/ci.sh app` needs Linux with the WebKitGTK system libraries that CI's app job installs). The reading is the work: 23 files, the longest being `app/src/theme.css` (1413 lines). Not Easy: each sentence has to be written from the file it describes.

Blocked by: {{#m-cli-mcp}}, {{#m-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-frontdoor}}, {{#m-it}}, {{#t-core}}, {{#t-tauri}}, {{#t-react}}

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, in this order: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is built after {{#m-cli-mcp}}; the other six map issues it names are the ones that write `app/src` lines or the `### it` section it adds to. The line numbers in this issue are those of the map on main at 19e0c65; the earlier map issues move them, so find each section by its `###` heading and each line by the path it starts with.

Done when:
1. The shell loop above prints nothing: every non-test source file under `app/src` and `app/src-tauri/src` has a line that starts with its backticked path.
2. These print 2 and 59 (56 `.ts`/`.tsx` files and 3 stylesheets, as the tree stands at 19e0c65; if a source file was added or removed since, the number follows the tree and the PR body says so):
   ```
   grep -c '^`app/src-tauri/src/' docs/code-map.md
   grep -c '^`app/src/' docs/code-map.md
   ```
3. No such line is over 400 bytes; this prints nothing:
   ```
   LC_ALL=C awk '/^`app\// && length($0) > 400' docs/code-map.md
   ```
4. No such line holds an issue number; this prints nothing:
   ```
   grep -n '^`app/.*#[0-9]' docs/code-map.md
   ```
5. The old directory line is gone; this prints nothing:
   ```
   grep -n '^`app/src-tauri/`' docs/code-map.md
   ```
6. `grep -c '^`app/src/sky/RoadmapView.tsx`' docs/code-map.md` prints 1.
7. `git diff main -- docs/code-map.md` shows no change inside the `reuses:` and `modules:` blocks.
8. `git diff main --stat` shows `docs/code-map.md` and at most `app/src-tauri/src/main.rs`, `app/src-tauri/src/commands.rs` and `app/src/App.tsx`, with no non-comment line changed in them.
9. `tools/ci.sh rust` passes (the core and CLI structural code map tests), `tools/ci.sh app` passes (the Tauri tree's structural test, `app/src-tauri/src/code_map_tests.rs`) and `tools/ci.sh face` passes (the React face's structural test, `app/src/code-map.test.ts`). `tools/ci.sh lint` passes (it runs `tools/check-notices.py`). The PR body carries all four outputs.
