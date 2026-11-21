// Temporary path harness until CORE registers share after the v3 seed handoff.
#![allow(dead_code)]
#![cfg_attr(
    windows,
    allow(
        clippy::duplicate_mod,
        reason = "Platform tests and common fixtures each own their private Windows ACL fixture"
    )
)]
#[path = "../src/desktop/mod.rs"]
mod desktop;
#[path = "../src/launch/mod.rs"]
mod launch;
#[path = "../src/platform/mod.rs"]
mod platform;
#[path = "../src/provider/mod.rs"]
mod provider;
#[path = "../src/registry/mod.rs"]
mod registry;
#[path = "../src/share/mod.rs"]
mod share;
#[cfg(unix)]
use share::skills;

#[cfg(unix)]
mod cases {
    use super::*;
    use std::{
        fs,
        io::Write,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    fn fixture() -> tempfile::TempDir {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        tempfile::Builder::new()
            .prefix("stage06-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap()
    }
    fn directory(path: &Path) {
        platform::create_directory(path).unwrap();
    }
    fn file(path: &Path, bytes: &[u8]) {
        platform::create_file(path)
            .unwrap()
            .file
            .write_all(bytes)
            .unwrap();
    }
    fn package(root: &Path) -> PathBuf {
        let source = root.join("source");
        directory(&source);
        file(&source.join("SKILL.md"), b"example\n");
        source
    }
    #[test]
    fn stage06_store_snapshot_digest_and_immutable_copy() {
        let temp = fixture();
        let source = package(temp.path());
        directory(&source.join("notes"));
        file(&source.join("notes/a.txt"), b"hello");
        let snapshot = skills::Package::read(&source).unwrap();
        assert_eq!(
            snapshot.sha256,
            "4227b8dec26c602e18d429988a2dd7daa17275ebe98e286414267f4f0492e282"
        );
        fs::write(source.join("notes/a.txt"), b"changed").unwrap();
        let destination = temp.path().join("store");
        snapshot.copy_new(&destination).unwrap();
        assert_eq!(fs::read(destination.join("notes/a.txt")).unwrap(), b"hello");
        assert_eq!(
            skills::Package::read(&destination).unwrap().sha256,
            snapshot.sha256
        );
        assert_eq!(
            snapshot.copy_new(&destination),
            Err(skills::Error::Collision)
        );
    }
    #[test]
    fn stage06_store_limits_files_bytes_depth() {
        let temp = fixture();
        let source = package(temp.path());
        let large = platform::create_file(&source.join("large")).unwrap().file;
        large.set_len(skills::MAX_BYTES + 1).unwrap();
        assert!(matches!(
            skills::Package::read(&source),
            Err(skills::Error::TooLarge)
        ));
        fs::remove_file(source.join("large")).unwrap();
        let mut nested = source.clone();
        for _ in 0..skills::MAX_DEPTH + 1 {
            nested = nested.join("d");
            directory(&nested);
        }
        assert!(matches!(
            skills::Package::read(&source),
            Err(skills::Error::TooLarge)
        ));
        // Remove only the known synthetic empty directory chain.
        for _ in 0..skills::MAX_DEPTH + 1 {
            fs::remove_dir(&nested).unwrap();
            nested.pop();
        }
        for i in 0..skills::MAX_FILES {
            file(&source.join(format!("f{i}")), b"");
        }
        assert!(matches!(
            skills::Package::read(&source),
            Err(skills::Error::TooLarge)
        ));
    }
    #[test]
    fn stage06_drift_traversal_hardlink_symlink_special_outside_sentinel() {
        let temp = fixture();
        let source = package(temp.path());
        let outside = temp.path().join("outside");
        file(&outside, b"sentinel");
        fs::hard_link(&outside, source.join("hardlink")).unwrap();
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("hardlink")).unwrap();
        std::os::unix::fs::symlink(&outside, source.join("link")).unwrap();
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("link")).unwrap();
        let fifo =
            std::ffi::CString::new(source.join("socket").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: NUL-terminated synthetic path; no descriptor or contents IO.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("socket")).unwrap();
        assert!(skills::Package::read(&source.join("../source")).is_err());
        std::os::unix::fs::symlink(&source, temp.path().join("alias")).unwrap();
        assert!(skills::Package::read(&temp.path().join("alias")).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
    }
    #[test]
    fn stage06_store_portable_collisions_and_unsafe_ancestry() {
        let temp = fixture();
        let source = package(temp.path());
        // Exercise journal validation even on volumes that cannot represent both spellings.
        let mut encoded = serde_json::to_value(skills::Package::read(&source).unwrap()).unwrap();
        encoded["files"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "path": "skill.MD", "bytes": []
            }));
        let colliding: skills::Package = serde_json::from_value(encoded).unwrap();
        assert_eq!(colliding.validate(), Err(skills::Error::Collision));
        if !source.join("skill.MD").exists() {
            file(&source.join("skill.MD"), b"collision");
            assert!(matches!(
                skills::Package::read(&source),
                Err(skills::Error::Collision)
            ));
            fs::remove_file(source.join("skill.MD")).unwrap();
        }
        assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), b"example\n");
        file(&source.join("a:b"), b"ads");
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("a:b")).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(skills::Package::read(&source).is_err());
        fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn stage06_links_remove_only_owned_link_and_preserve_store() {
        let temp = fixture();
        let source = package(temp.path());
        let snapshot = skills::Package::read(&source).unwrap();
        directory(&temp.path().join("shared"));
        directory(&temp.path().join("shared/skills"));
        snapshot
            .copy_new(&temp.path().join("shared/skills/example"))
            .unwrap();
        let link = temp.path().join("claude-work");
        let receipt = skills::OwnedLink::create(
            temp.path(),
            &registry::ProfileName::parse("example").unwrap(),
            &link,
            &snapshot.sha256,
        )
        .unwrap();
        assert!(matches!(
            skills::OwnedLink::create(
                temp.path(),
                &registry::ProfileName::parse("example").unwrap(),
                &link,
                &snapshot.sha256
            ),
            Err(skills::Error::Collision)
        ));
        receipt.verify(&link).unwrap();
        receipt.remove(&link).unwrap();
        assert!(source.join("SKILL.md").is_file());
        assert!(!link.exists());
    }
    #[test]
    fn stage06_drift_changed_link_occupied_directory_and_copy() {
        let temp = fixture();
        let source = package(temp.path());
        let snapshot = skills::Package::read(&source).unwrap();
        directory(&temp.path().join("shared"));
        directory(&temp.path().join("shared/skills"));
        snapshot
            .copy_new(&temp.path().join("shared/skills/example"))
            .unwrap();
        let link = temp.path().join("claude-work");
        let receipt = skills::OwnedLink::create(
            temp.path(),
            &registry::ProfileName::parse("example").unwrap(),
            &link,
            &snapshot.sha256,
        )
        .unwrap();
        fs::remove_file(&link).unwrap();
        directory(&link);
        file(&link.join("native"), b"keep");
        assert_eq!(receipt.remove(&link), Err(skills::Error::Drift));
        assert_eq!(fs::read(link.join("native")).unwrap(), b"keep");
        let copy = temp.path().join("codex-45");
        snapshot.copy_new(&copy).unwrap();
        let receipt = skills::OwnedCopy::capture(&copy, &snapshot.sha256).unwrap();
        fs::write(copy.join("SKILL.md"), b"changed").unwrap();
        assert_eq!(receipt.remove(&copy), Err(skills::Error::Drift));
        fs::write(copy.join("SKILL.md"), b"example\n").unwrap();
        receipt.remove(&copy).unwrap();
        assert!(!copy.exists());
        assert!(source.exists());
    }
    #[test]
    fn stage06_drift_nested_copy_outside_target_untouched() {
        let temp = fixture();
        let source = package(temp.path());
        directory(&source.join("nested"));
        file(&source.join("nested/sentinel"), b"inside");
        let snapshot = skills::Package::read(&source).unwrap();
        let copy = temp.path().join("codex-45");
        snapshot.copy_new(&copy).unwrap();
        let receipt = skills::OwnedCopy::capture(&copy, &snapshot.sha256).unwrap();
        let outside = temp.path().join("outside");
        directory(&outside);
        file(&outside.join("sentinel"), b"outside");
        fs::rename(copy.join("nested"), copy.join("saved")).unwrap();
        std::os::unix::fs::symlink(&outside, copy.join("nested")).unwrap();
        assert_eq!(receipt.remove(&copy), Err(skills::Error::Drift));
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
        fs::remove_file(copy.join("nested")).unwrap();
        fs::rename(copy.join("saved"), copy.join("nested")).unwrap();
        receipt.remove(&copy).unwrap();
        assert!(!copy.exists());
    }
    #[test]
    fn stage06_store_missing_skill_and_symlink_trailing_separator() {
        let temp = fixture();
        let source = package(temp.path());
        std::os::unix::fs::symlink(&source, temp.path().join("alias")).unwrap();
        assert!(skills::Package::read(&temp.path().join("alias/")).is_err());
        fs::remove_file(source.join("SKILL.md")).unwrap();
        assert!(skills::Package::read(&source).is_err());
    }
    #[test]
    fn stage06_drift_copy_replaced_file_same_bytes() {
        let temp = fixture();
        let source = package(temp.path());
        let snapshot = skills::Package::read(&source).unwrap();
        let receipt = skills::OwnedCopy::capture(&source, &snapshot.sha256).unwrap();
        let original = fs::File::open(source.join("SKILL.md")).unwrap();
        fs::remove_file(source.join("SKILL.md")).unwrap();
        file(&source.join("SKILL.md"), b"example\n");
        assert_eq!(receipt.remove(&source), Err(skills::Error::Drift));
        drop(original);
    }
}

