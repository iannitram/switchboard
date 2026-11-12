//! #298: a fleet child does not inherit the packaged app's Python and library variables.

use std::ffi::OsString;
use std::path::PathBuf;
use switchboard_core::fleet::clean_ld_library_path;

fn list(entries: &[&str]) -> OsString {
    std::env::join_paths(entries.iter().map(PathBuf::from)).unwrap()
}

#[test]
fn entries_inside_appdir_are_dropped_and_the_rest_keep_their_order() {
    let ld = list(&["/mnt/app/usr/lib", "/opt/a", "/mnt/app/lib", "/opt/b"]);
    let out = clean_ld_library_path(&ld, &OsString::from("/mnt/app")).unwrap();
    assert_eq!(out, list(&["/opt/a", "/opt/b"]));
}

#[test]
fn a_sibling_folder_with_the_same_prefix_text_is_kept() {
    let ld = list(&["/mnt/app2/lib", "/mnt/app/lib"]);
    let out = clean_ld_library_path(&ld, &OsString::from("/mnt/app")).unwrap();
    assert_eq!(out, list(&["/mnt/app2/lib"]));
}

#[test]
fn nothing_left_means_remove_the_variable() {
    let ld = list(&["/mnt/app/usr/lib", "/mnt/app/lib"]);
    assert_eq!(
        clean_ld_library_path(&ld, &OsString::from("/mnt/app")),
        None
    );
}

#[test]
fn no_appdir_leaves_the_value_alone() {
    let ld = list(&["/mnt/app/usr/lib", "/opt/a"]);
    assert_eq!(clean_ld_library_path(&ld, &OsString::new()), Some(ld));
}
