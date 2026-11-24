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
        fs,
        io::{self, Read, Write},
        os::fd::AsRawFd,
        os::unix::{ffi::OsStrExt, fs::symlink},
        process::{Child, Command, Stdio},
    };

    struct ReapChild(Child);
    impl Drop for ReapChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    for case in [
        "canonical",
        "other",
        "private-var",
        "var",
        "symlink",
        "slash",
        "missing",
        "relative",
        "duplicate",
        "other-var",
        "split",
    ] {
        let mut f = Fixture::new();
        // macOS temp paths expose the /var -> /private/var alias, independently of CI's home.
        let aliases = tempfile::tempdir().unwrap();
        let private_parent = aliases.path().canonicalize().unwrap();
        assert!(private_parent.starts_with("/private/var"));
        if matches!(case, "private-var" | "var" | "other-var") {
            f.root = private_parent.join("managed");
        }
        request(&f, &["add", "codex-45", "--provider", "codex"], 0);
        let desktop = f.root.join("profiles/1/desktop");
        let other = private_parent.join("other-desktop");
        create_directory(&other);
        let link = private_parent.join("desktop-link");
        symlink(&desktop, &link).unwrap();
        let dir = match case {
            "var" | "other-var" => std::path::Path::new("/").join(
                if case == "var" { &desktop } else { &other }
                    .strip_prefix("/private")
                    .unwrap(),
            ),
            "symlink" => link,
            "slash" => std::path::PathBuf::from(format!("{}/", desktop.display())),
            "missing" => f.temp.path().join("missing"),
            "relative" => std::path::PathBuf::from("relative"),
            "other" => other,
            _ => desktop.canonicalize().unwrap(),
        };
        let executable = f.temp.path().join("delete-blocker");
        let source = f.temp.path().join("delete-blocker.c");
        create_file(&source, include_bytes!("support/delete_blocker.c"));
        let mut compile = Command::new("/usr/bin/cc");
        compile
            .env_clear()
            .args(["-Wall", "-Wextra", "-Werror"])
            .arg(&source)
            .arg("-o")
            .arg(&executable);
        exec(compile, 0);
        let mut command = Command::new(&executable);
        command.env_clear().current_dir(f.temp.path());
        if case == "split" {
            command.arg("--user-data-dir").arg(&dir);
        } else {
            command.arg(format!("--user-data-dir={}", dir.display()));
        }
        if case == "duplicate" {
            command.arg(format!("--user-data-dir={}", desktop.display()));
        }
        let argv: Vec<_> = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|arg| arg.as_bytes().to_vec())
            .collect();
        let mut child = ReapChild(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        child.0.stdin.as_mut().unwrap().write_all(b"R").unwrap();
        let mut poll = libc::pollfd {
            fd: child.0.stdout.as_ref().unwrap().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll points to one initialized descriptor.
        assert_eq!(unsafe { libc::poll(&mut poll, 1, 5000) }, 1);
        let mut ready = [0u8; 1];
        let readiness = child.0.stdout.as_mut().unwrap().read_exact(&mut ready);
        let status = child.0.try_wait().unwrap();
        assert!(
            readiness.is_ok(),
            "readiness={readiness:?}, status={status:?}"
        );
        assert_eq!(ready, *b"R");
        assert!(status.is_none(), "status={status:?}");
        fs::remove_file(&executable).unwrap();

        let mut pids = [0i32; 4096];
        // SAFETY: the kernel writes at most the size of the initialized PID array.
        let length = unsafe {
            libc::proc_listpids(
                4,
                libc::geteuid(),
                pids.as_mut_ptr().cast(),
                std::mem::size_of_val(&pids) as i32,
            )
        };
        assert!(length > 0 && (length as usize) < std::mem::size_of_val(&pids));
        assert!(pids[..length as usize / size_of::<i32>()].contains(&(child.0.id() as i32)));

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

        let mut args = [0u8; 64 * 1024];
        let mut length = args.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, child.0.id() as i32];
        // SAFETY: query only our synthetic child into bounded initialized storage.
        assert_eq!(
            unsafe {
                libc::sysctl(
                    mib.as_mut_ptr(),
                    3,
                    args.as_mut_ptr().cast(),
                    &mut length,
                    std::ptr::null_mut(),
                    0,
                )
            },
            0
        );
        assert!(length > 4 && length < args.len());
        assert_eq!(
            i32::from_ne_bytes(args[..4].try_into().unwrap()) as usize,
            argv.len()
        );
        let mut fields = args[4..length]
            .split(|&byte| byte == 0)
            .filter(|field| !field.is_empty());
        assert_eq!(fields.next(), Some(executable.as_os_str().as_bytes()));
        for arg in &argv {
            assert_eq!(fields.next(), Some(arg.as_slice()));
        }

        let output =
            capture(f.command(&["cc", "profile", "--json", "delete", "codex-45", "--yes"]))
                .unwrap();
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited during {case}"
        );
        let code = if case.starts_with("other") { 0 } else { 1 };
        assert_eq!(
            output.status.code(),
            Some(code),
            "case={case}: {}",
            stdout(&output)
        );
        let out = envelope(&output);
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
