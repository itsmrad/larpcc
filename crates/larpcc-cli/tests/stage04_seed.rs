mod common;
use common::*;
use serde_json::{Value, json};
use std::fs;
fn request(f: &Fixture, args: &[&str], code: i32) -> Value {
    let out = capture(f.command(args)).unwrap();
    assert_eq!(out.status.code(), Some(code), "{}", stderr(&out));
    assert!(out.stderr.is_empty(), "{}", stderr(&out));
    assert!(out.stdout.ends_with(b"\n"));
    assert_eq!(out.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["ok"], code == 0);
    value
}
#[test]
fn stage04_layout_json_and_mutation_ids() {
    let f = Fixture::new();
    let before = f.outside_snapshot();
    let added = request(
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
    assert_eq!(
        added["data"],
        json!({"changed":true,"profile_id":"1","name":"claude-work","warnings":[]})
    );
    assert_eq!(added["revision"], "1");
    let container = f.root.join("profiles/1");
    assert!(container.join("cli").is_dir());
    assert!(container.join("desktop").is_dir());
    let list = request(&f, &["--json", "cc", "profile", "list"], 0);
    let p = &list["data"]["profiles"][0];
    assert_eq!(p["state"], "active");
    assert_eq!(p["provider"], "claude");
    assert_eq!(p["surfaces"]["cli"]["enabled"], true);
    assert_eq!(p["surfaces"]["desktop"]["enabled"], false);
    assert_eq!(p["aliases"], json!([]));
    assert_eq!(
        p["desktop_binding"],
        json!({"path":null,"recorded_version":null,"detected_version":null,"state":"unbound","qualified":false})
    );
    assert_eq!(
        request(&f, &["cc", "profile", "show", "claude-work", "--json"], 0)["data"],
        *p
    );
    let toggled = request(
        &f,
        &[
            "cc",
            "profile",
            "surface",
            "claude-work",
            "desktop",
            "on",
            "--json",
            "--expected-revision",
            "1",
        ],
        0,
    );
    assert_eq!(toggled["data"]["profile_id"], "1");
    assert_eq!(toggled["revision"], "2");
    let renamed = request(
        &f,
        &[
            "cc",
            "profile",
            "rename",
            "claude-work",
            "codex-45",
            "--json",
        ],
        0,
    );
    assert_eq!(renamed["data"]["profile_id"], "1");
    assert_eq!(renamed["data"]["name"], "codex-45");
    let forgotten = request(
        &f,
        &["cc", "profile", "forget", "codex-45", "--yes", "--json"],
        0,
    );
    assert_eq!(forgotten["data"]["profile_id"], "1");
    assert!(container.is_dir());
    assert_eq!(f.outside_snapshot(), before);
}
#[test]
fn stage04_expected_revision_name_reuse_and_precedence() {
    let f = Fixture::new();
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
            "rename",
            "claude-work",
            "codex-45",
            "--json",
            "--expected-revision",
            "1",
        ],
        0,
    );
    let new = request(
        &f,
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
    assert_eq!(new["data"]["profile_id"], "2");
    let before = fs::read(f.root.join("registry.json")).unwrap();
    for tail in [
        vec!["forget", "claude-work", "--yes"],
        vec!["rename", "claude-work", "other"],
        vec!["show", "absent"],
        vec!["list"],
        vec!["surface", "claude-work", "cli", "off"],
    ] {
        let args = [
            &["cc", "profile"][..],
            &tail,
            &["--json", "--expected-revision", "1"],
        ]
        .concat();
        let error = request(&f, &args, 1);
        assert_eq!(error["error"]["code"], "conflict");
        assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
    }
    let error = request(
        &f,
        &[
            "cc",
            "provider",
            "bind",
            "claude",
            "relative",
            "--json",
            "--expected-revision",
            "1",
        ],
        1,
    );
    assert_eq!(error["error"]["code"], "conflict");
}
#[test]
fn stage04_json_parser_help_and_native_gates() {
    let f = Fixture::new();
    for args in [
        vec!["--json"],
        vec!["cc", "profile", "show", "Bad-secret", "--json"],
        vec!["--bad-secret", "--json"],
        vec![
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "secret",
            "--json",
        ],
    ] {
        let v = request(&f, &args, 2);
        assert_eq!(v["command"], "parse");
        assert!(!v.to_string().contains("secret"));
    }
    assert_eq!(
        request(&f, &["cc", "profile", "forget", "claude-work", "--json"], 2)["error"]["code"],
        "usage"
    );
    for (args, command) in [
        (vec!["--json", "--help"], "help"),
        (vec!["--version", "--json"], "version"),
    ] {
        let v = request(&f, &args, 0);
        assert_eq!(v["command"], command);
        assert!(v["revision"].is_null());
    }
    assert_eq!(
        request(&f, &["--json", "--help", "--expected-revision", "0"], 2)["error"]["code"],
        "usage"
    );
    for op in ["run", "login", "logout"] {
        assert_eq!(
            request(
                &f,
                &[
                    "cc",
                    op,
                    "claude-work",
                    "--json",
                    "--expected-revision",
                    "1"
                ],
                2
            )["error"]["code"],
            "json_native_io"
        );
    }
    // Child --json is not a global option, and the closed native gate still has no root IO.
    let out = f.run(&["cc", "run", "claude-work", "--", "--json"], 1);
    assert!(stderr(&out).contains("provider operation is disabled"));
    assert!(!f.root.exists());
}
#[test]
fn stage04_legacy_read_unchanged_mutations_refuse() {
    for version in [1, 2] {
        let f = Fixture::new();
        let mut v = json!({"version":version,"revision":7,"next_id":3,"profiles":[{"id":2,"name":"codex-45","provider":"codex"}]});
        if version == 2 {
            v["bindings"] = json!([]);
        }
        let bytes = serde_json::to_vec(&v).unwrap();
        f.install_registry(&bytes);
        f.profile(&["list"], 0);
        f.profile(&["show", "codex-45"], 0);
        request(&f, &["cc", "profile", "list", "--json"], 0);
        for args in [
            vec!["add", "claude-work", "--provider", "claude"],
            vec!["rename", "codex-45", "claude-work"],
            vec!["forget", "codex-45", "--yes"],
            vec!["surface", "codex-45", "cli", "off"],
        ] {
            let args = [&["cc", "profile"][..], &args, &["--json"]].concat();
            assert_eq!(request(&f, &args, 1)["error"]["code"], "recovery_required");
            assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), bytes);
            assert!(!f.root.join("profiles").exists());
        }
    }
}
#[test]
fn stage04_tombstone_and_alias_reference_refusals() {
    let f = Fixture::new();
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
    let path = f.root.join("registry.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    v["aliases"] = json!([{"name":"codex-45","profile_id":1,"surface":"cli"}]);
    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(
        request(
            &f,
            &["cc", "profile", "forget", "claude-work", "--yes", "--json"],
            1
        )["error"]["code"],
        "conflict"
    );
    v["aliases"] = json!([]);
    v["profiles"][0]["state"] = json!("deleting");
    v["profiles"][0]["deletion_transaction"] = json!("delete-1");
    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    assert_eq!(
        request(&f, &["cc", "profile", "show", "claude-work", "--json"], 0)["data"]["state"],
        "deleting"
    );
    for args in [
        vec!["rename", "claude-work", "codex-45"],
        vec!["forget", "claude-work", "--yes"],
        vec!["surface", "claude-work", "cli", "off"],
    ] {
        let args = [&["cc", "profile"][..], &args, &["--json"]].concat();
        assert_eq!(request(&f, &args, 1)["error"]["code"], "recovery_required");
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn stage04_strict_v3_fields_enums_references_and_safe_errors() {
    let f = Fixture::new();
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
    let path = f.root.join("registry.json");
    let good: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut bad = Vec::new();
    for field in ["bindings", "aliases", "desktop_bindings"] {
        let mut v = good.clone();
        v.as_object_mut().unwrap().remove(field);
        bad.push(v);
    }
    for field in ["surfaces", "state", "deletion_transaction"] {
        let mut v = good.clone();
        v["profiles"][0].as_object_mut().unwrap().remove(field);
        bad.push(v);
    }
    for state in [json!(null), json!("unknown"), json!("deleting")] {
        let mut v = good.clone();
        v["profiles"][0]["state"] = state;
        bad.push(v);
    }
    let mut v = good.clone();
    v["profiles"][0]["deletion_transaction"] = json!("delete-1");
    bad.push(v);
    for aliases in [
        json!([{"name":"codex-45","profile_id":99,"surface":"cli"}]),
        json!([{"name":"codex-45","profile_id":1,"surface":"other"}]),
        json!([{"name":"codex-45","profile_id":1,"surface":"cli"},{"name":"codex-45","profile_id":1,"surface":"desktop"}]),
    ] {
        let mut v = good.clone();
        v["aliases"] = aliases;
        bad.push(v);
    }
    let mut v = good.clone();
    v["desktop_bindings"] =
        json!([{"provider":"claude","path":"relative","recorded_version":null}]);
    bad.push(v);
    for value in bad {
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            request(&f, &["cc", "profile", "list", "--json"], 1)["error"]["code"],
            "corrupt"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn stage04_concurrent_revision_requests_commit_once() {
    let f = Fixture::new();
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
    let args = [
        "cc",
        "profile",
        "rename",
        "claude-work",
        "codex-45",
        "--json",
        "--expected-revision",
        "1",
    ];
    let a = f
        .command(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let b = f
        .command(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut results = [wait_output(a).unwrap(), wait_output(b).unwrap()];
    results.sort_by_key(|o| o.status.code());
    assert_eq!(results[0].status.code(), Some(0));
    assert_eq!(results[1].status.code(), Some(1));
    let failed: Value = serde_json::from_slice(&results[1].stdout).unwrap();
    assert_eq!(failed["error"]["code"], "conflict");
    assert_eq!(
        request(&f, &["cc", "profile", "list", "--json"], 0)["revision"],
        "2"
    );
}

#[cfg(unix)]
#[test]
fn stage04_json_invalid_unicode_and_output_failure_are_silent() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let f = Fixture::new();
    let mut command = f.command(&["--json", "cc", "profile", "show"]);
    command.arg(OsString::from_vec(vec![b'a', 255, b'z']));
    let out = capture(command).unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["command"], "parse");
    #[cfg(target_os = "linux")]
    {
        let full = fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        let out = f
            .command(&["--json", "--help"])
            .stdout(full)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stderr.is_empty());
        assert!(out.stdout.is_empty());
    }
    assert!(!f.root.exists());
}
