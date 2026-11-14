#![cfg(all(feature = "synthetic-launch", unix))]
mod common;
use common::*;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
fn setup() -> (Fixture, String) {
    let f = Fixture::new();
    let fake = f.temp.path().join("native-fake");
    fs::copy(env!("CARGO_BIN_EXE_larpcc-fake-provider"), &fake).unwrap();
    private(&fake, false);
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    f.run(
        &["cc", "provider", "bind", "codex", fake.to_str().unwrap()],
        0,
    );
    f.profile(&["add", "work", "--provider", "codex"], 0);
    let digest = Sha256::digest(fs::read(fake).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (f, digest)
}
fn command(f: &Fixture, digest: &str, op: &str) -> Command {
    let mut command = f.command_for(
        Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
        &[
            "stage3",
            op,
            "work",
            "--provider",
            "codex",
            "--sha256",
            digest,
            "--yes",
        ],
    );
    command
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR");
    command
}
#[test]
fn invented_run_enters_fake_through_production_admission() {
    let (f, digest) = setup();
    let record = f.temp.path().join("record");
    let before = fs::read(f.root.join("registry.json")).unwrap();
    let mut cmd = command(&f, &digest, "run");
    cmd.env("LARPCC_FAKE_RECORD", &record);
    let out = capture(cmd).unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(value["args"].as_array().unwrap().len(), 1);
    assert!(stderr(&out).contains("Profiles select native state"));
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}
#[test]
fn invented_row_refuses_wrong_bytes_nonempty_arguments_and_environment() {
    let (f, digest) = setup();
    let out = capture(command(&f, &"00".repeat(32), "run")).unwrap();
    assert!(stderr(&out).contains("qualification is invalid"));
    let mut cmd = command(&f, &digest, "run");
    cmd.args(["--", "prompt"]);
    assert!(stderr(&capture(cmd).unwrap()).contains("unsupported launch argument"));
    let mut cmd = command(&f, &digest, "run");
    cmd.env("CODEX_HOME", "");
    assert!(
        stderr(&capture(cmd).unwrap()).contains("conflicting environment variables: CODEX_HOME")
    );
    assert!(!f.root.join("auth-locks").exists());
}

fn entry(path: &Path) -> serde_json::Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Ok(bytes) = fs::read(path)
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            return value;
        }
        assert!(std::time::Instant::now() < deadline, "fake entry timed out");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn all_operations_enter_fake_and_pass_exit_stdio_without_registry_changes() {
    use std::{io::Write, process::Stdio};
    let (f, digest) = setup();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    for op in ["run", "login", "status", "logout"] {
        let record = f.temp.path().join(format!("entry-{op}"));
        let mut cmd = command(&f, &digest, op);
        cmd.env("LARPCC_FAKE_MODE", "echo-io")
            .env("LARPCC_FAKE_EXIT", "37")
            .env("LARPCC_FAKE_RECORD", &record)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        let wrapper = child.id();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"synthetic\x00\xff\n")
            .unwrap();
        let out = wait_output(child).unwrap();
        assert_eq!(out.status.code(), Some(37), "{}", stderr(&out));
        assert_eq!(out.stdout, b"synthetic\x00\xff\n");
        assert_eq!(
            stderr(&out).matches("Profiles select native state").count(),
            1
        );
        assert_eq!(
            stderr(&out).matches("Quiesce relevant sessions").count(),
            usize::from(op != "run")
        );
        let value = entry(&record);
        let expected = if op == "run" {
            vec![]
        } else {
            vec![
                format!("invented-{op}")
                    .bytes()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            ]
        };
        assert_eq!(
            value["args"]
                .as_array()
                .unwrap()
                .iter()
                .skip(1)
                .cloned()
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|s| serde_json::json!(s))
                .collect::<Vec<_>>()
        );
        if op == "run" {
            assert_eq!(value["pid"], wrapper);
        } else {
            assert_ne!(value["pid"], wrapper);
        }
        assert_eq!(value["unix"]["open_descriptors"], serde_json::json!([]));
    }
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}
#[test]
fn auth_guard_busy_all_pairings_and_stable_after_reap_and_forget() {
    use std::{os::unix::fs::MetadataExt, process::Stdio};
    let (f, digest) = setup();
    let lock = f.root.join("auth-locks/1.lock");
    let mut identity = None;
    f.profile(&["add", "other", "--provider", "codex"], 0);
    for holding in ["login", "status", "logout"] {
        let record = f.temp.path().join(format!("holding-{holding}"));
        let mut cmd = command(&f, &digest, holding);
        cmd.env("LARPCC_FAKE_MODE", "echo-io")
            .env("LARPCC_FAKE_RECORD", &record)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        entry(&record);
        let meta = fs::metadata(&lock).unwrap();
        assert_eq!(meta.len(), 0);
        assert_eq!(meta.nlink(), 1);
        let actual = (meta.dev(), meta.ino());
        if let Some(expected) = identity {
            assert_eq!(actual, expected);
        } else {
            identity = Some(actual);
        }
        for other in ["login", "status", "logout"] {
            let out = capture(command(&f, &digest, other)).unwrap();
            assert_eq!(out.status.code(), Some(1));
            assert!(stderr(&out).contains("profile authentication operation is busy"));
        }
        let mut other = f.command_for(
            Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
            &[
                "stage3",
                "status",
                "other",
                "--provider",
                "codex",
                "--sha256",
                &digest,
            ],
        );
        other
            .env_remove("CODEX_HOME")
            .env_remove("CLAUDE_CONFIG_DIR");
        assert!(capture(other).unwrap().status.success());
        assert!(
            capture(command(&f, &digest, "run"))
                .unwrap()
                .status
                .success()
        );
        // Mutations remain possible while the native child waits: no registry transaction.
        f.profile(&["rename", "work", "renamed"], 0);
        f.profile(&["rename", "renamed", "work"], 0);
        drop(child.stdin.take());
        assert!(wait_output(child).unwrap().status.success());
        assert!(
            capture(command(&f, &digest, "status"))
                .unwrap()
                .status
                .success()
        );
    }
    f.profile(&["forget", "work", "--yes"], 0);
    assert_eq!(fs::metadata(&lock).unwrap().ino(), identity.unwrap().1);
}
#[test]
fn parent_directed_term_hup_reap_and_preserve_signal_status() {
    use std::{os::unix::process::ExitStatusExt, process::Stdio};
    let (f, digest) = setup();
    for signal in [libc::SIGTERM, libc::SIGHUP] {
        let record = f.temp.path().join(format!("signal-{signal}"));
        let mut cmd = command(&f, &digest, "status");
        cmd.env("LARPCC_FAKE_MODE", "sleep")
            .env("LARPCC_FAKE_RECORD", &record)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd.spawn().unwrap();
        let fake = entry(&record)["pid"].as_u64().unwrap() as i32;
        // SAFETY: test owns the wrapper process, obtained directly from spawn.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let out = wait_output(child).unwrap();
        assert_eq!(out.status.signal(), Some(signal));
        // SAFETY: signal 0 observes only the known direct fake after wrapper reap.
        assert_eq!(unsafe { libc::kill(fake, 0) }, -1);
        assert!(
            capture(command(&f, &digest, "status"))
                .unwrap()
                .status
                .success()
        );
    }
}
#[test]
fn auth_uses_real_foreground_pty_job_control_resize_and_interrupt() {
    let (f, digest) = setup();
    for signal in ["INT", "QUIT"] {
        let record = f.temp.path().join(format!("pty-record-{signal}"));
        let mut cmd = command(&f, &digest, "status");
        let args = cmd.get_args().map(|v| v.to_owned()).collect::<Vec<_>>();
        let environment = cmd
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(|v| v.to_owned())))
            .collect::<Vec<_>>();
        cmd = Command::new("python3");
        cmd.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/pty_lifecycle.py"
        ))
        .arg(env!("CARGO_BIN_EXE_larpcc-synthetic"))
        .args(args)
        .env_clear();
        for (key, value) in environment {
            if let Some(value) = value {
                cmd.env(key, value);
            }
        }
        cmd.env("LARPCC_PTY_FINAL_SIGNAL", signal)
            .env("LARPCC_PTY_AUTH", "1")
            .env("LARPCC_FAKE_MODE", "wait-ctrl")
            .env("LARPCC_FAKE_RECORD", record);
        let out = capture(cmd).unwrap();
        assert!(out.status.success(), "{} {}", stdout(&out), stderr(&out));
        assert!(stdout(&out).contains("real PTY"));
    }
}