mod common;
fn request(f: &common::Fixture, args: &[&str], code: i32) -> serde_json::Value {
    let out = common::capture(f.command(args)).unwrap();
    assert_eq!(out.status.code(), Some(code), "{}", common::stderr(&out));
    assert!(out.stderr.is_empty());
    assert_eq!(out.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn stage06_cli_schema1_expected_revision_and_complete_lifecycle() {
    use serde_json::json;
    let f = common::Fixture::new();
    let source = f.temp.path().join("source");
    common::create_directory(&source);
    common::create_file(&source.join("SKILL.md"), b"example\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            source.join("SKILL.md"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }
    let before = f.outside_snapshot();
    let empty = request(&f, &["cc", "skills", "list", "--json"], 0);
    assert_eq!(
        empty,
        json!({"schema":1,"command":"skills.list","ok":true,"revision":"0","data":{"skills":[]}})
    );
    assert!(!f.root.exists());
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    let added = request(
        &f,
        &[
            "cc",
            "skills",
            "add",
            source.to_str().unwrap(),
            "--name",
            "example",
            "--json",
            "--expected-revision",
            "1",
        ],
        0,
    );
    assert_eq!(
        added,
        json!({"schema":1,"command":"skills.add","ok":true,"revision":"2","data":{"changed":true,"profile_id":null,"name":"example","warnings":[]}})
    );
    let list = request(&f, &["cc", "skills", "list", "--json"], 0);
    assert_eq!(
        list["data"]["skills"][0]["switches"],
        json!([{"profile_id":"1","enabled":true}])
    );
    assert_eq!(list["data"]["skills"][0]["state"], "ok");
    let matrix = request(&f, &["cc", "skills", "matrix", "--json"], 0);
    assert_eq!(
        matrix,
        json!({"schema":1,"command":"skills.matrix","ok":true,"revision":"2","data":{"profiles":[{"id":"1","name":"claude-work","provider":"claude"}],"rows":[{"name":"example","enabled":[true]}]}})
    );
    let stale = request(
        &f,
        &[
            "cc",
            "skills",
            "disable",
            "missing",
            "missing",
            "--expected-revision",
            "1",
            "--json",
        ],
        1,
    );
    assert_eq!(stale["error"]["code"], "conflict");
    assert_eq!(
        request(&f, &["cc", "skills", "remove", "example", "--json"], 1)["error"]["code"],
        "conflict"
    );
    let disabled = request(
        &f,
        &[
            "cc",
            "skills",
            "disable",
            "example",
            "claude-work",
            "--json",
        ],
        0,
    );
    assert_eq!(
        disabled,
        json!({"schema":1,"command":"skills.disable","ok":true,"revision":"3","data":{"changed":true,"profile_id":"1","name":"example","warnings":[]}})
    );
    let noop = request(
        &f,
        &[
            "cc",
            "skills",
            "disable",
            "example",
            "claude-work",
            "--json",
        ],
        0,
    );
    assert_eq!(noop["data"]["changed"], false);
    assert_eq!(noop["revision"], "3");
    request(
        &f,
        &["cc", "skills", "enable", "example", "claude-work", "--json"],
        0,
    );
    request(
        &f,
        &[
            "cc",
            "skills",
            "disable",
            "example",
            "claude-work",
            "--json",
        ],
        0,
    );
    request(&f, &["cc", "skills", "remove", "example", "--json"], 0);
    assert_eq!(
        request(&f, &["cc", "skills", "sync", "--json"], 0)["data"]["changed"],
        false
    );
    assert_eq!(
        request(&f, &["cc", "skills", "list", "--json"], 0)["data"],
        json!({"skills":[]})
    );
    assert_eq!(f.outside_snapshot(), before);
}

#[test]
fn stage06_source_confinement_refuses_managed_and_vendor_packages() {
    let f = common::Fixture::new();
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    let cli = f.root.join("profiles/1/cli");
    common::create_file(&cli.join("SKILL.md"), b"synthetic");
    common::create_file(&cli.join(".credentials.json"), b"synthetic-secret");
    let registry = std::fs::read(f.root.join("registry.json")).unwrap();
    let mut sources = vec![cli, f.root.clone()];
    for home in [".claude-work", ".codex7", ".agents"] {
        let path = f.temp.path().join(home);
        common::create_directory(&path);
        common::create_file(&path.join("SKILL.md"), b"synthetic");
        common::create_file(&path.join("auth.json"), b"synthetic-secret");
        sources.push(path);
    }
    for source in sources {
        let result = request(
            &f,
            &[
                "cc",
                "skills",
                "add",
                source.to_str().unwrap(),
                "--name",
                "leak",
                "--json",
            ],
            1,
        );
        assert_eq!(result["error"]["code"], "forbidden_source");
        let text = result.to_string();
        assert!(!text.contains("synthetic-secret"));
        assert!(!text.contains(source.to_str().unwrap()));
        assert!(!f.root.join("shared/skills/leak").exists());
        assert_eq!(
            std::fs::read(f.root.join("registry.json")).unwrap(),
            registry
        );
    }
    let configured = f.temp.path().join("explicit-home");
    common::create_directory(&configured);
    common::create_file(&configured.join("SKILL.md"), b"synthetic");
    for variable in ["CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        let mut cmd = f.command(&[
            "cc",
            "skills",
            "add",
            configured.to_str().unwrap(),
            "--name",
            "leak",
            "--json",
        ]);
        cmd.env(variable, &configured);
        let out = common::capture(cmd).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["error"]["code"], "forbidden_source");
    }
    assert!(!f.root.join("share-journal.json").exists());
}
#[test]
fn stage06_human_output_and_version_labels_are_bounded() {
    let f = common::Fixture::new();
    assert_eq!(
        common::stdout(&f.run(&["cc", "skills", "list"], 0)),
        "No shared skills.\n"
    );
    assert!(!f.root.exists());
    for label in ["", "1\n2", "1/2", &"a".repeat(129)] {
        let result = request(
            &f,
            &[
                "cc",
                "app",
                "bind",
                "codex",
                "/invalid",
                "--version",
                label,
                "--json",
            ],
            2,
        );
        assert_eq!(result["error"]["code"], "usage");
        assert!(!f.root.exists());
    }
}

