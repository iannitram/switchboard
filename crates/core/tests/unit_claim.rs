//! unit-claim (#403): one claim file is the lock; two claims of one unit started together give
//! exactly one winner; open blockers refuse; release drops file, assignee and label; the reaper
//! releases stale claims.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use switchboard_core::clock::FixedClock;
use switchboard_core::error::CoreError;
use switchboard_core::unit::claim::{
    claim, reap, release, Activity, Claim, ClaimError, Mirror, Outcome, Store, IN_PROGRESS,
    NOT_NOW_EXIT, STALE_LIMIT,
};

const REPO: &str = "owner/project";
const BASE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

type Callback = Box<dyn FnOnce(&Fake) + Send>;

#[derive(Default)]
struct Fake {
    blockers: Vec<u64>,
    calls: Mutex<Vec<String>>,
    assignees: Mutex<BTreeMap<(String, u64), String>>,
    labels: Mutex<BTreeSet<(String, u64)>>,
    jobs: Mutex<BTreeSet<(String, u64)>>,
    prs: Mutex<BTreeSet<(String, u64)>>,
    activity_errors: Mutex<BTreeSet<&'static str>>,
    fail_once: Mutex<Option<&'static str>>,
    on_job_check: Mutex<Option<Callback>>,
    after_unlabel: Mutex<Option<Callback>>,
}

impl Fake {
    fn log(&self, operation: &str, s: String) -> Result<(), CoreError> {
        self.calls.lock().unwrap().push(s);
        let mut failure = self.fail_once.lock().unwrap();
        if *failure == Some(operation) {
            *failure = None;
            return Err(CoreError::GitHub(format!("injected {operation} failure")));
        }
        Ok(())
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn assert_mirrored(&self, repo: &str, issue: u64, worker: &str) {
        let key = (repo.to_string(), issue);
        assert_eq!(
            self.assignees.lock().unwrap().get(&key).map(String::as_str),
            Some(worker)
        );
        assert!(self.labels.lock().unwrap().contains(&key));
    }

    fn assert_cleared(&self, repo: &str, issue: u64) {
        let key = (repo.to_string(), issue);
        assert!(!self.assignees.lock().unwrap().contains_key(&key));
        assert!(!self.labels.lock().unwrap().contains(&key));
    }
}

impl Mirror for Fake {
    fn open_blockers(&self, _: &str, _: u64) -> Result<Vec<u64>, CoreError> {
        Ok(self.blockers.clone())
    }
    fn assign(&self, repo: &str, i: u64, w: &str) -> Result<(), CoreError> {
        self.log("assign", format!("assign {repo}#{i} {w}"))?;
        self.assignees
            .lock()
            .unwrap()
            .insert((repo.to_string(), i), w.to_string());
        Ok(())
    }
    fn unassign(&self, repo: &str, i: u64, w: &str) -> Result<(), CoreError> {
        self.log("unassign", format!("unassign {repo}#{i} {w}"))?;
        let key = (repo.to_string(), i);
        let mut assignees = self.assignees.lock().unwrap();
        if assignees.get(&key).is_some_and(|held| held == w) {
            assignees.remove(&key);
        }
        Ok(())
    }
    fn label(&self, repo: &str, i: u64, l: &str) -> Result<(), CoreError> {
        assert_eq!(l, IN_PROGRESS);
        self.log("label", format!("label {repo}#{i} {l}"))?;
        self.labels.lock().unwrap().insert((repo.to_string(), i));
        Ok(())
    }
    fn unlabel(&self, repo: &str, i: u64, l: &str) -> Result<(), CoreError> {
        assert_eq!(l, IN_PROGRESS);
        self.log("unlabel", format!("unlabel {repo}#{i} {l}"))?;
        self.labels.lock().unwrap().remove(&(repo.to_string(), i));
        let callback = self.after_unlabel.lock().unwrap().take();
        if let Some(callback) = callback {
            callback(self);
        }
        Ok(())
    }
}

impl Activity for Fake {
    fn running_job(&self, repo: &str, issue: u64) -> Result<bool, CoreError> {
        let callback = self.on_job_check.lock().unwrap().take();
        if let Some(callback) = callback {
            callback(self);
        }
        if self.activity_errors.lock().unwrap().contains("running_job") {
            return Err(CoreError::GitHub(
                "injected running job check failure".into(),
            ));
        }
        Ok(self
            .jobs
            .lock()
            .unwrap()
            .contains(&(repo.to_string(), issue)))
    }