#[test]
fn auth_confirmation_and_snapshot_races_use_real_terminal() {
    let (f, digest) = setup();
    for op in ["login", "logout"] {
        for answer in ["yes", "crlf", "no", "eof", "overlong", "second", "rename"] {
            let base = command(&f, &digest, op);
            let mut cmd = Command::new("python3");
            cmd.arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/auth_prompt.py"
            ))
            .args([
                answer,
                env!("CARGO_BIN_EXE_larp"),
                env!("CARGO_BIN_EXE_larpcc-synthetic"),
                &digest,
                op,
            ])
            .env_clear();
            for (key, value) in base.get_envs() {
                if let Some(value) = value {
                    cmd.env(key, value);
                }
            }
            let out = capture(cmd).unwrap();
            assert!(out.status.success(), "{} {}", stdout(&out), stderr(&out));
            if answer == "rename" {
                f.profile(&["rename", "renamed", "work"], 0);
            }
            assert!(
                capture(command(&f, &digest, "status"))
                    .unwrap()
                    .status
                    .success()
            );
        }
    }
}
#[test]
fn auth_without_terminal_requires_confirmation_before_fake() {
    let (f, digest) = setup();
    for op in ["login", "logout"] {
        let base = command(&f, &digest, op);
        let mut cmd = Command::new(base.get_program());
        cmd.args(base.get_args().filter(|v| *v != "--yes"))
            .env_clear();
        for (key, value) in base.get_envs() {
            if let Some(value) = value {
                cmd.env(key, value);
            }
        }
        let record = f.temp.path().join(format!("no-tty-{op}"));
        cmd.env("LARPCC_FAKE_RECORD", &record);
        let out = capture(cmd).unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(!record.exists());
        assert_eq!(
            stderr(&out),
            "error: authentication operation requires confirmation\nhint: use a terminal or explicitly pass --yes\n"
        );
    }
}