fn home_alias(target: &std::path::Path, alias: &std::path::Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, alias)
        .expect("native directory alias fixture required; no skipped acceptance");
}
fn assert_home_alias_forbidden(variable: Option<&str>, nested: bool, namespace: Option<&str>) {
    let f = common::Fixture::new();
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "codex-45",
            "--provider",
            "codex",
            "--json",
        ],
        0,
    );
    let home = f.temp.path().join("source-home");
    common::create_directory(&home);
    let package = if nested {
        let child = home.join("package");
        common::create_directory(&child);
        child
    } else {
        home.clone()
    };
    common::create_file(&package.join("SKILL.md"), b"synthetic package");
    common::create_file(&package.join("auth.json"), b"synthetic-secret");
    let alias = namespace
        .map(|n| f.temp.path().join("home").join(n))
        .unwrap_or_else(|| f.temp.path().join("configured-home"));
    home_alias(&home, &alias);
    let before = std::fs::read(f.root.join("registry.json")).unwrap();
    let mut cmd = f.command(&[
        "cc",
        "skills",
        "add",
        package.to_str().unwrap(),
        "--name",
        "leak",
        "--json",
    ]);
    if let Some(variable) = variable {
        cmd.env(variable, &alias);
    }
    let out = common::capture(cmd).unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", common::stdout(&out));
    assert!(out.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["code"], "forbidden_source");
    assert!(!common::stdout(&out).contains("synthetic-secret"));
    assert!(!f.root.join("shared/skills/leak").exists());
    assert_eq!(std::fs::read(f.root.join("registry.json")).unwrap(), before);
    assert!(!f.root.join("share-journal.json").exists());
}
#[test]
fn stage06_source_configured_symlinked_home_identity_and_source_ancestor() {
    for variable in ["CODEX_HOME", "CLAUDE_CONFIG_DIR"] {
        assert_home_alias_forbidden(Some(variable), false, None);
        assert_home_alias_forbidden(Some(variable), true, None);
    }
}
#[test]
fn stage06_source_default_vendor_home_alias_namespaces() {
    for namespace in [".codex7", ".claude-work", ".agents"] {
        assert_home_alias_forbidden(None, false, Some(namespace));
        assert_home_alias_forbidden(None, true, Some(namespace));
    }
}
#[test]
fn stage06_source_unresolvable_home_fails_closed_missing_home_allows_source() {
    let f = common::Fixture::new();
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    let source = f.temp.path().join("package");
    common::create_directory(&source);
    common::create_file(&source.join("SKILL.md"), b"synthetic");
    let missing = f.temp.path().join("absent-home");
    let dangling = f.temp.path().join("dangling-home");
    home_alias(&missing, &dangling);
    let not_a_directory = f.temp.path().join("file-home");
    common::create_file(&not_a_directory, b"not a directory");
    for variable in ["CODEX_HOME", "CLAUDE_CONFIG_DIR"] {
        for path in [&dangling, &not_a_directory] {
            let mut cmd = f.command(&[
                "cc",
                "skills",
                "add",
                source.to_str().unwrap(),
                "--name",
                "leak",
                "--json",
            ]);
            cmd.env(variable, path);
            let out = common::capture(cmd).unwrap();
            assert_eq!(out.status.code(), Some(1));
            assert!(out.stderr.is_empty());
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(value["error"]["code"], "forbidden_source");
        }
    }
    // Fixture::new already creates this synthetic config sentinel on every OS.
    // Reuse it: Windows create_file deliberately refuses an existing file.
    let config = f.temp.path().join("home/.claude.json");
    let config_before = std::fs::read(&config).unwrap();
    let mut cmd = f.command(&[
        "cc",
        "skills",
        "add",
        source.to_str().unwrap(),
        "--name",
        "example",
        "--json",
    ]);
    cmd.env("CODEX_HOME", &missing)
        .env("CLAUDE_CONFIG_DIR", &missing);
    let out = common::capture(cmd).unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", common::stdout(&out));
    assert!(out.stderr.is_empty());
    assert!(!missing.exists());
    assert!(!f.root.join("shared/skills/leak").exists());
    assert_eq!(std::fs::read(&config).unwrap(), config_before);
}

