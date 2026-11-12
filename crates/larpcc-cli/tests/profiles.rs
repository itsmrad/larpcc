#![cfg(not(windows))] // Restored with secure Windows metadata implementation.
mod common;
use common::*;
use std::{
    fs,
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

fn lifecycle(f: &Fixture) {
    let dir = |id| f.root.join("profiles").join(format!("{id}"));
    assert_eq!(
        stdout(&f.profile(&["add", "work", "--provider", "claude"], 0)),
        format!(
            "Added profile work (claude, id 1).\nState directory: {}\n",
            dir(1).display()
        )
    );
    assert_eq!(
        stdout(&f.profile(&["add", "personal", "--provider", "codex"], 0)),
        format!(
            "Added profile personal (codex, id 2).\nState directory: {}\n",
            dir(2).display()
        )
    );
    assert_eq!(
        stdout(&f.profile(&["list"], 0)),
        "NAME      PROVIDER  ID  STATE\npersonal  codex     2   ok\nwork      claude    1   ok\n"
    );
    assert_eq!(
        stdout(&f.profile(&["show", "work"], 0)),
        format!(
            "Name:      work\nProvider:  claude\nID:        1\nState dir: {}\nState:     ok\n",
            dir(1).display()
        )
    );
    let bytes = fs::read(f.root.join("registry.json")).unwrap();
    assert_eq!(
        stderr(&f.profile(&["forget", "work"], 2)),
        format!(
            "error: refusing to forget profile work without --yes\nhint: the registry entry would be removed; the state directory {} is kept and this version cannot attach it to a new profile\n",
            dir(1).display()
        )
    );
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), bytes);
    assert_eq!(
        stdout(&f.profile(&["rename", "work", "job"], 0)),
        format!(
            "Renamed profile work to job (id 1).\nState directory unchanged: {}\n",
            dir(1).display()
        )
    );
    assert_eq!(
        stdout(&f.profile(&["forget", "personal", "--yes"], 0)),
        format!(
            "Forgot profile personal (id 2).\nState directory kept: {}\n",
            dir(2).display()
        )
    );
    assert_eq!(
        stdout(&f.profile(&["list"], 0)),
        "NAME  PROVIDER  ID  STATE\njob   claude    1   ok\n"
    );
    assert_eq!(
        stdout(&f.profile(&["add", "personal", "--provider", "codex"], 0)),
        format!(
            "Added profile personal (codex, id 3).\nState directory: {}\n",
            dir(3).display()
        )
    );
    for id in 1..=3 {
        assert!(dir(id).is_dir());
    }
}

