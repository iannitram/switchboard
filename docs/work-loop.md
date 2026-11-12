# The work loop: what Craft Anything does, and what Switchboard takes

Written: 2026-11-11

**Status: analysis and proposal, not approved.** Nothing here is a Must. It is the input to the `local-only` feature's own Spec (SPEC.md, Regions after 1.0), where the work loop was ruled on 2026-11-07. Words are the glossary's.

Craft Anything (ours, MIT or Apache-2.0) already ships a working build loop: its Make feature takes a request, briefs one AI builder, checks the result itself and sends the builder back until the checks are good. This file records how that loop works at commit `f87a28c`, what a real run showed, and the design Switchboard should build from it. File references are paths in the Craft Anything repo.

## 1. The loop in Craft Anything

One user message is one run (`crates/ai/src/rebuild/mod.rs`, `Worker::run`):

```
study (first run only) -> brief -> builder turn -> harness checks -> refine prompt -+
                                        ^                                           |
                                        +-------------------------------------------+
```

1. **Study**, first run only: look at the original, write what was found to `capture/README.md`. The builder gets it as a file to read, not as prompt text.
2. **Brief** (`rebuild/brief.rs`): the request, the target, the stack, five working steps, the rules, and the fixed things the builder must produce (for a game: a README, a mechanics sheet with a source or "estimated" on every number, credits, a `--screenshot` option and a `--self-test` option).
3. **Builder turn**: the builder works alone, writing only in `app/`.
4. **Checks**, run by the harness, never by the builder (`rebuild/kinds/`): build, screenshot, self-test, mechanics, credits; which ones depends on the project kind (website, game, app, own project). The builder can ask for the same checks mid-turn through a tool; that is round 0 and does not count.
5. **Refine**: the first check that is not good goes back as a short prompt: "Check N: <note>. Fix this in app/, build again until it succeeds".
6. **End**: every check good, or a cap is hit.

A check result has three values, and this is the best idea in the loop:

- **passed**: it works;
- **good**: passed, every claim holds, similarity is over the bar, and nothing is unverified;
- **unverified**: a comparison nobody could make. It can never count as good, and the builder is told to stop retrying it and say so in its reply.

