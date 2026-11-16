//! unit-claim (#403): one claim file is the lock; two claims of one unit started together give
//! exactly one winner; open blockers refuse; release drops file, assignee and label; the reaper
//! releases stale claims.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use switchboard_core::clock::FixedClock;
use switchboard_core::error::CoreError;
use switchboard_core::unit::claim::{
    claim, reap, release, Mirror, Outcome, Store, IN_PROGRESS, NOT_NOW_EXIT, STALE_LIMIT,
};

#[derive(Default)]
struct Fake {
    blockers: Vec<u64>,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn log(&self, s: String) -> Result<(), CoreError> {
        self.calls.lock().unwrap().push(s);
        Ok(())
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Mirror for Fake {
    fn open_blockers(&self, _: u64) -> Result<Vec<u64>, CoreError> {
        Ok(self.blockers.clone())
    }
    fn assign(&self, i: u64, w: &str) -> Result<(), CoreError> {
        self.log(format!("assign {i} {w}"))
    }
    fn unassign(&self, i: u64, w: &str) -> Result<(), CoreError> {
        self.log(format!("unassign {i} {w}"))
    }
    fn label(&self, i: u64, l: &str) -> Result<(), CoreError> {
        self.log(format!("label {i} {l}"))
    }
    fn unlabel(&self, i: u64, l: &str) -> Result<(), CoreError> {
        self.log(format!("unlabel {i} {l}"))
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

#[test]
fn two_claims_of_one_unit_started_together_give_one_winner() {
    let (store, root) = store("race");
    let clock = FixedClock::at(1000);
    for round in 0..3u64 {
        let issue = 40 + round;
        let (a, b) = std::thread::scope(|s| {
            let a = s.spawn(|| claim(&store, &Fake::default(), &clock, issue, "alpha").unwrap());
            let b = s.spawn(|| claim(&store, &Fake::default(), &clock, issue, "beta").unwrap());
            (a.join().unwrap(), b.join().unwrap())
        });
        let mut codes = [a.exit_code(), b.exit_code()];
        codes.sort();
        assert_eq!(codes, [0, NOT_NOW_EXIT], "round {round}: {a:?} {b:?}");
        assert!(files(&store).contains(&format!("claims/{issue}.json")));
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
                let out = claim(store, &Fake::default(), clock, issue, "w").unwrap();
                assert_eq!(out.exit_code(), 0, "{out:?}");
            });
        }
    });
    let listed = files(&store);
    for issue in [7, 8, 9] {
        assert!(listed.contains(&format!("claims/{issue}.json")), "{listed}");
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
    let out = claim(&store, &fake, &FixedClock::at(1), 5, "w").unwrap();
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
    assert_eq!(claim(&store, &fake, &clock, 5, "w").unwrap().exit_code(), 0);
    assert_eq!(
        fake.calls(),
        vec!["assign 5 w".to_string(), format!("label 5 {IN_PROGRESS}")]
    );
    assert!(files(&store).contains("claims/5.json"));

    assert!(release(&store, &fake, 5).unwrap());
    assert!(!files(&store).contains("claims/"));
    let calls = fake.calls();
    assert_eq!(
        calls[2..],
        [
            "unassign 5 w".to_string(),
            format!("unlabel 5 {IN_PROGRESS}")
        ]
    );
    assert!(
        !release(&store, &fake, 5).unwrap(),
        "nothing left to release"
    );
    assert!(files(&store).contains("README"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_reaper_releases_only_claims_older_than_the_stale_limit() {
    let (store, root) = store("reap");
    let fake = Fake::default();
    let clock = FixedClock::at(1_000);
    claim(&store, &fake, &clock, 1, "old").unwrap();
    clock.advance(STALE_LIMIT);
    claim(&store, &fake, &clock, 2, "new").unwrap();
    clock.advance(1);
    assert_eq!(reap(&store, &fake, &clock, STALE_LIMIT).unwrap(), vec![1]);
    let listed = files(&store);
    assert!(
        !listed.contains("claims/1.json") && listed.contains("claims/2.json"),
        "{listed}"
    );
    assert!(fake.calls().contains(&"unassign 1 old".to_string()));
    let _ = std::fs::remove_dir_all(root);
}
