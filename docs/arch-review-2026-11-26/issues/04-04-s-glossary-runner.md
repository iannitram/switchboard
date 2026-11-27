What: Add a `## Runner` section to `GLOSSARY.md` that defines the four words the step runner's code already uses: Node, Unit, Spec and Run (owner ruling, 2026-11-26). The words are in the code and in `ROADMAP.md` but not in the glossary, so agents have no ruled meaning for them.

Add the section at the end of `GLOSSARY.md`, after the last entry of `## Routing` ("Claude handoff"), in the file's entry format (bold term with a colon, the definition on the next line, then an `_Avoid_:` line). Use this text:

```
## Runner

**Node**:
One piece of work on a project's roadmap graph: one line in ROADMAP.md's `nodes:` block with an id, a title, its feature, a type, a size, an optional kind and the nodes that must be done first. An issue is made from a node, never the other way round.
_Avoid_: star, issue, ticket

**Unit**:
The GitHub issue made from one node, carrying the label `node:<id>`. It is what the runner takes, claims, builds and proves. On the sky the same issue is a Star. The step engine also uses the word for whatever one run is about: in the bug loop that is a report or a bug.
_Avoid_: node, job, ticket

**Spec**:
A workflow spec: a JSON file under `specs/` that names the records, states, steps, scorers and invariants of one kind of run (`specs/roadmap-run.json`, `specs/bug-loop.json`). Loading a spec never runs its commands. It is not `SPEC.md`, a project's written Spec.
_Avoid_: recipe, pipeline, config

**Run**:
One unit taken through one spec by the step runner, from the state it starts in until a terminal state or a halt (escalate, park, ask the owner, fail). Its state is saved after every step, in one file per spec and unit, so a run can stop and go on later.
_Avoid_: session, job, attempt
```

Rules:

- Change no existing entry. In particular the Star entry keeps "_Avoid_: node, task, ticket" (line 21): "node" stays a word to avoid only when a Star is meant.
- Where the names come from, for checking only (do not edit these files): `crates/core/src/roadmap/graph.rs` `Node`; `crates/core/src/unit/frontier.rs` and `crates/core/src/roadmap/units.rs` (the `node:<id>` label); `crates/core/src/runner/spec.rs` `Spec`, `load`, `parse`; `crates/core/src/runner/state.rs` `RunState`, `Halt`, `StateStore`; `crates/core/src/runner/engine.rs` `Engine::step`; `crates/core/src/runner/bug_loop.rs` `run_end_to_end` (its `report_unit` and `bug_unit` arguments).

Where: `GLOSSARY.md` only. Code map section: the runner layout lines of `docs/code-map.md` (pasted below); the code map itself is not changed.

Size: Easy (about 20 added lines in 1 file; documents only).

Blocked by: none

{{#f-module}} adds the "State file" entry to another section of `GLOSSARY.md`; the two do not touch the same lines and may land in either order. If "State file" is already there, leave it as it is.

Done when:
1. `GLOSSARY.md` ends with a `## Runner` section holding exactly four entries, Node, Unit, Spec and Run, in that order, each with an `_Avoid_:` line, worded as above.
2. `git diff` for `GLOSSARY.md` shows added lines only; no existing line is changed or removed.
3. The Star entry's `_Avoid_:` line still reads "node, task, ticket".
4. No other file is in the diff.
5. `tools/ci.sh lint` passes; its output is in the PR body.

## Qwen brief

Expectation: after the change, `grep -c '^## Runner' GLOSSARY.md` prints `1`; `grep -c '^\*\*Node\*\*:\|^\*\*Unit\*\*:\|^\*\*Spec\*\*:\|^\*\*Run\*\*:' GLOSSARY.md` prints `4`; `grep -c '^_Avoid_: node, task, ticket' GLOSSARY.md` prints `1`; `git diff --stat` lists `GLOSSARY.md` only, with insertions and no deletions; `tools/ci.sh lint` passes.

Code map snippet (as on main at 19e0c65; {{#m-core}} rewords these Layout lines, and this issue does not touch the code map in either order):

```
  src/runner/                 workflow spec types, loading and validation; the step engine, its run state and the owner steps' waiting items
  src/roadmap/                the roadmap graph (ROADMAP's nodes and unspecified blocks, node spec files), its lint, its drift against the issues, and the units plan (issues and blocked-by links)
  src/unit/                   proof: the red check of a unit's test command; frontier: the unit frontier (open, unclaimed, unblocked units) and the next one to take; claim: the claim file lock, its release and the stale reaper; job: the fleet job and brief of a claimed unit, from its node spec file; land: the code map check, the pull request and the resolution of a unit
```