    fn open_pr(&self, repo: &str, issue: u64) -> Result<bool, CoreError> {
        if self.activity_errors.lock().unwrap().contains("open_pr") {
            return Err(CoreError::GitHub("injected open PR check failure".into()));
        }
        Ok(self
            .prs
            .lock()
            .unwrap()
            .contains(&(repo.to_string(), issue)))
    }
}

fn held(outcome: Outcome) -> Claim {
    match outcome {
        Outcome::Claimed(held) => held,
        other => panic!("expected a claim, got {other:?}"),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A bare remote whose fleet-state branch holds one file, and a store over it.
fn store(name: &str) -> (Store, PathBuf) {
    let root = std::env::temp_dir().join(format!("sb-claim-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let seed = root.join("seed");
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet", "-b", "fleet-state"]);
    std::fs::write(seed.join("README"), "state\n").unwrap();
    git(&seed, &["add", "README"]);
    git(&seed, &["commit", "--quiet", "-m", "seed"]);
    git(
        &root,
        &["init", "--bare", "--quiet", remote.to_str().unwrap()],
    );
    git(
        &seed,
        &["push", "--quiet", remote.to_str().unwrap(), "fleet-state"],
    );
    let scratch = root.join("scratch");
    (
        Store {
            remote: remote.to_str().unwrap().to_string(),
            scratch,
        },
        root,
    )
}

fn files(store: &Store) -> String {
    git(
        Path::new(&store.remote),
        &["ls-tree", "-r", "--name-only", "fleet-state"],
    )
}

fn copy_store(store: &Store) -> Store {
    Store {
        remote: store.remote.clone(),
        scratch: store.scratch.clone(),
    }
}

fn read_claim(store: &Store, repo: &str, issue: u64) -> serde_json::Value {
    serde_json::from_str(&git(
        Path::new(&store.remote),
        &["show", &format!("fleet-state:claims/{repo}/{issue}.json")],
    ))
    .unwrap()
}

/// Publish a changed claim from another writer, preserving the rest of fleet-state.
fn rewrite_claim(root: &Path, repo: &str, issue: u64, value: &serde_json::Value) {
    let seed = root.join("seed");
    let remote = root.join("remote.git");
    git(
        &seed,
        &["fetch", "--quiet", remote.to_str().unwrap(), "fleet-state"],
    );
    git(&seed, &["reset", "--hard", "--quiet", "FETCH_HEAD"]);
    let path = format!("claims/{repo}/{issue}.json");
    std::fs::write(seed.join(&path), value.to_string()).unwrap();
    git(&seed, &["add", &path]);
    git(&seed, &["commit", "--quiet", "-m", "update claim"]);
    git(
        &seed,
        &["push", "--quiet", remote.to_str().unwrap(), "fleet-state"],
    );
}

#[test]
fn two_claims_of_one_unit_started_together_give_one_winner() {
    let (store, root) = store("race");
    let clock = FixedClock::at(1000);
    for round in 0..3u64 {
        let issue = 40 + round;
        let (a, b) = std::thread::scope(|s| {
            let a = s.spawn(|| {
                claim(
                    &store,
                    &Fake::default(),
                    &clock,
                    REPO,
                    issue,
                    "alpha",
                    BASE_SHA,
                )
                .unwrap()
            });
            let b = s.spawn(|| {
                claim(
                    &store,
                    &Fake::default(),
                    &clock,
                    REPO,
                    issue,
                    "beta",
                    BASE_SHA,
                )
                .unwrap()
            });
            (a.join().unwrap(), b.join().unwrap())
        });
        let mut codes = [a.exit_code(), b.exit_code()];
        codes.sort();
        assert_eq!(codes, [0, NOT_NOW_EXIT], "round {round}: {a:?} {b:?}");
        assert!(files(&store).contains(&format!("claims/{REPO}/{issue}.json")));
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claims_of_different_units_at_once_both_stand() {
    let (store, root) = store("different");
    let clock = FixedClock::at(1000);
    std::thread::scope(|s| {
        for issue in [7u64, 8, 9] {
            let (store, clock) = (&store, &clock);
            s.spawn(move || {
                let out =
                    claim(store, &Fake::default(), clock, REPO, issue, "w", BASE_SHA).unwrap();
                assert_eq!(out.exit_code(), 0, "{out:?}");
            });
        }
    });
    let listed = files(&store);
    for issue in [7, 8, 9] {
        assert!(
            listed.contains(&format!("claims/{REPO}/{issue}.json")),
            "{listed}"
        );
    }
    assert!(listed.contains("README"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_unit_with_an_open_blocker_cannot_be_claimed() {
    let (store, root) = store("blocked");
    let fake = Fake {
        blockers: vec![12],
        ..Default::default()
    };
    let out = claim(&store, &fake, &FixedClock::at(1), REPO, 5, "w", BASE_SHA).unwrap();
    assert_eq!(out, Outcome::Blocked(vec![12]));
    assert_eq!(out.exit_code(), NOT_NOW_EXIT);
    assert!(!files(&store).contains("claims/"));
    assert!(fake.calls().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_claim_is_mirrored_and_release_removes_file_assignee_and_label() {
    let (store, root) = store("release");
    let fake = Fake::default();
    let clock = FixedClock::at(1);
    let acquired = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
    assert_eq!(
        fake.calls(),
        vec![
            format!("assign {REPO}#5 w"),
            format!("label {REPO}#5 {IN_PROGRESS}")
        ]
    );
    assert!(files(&store).contains(&format!("claims/{REPO}/5.json")));

    assert!(release(&store, &fake, REPO, 5, &acquired.token).unwrap());
    assert!(!files(&store).contains("claims/"));
    let calls = fake.calls();
    assert_eq!(
        calls[2..],
        [
            format!("unassign {REPO}#5 w"),
            format!("unlabel {REPO}#5 {IN_PROGRESS}")
        ]
    );
    assert!(
        !release(&store, &fake, REPO, 5, &acquired.token).unwrap(),
        "nothing left to release"
    );
    assert!(files(&store).contains("README"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failed_acquisition_mirror_write_returns_a_claim_that_can_be_released() {
    for operation in ["assign", "label"] {
        let (store, root) = store(&format!("acquisition-{operation}"));
        let fake = Fake::default();
        let clock = FixedClock::at(1_000);
        *fake.fail_once.lock().unwrap() = Some(operation);
        let failure = claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap_err();
        let acquired = match failure {
            ClaimError::Mirror { claim, error } => {
                assert_eq!(
                    error,
                    CoreError::GitHub(format!("injected {operation} failure"))
                );
                claim
            }
            other => panic!("expected a claim with the mirror error, got {other:?}"),
        };
        assert_eq!(acquired.issue, 5);
        assert_eq!(acquired.unit, format!("{REPO}#5"));
        assert_eq!(acquired.machine, "w");
        assert_eq!(acquired.claimed_at, 1_000);
        assert_eq!(acquired.base_sha, BASE_SHA);
        assert!(!acquired.token.is_empty());
        assert_eq!(read_claim(&store, REPO, 5)["token"], acquired.token);
        let key = (REPO.to_string(), 5);
        assert_eq!(
            fake.assignees.lock().unwrap().get(&key).map(String::as_str),
            if operation == "label" {
                Some("w")
            } else {
                None
            }
        );
        assert!(!fake.labels.lock().unwrap().contains(&key));
        let calls = fake.calls();
        assert_eq!(
            claim(&store, &fake, &clock, REPO, 5, "other", BASE_SHA).unwrap(),
            Outcome::Taken
        );
        assert_eq!(fake.calls(), calls);

        assert!(release(&store, &fake, REPO, 5, &acquired.token).unwrap());
        assert!(!files(&store).contains("claims/"));
        assert!(files(&store).contains("README"));
        fake.assert_cleared(REPO, 5);

        let fresh = held(claim(&store, &fake, &clock, REPO, 5, "other", BASE_SHA).unwrap());
        assert_ne!(fresh.token, acquired.token);
        fake.assert_mirrored(REPO, 5, "other");
        assert!(release(&store, &fake, REPO, 5, &fresh.token).unwrap());
        assert!(!files(&store).contains("claims/"));
        fake.assert_cleared(REPO, 5);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn the_reaper_releases_only_claims_older_than_the_stale_limit() {
    let (store, root) = store("reap");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    claim(&store, &fake, &clock, REPO, 1, "old", BASE_SHA).unwrap();
    clock.advance(STALE_LIMIT);
    claim(&store, &fake, &clock, REPO, 2, "new", BASE_SHA).unwrap();
    assert!(reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT)
        .unwrap()
        .is_empty());
    clock.advance(1);
    assert_eq!(
        reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT).unwrap(),
        vec![1]
    );
    let listed = files(&store);
    assert!(
        !listed.contains(&format!("claims/{REPO}/1.json"))
            && listed.contains(&format!("claims/{REPO}/2.json")),
        "{listed}"
    );
    assert!(fake.calls().contains(&format!("unassign {REPO}#1 old")));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_same_issue_in_two_repos_can_be_claimed_and_released_independently() {
    // Both different project names and different owners must have separate locks.
    for other_repo in ["owner/other", "other/project"] {
        let (store, root) = store(&format!("repos-{}", other_repo.replace('/', "-")));
        let fake = Fake::default();
        let clock = FixedClock::at(1_000);
        let (first, second) = std::thread::scope(|s| {
            let first =
                s.spawn(|| held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap()));
            let second = s.spawn(|| {
                held(claim(&store, &fake, &clock, other_repo, 5, "w", BASE_SHA).unwrap())
            });
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_ne!(first.token, second.token);
        assert!(files(&store).contains(&format!("claims/{REPO}/5.json")));
        assert!(files(&store).contains(&format!("claims/{other_repo}/5.json")));
        assert!(release(&store, &fake, REPO, 5, &first.token).unwrap());
        assert!(!files(&store).contains(&format!("claims/{REPO}/5.json")));
        assert_eq!(read_claim(&store, other_repo, 5)["token"], second.token);
        fake.assert_cleared(REPO, 5);
        fake.assert_mirrored(other_repo, 5, "w");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn claim_files_contain_the_required_fields_and_attempt_token() {
    let (store, root) = store("fields");
    let acquired = held(
        claim(
            &store,
            &Fake::default(),
            &FixedClock::at(1_000),
            REPO,
            5,
            "w",
            BASE_SHA,
        )
        .unwrap(),
    );
    assert_eq!(
        read_claim(&store, REPO, 5),
        serde_json::json!({
            "unit": format!("{REPO}#5"),
            "machine": "w",
            "claimed_at": 1_000,
            "base_sha": BASE_SHA,
            "token": acquired.token,
        })
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_claim_missing_any_required_field_is_never_deleted() {
    let (store, root) = store("missing-fields");
    let fake = Fake::default();
    let acquired = held(claim(&store, &fake, &FixedClock::at(1), REPO, 5, "w", BASE_SHA).unwrap());
    let original = read_claim(&store, REPO, 5);
    let calls = fake.calls();
    for field in ["unit", "machine", "claimed_at", "base_sha", "token"] {
        let mut malformed = original.clone();
        malformed.as_object_mut().unwrap().remove(field);
        rewrite_claim(&root, REPO, 5, &malformed);
        assert!(matches!(
            release(&store, &fake, REPO, 5, &acquired.token),
            Err(CoreError::Parse(_))
        ));
        assert_eq!(read_claim(&store, REPO, 5), malformed);
        assert_eq!(fake.calls(), calls);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_reaper_exempts_running_jobs_and_open_prs_and_scopes_checks_to_the_repo() {
    let (store, root) = store("activity");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    for issue in 1..=3 {
        claim(&store, &fake, &clock, REPO, issue, "w", BASE_SHA).unwrap();
    }
    // Activity in another repository must neither exempt our issue nor be reaped here.
    claim(&store, &fake, &clock, "owner/other", 3, "w", BASE_SHA).unwrap();
    fake.jobs.lock().unwrap().insert((REPO.into(), 1));
    fake.prs.lock().unwrap().insert((REPO.into(), 2));
    fake.jobs.lock().unwrap().insert(("owner/other".into(), 3));
    fake.prs.lock().unwrap().insert(("owner/other".into(), 3));
    clock.advance(STALE_LIMIT + 1);
    assert_eq!(
        reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT).unwrap(),
        vec![3]
    );
    for issue in [1, 2] {
        assert!(files(&store).contains(&format!("claims/{REPO}/{issue}.json")));
        fake.assert_mirrored(REPO, issue, "w");
    }
    fake.assert_mirrored("owner/other", 3, "w");
    assert!(files(&store).contains("claims/owner/other/3.json"));
    fake.assert_cleared(REPO, 3);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_error_from_either_activity_check_keeps_the_claim() {
    for operation in ["running_job", "open_pr"] {
        let (store, root) = store(operation);
        let fake = Fake::default();
        let clock = FixedClock::at(1_000);
        claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap();
        let calls = fake.calls();
        fake.activity_errors.lock().unwrap().insert(operation);
        clock.advance(STALE_LIMIT + 1);
        assert!(reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT)
            .unwrap()
            .is_empty());
        assert!(files(&store).contains(&format!("claims/{REPO}/5.json")));
        assert_eq!(fake.calls(), calls);
        fake.assert_mirrored(REPO, 5, "w");
        fake.activity_errors.lock().unwrap().clear();
        assert_eq!(
            reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT).unwrap(),
            vec![5]
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn a_failed_mirror_cleanup_keeps_the_claim_and_a_retry_finishes() {
    for operation in ["unassign", "unlabel"] {
        let (store, root) = store(operation);
        let fake = Fake::default();
        let clock = FixedClock::at(1_000);
        let acquired = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
        *fake.fail_once.lock().unwrap() = Some(operation);
        assert!(matches!(
            release(&store, &fake, REPO, 5, &acquired.token),
            Err(CoreError::GitHub(_))
        ));
        assert_eq!(read_claim(&store, REPO, 5)["token"], acquired.token);
        assert_eq!(
            claim(&store, &fake, &clock, REPO, 5, "other", BASE_SHA).unwrap(),
            Outcome::Taken
        );
        assert!(release(&store, &fake, REPO, 5, &acquired.token).unwrap());
        assert!(!files(&store).contains("claims/"));
        fake.assert_cleared(REPO, 5);
        let cleanup_call = format!("unassign {REPO}#5 w");
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| *call == &cleanup_call)
                .count(),
            2
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn mirror_cleanup_happens_while_the_claim_still_blocks_new_workers() {
    let (store, root) = store("cleanup-order");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    let acquired = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
    let contender = copy_store(&store);
    *fake.after_unlabel.lock().unwrap() = Some(Box::new(move |_| {
        assert!(files(&contender).contains(&format!("claims/{REPO}/5.json")));
        assert_eq!(
            claim(
                &contender,
                &Fake::default(),
                &FixedClock::at(2_000),
                REPO,
                5,
                "other",
                BASE_SHA
            )
            .unwrap(),
            Outcome::Taken
        );
    }));
    assert!(release(&store, &fake, REPO, 5, &acquired.token).unwrap());
    assert!(!files(&store).contains("claims/"));
    fake.assert_cleared(REPO, 5);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_old_release_token_touches_neither_the_replacement_nor_its_mirror() {
    let (store, root) = store("old-token");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    let old = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
    assert!(release(&store, &fake, REPO, 5, &old.token).unwrap());
    // Even the same machine acquiring again must have a new token.
    let replacement = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
    let calls = fake.calls();
    assert_ne!(old.token, replacement.token);
    assert!(!release(&store, &fake, REPO, 5, &old.token).unwrap());
    assert_eq!(read_claim(&store, REPO, 5)["token"], replacement.token);
    assert_eq!(fake.calls(), calls);
    fake.assert_mirrored(REPO, 5, "w");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_replacement_after_the_reaper_listing_is_never_deleted() {
    let (store, root) = store("replaced-after-list");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    let old = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
    clock.advance(STALE_LIMIT + 1);
    let other = copy_store(&store);
    *fake.on_job_check.lock().unwrap() = Some(Box::new(move |fake| {
        // The reaper has listed the expired token, but has not fetched its deletion base.
        assert!(release(&other, fake, REPO, 5, &old.token).unwrap());
        held(
            claim(
                &other,
                fake,
                &FixedClock::at(1_000 + STALE_LIMIT + 1),
                REPO,
                5,
                "replacement",
                BASE_SHA,
            )
            .unwrap(),
        );
        fake.calls.lock().unwrap().clear();
    }));
    assert!(reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT)
        .unwrap()
        .is_empty());
    assert_eq!(read_claim(&store, REPO, 5)["machine"], "replacement");
    assert!(
        fake.calls().is_empty(),
        "the reaper must not touch the new mirror"
    );
    fake.assert_mirrored(REPO, 5, "replacement");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn removal_rechecks_the_token_after_a_rejected_deletion_push() {
    for reaper in [false, true] {
        let (store, root) = store(&format!("replaced-during-push-{reaper}"));
        let fake = Fake::default();
        let clock = FixedClock::at(1_000);
        let old = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
        let old_token = old.token.clone();
        let other = copy_store(&store);
        *fake.after_unlabel.lock().unwrap() = Some(Box::new(move |fake| {
            // A second release/acquisition moves the remote after the first deletion's fetch.
            assert!(release(&other, fake, REPO, 5, &old_token).unwrap());
            // Keep the replacement expired as well: its token alone must protect it on retry.
            held(
                claim(
                    &other,
                    fake,
                    &FixedClock::at(1_000),
                    REPO,
                    5,
                    "replacement",
                    BASE_SHA,
                )
                .unwrap(),
            );
            fake.calls.lock().unwrap().clear();
        }));
        if reaper {
            clock.advance(STALE_LIMIT + 1);
            assert!(reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT)
                .unwrap()
                .is_empty());
        } else {
            assert!(!release(&store, &fake, REPO, 5, &old.token).unwrap());
        }
        assert_ne!(read_claim(&store, REPO, 5)["token"], old.token);
        assert_eq!(read_claim(&store, REPO, 5)["machine"], "replacement");
        assert!(
            fake.calls().is_empty(),
            "the deletion retry must leave the new mirror alone"
        );
        fake.assert_mirrored(REPO, 5, "replacement");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn the_reaper_rechecks_expiry_after_a_rejected_deletion_push() {
    let (store, root) = store("expiry-during-push");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    let acquired = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
    clock.advance(STALE_LIMIT + 1);
    let other_root = root.clone();
    let other = copy_store(&store);
    *fake.after_unlabel.lock().unwrap() = Some(Box::new(move |fake| {
        let mut refreshed = read_claim(&other, REPO, 5);
        // Preserve the token so only the renewed timestamp protects this claim on retry.
        refreshed["claimed_at"] = serde_json::json!(1_000 + STALE_LIMIT + 1);
        rewrite_claim(&other_root, REPO, 5, &refreshed);
        fake.assign(REPO, 5, "w").unwrap();
        fake.label(REPO, 5, IN_PROGRESS).unwrap();
        fake.calls.lock().unwrap().clear();
    }));
    assert!(reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT)
        .unwrap()
        .is_empty());
    assert_eq!(read_claim(&store, REPO, 5)["token"], acquired.token);
    assert_eq!(
        read_claim(&store, REPO, 5)["claimed_at"],
        1_000 + STALE_LIMIT + 1
    );
    assert!(fake.calls().is_empty());
    fake.assert_mirrored(REPO, 5, "w");
    let _ = std::fs::remove_dir_all(root);
}

fn remote_head(store: &Store) -> String {
    git(Path::new(&store.remote), &["rev-parse", "fleet-state"])
}

#[test]
fn a_reclaim_by_the_holder_exits_zero_and_writes_nothing() {
    let (store, root) = store("reclaim-holder");
    let mut fake = Fake::default();
    let first = held(
        claim(
            &store,
            &fake,
            &FixedClock::at(1_000),
            REPO,
            5,
            "w",
            BASE_SHA,
        )
        .unwrap(),
    );
    let head = remote_head(&store);
    let calls = fake.calls();
    // A retry by the holder stays successful even if blockers appeared after acquisition.
    fake.blockers = vec![12];
    let again = claim(
        &store,
        &fake,
        &FixedClock::at(2_000),
        REPO,
        5,
        "w",
        BASE_SHA,
    )
    .unwrap();
    assert_eq!(again.exit_code(), 0, "{again:?}");
    assert_eq!(again, Outcome::Claimed(first.clone()));
    assert_eq!(remote_head(&store), head);
    let on_disk = read_claim(&store, REPO, 5);
    assert_eq!(on_disk["token"], first.token);
    assert_eq!(on_disk["claimed_at"], 1_000);
    assert_eq!(fake.calls(), calls);
    fake.assert_mirrored(REPO, 5, "w");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_refused_claim_changes_nothing_in_the_file_the_assignee_or_the_label() {
    let (store, root) = store("refused-taken");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    let first = held(claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap());
    let head = remote_head(&store);
    let calls = fake.calls();
    assert_eq!(
        claim(&store, &fake, &clock, REPO, 5, "other", BASE_SHA).unwrap(),
        Outcome::Taken
    );
    assert_eq!(remote_head(&store), head);
    assert_eq!(read_claim(&store, REPO, 5)["token"], first.token);
    assert_eq!(read_claim(&store, REPO, 5)["machine"], "w");
    fake.assert_mirrored(REPO, 5, "w");
    assert_eq!(fake.calls(), calls);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_blocked_claim_changes_nothing_in_the_file_the_assignee_or_the_label() {
    let (store, root) = store("refused-blocked");
    let fake = Fake {
        blockers: vec![12],
        ..Default::default()
    };
    let head = remote_head(&store);
    let out = claim(&store, &fake, &FixedClock::at(1), REPO, 5, "w", BASE_SHA).unwrap();
    assert_eq!(out, Outcome::Blocked(vec![12]));
    assert_eq!(remote_head(&store), head);
    assert!(!files(&store).contains("claims/"));
    let key = (REPO.to_string(), 5);
    assert!(!fake.assignees.lock().unwrap().contains_key(&key));
    assert!(!fake.labels.lock().unwrap().contains(&key));
    assert!(fake.calls().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_back_dated_claim_is_reaped_without_waiting_and_other_repos_are_left_alone() {
    let (store, root) = store("back-dated");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000_000);
    claim(&store, &fake, &clock, REPO, 5, "w", BASE_SHA).unwrap();
    claim(&store, &fake, &clock, REPO, 6, "w", BASE_SHA).unwrap();
    claim(&store, &fake, &clock, "owner/other", 5, "w", BASE_SHA).unwrap();
    // Write the claimed times into the past instead of advancing the clock or sleeping.
    for (repo, issue, claimed_at) in [
        (REPO, 5, 1_000_000 - STALE_LIMIT - 1),
        ("owner/other", 5, 1_000_000 - STALE_LIMIT - 1),
    ] {
        let mut value = read_claim(&store, repo, issue);
        value["claimed_at"] = serde_json::json!(claimed_at);
        rewrite_claim(&root, repo, issue, &value);
    }
    assert_eq!(
        reap(&store, &fake, &fake, &clock, REPO, STALE_LIMIT).unwrap(),
        vec![5]
    );
    let listed = files(&store);
    assert!(
        !listed.contains(&format!("claims/{REPO}/5.json")),
        "{listed}"
    );
    assert!(
        listed.contains(&format!("claims/{REPO}/6.json")),
        "{listed}"
    );
    assert!(listed.contains("claims/owner/other/5.json"), "{listed}");
    fake.assert_cleared(REPO, 5);
    fake.assert_mirrored(REPO, 6, "w");
    fake.assert_mirrored("owner/other", 5, "w");
    let _ = std::fs::remove_dir_all(root);
}
