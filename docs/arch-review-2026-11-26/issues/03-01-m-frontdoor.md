What: Rewrite the `### frontdoor` section of `docs/code-map.md` to one short line per source file, and move the behaviour detail cut from the map into each file's header comment. Today the section is one directory line and 37 bullets; none of the 12 files in `crates/core/src/frontdoor/` has a line of its own, and neither has `app/src/views/Chat.tsx`.

Where: `docs/code-map.md` (code map: section `### frontdoor`), the 12 files of `crates/core/src/frontdoor/`, and `app/src/views/Chat.tsx`.

Map lines today (main at 19e0c65; sizes in bytes, line ends not counted): the section is lines 234-272. Line 235 is the directory line `crates/core/src/frontdoor/`; lines 236-272 are 37 bullets; together 38 lines, 8553 bytes. Over 400 bytes: 235 (506), 240 (440), 255 (649), 258 (458), 260 (846), 264 (712), 265 (747), 272 (1377). Ten lines carry issue numbers (258, 260 to 267, 272), 14 numbers in all.

Source files that get a line and whose header comment receives the detail. None has a map line today. All 12 Rust files have a `//!` header. The bullet numbers say where each bullet's names are defined (checked on main at 19e0c65):
- `crates/core/src/frontdoor/mod.rs`
- `crates/core/src/frontdoor/router.rs` (bullets 253, 259, 268)
- `crates/core/src/frontdoor/labeller.rs` (239, and the three labellers of 240)
- `crates/core/src/frontdoor/adapters.rs` (242 to 247)
- `crates/core/src/frontdoor/packet.rs` (236, 237, 252)
- `crates/core/src/frontdoor/sticky.rs` (248, 251, 257)
- `crates/core/src/frontdoor/thread.rs` (241, 267, and `ThreadStore::upsert`, `turns`, `is_turn` of 250)
- `crates/core/src/frontdoor/rating_log.rs` (238, the `log_rating` part of 250, and `rate_turn` of 272)
- `crates/core/src/frontdoor/implicit_log.rs` (249, and the `log_implicit` part of 250)
- `crates/core/src/frontdoor/chat.rs` (254, `live_labeller` and `local_labeller` of 240, and the `chat::` names of 258, 260, 264, 265, 272)
- `crates/core/src/frontdoor/answer.rs` (255, 256, and the `answer::` names of 258, 260 to 265)
- `crates/core/src/frontdoor/ask.rs` (272, and `ask::send_as_job`, `job_why` of 262)
- `app/src/views/Chat.tsx` (266, 269, 270, 271, and what the Chat view shows in 256, 260, 261, 264, 265). Its header comment is the comment block under the imports that starts at line 11.

