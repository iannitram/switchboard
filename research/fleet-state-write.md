# Creating fleet-state files safely

Research for [#401](https://github.com/iannitram/switchboard/issues/401), 2026-11-11.

## Answer and recommendation

**Yes: check absence in commit B, build C with only parent B, and publish using an explicit expected-SHA lease on `refs/heads/fleet-state`.** Git compares the ref; Switchboard checks the tree. Preparation and contention retries surround one successful publishing push. This protocol is derived from [Git lease semantics](https://git-scm.com/docs/git-push).

Recommend this protocol for runner claim files because its concurrency condition is explicit. It is a proposal for implementation, not a writer already present in Switchboard. The runner roadmap asks this question before `unit-claim`; the draft says a claim file is the lock. Sources: [roadmap](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/ROADMAP.md#nodes), [workflow draft](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/specs/README.md#settled).

## What Switchboard does today

`FleetBridge` invokes the configured pipeline checkout's `tools/fleet/fleet.py ... --json`; `FLEET_ROOT` can override that checkout. It handles process failures, bad JSON and timeouts, but contains no direct fleet-state Git writer. The target must therefore be the configured pipeline repository's fleet-state branch, not this Switchboard worktree's `origin`. This checkout does not establish the pipeline tools' internal push or lock algorithm. Sources: [fleet.rs](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/crates/core/src/fleet.rs), [code map](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/docs/code-map.md#core-fleet).

`fleet_errors.rs` persists a local `fleet-errors.jsonl` under a local file lock; it does not publish that file to fleet-state. IT tickets go through `fleet errors file --records -`, and held requests through `fleet attach --request -`. Sources: [fleet_errors.rs](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/crates/core/src/fleet_errors.rs), [it.rs](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/crates/core/src/it.rs).

Keep a future writer in the core's write path; a relay must still pull fleet jobs after a wake ping. Sources: [code map](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/docs/code-map.md#layout), [accepted relay decision](https://github.com/iannitram/switchboard/blob/b417cbbc9d06b7a5098cd81c3f2a92d52ceba546/docs/adr/0001-relay-wake-then-pull.md).

## Options

| Option | Condition enforced | Cost and suitability |
| --- | --- | --- |
| Git push with explicit lease | Remote ref must equal the supplied B. Absence is a separate tree check. | Local object preparation, one publishing push; recommended. [Git push](https://git-scm.com/docs/git-push) |
| Contents API, PUT without `sha` | Updating an existing file requires its blob SHA; omission expresses creation. | One write request, one file; simpler API alternative. [Contents API](https://docs.github.com/en/rest/repos/contents#create-or-update-file-contents) |
| Git Data API, then PATCH ref with `force=false` | Ref update must be a fast-forward; this is not an expected-old-SHA parameter. | Separate tree/commit/ref calls; supports a prepared multi-file tree. [Trees](https://docs.github.com/en/rest/git/trees#create-a-tree), [commits](https://docs.github.com/en/rest/git/commits#create-a-commit), [refs](https://docs.github.com/en/rest/git/refs#update-a-reference) |

### Git: proposed transaction

The following is an implementation recipe, not commands run during this research:

1. Fetch only the target branch into a private per-attempt repository or ref, and capture its full commit ID as B. Avoid sharing mutable `FETCH_HEAD` between writers. A fetch records fetched object IDs there. [Git fetch](https://git-scm.com/docs/git-fetch#_description)
2. Inspect B's tree, not the working directory. Require the exact repository-relative path P to have no entry of any type, and require every existing ancestor to be a directory. Reject invalid paths and file/directory conflicts. Tree inspection supports explicit tree objects and NUL-delimited output; these additional rejection rules are proposed client policy. [Git ls-tree](https://git-scm.com/docs/git-ls-tree)
3. Proposed claim policy: put a unique acquisition-attempt token in P so competing claims produce different trees. Preserve all entries of B, adding only P. Make C with exactly one parent, B; verify its tree difference before publishing. `git commit-tree` accepts a tree and explicit parent. [Git commit-tree](https://git-scm.com/docs/git-commit-tree)
4. Substitute captured IDs and the pipeline remote below. Never use implicit leases, plain force, or a newer B with an old C. [Git push](https://git-scm.com/docs/git-push)

```text
git push --porcelain --force-with-lease=refs/heads/fleet-state:<B> <pipeline-remote> <C>:refs/heads/fleet-state
```

5. On rejection, propose bounded retries with backoff, repeating the absence check and construction. The published commit names the entire tree. [Git commit-tree](https://git-scm.com/docs/git-commit-tree) `--atomic` concerns multiple refs, not file existence. [Git push](https://git-scm.com/docs/git-push)

Inspect the per-ref result: an “up to date” result is not a fresh acquisition. In particular, identical candidate commits must not let two callers infer ownership from exit zero alone. This is proposed client handling of Git's distinct up-to-date status. [Git push output](https://git-scm.com/docs/git-push#_output)

**Race proof (inference):** A and B read the same base O with P absent, then build distinct sibling commits using different attempt tokens. Once A publishes, B's expected O no longer matches, including if both pushes passed initial discovery before either finished. The server validates the old object ID again before updating the ref. B's retry sees P and refuses. If the writers chose different paths, the loser instead rebuilds from A's commit, retaining A's file. No stale tree is republished. [Git protocol](https://git-scm.com/docs/gitprotocol-pack#_reference_update_request_and_packfile_transfer)

### GitHub Contents API

Use `PUT /repos/{owner}/{repo}/contents/{path}` with `branch: "fleet-state"`, `message`, Base64 `content`, and **no `sha`**. The branch otherwise defaults to the repository default; updating requires the existing blob SHA. Creation returns 201. An existing path without `sha` should fail validation (422); this is inferred from the required-update-SHA contract, not an endpoint-specific status promise. The docs also list 409 and warn that concurrent create/update and delete requests conflict. [Contents API](https://docs.github.com/en/rest/repos/contents#create-or-update-file-contents)

**Race assessment (inference):** same-path creations should yield one winner and an error, without overwriting. Different-path operations may conflict too; retry without adding `sha`. A 422 can mean other validation failures. Exact race statuses are undocumented here and unverified, making this a less explicit claim protocol. [Contents API](https://docs.github.com/en/rest/repos/contents#create-or-update-file-contents)

### GitHub Git Data API

Read `heads/fleet-state` as B. [Get a ref](https://docs.github.com/en/rest/git/refs#get-a-reference) Read its commit/tree, perform the same absence check, POST a tree using B's `base_tree` plus the new path, and POST a commit with `parents: [B]`. Omitting `base_tree` drops unlisted files; an existing path is overwritten, so tree creation itself is not create-if-absent. Sources: [trees](https://docs.github.com/en/rest/git/trees#create-a-tree), [commits](https://docs.github.com/en/rest/git/commits#create-a-commit).

Finally PATCH `git/refs/heads/fleet-state` with `{ "sha": C, "force": false }`; this publishes to the branch. [Update a ref](https://docs.github.com/en/rest/git/refs#update-a-reference)

**Race assessment (inference):** under forward-only branch history, fresh single-parent sibling commits cannot both fast-forward the ref. The loser must rebuild and recheck absence, as above. However, `force=false` is an ancestry condition, not equality with B: a rewind to an ancestor could still permit C. Require forward-only writers and never retry by merging a stale tree that replaces a winner's file. The API exposes `sha` and `force`, with no expected-old-SHA field. [Ref update contract](https://docs.github.com/en/rest/git/refs#update-a-reference)

## Boundaries and implementation checks

The guarantee is absence at publication, not “never existed in history.” Even an exact ref comparison cannot detect a ref changing away from B and back to B. A claim's expiry, release, ownership token and protection from a stale worker need their own protocol; this follows from comparing object IDs rather than a monotonic transaction counter. [Git update-ref](https://git-scm.com/docs/git-update-ref#_description)

All writers must preserve unrelated state and follow the agreed history rules. Branch permissions and required checks can still reject publication; this proposal requires access compatible with direct state commits, without assuming current repository settings. [GitHub protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)

Before executing a claimed job, require confirmed publication. If the reply is lost, reconcile by reading the remote tree and matching a unique attempt token; an existing file alone does not prove this writer owns it. This is proposed recovery policy because the protocol reports ref success separately from object transfer. [Git protocol status](https://git-scm.com/docs/gitprotocol-pack#_report_status)

Implementation acceptance checks should cover an existing path, same-path racing writers, different-path contention preserving both files, a stale base, an unrelated background fetch, identical candidates reported up to date, file/directory conflicts, and a lost reply. These are proposed checks derived from the protocol above, not tests completed here. Research was performed on Windows; no product code was changed or built, and no live fleet-state writes or API race experiments were performed.
