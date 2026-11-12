#![cfg(all(feature = "synthetic-launch", unix))]
mod common;
use common::*;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn driver(f: &Fixture, name: &str, args: &[&str]) -> Command {
    let mut command = f.command_for(
        Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
        &[&["cc", "run", name, "--"][..], args].concat(),
    );
    command
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .current_dir(f.temp.path())
        .env("PWD", f.temp.path());
    command
}
fn setup() -> Fixture {
    let f = Fixture::new();
    let fake = f.temp.path().join("native-fake");
    fs::copy(env!("CARGO_BIN_EXE_larpcc-fake-provider"), &fake).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    for provider in ["claude", "codex"] {
        f.run(
            &["cc", "provider", "bind", provider, fake.to_str().unwrap()],
            0,
        );
        for suffix in ["a", "b"] {
            f.profile(
                &[
                    "add",
                    &format!("{provider}{suffix}"),
                    "--provider",
                    provider,
                ],
                0,
            );
        }
    }
    f
}
fn hex(s: &std::ffi::OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
#[test]
fn four_homes_exact_argv_cwd_pid_and_unchanged_metadata() {
    let f = setup();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    let mut homes = std::collections::HashSet::new();
    for name in ["claudea", "claudeb", "codexa", "codexb"] {
        let record = f.temp.path().join(format!("record-{name}"));
        let args = [
            "--label",
            "login",
            "prompt",
            "",
            "spaces and quotes\"",
            "\\trailing\\",
            ";$(x)",
            "é",
            "--",
            "--config",
        ];
        let mut command = driver(&f, name, &args);
        command
            .env("LARPCC_FAKE_RECORD", &record)
            .env("LARPCC_TEST_VALUE", "invented-secret")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().unwrap();
        let pid = child.id();
        let out = wait_output(child).unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
        assert_eq!(value["pid"], pid);
        assert_eq!(value["unix"]["open_descriptors"], serde_json::json!([]));
        assert_eq!(value["cwd"], hex(f.temp.path().as_os_str()));
        assert_eq!(value["PWD"], hex(f.temp.path().as_os_str()));
        let actual = value["args"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .map(|v| v.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            args.iter()
                .map(|a| hex(std::ffi::OsStr::new(a)))
                .collect::<Vec<_>>()
        );
        let key = if name.starts_with("claude") {
            "CLAUDE_CONFIG_DIR"
        } else {
            "CODEX_HOME"
        };
        homes.insert(value[key].as_str().unwrap().to_owned());
    }
    assert_eq!(homes.len(), 4);
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}
#[test]
fn exit_status_output_and_policy_refusal() {
    let f = setup();
    for code in [0, 1, 2, 42, 255] {
        let mut command = driver(&f, "claudea", &["prompt"]);
        command.env("LARPCC_FAKE_EXIT", code.to_string());
        let output = capture(command).unwrap();
        assert_eq!(output.status.code(), Some(code));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    let mut output = driver(&f, "claudea", &["prompt"]);
    output.env("LARPCC_FAKE_MODE", "output");
    assert_eq!(capture(output).unwrap().stdout, vec![b'x'; 8 * 1024 * 1024]);
    for args in [vec!["login"], vec!["--config=secret", "prompt"]] {
        let record = f.temp.path().join("must-not-exist");
        let mut command = driver(&f, "claudea", &args);
        command.env("LARPCC_FAKE_RECORD", &record);
        let out = capture(command).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(!record.exists());
        assert!(!stderr(&out).contains("secret"));
    }
    let record = f.temp.path().join("env-must-not-exist");
    let mut command = driver(&f, "codexa", &["prompt"]);
    command
        .env("CODEX_HOME", "invented-secret")
        .env("LARPCC_FAKE_RECORD", &record);
    let out = capture(command).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!record.exists());
    assert!(!stderr(&out).contains("invented-secret"));
}
#[test]
fn unsafe_other_home_blocks_launch_but_diagnostics_and_forget_work() {
    use std::os::unix::fs::PermissionsExt;
    let f = setup();
    let unsafe_home = f.root.join("profiles/2");
    fs::set_permissions(&unsafe_home, fs::Permissions::from_mode(0o755)).unwrap();
    let out = capture(driver(&f, "codexa", &["prompt"])).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("unsafe data path"));
    f.profile(&["show", "claudeb"], 0);
    f.profile(&["rename", "claudeb", "renamed"], 0);
    f.profile(&["forget", "renamed", "--yes"], 0);
    assert!(
        capture(driver(&f, "codexa", &["prompt"]))
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn native_signals_and_direct_launch_signal_baseline() {
    use std::os::unix::process::ExitStatusExt;
    let f = setup();
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
        let record = f.temp.path().join(format!("signal-{signal}"));
        let mut command = driver(&f, "claudea", &["prompt"]);
        command
            .env("LARPCC_FAKE_RECORD", &record)
            .env("LARPCC_FAKE_MODE", "sleep")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().unwrap();
        let pid = child.id();
        let end = Instant::now() + Duration::from_secs(2);
        while !record.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(record.exists());
        // SAFETY: pid belongs to the known live direct child; signal is a fixed native signal.
        assert_eq!(unsafe { libc::kill(pid as i32, signal) }, 0);
        assert_eq!(wait_output(child).unwrap().status.signal(), Some(signal));
    }
    let direct_record = f.temp.path().join("direct");
    let exec_record = f.temp.path().join("exec");
    let mut direct = f.command_for(&f.temp.path().join("native-fake"), &["prompt"]);
    direct.env("LARPCC_FAKE_RECORD", &direct_record);
    assert!(capture(direct).unwrap().status.success());
    let mut launched = driver(&f, "claudea", &["prompt"]);
    launched.env("LARPCC_FAKE_RECORD", &exec_record);
    assert!(capture(launched).unwrap().status.success());
    let read = |p| serde_json::from_slice::<serde_json::Value>(&fs::read(p).unwrap()).unwrap();
    assert_eq!(
        read(direct_record)["unix"]["signals"],
        read(exec_record)["unix"]["signals"]
    );
}

#[test]
fn real_pty_foreground_interrupt_suspend_resume_and_resize() {
    let f = setup();
    let record = f.temp.path().join("pty-record");
    let mut command = f.command_for(Path::new("/usr/bin/python3"), &[]);
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/pty_lifecycle.py"))
        .args([
            env!("CARGO_BIN_EXE_larpcc-synthetic"),
            "cc",
            "run",
            "claudea",
            "--",
            "prompt",
        ])
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env("LARPCC_FAKE_MODE", "wait-ctrl")
        .env("LARPCC_FAKE_RECORD", record);
    let output = capture(command).unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("real PTY"));
}

#[test]
fn inherited_mask_and_ignored_dispositions_match_direct_native_baseline() {
    let f = setup();
    let mut command = f.command_for(Path::new("/usr/bin/python3"), &[]);
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/signal_baseline.py"))
        .arg(env!("CARGO_BIN_EXE_larpcc-synthetic"))
        .arg(f.temp.path().join("native-fake"))
        .arg(f.temp.path().join("signal-baseline"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME");
    let output = capture(command).unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("match direct baseline"));
}

#[test]
fn piped_input_and_output_are_exact_without_supervisor() {
    use std::io::Write;
    let f = setup();
    let bytes = vec![b'q'; 8 * 1024 * 1024];
    let mut command = driver(&f, "claudea", &["prompt"]);
    command
        .env("LARPCC_FAKE_MODE", "echo-io")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let payload = bytes.clone();
    let writer = std::thread::spawn(move || input.write_all(&payload));
    let out = wait_output(child).unwrap();
    writer.join().unwrap().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, bytes);
    assert!(out.stderr.is_empty());
}
#[test]
fn bounded_grandchild_inherits_exact_selected_home() {
    let f = setup();
    let record = f.temp.path().join("descendant");
    let mut command = driver(&f, "claudea", &["prompt"]);
    command
        .env("LARPCC_FAKE_MODE", "grandchild")
        .env("LARPCC_FAKE_RECORD", &record);
    let started = Instant::now();
    let out = capture(command).unwrap();
    assert!(out.status.success());
    assert!(started.elapsed() < Duration::from_secs(2));
    let child_record = record.with_extension("grandchild");
    let done = f.temp.path().join("descendant.grandchild.done");
    let end = Instant::now() + Duration::from_secs(5);
    while !done.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(done.exists(), "bounded grandchild did not complete");
    let read =
        |path| serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        read(record)["CLAUDE_CONFIG_DIR"],
        read(child_record)["CLAUDE_CONFIG_DIR"]
    );
}

