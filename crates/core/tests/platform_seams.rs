//! Locked tests for issue 7 (the platform seams, R04). Qwen never edits this file.

use std::path::{Path, PathBuf};
use switchboard_core::platform::{
    default_dev_root, default_nextcloud_dir, loadout_probes, python_launcher, Os,
};

fn home() -> PathBuf {
    PathBuf::from("/home/someone")
}

#[test]
fn linux_and_mac_run_python3_isolated() {
    assert_eq!(python_launcher(Os::Linux), ("python3", &["-I"][..]));
    assert_eq!(python_launcher(Os::Mac), ("python3", &["-I"][..]));
}

#[test]
fn windows_runs_the_py_launcher_isolated() {
    assert_eq!(python_launcher(Os::Windows), ("py", &["-3", "-I"][..]));
}

#[test]
fn dev_root_is_home_dev_on_linux_and_mac() {
    let h = home();
    assert_eq!(default_dev_root(Os::Linux, &h), h.join("dev"));
    assert_eq!(default_dev_root(Os::Mac, &h), h.join("dev"));
}

#[test]
fn dev_root_is_the_code_drive_on_windows_whatever_the_home() {
    assert_eq!(
        default_dev_root(Os::Windows, &home()),
        PathBuf::from("D:\\dev")
    );
    assert_eq!(
        default_dev_root(Os::Windows, Path::new("C:\\Users\\someone")),
        PathBuf::from("D:\\dev")
    );
}

#[test]
fn nextcloud_is_home_nextcloud_on_every_os() {
    let h = home();
    for os in [Os::Linux, Os::Mac, Os::Windows] {
        assert_eq!(default_nextcloud_dir(os, &h), h.join("Nextcloud"), "{os:?}");
    }
}

#[test]
fn mac_and_windows_have_no_loadout_probes_yet() {
    assert!(loadout_probes(Os::Mac).is_empty());
    assert!(loadout_probes(Os::Windows).is_empty());
}

#[test]
fn the_home_passed_in_is_not_changed() {
    let h = home();
    let before = h.clone();
    let _ = default_dev_root(Os::Linux, &h);
    let _ = default_nextcloud_dir(Os::Linux, &h);
    assert_eq!(h, before);
}

#[test]
fn the_code_map_shows_core_platform_present() {
    let map = include_str!("../../../docs/code-map.md");
    let line = map
        .lines()
        .skip_while(|l| *l != "### core-platform")
        .nth(1)
        .unwrap();
    assert!(line.contains("· present."), "{line}");
}
