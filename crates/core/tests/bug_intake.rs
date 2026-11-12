use std::path::PathBuf;
use switchboard_core::bug::intake::{self, Report};
use switchboard_core::bug::{parse_owners, Bug};
use switchboard_core::CoreError;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-bug-intake-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn report(id: &str, sig: Option<&str>) -> Report {
    Report {
        id: id.into(),
        source: "drop".into(),
        app: "notes".into(),
        version: Some("1.2".into()),
        report_class: "bug".into(),
        symptom: "Saving loses the last line".into(),
        steps: vec!["Open a note".into(), "Save".into()],
        error_signature: sig.map(Into::into),
        severity: "data-loss".into(),
        raw_copy: "raw/x".into(),
    }
}

fn bug(id: &str, repo: &str, sig: &str, status: &str) -> Bug {
    Bug {
        id: id.into(),
        repo: repo.into(),
        issue: 1,
        reports: vec!["old".into()],
        error_signature: Some(sig.into()),
        severity: "crash".into(),
        status: status.into(),
        attempts: 0,
    }
}

#[test]
fn sha256_known_answers() {
    assert_eq!(
        intake::sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        intake::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn scan_finds_new_zips_and_skips_archive_and_known() {
    let inbox = tmp("scan");
    std::fs::create_dir_all(inbox.join("sub")).unwrap();
    std::fs::create_dir_all(inbox.join("_archive")).unwrap();
    std::fs::write(inbox.join("a.zip"), b"one").unwrap();
    std::fs::write(inbox.join("sub/b.zip"), b"two").unwrap();
    std::fs::write(inbox.join("_archive/c.zip"), b"three").unwrap();
    std::fs::write(inbox.join("note.txt"), b"x").unwrap();
    let all = intake::scan(&inbox, &[]).unwrap();
    assert_eq!(all.len(), 2);
    let mut known = bug("b", "o/r", "e", "open");
    known.reports = vec![intake::sha256_hex(b"one")];
    let rest = intake::scan(&inbox, &[known]).unwrap();
    assert_eq!(rest.len(), 1);
    assert!(rest[0].path.ends_with("sub/b.zip"));
}

#[test]
fn owner_missing_names_the_app() {
    let owners = parse_owners(r#"[{"app":"notes","repo":"o/notes"}]"#).unwrap();
    assert_eq!(intake::owner(&owners, "notes").unwrap().repo, "o/notes");
    let err = intake::owner(&owners, "paint").unwrap_err();
    assert!(
        matches!(&err, CoreError::Refused(m) if m.contains("paint")),
        "{err}"
    );
}

#[test]
fn duplicate_in_same_repo_is_linked_not_filed() {
    let dir = tmp("dup");
    let ledger = dir.join("bugs.jsonl");
    let first = report("r1", Some("panic at  src/save.rs"));
    let filed = intake::record(&ledger, &first, "o/notes", 7).unwrap();
    let bugs = intake::read_bugs(&ledger).unwrap();
    let hit = intake::dedupe(&bugs, "o/notes", Some("PANIC at src/save.rs")).unwrap();
    assert_eq!(hit.id, filed.id);
    assert!(intake::dedupe(&bugs, "o/other", Some("panic at src/save.rs")).is_none());
    assert!(intake::dedupe(&bugs, "o/notes", None).is_none());
    intake::link_duplicate(&ledger, &hit.id, "r2").unwrap();
    intake::link_duplicate(&ledger, &hit.id, "r2").unwrap();
    let bugs = intake::read_bugs(&ledger).unwrap();
    assert_eq!(bugs.len(), 1);
    assert_eq!(bugs[0].reports, vec!["r1", "r2"]);
}

#[test]
fn closed_bug_is_not_a_duplicate() {
    let bugs = vec![bug("b", "o/r", "boom", "closed")];
    assert!(intake::dedupe(&bugs, "o/r", Some("boom")).is_none());
}

#[test]
fn record_is_idempotent_and_opens_the_bug() {
    let dir = tmp("record");
    let ledger = dir.join("bugs.jsonl");
    let r = report("r1", Some("sig"));
    let a = intake::record(&ledger, &r, "o/notes", 7).unwrap();
    let b = intake::record(&ledger, &r, "o/notes", 8).unwrap();
    assert_eq!(a, b);
    assert_eq!((a.status.as_str(), a.attempts, a.issue), ("open", 0, 7));
    assert_eq!(intake::read_bugs(&ledger).unwrap().len(), 1);
}

#[test]
fn private_word_is_refused_before_any_text_is_written() {
    let words = vec!["Zelda".to_string()];
    let mut r = report("r1", None);
    r.symptom = "crash on zelda's laptop".into();
    let err = intake::issue_text(&r, &words).unwrap_err();
    assert!(
        matches!(&err, CoreError::Refused(m) if !m.contains("zelda")),
        "{err}"
    );
    let text = serde_json::to_string(&r).unwrap();
    assert!(matches!(
        intake::validate(&text, &words),
        Err(CoreError::Refused(_))
    ));
    // a word inside another word is not a match
    r.symptom = "zeldaesque crash".into();
    assert!(intake::issue_text(&r, &words).is_ok());
}

#[test]
fn validate_checks_the_form() {
    let ok = serde_json::to_string(&report("r1", Some("s"))).unwrap();
    assert_eq!(intake::validate(&ok, &[]).unwrap().app, "notes");
    let bad = ok.replace("data-loss", "meh");
    assert!(
        matches!(intake::validate(&bad, &[]), Err(CoreError::Refused(m)) if m.contains("severity"))
    );
    assert!(matches!(
        intake::validate("{}", &[]),
        Err(CoreError::Parse(_))
    ));
    let (title, body) = intake::issue_text(&report("r1", Some("s")), &[]).unwrap();
    assert_eq!(title, "Saving loses the last line");
    assert!(body.contains("1. Open a note") && body.contains("`s`"));
}

#[test]
fn archive_only_when_the_raw_copy_matches() {
    let dir = tmp("archive");
    let inbox = dir.join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let original = inbox.join("r.zip");
    std::fs::write(&original, b"report bytes").unwrap();
    let raw = intake::capture(&original, &dir.join("raw"), "2026-11-12").unwrap();
    let sums = std::fs::read_to_string(raw.join("SHA256SUMS")).unwrap();
    assert!(sums.starts_with(&intake::sha256_hex(b"report bytes")));

    // damage the raw copy: the original must stay
    std::fs::write(raw.join("r.zip"), b"damaged").unwrap();
    let err = intake::archive(&original, &raw, &dir.join("_archive"), "2026-11-12").unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)));
    assert!(original.exists());

    std::fs::write(raw.join("r.zip"), b"report bytes").unwrap();
    let to = intake::archive(&original, &raw, &dir.join("_archive"), "2026-11-12").unwrap();
    assert!(!original.exists());
    assert_eq!(std::fs::read(to).unwrap(), b"report bytes");
}