#[test]
fn auth_lock_directory_is_directory_and_existing_files_are_private_single_link() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    let (f, digest) = setup();
    assert!(
        capture(command(&f, &digest, "status"))
            .unwrap()
            .status
            .success()
    );
    let dir = f.root.join("auth-locks");
    let lock = dir.join("1.lock");
    assert!(fs::metadata(&dir).unwrap().nlink() >= 2);
    let alias = f.temp.path().join("lock-alias");
    fs::hard_link(&lock, &alias).unwrap();
    let out = capture(command(&f, &digest, "status")).unwrap();
    assert!(stderr(&out).contains("namespace cannot be qualified"));
    fs::remove_file(alias).unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(
        !capture(command(&f, &digest, "status"))
            .unwrap()
            .status
            .success()
    );
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(&lock, dir.join("saved")).unwrap();
    symlink(dir.join("saved"), &lock).unwrap();
    assert!(
        !capture(command(&f, &digest, "status"))
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::metadata(dir.join("saved")).unwrap().len(), 0);
}

#[test]
fn auth_setup_wait_and_cancellation_failures_release_only_after_cleanup() {
    use std::os::unix::process::ExitStatusExt;
    let (f, digest) = setup();
    for failure in ["setup", "wait", "cancel"] {
        let mut cmd = command(&f, &digest, "status");
        let record = f.temp.path().join(format!("failure-{failure}"));
        let state = record.with_extension("state");
        cmd.args([
            "--failure",
            failure,
            "--observation",
            state.to_str().unwrap(),
        ])
        .env("LARPCC_FAKE_MODE", "sleep")
        .env("LARPCC_FAKE_RECORD", &record);
        let out = capture(cmd).unwrap();
        if failure == "cancel" {
            assert_eq!(out.status.signal(), Some(libc::SIGTERM));
            assert!(!record.exists());
        } else {
            assert_eq!(out.status.code(), Some(1));
            let observed: serde_json::Value =
                serde_json::from_slice(&fs::read(state).unwrap()).unwrap();
            assert_eq!(
                observed,
                serde_json::json!({"signals_unchanged":true,"env_unchanged":true})
            );
            if let Ok(bytes) = fs::read(&record) {
                let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let pid = value["pid"].as_u64().unwrap() as i32;
                // SAFETY: signal 0 only observes our known synthetic direct child.
                assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            }
        }
        assert!(
            capture(command(&f, &digest, "status"))
                .unwrap()
                .status
                .success()
        );
    }
}
#[test]
fn invalid_native_fixture_spawn_failure_restores_state_and_guard() {
    let (f, _) = setup();
    let fake = f.temp.path().join("native-fake");
    #[cfg(target_os = "linux")]
    let magic = b"\x7fELF";
    #[cfg(target_os = "macos")]
    let magic = b"\xcf\xfa\xed\xfe";
    fs::write(&fake, magic).unwrap();
    let digest = Sha256::digest(magic)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let state = f.temp.path().join("spawn-state");
    let mut cmd = command(&f, &digest, "status");
    cmd.args(["--observation", state.to_str().unwrap()]);
    let out = capture(cmd).unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{} {}",
        stdout(&out),
        stderr(&out)
    );
    assert!(stderr(&out).contains("cannot launch /"));
    let observed: serde_json::Value = serde_json::from_slice(&fs::read(state).unwrap()).unwrap();
    assert_eq!(
        observed,
        serde_json::json!({"signals_unchanged":true,"env_unchanged":true})
    );
    // A second invocation reaches spawn again instead of finding a busy guard.
    assert!(stderr(&capture(command(&f, &digest, "status")).unwrap()).contains("cannot launch /"));
}

