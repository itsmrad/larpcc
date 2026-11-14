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
    assert!(stderr(&out).contains("disabled for this combination"));
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
