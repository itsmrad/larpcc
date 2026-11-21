// Crash/fault-injected add and remove phases are covered by the registry unit tests
// (`stage05_transaction_interrupted_*`); these drive the shipped binary end to end.
mod common;
use common::*;
#[cfg(unix)]
use std::fs;

#[cfg(unix)]
const INVALID: &str = "error: alias launcher, receipt or mapping cannot be verified\n";

#[cfg(unix)]
fn setup() -> Fixture {
    let f = Fixture::new();
    f.larp_json(
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "codex",
            "--json",
        ],
        0,
    );
    f
}
fn code(value: &serde_json::Value) -> &str {
    value["error"]["code"].as_str().unwrap()
}

#[cfg(unix)]
#[test]
fn stage05_transaction_concurrent_writers_commit_once() {
    let f = setup();
    let larp = f.install_larp();
    let names = ["codex-45", "codex-45", "alpha", "beta", "gamma", "codex-45"];
    let children: Vec<_> = names
        .iter()
        .map(|name| {
            let mut command = f.command_for(
                &larp,
                &["cc", "alias", "add", name, "claude-work", "--json"],
            );
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            spawn(&mut command).unwrap()
        })
        .collect();
    let mut revisions = vec![];
    let mut added = vec![];
    for (name, child) in names.iter().zip(children) {
        let output = wait_output(child).unwrap();
        let value = envelope(&output);
        if output.status.code() == Some(0) {
            revisions.push(value["revision"].as_str().unwrap().to_owned());
            added.push(*name);
        } else {
            assert!(matches!(code(&value), "conflict" | "busy"), "{value}");
        }
    }
    revisions.sort();
    revisions.dedup();
    assert_eq!(revisions.len(), added.len());
    assert_eq!(added.iter().filter(|n| **n == "codex-45").count(), 1);
    let list = f.larp_json(&["cc", "alias", "list", "--json"], 0);
    let mut listed: Vec<_> = list["data"]["aliases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            assert_eq!(a["state"], "ok");
            a["name"].as_str().unwrap().to_owned()
        })
        .collect();
    added.sort();
    listed.sort();
    assert_eq!(listed, added);
    assert!(!f.root.join("share-journal.json").exists());
    for name in ["alpha", "beta", "gamma"] {
        assert_eq!(f.launcher(name).exists(), added.contains(&name));
    }
}

#[cfg(unix)]
#[test]
fn stage05_transaction_expected_revision_and_pending_gate() {
    let f = setup();
    let refused = f.larp_json(
        &[
            "cc",
            "alias",
            "add",
            "codex-45",
            "claude-work",
            "--expected-revision",
            "0",
            "--json",
        ],
        1,
    );
    assert_eq!(code(&refused), "conflict");
    assert!(!f.root.join("bin").exists());
    f.larp_json(
        &[
            "cc",
            "alias",
            "add",
            "codex-45",
            "claude-work",
            "--expected-revision",
            "1",
            "--json",
        ],
        0,
    );
    assert_eq!(
        code(&f.larp_json(
            &[
                "cc",
                "alias",
                "remove",
                "codex-45",
                "--expected-revision",
                "1",
                "--json"
            ],
            1
        )),
        "conflict"
    );
    assert!(f.launcher("codex-45").exists());
    // Another owner's pending journal blocks dispatch and alias commands alike.
    let journal = f.root.join("share-journal.json");
    create_file(&journal, br#"{"version":1,"kind":"skills"}"#);
    let output = exec(f.command_for(&f.launcher("codex-45"), &[]), 1);
    assert!(output.stdout.is_empty());
    assert!(
        stderr(&output).starts_with("error: explicit profile migration or recovery is required")
    );
    for args in [
        vec!["cc", "alias", "list", "--json"],
        vec!["cc", "alias", "remove", "codex-45", "--json"],
    ] {
        assert_eq!(code(&f.larp_json(&args, 1)), "recovery_required");
    }
    assert_eq!(
        fs::read(&journal).unwrap(),
        br#"{"version":1,"kind":"skills"}"#
    );
    fs::remove_file(&journal).unwrap();
    f.larp_json(
        &[
            "cc",
            "alias",
            "remove",
            "codex-45",
            "--expected-revision",
            "2",
            "--json",
        ],
        0,
    );
}

#[cfg(unix)]
#[test]
fn stage05_transaction_forged_receipt_and_drift_refuse() {
    let f = setup();
    f.larp_json(
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
        0,
    );
    let receipt = f.alias_receipt("codex-45");
    let original = fs::read(&receipt).unwrap();
    let mut forged: serde_json::Value = serde_json::from_slice(&original).unwrap();
    forged["root"] = f.temp.path().join("other").to_str().unwrap().into();
    for bytes in [serde_json::to_vec(&forged).unwrap(), b"{}".to_vec()] {
        fs::write(&receipt, &bytes).unwrap();
        let output = exec(f.command_for(&f.launcher("codex-45"), &[]), 1);
        assert!(output.stdout.is_empty() && stderr(&output).starts_with(INVALID));
        let list = f.larp_json(&["cc", "alias", "list", "--json"], 0);
        assert_eq!(list["data"]["aliases"][0]["state"], "drift");
        let refused = f.larp_json(&["cc", "alias", "remove", "codex-45", "--json"], 1);
        assert_eq!(code(&refused), "drift");
        assert_eq!(fs::read(&receipt).unwrap(), bytes);
        assert!(f.launcher("codex-45").exists());
    }
    fs::write(&receipt, &original).unwrap();
    // Changed launcher bytes are drift; nothing is removed by marker or name alone.
    let launcher = f.launcher("codex-45");
    let image = fs::read(&launcher).unwrap();
    let mut changed = image.clone();
    changed.push(0);
    fs::write(&launcher, &changed).unwrap();
    let output = exec(f.command_for(&launcher, &[]), 1);
    assert!(stderr(&output).starts_with(INVALID));
    assert_eq!(
        code(&f.larp_json(&["cc", "alias", "remove", "codex-45", "--json"], 1)),
        "drift"
    );
    assert_eq!(fs::read(&launcher).unwrap(), changed);
    fs::write(&launcher, &image).unwrap();
    let list = f.larp_json(&["cc", "alias", "list", "--json"], 0);
    assert_eq!(list["data"]["aliases"][0]["state"], "ok");
    f.larp_json(&["cc", "alias", "remove", "codex-45", "--json"], 0);
    assert!(!launcher.exists() && !receipt.exists());
}

#[test]
fn stage05_transaction_stale_revision_leaves_missing_root_untouched() {
    let f = Fixture::new();
    let value = f.larp_json(
        &[
            "cc",
            "alias",
            "add",
            "codex-45",
            "claude-work",
            "--expected-revision",
            "1",
            "--json",
        ],
        1,
    );
    assert_eq!(code(&value), "conflict");
    assert!(!f.root.exists());
}

#[cfg(unix)]
#[test]
fn stage05_transaction_non_unicode_root_refuses_before_alias_artifacts() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let mut f = Fixture::new();
    f.root = f.temp.path().join(OsStr::from_bytes(b"managed-\xff"));
    f.profile(&["add", "claude-work", "--provider", "codex"], 0);
    let value = f.larp_json(
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
        1,
    );
    assert_eq!(code(&value), "unsafe_path");
    assert!(!f.root.join("bin").exists());
    assert!(!f.root.join("share-journal.json").exists());
}