fn assert_parent_source_forbidden(variable: Option<&str>, alias: bool) {
    let f = common::Fixture::new();
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "codex-45",
            "--provider",
            "codex",
            "--json",
        ],
        0,
    );
    let source = if variable.is_none() {
        f.temp.path().join("home")
    } else {
        let p = f.temp.path().join("package");
        common::create_directory(&p);
        p
    };
    common::create_file(&source.join("SKILL.md"), b"synthetic package");
    let vendor = source.join(if variable.is_none() { ".claude" } else { "cfg" });
    common::create_directory(&vendor);
    common::create_file(
        &vendor.join(if variable == Some("CODEX_HOME") {
            "auth.json"
        } else {
            ".credentials.json"
        }),
        b"synthetic-private-data",
    );
    let home = if alias {
        let link = f.temp.path().join("home-alias");
        home_alias(&vendor, &link);
        link
    } else {
        vendor
    };
    let before = std::fs::read(f.root.join("registry.json")).unwrap();
    let mut cmd = f.command(&[
        "cc",
        "skills",
        "add",
        source.to_str().unwrap(),
        "--name",
        "leak",
        "--json",
    ]);
    if let Some(variable) = variable {
        cmd.env(variable, home);
    }
    let out = common::capture(cmd).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["code"], "forbidden_source");
    assert_eq!(std::fs::read(f.root.join("registry.json")).unwrap(), before);
    assert!(!f.root.join("shared").exists());
    assert!(!f.root.join("share-journal.json").exists());
    for id in [1, 2] {
        assert_eq!(
            std::fs::read_dir(f.root.join(format!("profiles/{id}/cli")))
                .unwrap()
                .count(),
            0
        );
    }
    assert!(!common::stdout(&out).contains("synthetic-private-data"));
}
#[test]
fn stage06_source_parent_containing_configured_or_default_vendor_home_refuses() {
    for variable in ["CODEX_HOME", "CLAUDE_CONFIG_DIR"] {
        for alias in [false, true] {
            assert_parent_source_forbidden(Some(variable), alias);
        }
    }
    assert_parent_source_forbidden(None, false);
}
#[test]
fn stage06_source_parent_containing_managed_root_refuses() {
    let f = common::Fixture::new();
    request(
        &f,
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    common::create_file(&f.temp.path().join("SKILL.md"), b"synthetic package");
    let before = std::fs::read(f.root.join("registry.json")).unwrap();
    let value = request(
        &f,
        &[
            "cc",
            "skills",
            "add",
            f.temp.path().to_str().unwrap(),
            "--name",
            "leak",
            "--json",
        ],
        1,
    );
    assert_eq!(value["error"]["code"], "forbidden_source");
    assert_eq!(std::fs::read(f.root.join("registry.json")).unwrap(), before);
    assert!(!f.root.join("shared").exists());
}
#[test]
fn stage06_store_credential_shaped_entries_refuse_without_copying() {
    for denied in [
        "auth.json",
        "AUTH.JSON",
        "nested/id_rsa.pub",
        "nested/.credentials.json",
        "nested/key.pem",
        "nested/KEY.PEM",
        "nested/id_rsa",
        "nested/.env",
        "nested/.env.local",
        "nested/.git/config",
        "nested/.hidden",
    ] {
        let f = common::Fixture::new();
        request(
            &f,
            &[
                "cc",
                "profile",
                "add",
                "claude-work",
                "--provider",
                "claude",
                "--json",
            ],
            0,
        );
        let source = f.temp.path().join("package");
        common::create_directory(&source);
        common::create_file(&source.join("SKILL.md"), b"synthetic");
        let file = source.join(denied);
        let parent = file.parent().unwrap();
        let mut current = source.clone();
        for component in parent.strip_prefix(&source).unwrap().components() {
            current.push(component);
            common::create_directory(&current);
        }
        common::create_file(&file, b"synthetic-private-data");
        let before = std::fs::read(f.root.join("registry.json")).unwrap();
        let result = request(
            &f,
            &[
                "cc",
                "skills",
                "add",
                source.to_str().unwrap(),
                "--name",
                "leak",
                "--json",
            ],
            1,
        );
        assert_eq!(result["error"]["code"], "unsafe_path");
        assert_eq!(std::fs::read(f.root.join("registry.json")).unwrap(), before);
        assert!(!f.root.join("shared").exists());
        assert!(!f.root.join("share-journal.json").exists());
        assert_eq!(
            std::fs::read_dir(f.root.join("profiles/1/cli"))
                .unwrap()
                .count(),
            0
        );
        assert!(!result.to_string().contains("synthetic-private-data"));
    }
}
#[cfg(unix)]
#[test]
fn stage06_source_vendor_home_0775_does_not_block_unrelated_package() {
    use std::os::unix::fs::PermissionsExt;
    for configured in [false, true] {
        let f = common::Fixture::new();
        let source = f.temp.path().join("package");
        common::create_directory(&source);
        common::create_file(&source.join("SKILL.md"), b"synthetic");
        let vendor = f.temp.path().join("home/.codex");
        common::create_directory(&vendor);
        std::fs::set_permissions(&vendor, std::fs::Permissions::from_mode(0o775)).unwrap();
        let mut cmd = f.command(&[
            "cc",
            "skills",
            "add",
            source.to_str().unwrap(),
            "--name",
            "example",
            "--json",
        ]);
        if configured {
            cmd.env("CODEX_HOME", &vendor);
        }
        let out = common::capture(cmd).unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", common::stdout(&out));
        assert!(out.stderr.is_empty());
    }
}
#[test]
fn stage06_source_default_claude_config_file_alias_is_not_a_home() {
    for dangling in [false, true] {
        let f = common::Fixture::new();
        let source = f.temp.path().join("package");
        common::create_directory(&source);
        common::create_file(&source.join("SKILL.md"), b"synthetic");
        let target = f.temp.path().join("settings.json");
        if !dangling {
            common::create_file(&target, b"synthetic config");
        }
        let alias = f.temp.path().join("home/.claude.json");
        std::fs::remove_file(&alias).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &alias)
            .expect("native file alias fixture required");
        request(
            &f,
            &[
                "cc",
                "skills",
                "add",
                source.to_str().unwrap(),
                "--name",
                "example",
                "--json",
            ],
            0,
        );
    }
}

