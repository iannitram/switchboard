//! #364: `switchboard backup <repo>` says so when GitHub answered for another name than the one
//! asked for (the repo was renamed or moved), the name the manifest records as `resolved_repo`.

#[path = "../src/main.rs"]
mod app;

// `cli.rs` reads `super::parse_args`, so it sits at this root beside the parser's import.
#[path = "../src/cli.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod cli;

#[allow(unused_imports)]
use app::{parse_args, Args};

use std::process::ExitCode;

use switchboard_core::backup::{BackupOutcome, Manifest};
use switchboard_core::settings::Settings;

#[test]
fn a_backup_that_followed_a_rename_says_so_on_its_line() {
    let root = std::env::temp_dir().join(format!("sb-bak-renamed-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, backups) = (root.join("data"), root.join("backups"));
    std::fs::create_dir_all(&data).unwrap();
    let settings = Settings {
        nextcloud_backups: Some(backups.clone()),
        ..Default::default()
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::backup_with(
        &settings,
        &data,
        &["a/old".to_string()],
        1_700_000_000,
        &|repo| {
            Ok(BackupOutcome::Written(Manifest {
                repo: repo.to_string(),
                resolved_repo: Some("b/new".to_string()),
                ..Default::default()
            }))
        },
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let folder = backups.join("github").join("a").join("old");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!(
            "a/old: backed up to {} (GitHub answered for b/new: renamed or moved)\n",
            folder.display()
        )
    );
    assert_eq!(String::from_utf8(err).unwrap(), "");
    let _ = std::fs::remove_dir_all(&root);
}
