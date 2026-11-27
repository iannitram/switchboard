What: Rewrite the `sky-face` and `sky-layers` sections of `docs/code-map.md` so every Sky file in the React face has one short entry line, and make sure the Roadmap lens detail cut from the map is in the comments of the files that hold it. One of the code map revision issues (owner ruling, 2026-11-26); comments and docs only. The core sky sections are {{#m-sky-core}}.

Where: `docs/code-map.md` lines 168-172 (code map: `### sky-face`, `### sky-layers`; 1,568 bytes), and, only where a cut fact is missing, the comments of `app/src/sky/RoadmapView.tsx`, `app/src/sky/roadmap.ts` and `app/src/sky/layers.ts`.

Size: Medium (about 50 changed lines in 1 to 4 files; Node and Rust toolchains for the proof, no code change). The diff is small, but the builder must read 30 source files to write a true sentence for each, which is why this is not Easy.

Blocked by: {{#m-sky-core}}, {{#t-core}}, {{#t-react}}

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, each blocked by the one before it: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is the fourth: {{#m-core}}, {{#m-github}} and {{#m-sky-core}} have landed before it and {{#m-bug-waiting-reports}} is next.

Line numbers in this issue are those of the map on main at commit 19e0c65. The three earlier map issues have rewritten the lines above these two sections, so the numbers have moved; they leave the two sections as they are. Find them by their `###` headings.

What is there (line numbers as at 19e0c65):
- Two path-led lines, both over 400 bytes and both with issue numbers: line 169 (1,086 bytes; names `app/src/views/Sky.tsx`, `app/src/sky3d/Sky3D.tsx` and the directory `app/src/sky/` in one line) and line 172 (451 bytes; `app/src/sky/layers.ts`).
- Most of line 169 is the Roadmap lens's column rules (one column per dependency layer, then stage columns, then Unstaged; where a star with no node goes). That code is in `app/src/sky/RoadmapView.tsx` and `app/src/sky/roadmap.ts`. Neither file has a top comment, but the comment on `layoutRoadmap` (`roadmap.ts:20-26`) and the comment above `RoadmapView` (`RoadmapView.tsx`, ending at line 16) already state most of these rules.
- 28 `.ts`/`.tsx` source files here have no line of their own. Only `Sky.tsx` and `layers.ts` start a line today; `Sky3D.tsx` is named only inside line 169.
- `roadmap/nodes/sky-roadmap-graph.json` names `app/src/sky/RoadmapView.tsx` as a code map entry. `code_map_lines` in `crates/core/src/unit/job.rs` finds an entry by a line that starts with its backticked path and refuses when there is none, so today that node's job brief cannot be built. This issue fixes that by giving the file its line.

Files that get one entry line each (32 lines):
- `app/src/views/Sky.tsx`
- `app/src/sky/` (12): `ConstellationView.tsx`, `DriftList.tsx`, `ReuseLines.tsx`, `RoadmapView.tsx`, `SkyMap.tsx`, `StarPanel.tsx`, `chain.ts`, `geometry.ts`, `layers.ts`, `model.ts`, `roadmap.ts`, `stars.ts`
- `app/src/sky3d/` (17): `Sky3D.tsx`, `SkyScene.ts`, `anchor.ts`, `camera.ts`, `galaxy.ts`, `labels.ts`, `layout.ts`, `milkyway.ts`, `motion.ts`, `nebula.ts`, `optics.ts`, `physics.ts`, `post.ts`, `random.ts`, `support.ts`, `wayfind.ts`, `wink.ts`
- The two stylesheets `app/src/sky/roadmap.css` and `app/src/sky3d/sky3d.css`. A `.css` file under `app/src` is a source file and gets its own map line; the every-file check in {{#m-guard}} uses the same rule. `app/src/theme.css` gets its line in {{#m-shell}}, not here.

Test files (`*.test.ts`, `*.test.tsx`) get no line.

Placement: `Sky.tsx` and the `app/src/sky/` files except `layers.ts` go under `### sky-face`; `layers.ts` stays under `### sky-layers`; add a new heading `### sky-3d` after the `sky-layers` section for the `app/src/sky3d/` files. Each stylesheet goes with its folder: `roadmap.css` under `### sky-face`, `sky3d.css` under `### sky-3d`. The structural code map tests from {{#t-core}} and {{#t-react}} do not read headings, so the new heading needs no test change.

Rules (owner ruling, 2026-11-26):
- Entry format: one line per source file, starting with the backticked repo-relative path: `` `path` · status · one sentence ``. Status is `present` or `planned`; all 32 are `present`. The sentence says what the file owns, its Must ids where it carries one (SKY-1 to SKY-8), and the few exported names a caller uses. At most 400 bytes (UTF-8; `·` is 2 bytes) and no `#` followed by digits.
- Write each sentence from the file itself. 21 of the 30 `.ts`/`.tsx` files have a top comment that says what the file is for; the nine that have none are `Sky.tsx`, `Sky3D.tsx`, `ConstellationView.tsx`, `DriftList.tsx`, `ReuseLines.tsx`, `RoadmapView.tsx`, `SkyMap.tsx`, `StarPanel.tsx`, `roadmap.ts`.
- Behaviour detail cut from lines 169 and 172 goes into a `//` comment at the top of the file that holds the code, unless that file's top comment or the comment on the named item already says it. Fix history (issue numbers) and the list of test files are dropped; `CHANGELOG.md` has the history. A file gets a new top comment only when cut detail lands in it.
- In `layers.ts`, keep the notice in its top comment (the code is ported under MIT). The map line no longer carries the credit; `NOTICE` and the file do.
- Line 172 says nothing in the face calls `dependencyLayers`; that is true today (`roadmap.ts` calls only `orderWithinLayers`). If the `layers.ts` line or comment says it, check it again first.
- Do not change the `reuses:` and `modules:` blocks (lines 9-26) or any existing `###` heading. Set the first `Updated:` line to the day of the PR.
- No earlier map issue writes a line for these 32 paths. {{#m-shell}} later lists the face files still without a line and finds these done.

Done when:
1. Each of the 32 paths above starts exactly one line of `docs/code-map.md`, in the entry format. The three commands below print 13, 18 and 1:
   ```
   grep -c '^`app/src/sky/' docs/code-map.md
   grep -c '^`app/src/sky3d/' docs/code-map.md
   grep -c '^`app/src/views/Sky.tsx`' docs/code-map.md
   ```
2. Each of these prints nothing:
   ```
   LC_ALL=C awk '/^`app\/src\/(sky|sky3d|views\/Sky)/ && length($0) > 400' docs/code-map.md
   grep -nE '^`app/src/(sky|sky3d|views/Sky).*#[0-9]' docs/code-map.md
   ```
3. Every exported name an entry line gives exists in the file that line names.
4. Comments in `app/src/sky/RoadmapView.tsx` and `app/src/sky/roadmap.ts` between them state every column rule cut from line 169: one column per dependency layer, headed `Layer n`, when the core gives `nodeLayers`; then the stage columns; then Unstaged; a star with no node or an unknown one goes in its feature's stage column; unlisted features and dark stars go in Unstaged; a project with neither a `nodes:` block nor a stages table says so.
5. The PR body lists each fact cut from the map with where it is now (the file and "already there" or "added") or "dropped as history".
6. `git diff --stat` shows only `docs/code-map.md` and files under `app/src/sky/`, and every source hunk is a comment.
7. The first `Updated:` line has the PR's date.
8. `tools/ci.sh face` and `tools/ci.sh rust` pass; their output is in the PR body.