#[cfg(unix)]
#[test]
fn stage06_many_profiles_add_under_bounded_descriptor_limit() {
    use std::os::unix::process::CommandExt;
    #[cfg(target_os = "linux")]
    let cases = [(8, 256 as libc::rlim_t), (40, 1024)];
    #[cfg(not(target_os = "linux"))]
    let cases = [(8, 256 as libc::rlim_t)];
    for (profiles, bound) in cases {
        let mut f = common::Fixture::new();
        let nested = f.temp.path().join("nested");
        common::create_directory(&nested);
        f.root = nested.join("managed");
        for i in 0..profiles {
            request(
                &f,
                &[
                    "cc",
                    "profile",
                    "add",
                    &format!("claude-work-{i}"),
                    "--provider",
                    "claude",
                    "--json",
                ],
                0,
            );
        }
        let source = f.temp.path().join("package");
        common::create_directory(&source);
        common::create_file(&source.join("SKILL.md"), b"synthetic package");
        let mut cmd = f.command(&[
            "cc",
            "skills",
            "add",
            source.to_str().unwrap(),
            "--name",
            "example",
            "--json",
        ]);
        // SAFETY: the child closure uses only async-signal-safe resource-limit
        // syscalls and captures a plain limit value; the parent limit is unchanged.
        unsafe {
            cmd.pre_exec(move || {
                let mut limit = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                limit.rlim_cur = bound.min(limit.rlim_max);
                if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let out = common::capture(cmd).unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "profiles={profiles},limit={bound}: {} {}",
            common::stdout(&out),
            common::stderr(&out)
        );
        assert!(out.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["data"]["changed"], true);
        for id in 1..=profiles {
            assert!(
                f.root
                    .join(format!("profiles/{id}/cli/skills/example/SKILL.md"))
                    .is_file()
            );
        }
    }
}