#[test]
fn every_operation_passes_binary_stdout_and_stderr_beyond_pipe_capacity() {
    let (f, digest) = setup();
    let bytes = [0, 255, 128, 27].repeat(32768);
    let notice = "Profiles select native state, not a verified account, model or endpoint. Native project, configuration and administrator policy still apply. Provider restrictions remain possible.\n";
    for op in ["run", "login", "status", "logout"] {
        let mut cmd = command(&f, &digest, op);
        cmd.env("LARPCC_FAKE_MODE", "binary-output");
        let out = capture(cmd).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, bytes);
        let mut expected = notice.as_bytes().to_vec();
        if op != "run" {
            expected.extend_from_slice(b"Quiesce relevant sessions before authentication changes. Refresh and logout may affect grants and integrations.\n");
        }
        expected.extend_from_slice(&bytes);
        assert_eq!(out.stderr, expected);
    }
}

#[test]
fn auth_preserves_inherited_ignored_dispositions_and_signal_mask() {
    let (f, digest) = setup();
    for op in ["run", "login", "status", "logout"] {
        let base = command(&f, &digest, op);
        let mut cmd = Command::new("python3");
        cmd.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/signal_baseline.py"
        ))
        .arg(env!("CARGO_BIN_EXE_larpcc-synthetic"))
        .arg(f.temp.path().join("native-fake"))
        .arg(f.temp.path().join(format!("inherited-{op}")))
        .args([&digest, op])
        .env_clear();
        for (key, value) in base.get_envs() {
            if let Some(value) = value {
                cmd.env(key, value);
            }
        }
        let out = capture(cmd).unwrap();
        assert!(out.status.success(), "{} {}", stdout(&out), stderr(&out));
    }
}

#[test]
fn actual_row_argument_and_raw_environment_policy_preserves_unrelated_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let (f, digest) = setup();
    for args in [
        vec![""],
        vec!["--"],
        vec!["--model", "invented"],
        vec!["prompt"],
        vec!["login"],
        vec!["--config=invented-secret"],
    ] {
        let mut cmd = command(&f, &digest, "run");
        cmd.arg("--").args(args);
        let out = capture(cmd).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "error: unsupported launch argument: argument syntax\nhint: the argument form has not been admitted by the launch policy\n"
        );
    }
    let mut cmd = command(&f, &digest, "run");
    cmd.env(
        std::ffi::OsString::from_vec(b"LARPCC_SYNTHETIC_DENY_\xff".to_vec()),
        "invented-secret",
    );
    let out = capture(cmd).unwrap();
    assert!(stderr(&out).contains("conflicting environment variables: LARPCC_SYNTHETIC_DENY_"));
    assert!(!stderr(&out).contains("invented-secret"));
    let record = f.temp.path().join("raw-env");
    let mut cmd = command(&f, &digest, "status");
    cmd.env("codex_home", "unrelated-on-Unix")
        .env(
            "LARPCC_TEST_VALUE",
            std::ffi::OsString::from_vec(vec![255, 254]),
        )
        .env("LARPCC_FAKE_RECORD", &record)
        .current_dir(f.temp.path());
    assert!(capture(cmd).unwrap().status.success());
    assert_eq!(entry(&record)["LARPCC_TEST_VALUE"], "fffe");
}