Everything the loop knows is on disk in the project folder: the project file (status, checks, the builder's session id), the conversation feed, usage, the stall guard's fingerprint, check results per round, and an investigation memory (plan, unknowns, findings, handoff) that the builder reads and writes through tools.

## 2. How many loops

| Cap | Value | When it is hit |
|---|---|---|
| Refine rounds (`max_rounds`) | 6 (CLI default 2, slider 0 to 20) | The loop ends. If the last check passed the run says "Ready", even with checks that are not good. |
| Good enough | 90% similarity | Stops refining that check. |
| Unchanged turns | 2 turns that leave `app/` byte-identical (a hash of the folder) | The run stops: no build progress. |
| Not-working checks in a row | 10, counted across runs | The run stops and needs the person. |
| Retries of a failed turn | 3, waiting 30 s times the try, or the service's own delay up to 15 minutes | The error is shown. No other builder is tried. |
| Retry of a check round | 1 | The result stands. |
| Steps inside one turn (in-process loop only) | 400 for a builder (40 for a chat), 25 failed builds | The turn ends with an error. |
| One turn's wall clock (CLI builders) | 24 hours, 8 hours quiet | The process is killed. |
| Money | none, apart from a 5 USD cap on generated pictures and models | - |

There is **no outer loop**. One message buys one builder turn plus up to six fix-ups. Nothing breaks a request into tasks, and nothing keeps going until the whole request is delivered. The only plan is a 3 to 7 step checklist the builder writes for itself.

## 3. Context

Who owns the conversation decides everything:

| Builder class | Owns the context | What the harness does about size |
|---|---|---|
| A CLI agent run as a process (Claude Code, Codex, opencode, aider, DeepSeek Harness) | the agent | Nothing. One process per turn; the harness keeps only a session id. |
| An ACP agent | the agent | Nothing. A stop at the agent's length limit is treated as a finished turn. |
| An API or local model in Craft Anything's own tool loop | the harness | See below. |

The in-process loop (`crates/ai/src/agent.rs`, `context.rs`):

- **Budget**: the configured size, else the window the server reported minus a quarter, else 24,000 tokens for a local model and 150,000 for a cloud one. Tokens are estimated as characters divided by 4.
- **The window is never probed.** It is learned only from the text of an overflow error, then the turn is refitted once.
- **Trim**: the last 12 messages are kept whole; older tool results are cut to 400 characters and older thinking is dropped.
- **Summarise**: if that is still over three quarters of the budget, the same model, at low effort, writes a summary of the oldest two thirds, and one message replaces them. If the summary fails the model is told to look at the files.
- **Sizes**: a tool result is capped at 16 KiB, a file read at 256 KiB, command output at its last 24 KiB, the investigation memory in the system prompt at 24 KiB.
- **Weak models**: a model that refuses tool calls is switched to a text protocol (ReAct); a local builder gets a shorter tool list.

## 4. Getting a fresh-context builder up to date

This is the loop's weakest part.

| The builder starts... | What it is given |
|---|---|
| For the first time | The full brief. |
| With a session it can resume (Claude Code and Antigravity only; the in-process loop from its saved transcript) | A two-line follow-up: "The person asks: ... Make the change in app/". |
| With no session to resume (Codex, opencode, aider, DeepSeek Harness and the rest) | **The full first brief again**, with "The person also said: ..." added at the end. |
| On another machine, session not loadable | The brief, the last reply (2,000 characters) and the handoff (16,000 characters). This is the only case where either is pasted. |
| As a different builder than last time | The old session is thrown away. Nothing is pasted. |

What is never put in a prompt: the conversation feed, the check history (only the newest round's note, in the refine prompt), the README, the mechanics sheet, and what changed last turn. The builder is expected to find them on disk.

The handoff file is written **by the model**, when it remembers to call the tool. The harness writes a handoff in one case only (a lost Claude Code session). There is no rolling summary for CLI or ACP builders.

Refine prompts, added notes and "carry on" prompts are short because they assume the session continues. A builder with no session gets them in a new process with no context at all.

## 5. What a real run showed

A test run on 2026-11-11: a five-feature game request (a full remake, a merged second campaign, a soldier editor, a zooming globe, photoreal looks), DeepSeek Harness as the builder, Rust and macroquad. 58 minutes of builder time, 3 builder turns, about 4,800 lines of Rust.

- **It worked as far as it went.** One pass produced a building game with every fixed file, and the final reply said plainly what was missing and what was unverified.
- **The request went out as one brief.** The builder called it "a HUGE ask" and cut the scope itself, without asking, although the rules say to ask.
- **A one-line follow-up cost 30 minutes.** DeepSeek Harness keeps no session, so "make the land masses match Earth's" re-sent the whole brief. The builder worked out that code already existed by reading the folder, listed all the first requirements again and reworked screens nobody asked about.
- **The check-and-refine loop never ran.** Only the builder's own round-0 self-checks happened (4 of 5 checks pass, 1 unverified). No round 1 exists.
- **The checks were shallow against the request.** One screenshot and a three-step self-test passed while most of the requested features were unchecked. The mechanics check passes whenever the sheet can be read and is always unverified: nothing measures the game against it.
- **Study picked the wrong file**: a 324-byte preferences file as "the program", so its first analysis was noise. The real executable was only decompiled in the third turn, after a reminder that fired too late to change the reply.
- **The builder wrote outside `app/`** and nothing noticed.
- **No usage was recorded**: 0 tokens, no cost. That builder reports none.

## 6. What Switchboard takes, and what it changes

Take as they are:

1. **The harness is the judge.** A builder never decides it is done.
2. **Three-valued check results**, with unverified never counting as good and named in the report (it fits the fixed report's "what wasn't checked").
3. **State on disk**, so any builder can pick a star up cold.
4. **The stall guard**: hash the worktree before and after each turn; two unchanged turns stop the star.
5. **Caps that are counted, saved and survive a restart.**

Change:

| Craft Anything | Switchboard |
|---|---|
| One brief for the whole request | The request becomes stars. The Project Manager job and the prep loop split it; the work loop runs stars in dependency order. A star too big for the model's window is split, never trimmed. |
| One message, one pass | An **outer loop** over the backlog: it runs until every star is good, parked in Waiting on you, or no pool has budget. |
| One builder for the whole job | **The router picks the pool and model for every turn** (the `first` list against budget, Readiness, the escalation ladder). Every turn starts with a fresh context, so a swap costs nothing. |
| Session resume where the agent has it; otherwise the full brief again | **No session is ever relied on.** Every turn gets a catch-up packet written by the harness. |
| The model writes the handoff if it remembers | **The harness writes the handoff** after every turn, from facts: files changed, the diff stat, check results, the tail of the failing output, open unknowns. The model's own closing lines are added to it, never trusted alone. |
| The window is learned from an overflow error | **Probe the window first** (the local server's model properties; the abilities index for paid models), and build the packet to a budget of that window. |
| Checks stop at the first failure; only the first not-good check is fed back | Run every check each round and feed back all that are not good. |
| Checks per project kind, the same for every request | **Checks per star**, locked before the build starts (the prep loop's draft locked tests), so a builder cannot pass by building less. |
| 3 retries, then stop | 3 tries per model, then the next size up or the next pool (R25's ladder, already in `autopilot::review`). With Local only on: split the star, then park it. |
| No money cap, a 24-hour turn | `may_spend` before every turn (AUTO-1), and a wall-clock limit per turn set by the star's size. |
| "Ready" when rounds run out and the last check passed | Out of rounds with checks not good is **not done**: the star is parked with its report. |
| The builder runs as a process the app starts | A turn is a fleet job (`fleet run`), in the star's worktree. No face runs a shell command; that Won't stands. |
| A written rule against writing outside `app/` | `files_allowed` on the job, checked by the harness after the turn. |
| Usage only where the agent reports it | Every turn is a job-log row with tokens, cost and the model, as today. |

### The catch-up packet

What a fresh-context builder gets at the start of every turn, in this order, built by code to a size budget (a share of the probed window; the rest is the model's working room):

1. **The star's brief**: what to build, the files and symbols named, the locked checks. Never trimmed. If it does not fit, the star is not ready (Readiness).
2. **The code-map snippet** for the modules the star touches.
3. **Where it stands**: the round number, tries left, and the last round's check results, each as passed, not good or unverified with a reason.
4. **The handoff** from the last turn (harness-written).
5. **The failing output**: the tail of each not-good check, newest first, cut to fit.
6. **Open unknowns and notes from me** added since the last turn.

A first turn is the same packet with items 3 to 5 empty. There is one prompt shape, so a turn never depends on which model ran before it. `frontdoor::packet` already builds and checks a summary packet for a chat that moves between models; the star packet is the same idea for jobs and should share its code.

### The loop

```
backlog (stars in dependency order, each with locked checks)
  for each ready star:
    round = 0
    loop:
      may_spend?            no  -> wait for budget (or the next pool)
      pick pool and model       (router; local first when Readiness allows)
      build the packet to the model's window
      run one builder turn as a fleet job in the star's worktree
      hash the worktree; unchanged twice -> stuck
      run every locked check    (the harness; three-valued)
      write the handoff, the round's results and a job-log row
      all good               -> open the PR; R25's review takes over (AUTO-3)
      tries on this model used up -> next size up, or split, or park
      round cap reached      -> park in Waiting on you with the report
```

Caps to start from (to be ruled in the feature's Spec): 6 rounds per star; 3 tries per model; 2 unchanged turns; one turn's wall clock by size; no star more than once at a time; at most Qwen plus one service job (AUTO-6, R10).

## 7. Open questions for the feature's Spec

1. Is the work loop only for Local only, or the normal way every star is built, with Local only as a routing rule on top?
2. Who writes a star's locked checks when the prep loop has not run: the Project Manager job, or is such a star simply not ready?
3. Round and try caps: the numbers above, or others.
4. How much of the window the packet may use (a half is a starting point).
5. Does a model swap happen only between turns (simple, proposed here), or also inside a turn for API models (possible only where the harness owns the context)?
6. A request made in the chat ("build me X"): does the front door turn it into stars through the Project Manager job, with my yes on the list before the loop starts?
7. How a star that is good but has unverified checks is shown on the sky, and whether it may merge.
8. Which parts are lifted from Craft Anything as code (the stall guard, the check result shape, the context trim) and which are rewritten.
