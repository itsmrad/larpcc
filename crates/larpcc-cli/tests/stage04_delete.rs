mod common;
use common::*;

fn request(f: &Fixture, args: &[&str], code: i32) -> serde_json::Value {
    envelope(&exec(
        f.command(&[&["cc", "profile", "--json"][..], args].concat()),
        code,
    ))
}

#[test]
fn stage04_delete_confirmation_and_logout_gates() {
    let f = Fixture::new();
    for (action, args, code, error) in [
        ("delete", &["delete", "claude-work"][..], 2, "usage"),
        ("recover", &["recover", "claude-work"][..], 2, "usage"),
        (
            "delete",
            &["delete", "claude-work", "--yes", "--logout"][..],
            2,
            "json_native_io",
        ),
    ] {
        let result = request(&f, args, code);
        assert_eq!(result["command"], format!("profile.{action}"));
        assert_eq!(result["error"]["code"], error);
        assert!(!f.root.exists());
    }
    assert!(
        stderr(&f.profile(&["delete", "claude-work", "--yes", "--logout"], 1)).contains("qualif")
    );
    assert!(!f.root.exists());
}

#[cfg(unix)]
#[test]
fn stage04_delete() {
    let f = Fixture::new();
    let old = request(&f, &["add", "codex-45", "--provider", "codex"], 0);
    assert_eq!(old["data"]["profile_id"], "1");
    request(&f, &["forget", "codex-45", "--yes"], 0);
    assert!(f.root.join("profiles/1/cli").is_dir());
    request(&f, &["add", "codex-45", "--provider", "codex"], 0);
    create_file(&f.root.join("profiles/2/cli/synthetic-state"), b"fixture");
    let out = request(&f, &["delete", "codex-45", "--yes"], 0);
    assert_eq!(out["command"], "profile.delete");
    let examples: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../docs/contracts/cli-v1.examples.json")).unwrap();
    let example = examples
        .iter()
        .find(|v| v["command"] == "profile.delete" && v["ok"] == true)
        .unwrap();
    assert_eq!(out["data"], example["data"]);
    assert!(!f.root.join("profiles/2").exists());
    assert_eq!(
        request(&f, &["add", "codex-45", "--provider", "codex"], 0)["data"]["profile_id"],
        "3"
    );
}

#[cfg(unix)]
#[test]
fn stage04_delete_cleans_owned_aliases_and_sharing_references() {
    use std::fs;
    let f = Fixture::new();
    request(&f, &["add", "claude-work", "--provider", "claude"], 0);
    f.larp_json(
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
        0,
    );
    let source = f.temp.path().join("source");
    create_directory(&source);
    create_file(&source.join("SKILL.md"), b"synthetic");
    for skill in ["example", "sample"] {
        f.run(
            &[
                "cc",
                "skills",
                "add",
                source.to_str().unwrap(),
                "--name",
                skill,
            ],
            0,
        );
    }
    f.run(&["cc", "skills", "disable", "sample", "claude-work"], 0);
    assert_eq!(
        request(&f, &["forget", "claude-work", "--yes"], 1)["error"]["code"],
        "conflict"
    );
    assert!(f.launcher("codex-45").is_file());
    assert!(f.root.join("profiles/1/cli/skills/example").is_symlink());
    request(&f, &["delete", "claude-work", "--yes"], 0);
    assert!(!f.launcher("codex-45").exists());
    assert!(!f.alias_receipt("codex-45").exists());
    assert!(!f.root.join("profiles/1").exists());
    assert!(f.root.join("shared/skills/example/SKILL.md").is_file());
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(f.root.join("skills.json")).unwrap()).unwrap();
    assert!(
        index["skills"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["disabled"] == serde_json::json!([]))
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(f.root.join("owned-files.json")).unwrap()).unwrap();
    assert!(
        manifest["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["profile"].is_null())
    );
}

#[cfg(target_os = "macos")]
#[test]
fn stage04_delete_recovery_refuses_uncertain_desktop_activity() {
    let f = Fixture::new();
    request(&f, &["add", "codex-45", "--provider", "codex"], 0);
    let desktop = f.root.join("profiles/1/desktop");
    create_file(&desktop.join("SingletonLock"), b"unverifiable");
    let out = request(&f, &["delete", "codex-45", "--yes"], 1);
    assert_eq!(out["error"]["code"], "busy");
    assert!(desktop.is_dir());
    assert!(!f.root.join("share-journal.json").exists());
}

#[test]
fn stage04_delete_and_recover_refuse_stale_name_reuse() {
    let f = Fixture::new();
    request(&f, &["add", "claude-work", "--provider", "claude"], 0);
    let old = request(&f, &["add", "codex-45", "--provider", "codex"], 0);
    let revision = old["revision"].as_str().unwrap();
    request(&f, &["rename", "codex-45", "codex-work"], 0);
    request(&f, &["add", "codex-45", "--provider", "codex"], 0);
    let snapshot = std::fs::read(f.root.join("registry.json")).unwrap();
    for action in ["delete", "recover"] {
        let out = request(
            &f,
            &[action, "codex-45", "--yes", "--expected-revision", revision],
            1,
        );
        assert_eq!(out["error"]["code"], "conflict");
        assert_eq!(
            std::fs::read(f.root.join("registry.json")).unwrap(),
            snapshot
        );
        assert!(f.root.join("profiles/2/cli").is_dir());
        assert!(f.root.join("profiles/3/cli").is_dir());
        assert!(!f.root.join("share-journal.json").exists());
    }
}

#[cfg(windows)]
#[test]
fn stage04_delete() {
    let f = Fixture::new();
    request(&f, &["add", "codex-45", "--provider", "codex"], 0);
    let out = request(&f, &["delete", "codex-45", "--yes"], 1);
    assert_eq!(out["error"]["code"], "recovery_required");
    assert!(f.root.join("profiles/1/cli").is_dir());
    assert!(!f.root.join("share-journal.json").exists());
}