Rules for the rewrite (owner ruling, 2026-11-26; none of this is open):
1. Each of the 13 files gets exactly one line that starts with its backticked repo-relative path, in the form `` `path` · status · one sentence `` (the separator is space, U+00B7 MIDDLE DOT, space). Status is `present` or `planned`; all 13 are `present`. The sentence says what the file owns, its Must ids (FD-1 to FD-13) and the few public names a caller uses. The line is at most 400 bytes and holds no `#<number>`.
2. All 13 lines sit under the `### frontdoor` heading, the 12 Rust files first, `app/src/views/Chat.tsx` last.
3. The directory line 235 and all 37 bullets go. For each sentence cut: if the file's header comment or an item's doc comment already says it, drop it; if not, add it to the header comment of the file it describes. Fix history (which issue or dated ruling changed what) is dropped; CHANGELOG has it.
4. A name defined outside these 13 files stays a short mention in the file that uses it, and its own file is not edited here: the Tauri commands `chat_route`, `chat_rate`, `chat_implicit` and `chat_send_as_job` (`app/src-tauri/src/commands.rs`), `reports::moves` and `CLAUDE_MODEL_UNCHOSEN` (`crates/core/src/reports.rs`), `covers_turns` and `covers_hash` on `JobRow` (`crates/core/src/joblog.rs`), `AbilitiesIndex::claude_tiers` (`crates/core/src/abilities.rs`), and the CLI and MCP commands of bullet 262 (`crates/cli/src/`; {{#m-cli-mcp}} writes those files' lines).
5. The FD-4 learned pick is marked `planned` (owner ruling, 2026-11-26). The `router.rs` line has the status `present`, because the rest of the file is in use, and its sentence ends with these words: ``the FD-4 learned pick (`sample_size`, `learned_pick`) is planned: only tests call it``. That is the fact today: `sample_size` (`crates/core/src/frontdoor/router.rs:112`) and `learned_pick` (`:127`) are called only from tests (`crates/core/tests/fd_learned_pick.rs`, `fd_qwen_fallback.rs`, `fd_rating_log.rs`); the one other mention is a doc comment at `crates/core/src/frontdoor/rating_log.rs:70`. Bullet 253 says "present" on main; it goes with the other bullets, whatever it says when you start. Put the same sentence in the `//!` header of `router.rs`, unless the header already says it. No other frontdoor line and no other header calls FD-4 learned routing present. {{#s-musts}} marks FD-4 planned in `SPEC.md`; {{#x-fd4-learned-pick}} wires the learned pick into routing or rules it out. Do not wire it here and do not edit `SPEC.md`.
6. A backticked path in a new line that starts with `crates/core/`, `crates/cli/`, `app/src-tauri/` or `app/src/` must be on disk; the structural code map tests check it.
7. The lines for `app/src/brocrew/PromptComposer.tsx`, `UserMessage.tsx` and `AssistantMessage.tsx` (bullets 269, 270) are not written here; {{#m-shell}} writes them.
8. Unchanged: the `### frontdoor` heading, the `reuses:` block, the `modules:` block, the Layout block. Set the first `Updated:` line to the day of the PR (leave it if main already has that day or later).
9. Source files change in comments only. No code changes.

Size: Hard (about 200 changed lines; Rust and Node toolchains for the checks; any platform). This is an oversize change and is reviewed as one: 14 files (the map and 13 header comments), over the usual 6, because the owner ruling puts a section's lines and its files' header comments in one PR. Every change is a comment or a doc line.

Blocked by: {{#m-backup}}, {{#t-core}}

Landing order (owner ruling, 2026-11-26): every map issue edits `docs/code-map.md`, so they are built one at a time, in this order: {{#m-core}}, {{#m-github}}, {{#m-sky-core}}, {{#m-sky-face}}, {{#m-bug-waiting-reports}}, {{#m-budget}}, {{#m-backup}}, {{#m-frontdoor}}, {{#m-autopilot}}, {{#m-runner}}, {{#m-it}}, {{#m-cli-mcp}}, {{#m-shell}}, {{#m-guard}}. This issue is built after {{#m-backup}}. The line numbers in this issue are those of the map on main at 19e0c65; the earlier map issues move them, so find the section by its `### frontdoor` heading and each line by the words it starts with.

{{#s-musts}} is not in this order and does not block this issue. It rewords bullet 253 and adds to the headers of `router.rs`, `chat.rs` and `sticky.rs`. If it has landed, add to each header as it stands and do not repeat a sentence it already has.

Four tests under `crates/core/tests` assert a bullet of this section today (`fd_chat_route.rs`, `fd_implicit_log.rs`, `fd_pick_for_kind.rs`, `fd_rating_log.rs`); {{#t-core}} removes those assertions.

Done when:
1. This prints 12 (one line per Rust file; the directory line is gone):
   ```
   grep -c '^`crates/core/src/frontdoor/' docs/code-map.md
   ```
   and this prints 1:
   ```
   grep -c '^`app/src/views/Chat.tsx`' docs/code-map.md
   ```
2. No such line is over 400 bytes; this prints nothing:
   ```
   LC_ALL=C awk '/^`(crates\/core\/src\/frontdoor\/|app\/src\/views\/Chat\.tsx)/ && length($0) > 400' docs/code-map.md
   ```
3. No such line holds an issue number; this prints nothing:
   ```
   grep -nE '^`(crates/core/src/frontdoor/|app/src/views/Chat\.tsx).*#[0-9]' docs/code-map.md
   ```
4. Between the `### frontdoor` heading and the `### autopilot` heading the map holds only those 13 lines and blank lines.
5. The `router.rs` line has the status `present`, its sentence says the FD-4 learned pick is planned and only tests call it, and the `//!` header of `router.rs` says the same. No other line of the section calls the learned pick present.
6. `git diff main -- docs/code-map.md` shows no change inside the `reuses:` and `modules:` blocks.
7. The PR body lists, for each of the 8 oversize lines, where its detail went (header comment of which file, already there, or dropped as history).
8. `git diff main --stat` shows only `docs/code-map.md` and files among the 13 listed, and no non-comment line changed in a source file.
9. `tools/ci.sh rust` passes (rustfmt over the edited headers, and the structural code map tests).
10. `tools/ci.sh face` passes (`Chat.tsx` still builds and its tests pass).
