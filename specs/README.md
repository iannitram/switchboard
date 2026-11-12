# Specs, draft 2

## What each workflow does

**roadmap-run** builds a project. You agree what it is; a strong model writes the plan; you approve it. Code turns each plan item into an issue, picks the next ready one and locks it. A small model writes tests, builds and updates the code map; another service's small model reviews; code merges and records the result.

**bug-loop** fixes reported bugs. Code collects each report, finds the app's repo, drops duplicates and files one issue. A small model writes a failing test, a fix, then one lesson line. After review and merge, code checks sister apps and closes the bug.

## Steps by who acts

|roadmap-run|Steps|
|---|---:|
|Code|25|
|Small model|5|
|Strong model|4|
|Owner|5|
|Total|39|

|bug-loop|Steps|
|---|---:|
|Code|28|
|Small model|7|
|Strong model|1|
|Owner|2|
|Total|38|

## What the owner is asked

Seven questions, all in Waiting on you; recommended in bold.

|Workflow|What you must do|Choices|
|---|---|---|
|roadmap-run|Read the project's description and say whether it is right.|**Approve**, Needs changes|
|roadmap-run|Look over the build plan and its order, and approve it.|**Approve**, Send back|
|roadmap-run|Answer one question about this project that only you can decide.|**Use suggested answer**, Give another answer, Skip for now|
|roadmap-run|Decide whether to change the plan for a stuck piece of work or drop it.|**Change the plan**, Drop it|
|roadmap-run|Accept or reject a suggested change to how the build process works.|**Accept change**, Reject change|
|bug-loop|Decide whether to try again on a stuck bug or drop it.|**Try again**, Drop it|
|bug-loop|Accept or reject a suggested change to how the bug-fixing process works.|**Accept change**, Reject change|

## Commands to build in Switchboard

30 total, run as `switchboard <name>`.

Shared (6):
- `waiting list`: open questions, oldest first
- `unit claim`: lock one issue
- `unit open-pr`: open the pull request
- `unit red-check`: prove a test fails
- `score sample`: grade sampled runs
- `spec validate`: check these specs

roadmap-run (10):
- `roadmap lint`: check the plan
- `roadmap units`: plan items to issues
- `roadmap wire`: link issues in order
- `roadmap drift`: plan versus issues
- `unit frontier`: list ready work
- `unit next`: choose the next item
- `unit release`: drop a lock
- `unit job`: write the build job
- `unit codemap-check`: code map matches changes
- `unit resolve`: record result, close

bug-loop (14):
- `bug scan`: find new reports
- `bug capture`: save the raw report
- `bug validate`: check report, private words
- `bug owner`: find the app's repo
- `bug dedupe`: spot repeats
- `bug record`: write the bug row
- `bug archive`: archive the original
- `bug next`: choose the next bug
- `bug job`: write the next job
- `bug attempts`: count tries, stop repeats
- `bug lesson-check`: one lesson line added
- `bug signature-check`: test search patterns
- `bug status-row`: update status table
- `bug sweep`: search sister apps

## Dev repo commands Switchboard calls

`fleet pick`, `fleet budget allow`, `fleet run`, `fleet ghq` (add, add-batch, run, status), `fleet errors classify`, `fleet gh`, `merge_pr.py` (check, merge). `fleet ledger` is not called directly.

## Settled

1. Switchboard runs the specs directly.
2. New commands are Rust in Switchboard; existing ones stay.
3. The plan lives in ROADMAP.md plus one file per item.
4. A claim file is the lock; GitHub only mirrors it.
5. Another service's small model reviews; samples are spot-checked.
6. A small model writes locked tests; code proves they fail.
7. One checked pitfalls line per fixed bug, 60 words, app's repo.
8. A bug closes at release, or at merge for apps without releases.
9. Three attempts per bug; no dollar cap.
10. Every owner question appears in Waiting on you.

## Not confirmed

- `switchboard waiting --json` exists, newest first; `waiting list` does not.
- Switchboard never calls `fleet pick` or `fleet gh` today; it reads only exits 0 and 75.
- Whether Switchboard can write fleet-state files: research/fleet-state-write.md (#401) gives a create-if-absent protocol (tree check, then an expected-SHA push lease); no live write has tried it.
- Whether `fleet run` honours a picked lane or reuses a branch.
- Plain `git` and `gh` steps are marked as Switchboard's.
- Who writes the approval lines after the owner answers.
- Warp steps rest on the task brief.
- bug-loop still lists the job card's recorded cost, not a cap.
