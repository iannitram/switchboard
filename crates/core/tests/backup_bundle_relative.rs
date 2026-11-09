//! #90 review: `bundle` with relative paths. Its own test binary because it changes the
//! working directory, which no other test in the same process may see.

use std::process::Command;
use switchboard_core::backup::bundle;

#[test]
fn relative_mirror_and_out_dir_resolve_against_the_working_dir() {
    let root = std::env::temp_dir().join(format!("sb-bak1-rel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let ok = Command::new("git")
        .current_dir(&root)
        .args(["init", "-q", "--bare", "mirror.git"])
        .status()
        .unwrap()
        .success()
        && Command::new("git")
            .current_dir(&root)
            .args([
                "--git-dir=mirror.git",
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@e.invalid",
            ])
            .args([
                "commit-tree",
                "-m",
                "x",
                "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            ])
            .output()
            .map(|o| {
                let sha = String::from_utf8_lossy(&o.stdout).trim().to_string();
                Command::new("git")
                    .current_dir(&root)
                    .args([
                        "--git-dir=mirror.git",
                        "update-ref",
                        "refs/heads/main",
                        &sha,
                    ])
                    .status()
                    .unwrap()
                    .success()
            })
            .unwrap();
    assert!(ok, "git setup");
    std::env::set_current_dir(&root).unwrap();

    assert_eq!(bundle("mirror.git".as_ref(), "out".as_ref()).unwrap(), 1);
    assert!(root.join("out").join("repo.bundle").is_file());
    assert!(!root.join("mirror.git").join("out").exists());
    let _ = std::fs::remove_dir_all(&root);
}
