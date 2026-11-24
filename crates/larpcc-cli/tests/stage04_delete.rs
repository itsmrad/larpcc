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
fn stage04_delete_checks_live_processes_with_unlinked_executables() {
    use std::{
        fs, io,
        process::{Child, Command, Stdio},
    };

    struct ReapChild(Child);
    impl Drop for ReapChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let f = Fixture::new();
    request(&f, &["add", "codex-45", "--provider", "codex"], 0);
    let desktop = f.root.join("profiles/1/desktop");
    let other = f.temp.path().join("other-desktop");
    let executable = f.temp.path().join("delete-test-cat");
    for (dir, code) in [(&desktop, 1), (&other, 0)] {
        copy_executable(std::path::Path::new("/bin/cat"), &executable);
        // Cat blocks on its stdin pipe before it can open the second operand.
        let mut child = ReapChild(
            Command::new(&executable)
                .env_clear()
                .current_dir(f.temp.path())
                .args(["-n", "--", "-"])
                .arg(format!("--user-data-dir={}", dir.display()))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        fs::remove_file(&executable).unwrap();

        let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: proc_pidpath can write at most path.len() bytes into path.
        let len = unsafe {
            libc::proc_pidpath(
                child.0.id() as i32,
                path.as_mut_ptr().cast(),
                path.len() as u32,
            )
        };
        let errno = io::Error::last_os_error().raw_os_error();
        assert!(len <= 0, "proc_pidpath returned a path after unlink");
        assert_eq!(errno, Some(libc::ENOENT), "expected ENOENT after unlink");
        assert!(child.0.try_wait().unwrap().is_none());

        let out = request(&f, &["delete", "codex-45", "--yes"], code);
        assert!(child.0.try_wait().unwrap().is_none());
        if code == 1 {
            assert_eq!(out["error"]["code"], "busy");
            assert!(desktop.is_dir());
        } else {
            assert_eq!(out["ok"], true);
            assert!(!desktop.exists());
        }
    }
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

#[test]
fn stage04_delete_stale_revision_does_not_create_missing_root() {
    let f = Fixture::new();
    for args in [
        &["delete", "codex-45", "--yes", "--expected-revision", "1"][..],
        &["recover", "codex-45", "--yes", "--expected-revision", "1"][..],
        &["migrate", "--expected-revision", "1"][..],
    ] {
        assert_eq!(request(&f, args, 1)["error"]["code"], "conflict");
        assert!(!f.root.exists());
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