#[test]
fn lifecycle_and_outside_root_nonmutation() {
    let f = Fixture::new();
    let before = f.outside_snapshot();
    lifecycle(&f);
    assert_eq!(before, f.outside_snapshot());
    let mut entries: Vec<_> = fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, ["profiles", "registry.json", "registry.lock"]);
}
#[test]
fn operational_errors_and_read_only_missing_root() {
    let f = Fixture::new();
    assert_eq!(stdout(&f.profile(&["list"], 0)), "No profiles.\n");
    let missing = "error: no profile named absent\nhint: run 'larp cc profile list'\n";
    for args in [vec!["show", "absent"], vec!["forget", "absent"]] {
        assert_eq!(stderr(&f.profile(&args, 1)), missing);
        assert!(!f.root.exists());
    }
    // Confirmed forget is a write transaction: first-write scaffolding is allowed (§5.2).
    let outside = f.outside_snapshot();
    assert_eq!(
        stderr(&f.profile(&["forget", "absent", "--yes"], 1)),
        missing
    );
    assert!(!f.root.join("registry.json").exists());
    assert!(!f.root.join("profiles").exists());
    assert_eq!(outside, f.outside_snapshot());
    f.profile(&["add", "a", "--provider", "claude"], 0);
    f.profile(&["add", "b", "--provider", "codex"], 0);
    let existing = fs::read(f.root.join("registry.json")).unwrap();
    let outside = f.outside_snapshot();
    assert_eq!(
        stderr(&f.profile(&["forget", "absent", "--yes"], 1)),
        missing
    );
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), existing);
    assert_eq!(fs::read_dir(f.root.join("profiles")).unwrap().count(), 2);
    assert_eq!(outside, f.outside_snapshot());

    for args in [
        vec!["add", "a", "--provider", "codex"],
        vec!["rename", "b", "a"],
        vec!["rename", "a", "a"],
    ] {
        assert_eq!(
            stderr(&f.profile(&args, 1)),
            "error: a profile named a already exists\n"
        );
    }
    assert_eq!(stderr(&f.profile(&["rename", "absent", "c"], 1)), missing);
}
#[test]
fn registry_fixtures_preserved_on_failure() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/registry");
    for kind in ["valid", "invalid", "newer"] {
        for file in fs::read_dir(base.join(kind)).unwrap() {
            let f = Fixture::new();
            let bytes = fs::read(file.unwrap().path()).unwrap();
            f.install_registry(&bytes);
            if kind == "valid" {
                assert_eq!(
                    stdout(&f.profile(&["list"], 0)),
                    "NAME  PROVIDER  ID  STATE\nwork  claude    1   missing\n"
                );
            } else {
                for args in [vec!["list"], vec!["add", "x", "--provider", "codex"]] {
                    let err = stderr(&f.profile(&args, 1));
                    assert!(err.starts_with("error: registry "), "{err}");
                    if kind == "newer" {
                        assert_eq!(
                            err,
                            format!(
                                "error: registry {} uses schema version 2; this Larp CC supports version 1\nhint: upgrade Larp CC; the file was not modified\n",
                                f.root.join("registry.json").display()
                            )
                        );
                    }
                }
            }
            assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), bytes);
        }
    }
}
#[test]
fn concurrent_mutations_and_readers() {
    for same in [false, true] {
        let f = Fixture::new();
        let count = if same { 8 } else { 16 };
        let children: Vec<_> = (0..count)
            .map(|i| {
                let name = if same { "same".into() } else { format!("p{i}") };
                f.command(&["cc", "profile", "add", &name, "--provider", "codex"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let mut successes = 0;
        for child in children {
            let result = child.wait_with_output().unwrap();
            match result.status.code() {
                Some(0) => {
                    successes += 1;
                    assert!(stderr(&result).is_empty());
                    assert!(stdout(&result).starts_with("Added profile "));
                }
                Some(1) if same => {
                    assert!(result.stdout.is_empty());
                    assert_eq!(
                        stderr(&result),
                        "error: a profile named same already exists\n"
                    );
                }
                _ => panic!("{}", stderr(&result)),
            }
        }
        assert_eq!(successes, if same { 1 } else { count });
        let registry: serde_json::Value =
            serde_json::from_slice(&fs::read(f.root.join("registry.json")).unwrap()).unwrap();
        assert_eq!(registry["revision"], successes);
        assert_eq!(registry["profiles"].as_array().unwrap().len(), successes);
        assert_eq!(
            fs::read_dir(f.root.join("profiles")).unwrap().count(),
            successes
        );
        assert_eq!(
            stdout(&f.profile(&["list"], 0)).lines().count(),
            successes + 1
        );
    }
    let f = Fixture::new();
    let children: Vec<_> = (0..8)
        .map(|i| {
            f.command(&[
                "cc",
                "profile",
                "add",
                &format!("p{i}"),
                "--provider",
                "claude",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
        })
        .collect();
    for _ in 0..16 {
        f.profile(&["list"], 0);
    }
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
    }
}
#[test]
fn waits_for_writer_lock() {
    let f = Fixture::new();
    f.profile(&["add", "a", "--provider", "codex"], 0);
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.root.join("registry.lock"))
        .unwrap();
    file.lock().unwrap();
    let start = Instant::now();
    let mut child = f
        .command(&["cc", "profile", "add", "b", "--provider", "codex"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(500));
    assert!(child.try_wait().unwrap().is_none());
    file.unlock().unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(start.elapsed() >= Duration::from_millis(500));
    assert!(stdout(&out).starts_with("Added profile b (codex, id 2)."));
}
#[test]
fn killed_writes_recover() {
    let f = Fixture::new();
    let mut seed = 17u64;
    for i in 0..30 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut child = f
            .command(&[
                "cc",
                "profile",
                "add",
                &format!("k{i}"),
                "--provider",
                "claude",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        thread::sleep(Duration::from_millis(seed % 21));
        // It may have finished before the fixed-seed delay expires.
        let _ = child.kill();
        child.wait().unwrap();
        f.profile(&["list"], 0);
        f.profile(&["add", &format!("after{i}"), "--provider", "codex"], 0);
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(f.root.join("registry.json")).unwrap()).unwrap();
        for profile in value["profiles"].as_array().unwrap() {
            assert!(
                f.root
                    .join("profiles")
                    .join(profile["id"].as_u64().unwrap().to_string())
                    .is_dir()
            );
        }
    }
}
#[test]
fn unicode_root_and_invalid_roots() {
    let mut f = Fixture::new();
    let parent = f.temp.path().join("dir with space");
    fs::create_dir(&parent).unwrap();
    f.root = parent.join("ünï cödé");
    lifecycle(&f);
    for path in ["relative".into(), f.temp.path().join("..\u{2f}bad")] {
        let out = f
            .command(&["cc", "profile", "list"])
            .env("LARPCC_HOME", &path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        let reason = if path.is_absolute() {
            "must not contain '..'"
        } else {
            "must be an absolute path"
        };
        assert_eq!(
            stderr(&out),
            format!(
                "error: invalid data directory {}: {reason}\nhint: set LARPCC_HOME to an absolute path\n",
                path.display()
            )
        );
    }
}
#[test]
fn default_root_resolution() {
    let f = Fixture::new();
    #[cfg(target_os = "linux")]
    let root = f.temp.path().join("xdg/larpcc");
    #[cfg(target_os = "macos")]
    let root = f
        .temp
        .path()
        .join("home/Library/Application Support/larpcc");
    #[cfg(windows)]
    let root = f.temp.path().join("local").join("larpcc");
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    let out = f
        .command(&["cc", "profile", "add", "a", "--provider", "codex"])
        .env_remove("LARPCC_HOME")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let out = f
        .command(&["cc", "profile", "show", "a"])
        .env_remove("LARPCC_HOME")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let state_dir = root.join("profiles").join("1");
    assert_eq!(
        stdout(&out),
        format!(
            "Name:      a\nProvider:  codex\nID:        1\nState dir: {}\nState:     ok\n",
            state_dir.display()
        )
    );
    assert!(state_dir.is_dir());
    assert!(!f.root.exists());
}
#[cfg(unix)]
#[test]
fn provider_traps_not_executed() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let bin = f.temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    for name in ["claude", "codex"] {
        let path = bin.join(name);
        fs::write(&path, b"#!/bin/sh\n: > \"$HOME/provider-executed\"\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    // Each lifecycle command receives only the trap directory in PATH.
    for args in [
        vec!["add", "a", "--provider", "claude"],
        vec!["list"],
        vec!["show", "a"],
        vec!["rename", "a", "b"],
        vec!["forget", "b", "--yes"],
    ] {
        let out = f
            .command(&[&["cc", "profile"][..], &args].concat())
            .env("PATH", &bin)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
    }
    assert!(!f.temp.path().join("home/provider-executed").exists());
}
#[cfg(unix)]
#[test]
fn unsafe_and_read_only_roots() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let f = Fixture::new();
    // This fixture owns the directory; its uid identifies root without a second unsafe call.
    if fs::metadata(f.temp.path()).unwrap().uid() == 0 {
        return;
    }
    f.profile(&["add", "a", "--provider", "claude"], 0);
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o755)).unwrap();
    let err = stderr(&f.profile(&["list"], 1));
    assert_eq!(
        err,
        format!(
            "error: unsafe data path {}: has mode 0755; group and other access must be removed\nhint: run: chmod 700 {}\n",
            f.root.display(),
            f.root.display()
        )
    );
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o500)).unwrap();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    f.profile(&["add", "b", "--provider", "codex"], 1);
    assert_eq!(before, fs::read(f.root.join("registry.json")).unwrap());
    assert!(!f.root.join("profiles/2").exists());
    private(&f.root, true);
}
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_root_and_stdout_full() {
    use std::os::unix::ffi::OsStringExt;
    let mut f = Fixture::new();
    f.root = f
        .temp
        .path()
        .join(std::ffi::OsString::from_vec(b"root-\xff".to_vec()));
    lifecycle(&f);
    let out = f
        .command(&["cc", "profile", "list"])
        .stdout(
            fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        )
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).starts_with("error: cannot write output:"));
    assert!(!stderr(&out).contains("panicked"));
}
#[cfg(windows)]
#[test]
fn junction_root_rejected() {
    let f = Fixture::new();
    let target = f.temp.path().join("junction-target");
    fs::create_dir(&target).unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&f.root)
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());
    let err = stderr(&f.profile(&["list"], 1));
    assert!(err.contains("is a symbolic link or reparse point"));
    assert!(fs::read_dir(&target).unwrap().next().is_none());
}

#[test]
fn missing_and_unsafe_state_are_informational() {
    let f = Fixture::new();
    f.profile(&["add", "work", "--provider", "claude"], 0);
    let state = f.root.join("profiles/1");
    fs::remove_dir(&state).unwrap();
    assert!(stdout(&f.profile(&["show", "work"], 0)).ends_with("State:     missing\n"));
    assert_eq!(
        stdout(&f.profile(&["list"], 0)),
        "NAME  PROVIDER  ID  STATE\nwork  claude    1   missing\n"
    );
    fs::write(&state, b"synthetic file in place of a directory").unwrap();
    assert!(
        stdout(&f.profile(&["show", "work"], 0))
            .ends_with("State:     unsafe: is not a directory\n")
    );
    assert_eq!(
        stdout(&f.profile(&["list"], 0)),
        "NAME  PROVIDER  ID  STATE\nwork  claude    1   unsafe\n"
    );
    f.profile(&["rename", "work", "job"], 0);
    f.profile(&["forget", "job", "--yes"], 0);
    assert_eq!(
        fs::read(&state).unwrap(),
        b"synthetic file in place of a directory"
    );
}
