#![cfg(target_os = "macos")]
mod common;
use common::*;
use std::{fs, process::Command};
#[test]
fn extended_allow_acl_refuses_each_managed_class_without_content_changes() {
    for class in [
        "",
        "profiles",
        "registry.json",
        "registry.lock",
        "registry.json.tmp",
        "profiles/1",
    ] {
        let f = Fixture::new();
        f.profile(&["add", "work", "--provider", "claude"], 0);
        let path = if class.is_empty() {
            f.root.clone()
        } else {
            f.root.join(class)
        };
        if class == "registry.json.tmp" {
            fs::write(&path, b"synthetic stale metadata").unwrap();
            private(&path, false);
        }
        let registry = fs::read(f.root.join("registry.json")).unwrap();
        let grant = if path.is_dir() {
            "everyone allow list"
        } else {
            "everyone allow read"
        };
        let status = Command::new("/bin/chmod")
            .env_clear()
            .args(["+a", grant])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let output = f.profile(&["list"], if class == "profiles/1" { 0 } else { 1 });
        if class == "profiles/1" {
            assert!(stdout(&output).contains("unsafe"));
        } else {
            assert!(stderr(&output).contains("has an extended ACL that grants access"));
        }
        assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), registry);
        assert!(
            Command::new("/bin/chmod")
                .env_clear()
                .arg("-N")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
    }
}
