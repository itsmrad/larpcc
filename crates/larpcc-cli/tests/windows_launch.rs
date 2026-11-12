#![cfg(all(windows, feature = "synthetic-launch"))]
mod common;
use common::*;
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        process::CommandExt,
    },
    path::Path,
    process::{Command, Stdio},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
fn hex(value: &OsStr) -> String {
    value.encode_wide().map(|u| format!("{u:04x}")).collect()
}
fn setup() -> Fixture {
    let f = Fixture::new();
    let fake = f.temp.path().join("synthetic-native.exe");
    fs::copy(env!("CARGO_BIN_EXE_larpcc-fake-provider"), &fake).unwrap();
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
fn driver(f: &Fixture, name: &str) -> Command {
    let mut command = f.command_for(
        Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
        &["cc", "run", name, "--"],
    );
    command
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .current_dir(f.temp.path())
        .env("PWD", f.temp.path())
        .creation_flags(CREATE_NO_WINDOW);
    command
}
#[test]
fn p01_l01_four_homes_exact_wtf16_argv_cwd_and_unchanged_registry() {
    let f = setup();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    let args: Vec<OsString> = [
        "--label",
        "login",
        "prompt",
        "",
        "a b",
        "a\"b",
        r"trailing\",
        ";$(x)&|<>^%",
        "é",
        "--",
        "--config",
    ]
    .map(OsString::from)
    .into_iter()
    .chain([OsString::from_wide(&[0xd800, 0x61, 0xdcff])])
    .collect();
    let mut homes = std::collections::HashSet::new();
    for (index, name) in ["claudea", "claudeb", "codexa", "codexb"]
        .iter()
        .enumerate()
    {
        let record = f.temp.path().join(format!("record-{name}.json"));
        let mut command = driver(&f, name);
        command
            .args(&args)
            .env("LARPCC_FAKE_RECORD", &record)
            .env("LARPCC_TEST_VALUE", "invented-sentinel");
        let output = capture(command).unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
        let actual: Vec<_> = value["args"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(actual, args.iter().map(|a| hex(a)).collect::<Vec<_>>());
        assert_eq!(value["cwd"], hex(f.temp.path().as_os_str()));
        assert_eq!(value["PWD"], hex(f.temp.path().as_os_str()));
        let selected = if name.starts_with("claude") {
            "CLAUDE_CONFIG_DIR"
        } else {
            "CODEX_HOME"
        };
        assert_eq!(
            value[selected],
            hex(f
                .root
                .join("profiles")
                .join((index + 1).to_string())
                .as_os_str())
        );
        homes.insert(value[selected].as_str().unwrap().to_owned());
        assert_eq!(value["stdin_terminal"], false);
        assert_eq!(value["stdout_terminal"], false);
    }
    assert_eq!(homes.len(), 4);
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}
#[test]
fn l02_w05_direct_child_full_u32_status_and_no_launcher_output() {
    let f = setup();
    for code in [0u32, 1, 2, 42, 255, 256, 0xc000013a, u32::MAX] {
        let mut command = driver(&f, "claudea");
        command
            .arg("prompt")
            .env("LARPCC_FAKE_EXIT", code.to_string());
        let out = capture(command).unwrap();
        assert_eq!(out.status.code().map(|n| n as u32), Some(code));
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
    }
}
#[test]
fn l04_w05_piped_input_and_eight_megabyte_output_are_exact() {
    let f = setup();
    let mut command = driver(&f, "claudea");
    command
        .arg("prompt")
        .env("LARPCC_FAKE_MODE", "echo-io")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let input = b"synthetic\0binary\xff\r\n";
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = wait_output(child).unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(out.stdout, input);
    assert!(out.stderr.is_empty());
    let mut command = driver(&f, "claudea");
    command.arg("prompt").env("LARPCC_FAKE_MODE", "output");
    let out = capture(command).unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(out.stdout, vec![b'x'; 8 * 1024 * 1024]);
    assert!(out.stderr.is_empty());
}
#[test]
fn p02_p04_provisional_conflicts_refuse_without_child_or_registry_changes() {
    let f = setup();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    for (index, args) in [
        vec!["login"],
        vec!["--config=synthetic", "prompt"],
        vec!["--label", "x", "--label", "y", "prompt"],
    ]
    .into_iter()
    .enumerate()
    {
        let record = f.temp.path().join(format!("refused-{index}"));
        let mut command = driver(&f, "claudea");
        command.args(args).env("LARPCC_FAKE_RECORD", &record);
        let out = capture(command).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(!record.exists());
    }
    for key in ["claude_config_dir", "Larpcc_Synthetic_Deny_"] {
        let record = f.temp.path().join(format!("refused-{key}"));
        let mut command = driver(&f, "claudea");
        command
            .arg("prompt")
            .env(key, "")
            .env("LARPCC_FAKE_RECORD", &record);
        let out = capture(command).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(!record.exists());
    }
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}
