mod common;

use common::{Fixture, capture, create_file};
use serde_json::{Value, json};
use std::{fs, path::Path, thread};

fn json_run(fixture: &Fixture, args: &[&str], expected: i32) -> Value {
    let output = capture(fixture.command(&[&["--json", "cc"][..], args].concat())).unwrap();
    assert!(output.stderr.is_empty(), "{args:?}");
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{args:?}: {:?}",
        output.stdout
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(
        output.stdout.iter().filter(|&&byte| byte == b'\n').count(),
        1
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn stage13_hosts_crud_exact_fixtures_and_no_ssh() {
    let fixture = Fixture::new();
    let examples: Vec<Value> =
        serde_json::from_str(include_str!("../../../docs/contracts/cli-v1.examples.json")).unwrap();
    let find = |command| {
        examples
            .iter()
            .find(|value| value["ok"] == true && value["command"] == command)
            .unwrap()
    };

    let fresh = json_run(&fixture, &["host", "list", "--expected-revision", "0"], 0);
    assert_eq!(
        fresh,
        json!({"schema":1,"command":"host.list","ok":true,"revision":"0","data":{"hosts":[]}})
    );
    assert!(!fixture.root.exists());
    let private_path = if cfg!(windows) {
        r"C:\example\keys\dev-vm"
    } else {
        "/example/keys/dev-vm"
    };
    let with = |args: &[&str], exit| json_run(&fixture, args, exit);
    let added = with(
        &["host", "add", "codex-45", "--ssh-config-host", "work.vm"],
        0,
    );
    let mut expected = find("host.add").clone();
    expected["data"]["name"] = json!("codex-45");
    assert_eq!(added, expected);
    let mut expected = find("host.show").clone();
    expected["data"]["host"]["name"] = json!("codex-45");
    assert_eq!(with(&["host", "show", "codex-45"], 0), expected);
    assert_eq!(
        with(
            &["host", "add", "codex-45", "--ssh-config-host", "work.vm"],
            1
        )["error"]["code"],
        "conflict"
    );
    let _ = with(
        &[
            "host",
            "add",
            "claude-work",
            "--ssh",
            "dev@sandbox.example:2222",
            "--larp",
            "/opt/Larp Tools/larp",
            "--identity-file",
            private_path,
        ],
        0,
    );
    let mut expected = find("host.list").clone();
    expected["data"]["hosts"][0]["name"] = json!("claude-work");
    expected["data"]["hosts"][0]["identity_file"] = json!(private_path);
    expected["data"]["hosts"][1]["name"] = json!("codex-45");
    assert_eq!(with(&["host", "list"], 0), expected);
    assert_eq!(
        with(
            &["host", "show", "claude-work", "--expected-revision", "1"],
            1
        )["error"]["code"],
        "conflict"
    );
    assert_eq!(
        with(&["host", "remove", "claude-work"], 2)["error"]["code"],
        "usage"
    );
    let remove = with(&["host", "remove", "claude-work", "--yes"], 0);
    let mut expected = find("host.remove").clone();
    expected["data"]["name"] = json!("claude-work");
    expected["revision"] = json!("3");
    assert_eq!(remove, expected);
    assert!(!fixture.root.join("registry.json").exists());
}

#[test]
fn stage13_hosts_strict_files_and_concurrent_name_reuse() {
    let fixture = Fixture::new();
    let root = fixture.root.clone();
    thread::scope(|scope| {
        let add = || {
            capture(fixture.command(&[
                "--json",
                "cc",
                "host",
                "add",
                "claude-work",
                "--ssh-config-host",
                "host.test",
            ]))
            .unwrap()
            .status
            .code()
            .unwrap()
        };
        let left = scope.spawn(add);
        let right = scope.spawn(add);
        let mut exits = [left.join().unwrap(), right.join().unwrap()];
        exits.sort_unstable();
        assert_eq!(exits, [0, 1]);
    });
    let path = root.join("hosts.json");
    let good = fs::read(&path).unwrap();
    let value: Value = serde_json::from_slice(&good).unwrap();
    assert_eq!(value["revision"], "1");
    for invalid in [
        json!({"version":1,"revision":"01","hosts":[]}),
        json!({"version":1,"revision":"18446744073709551616","hosts":[]}),
        json!({"version":1,"revision":"1","hosts":[value["hosts"][0],value["hosts"][0]]}),
        json!({"version":1,"revision":"1","hosts":[{"name":"claude-work","ssh":{"mode":"config","alias":"host.test"},"larp":null,"identity_file":null}]}),
        json!({"version":1,"revision":"1","hosts":[{"name":"claude-work","ssh":{"mode":"config","alias":"host.test","unknown":true},"larp":null,"larpcc_home":null,"identity_file":null}]}),
    ] {
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert_eq!(
            json_run(&fixture, &["host", "list"], 1)["error"]["code"],
            "corrupt"
        );
    }
    fs::write(&path, [b'x'; 4096].repeat(257)).unwrap();
    assert_eq!(
        json_run(&fixture, &["host", "list"], 1)["error"]["code"],
        "corrupt"
    );
    fs::write(&path, &good).unwrap();
    assert_eq!(
        json_run(&fixture, &["host", "show", "claude-work"], 0)["revision"],
        "1"
    );
    assert!(!root.join("hosts.json.tmp").exists());
}

#[test]
fn stage13_revision_native_and_receiver_gates_before_effects() {
    let fixture = Fixture::new();
    let refused =
        |args, exit, error| assert_eq!(json_run(&fixture, args, exit)["error"]["code"], error);
    refused(
        &["--host", "claude-work", "run", "codex-45"][..],
        2,
        "json_native_io",
    );
    refused(
        &["--host", "claude-work", "login", "codex-45"][..],
        2,
        "json_native_io",
    );
    refused(
        &["--host", "claude-work", "doctor", "--detect-versions"][..],
        2,
        "unsupported",
    );
    refused(
        &[
            "--remote-request",
            "--host",
            "claude-work",
            "profile",
            "list",
        ][..],
        2,
        "unsupported",
    );
    refused(
        &[
            "--remote-request",
            "host",
            "add",
            "codex-45",
            "--ssh-config-host",
            "host.test",
        ][..],
        2,
        "unsupported",
    );
    refused(
        &["host", "catalog", "export", "skills", "codex-45"][..],
        2,
        "unsupported",
    );
    assert!(!fixture.root.exists());
}

// Cargo exposes only the built local fixture here. Production SSH discovery stays independent.
#[test]
fn stage13_transport_fixture_is_executable() {
    let path = Path::new(env!("CARGO_BIN_EXE_larpcc-fake-ssh"));
    let output = std::process::Command::new(path)
        .arg("-V")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("OpenSSH")
    );
}

#[test]
fn stage13_catalog_local_sharing_inheritance_and_copy_route_rules() {
    let fixture = Fixture::new();
    let entry = "codex-45";
    let with = |args: &[&str], code| json_run(&fixture, args, code);
    let empty = with(&["host", "catalog", "list", "skills"], 0);
    assert_eq!(empty["revision"], "0");
    assert_eq!(empty["data"]["entries"], json!([]));
    let source = fixture.temp.path().join("package");
    common::create_directory(&source);
    create_file(&source.join("SKILL.md"), b"# Fixture\n");
    let added = with(
        &["skills", "add", source.to_str().unwrap(), "--name", entry],
        0,
    );
    let revision = added["revision"].as_str().unwrap();
    let shared = with(
        &[
            "host",
            "catalog",
            "share",
            "skills",
            entry,
            "on",
            "--expected-revision",
            revision,
        ],
        0,
    );
    assert_eq!(shared["data"]["changed"], true);
    let next = shared["revision"].as_str().unwrap();
    let list = with(
        &[
            "host",
            "catalog",
            "list",
            "skills",
            "--expected-revision",
            next,
        ],
        0,
    );
    assert_eq!(list["data"]["entries"][0]["all_machines"], true);
    let digest = list["data"]["entries"][0]["content_sha256"]
        .as_str()
        .unwrap();
    let noop = with(
        &[
            "host",
            "catalog",
            "share",
            "skills",
            entry,
            "on",
            "--expected-revision",
            next,
        ],
        0,
    );
    assert_eq!(noop["data"]["changed"], false);
    assert_eq!(noop["revision"], next);
    let profile = with(
        &["profile", "add", "claude-work", "--provider", "claude"],
        0,
    );
    let matrix = with(&["skills", "matrix"], 0);
    assert_eq!(matrix["data"]["rows"][0]["enabled"], json!([true]));
    let newer = profile["revision"].as_str().unwrap();
    let bad = with(
        &[
            "host",
            "catalog",
            "copy",
            "skills",
            entry,
            "--from",
            "@local",
            "--to",
            "@local",
            "--expected-source-revision",
            newer,
            "--expected-revision",
            newer,
            "--sha256",
            digest,
        ],
        2,
    );
    assert_eq!(bad["error"]["code"], "usage");
}

#[test]
fn stage13_mcp_references_unset_name_without_server_launch() {
    let fixture = Fixture::new();
    let server = fixture
        .temp
        .path()
        .join(format!("fixture{}", std::env::consts::EXE_SUFFIX));
    // The receiver checks the file only. Its contents cannot be executed.
    create_file(&server, b"fixture");
    let env_ref = "LARPCC_STAGE13_UNSET_REFERENCE";
    let apply = json_run(
        &fixture,
        &[
            "--remote-request",
            "host",
            "catalog",
            "apply",
            "mcp",
            "codex-45",
            "--command",
            server.to_str().unwrap(),
            "--env-ref",
            env_ref,
            "--arg=--stdio",
            "--expected-revision",
            "0",
        ],
        0,
    );
    assert_eq!(apply["command"], "host.catalog.apply");
    assert_eq!(apply["data"]["changed"], true);
    let list = json_run(&fixture, &["mcp", "list"], 0);
    assert_eq!(
        list["data"]["servers"][0]["secret_refs"],
        json!([{"kind":"env","name":env_ref}])
    );
    let absent = fixture.temp.path().join("absent");
    let missing = json_run(
        &fixture,
        &[
            "--remote-request",
            "host",
            "catalog",
            "apply",
            "mcp",
            "claude-work",
            "--command",
            absent.to_str().unwrap(),
            "--env-ref",
            env_ref,
        ],
        1,
    );
    assert_eq!(missing["error"]["code"], "not_found");
}

#[test]
fn stage13_skill_transfer_receiver_keeps_stage_private_and_accepts_verified_bytes() {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        process::Stdio,
    };

    let source = Fixture::new();
    let target = Fixture::new();
    let denied = Fixture::new();
    let entry = "codex-45";
    let package = source.temp.path().join("package");
    common::create_directory(&package);
    common::create_directory(&package.join("support"));
    create_file(&package.join("SKILL.md"), b"# Test package\n");
    create_file(&package.join("support/data.bin"), b"\x00\xff\x03");
    let added = json_run(
        &source,
        &["skills", "add", package.to_str().unwrap(), "--name", entry],
        0,
    );
    let revision = added["revision"].as_str().unwrap();
    let list = json_run(&source, &["host", "catalog", "list", "skills"], 0);
    let digest = list["data"]["entries"][0]["content_sha256"]
        .as_str()
        .unwrap();
    let export = capture(source.command(&[
        "--json",
        "--remote-request",
        "cc",
        "host",
        "catalog",
        "export",
        "skills",
        entry,
        "--expected-revision",
        revision,
    ]))
    .unwrap();
    assert!(export.status.success());
    assert!(export.stderr.is_empty());
    assert!(export.stdout.starts_with(b"LARPCC-SKILLS/1\n"));
    let import = |fixture: &Fixture, payload: Option<&[u8]>, revision: &str, expected: i32| {
        let mut child = fixture
            .command(&[
                "--json",
                "--remote-request",
                "cc",
                "host",
                "catalog",
                "import",
                "skills",
                entry,
                "--expected-revision",
                revision,
                "--sha256",
                digest,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        stdout.read_line(&mut first).unwrap();
        let mut input = child.stdin.take().unwrap();
        let data = if let Some(payload) = payload {
            assert_eq!(first, "LARPCC-IMPORT/1 READY\n");
            input.write_all(payload).unwrap();
            drop(input);
            let mut receipt = Vec::new();
            stdout.read_to_end(&mut receipt).unwrap();
            receipt
        } else {
            drop(input);
            first.into_bytes()
        };
        let result = child.wait_with_output().unwrap();
        assert!(result.stderr.is_empty());
        assert_eq!(result.status.code(), Some(expected), "{data:?}");
        serde_json::from_slice::<Value>(&data).unwrap()
    };
    let stale = import(&denied, None, "1", 1);
    assert_eq!(stale["error"]["code"], "conflict");
    assert!(!denied.root.exists());
    let corrupt = import(
        &denied,
        Some(&export.stdout[..export.stdout.len() - 1]),
        "0",
        1,
    );
    assert_eq!(corrupt["error"]["code"], "corrupt");
    assert!(!denied.root.exists());
    let imported = import(&target, Some(&export.stdout), "0", 0);
    assert_eq!(imported["data"]["changed"], true);
    assert_eq!(imported["command"], "host.catalog.import");
    let destination = target.root.join("shared/skills").join(entry);
    assert_eq!(
        fs::read(destination.join("SKILL.md")).unwrap(),
        b"# Test package\n"
    );
    assert_eq!(
        fs::read(destination.join("support/data.bin")).unwrap(),
        b"\x00\xff\x03"
    );
    let revision = imported["revision"].as_str().unwrap();
    let duplicate = import(&target, Some(&export.stdout), revision, 0);
    assert_eq!(duplicate["data"]["changed"], false);
    assert_eq!(duplicate["revision"], revision);
    assert!(!fs::read_dir(&target.root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("skill-import-")
    }));
}