#[test]
fn malformed_native_refuses_without_shell_and_restores_failed_exec_state() {
    let f = setup();
    let executable = f.temp.path().join("native-fake");
    #[cfg(target_os = "macos")]
    let header = [0xcf, 0xfa, 0xed, 0xfe];
    #[cfg(not(target_os = "macos"))]
    let header = *b"\x7fELF";
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(b"\nprintf synthetic > \"$LARPCC_FAKE_RECORD\"\n");
    fs::write(executable, bytes).unwrap();
    let marker = f.temp.path().join("shell-must-not-create");
    let mut command = driver(&f, "claudea", &["prompt"]);
    command
        .env("LARPCC_FAKE_RECORD", &marker)
        .env("LARPCC_SYNTHETIC_OBSERVE_FAILURE", "1");
    let out = capture(command).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("cannot launch"));
    assert!(!marker.exists());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report,
        serde_json::json!({"env_unchanged":true,"signals_unchanged":true})
    );
}

#[test]
fn unrelated_provider_home_and_parent_values_remain_unchanged() {
    let f = setup();
    let record = f.temp.path().join("other-home");
    let other = f.temp.path().join("codex");
    let mut command = driver(&f, "claudea", &["prompt"]);
    command
        .env("CODEX_HOME", &other)
        .env("LARPCC_FAKE_RECORD", &record);
    assert!(capture(command).unwrap().status.success());
    let observed: serde_json::Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(observed["CODEX_HOME"], hex(other.as_os_str()));
    assert_eq!(
        observed["HOME"],
        hex(f.temp.path().join("home").as_os_str())
    );
    assert_eq!(
        observed["USERPROFILE"],
        hex(f.temp.path().join("home").as_os_str())
    );
}

#[cfg(target_os = "linux")]
#[test]
fn native_invalid_utf8_argument_is_preserved() {
    use std::os::unix::ffi::OsStringExt;
    let f = setup();
    let record = f.temp.path().join("raw-argv");
    let raw = std::ffi::OsString::from_vec(vec![0xff, 0xfe, b' ', b'\\']);
    let mut command = driver(&f, "claudea", &["prompt"]);
    command.arg(raw).env("LARPCC_FAKE_RECORD", &record);
    assert!(capture(command).unwrap().status.success());
    let value: serde_json::Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(
        value["args"].as_array().unwrap().last().unwrap(),
        "fffe205c"
    );
}
