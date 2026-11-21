//! Stage 07 CLI e2e: exact cli-v1 envelopes for every MCP command, the Mac
//! app's exact argv shapes, and zero secret bytes in output or files.
//! Synthetic private roots only; names claude-work and codex-45; no MCP child.
mod common;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn request(f: &common::Fixture, args: &[&str], code: i32) -> Value {
    let out = common::capture(f.command(args)).unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "args={args:?} stdout={}",
        common::stdout(&out)
    );
    assert!(out.stderr.is_empty(), "{}", common::stderr(&out));
    assert_eq!(out.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    serde_json::from_slice(&out.stdout).unwrap()
}
fn revision(v: &Value) -> u64 {
    v["revision"].as_str().unwrap().parse().unwrap()
}
fn command_file(f: &common::Fixture) -> String {
    let bin = f.temp.path().join("bin");
    common::create_directory(&bin);
    let exe = bin.join(if cfg!(windows) {
        "files-mcp.exe"
    } else {
        "files-mcp"
    });
    common::create_file(&exe, b"synthetic server; never executed\n");
    exe.to_str().unwrap().to_owned()
}
/// Same keys and value kinds as the frozen contract example, recursively.
fn same_shape(actual: &Value, example: &Value) -> bool {
    match (actual, example) {
        (Value::Object(a), Value::Object(e)) => {
            a.len() == e.len()
                && e.iter().all(|(k, v)| {
                    a.get(k)
                        .is_some_and(|x| same_shape(x, v) || v.is_null() || x.is_null())
                })
        }
        (Value::Array(a), Value::Array(e)) => match e.first() {
            Some(first) => a.iter().all(|x| same_shape(x, first)),
            None => a.is_empty(),
        },
        (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_)) => true,
        (a, e) => a == e,
    }
}
fn example(command: &str) -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/cli-v1.examples.json");
    let all: Vec<Value> = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    all.into_iter()
        .find(|e| e["command"] == command && e["ok"] == true)
        .unwrap_or_else(|| panic!("no example for {command}"))
}
fn files_containing(root: &Path, needle: &[u8]) -> Vec<std::path::PathBuf> {
    let mut found = vec![];
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let meta = fs::symlink_metadata(&path).unwrap();
        if meta.is_dir() {
            found.extend(files_containing(&path, needle));
        } else if meta.is_file()
            && fs::read(&path)
                .unwrap()
                .windows(needle.len())
                .any(|w| w == needle)
        {
            found.push(path);
        }
    }
    found
}

#[test]
fn stage07_matrix_cli_gui_argv_exact_receipts_and_views() {
    let f = common::Fixture::new();
    let exe = command_file(&f);
    let empty = request(&f, &["cc", "mcp", "list", "--json"], 0);
    assert_eq!(
        empty,
        json!({"schema":1,"command":"mcp.list","ok":true,"revision":"0","data":{"sync_all":false,"servers":[]}})
    );
    assert!(!f.root.exists());
    for (name, provider) in [("claude-work", "claude"), ("codex-45", "codex")] {
        request(
            &f,
            &[
                "cc",
                "profile",
                "add",
                name,
                "--provider",
                provider,
                "--json",
            ],
            0,
        );
    }
    let rev = revision(&request(&f, &["cc", "profile", "list", "--json"], 0));
    // Exact Mac app argv (ProcessLarpClient): single-token --arg=VALUE and --env-ref NAME.
    let rev_s = rev.to_string();
    let added = request(
        &f,
        &[
            "cc",
            "mcp",
            "add",
            "files",
            "--command",
            &exe,
            "--arg=--stdio",
            "--env-ref",
            "FILES_TOKEN",
            "--env-ref",
            "HOME_DIR",
            "--expected-revision",
            &rev_s,
            "--json",
        ],
        0,
    );
    assert_eq!(
        added,
        json!({"schema":1,"command":"mcp.add","ok":true,"revision":(rev+1).to_string(),
               "data":{"changed":true,"profile_id":null,"name":"files","warnings":[]}})
    );
    assert!(same_shape(&added, &example("mcp.add")));
    request(
        &f,
        &[
            "cc",
            "mcp",
            "add",
            "tools",
            "--command",
            &exe,
            "--arg=--transport=stdio",
            "--json",
        ],
        0,
    );
    // Space form is also accepted for the documented grammar.
    request(
        &f,
        &[
            "cc",
            "mcp",
            "add",
            "spaced",
            "--command",
            &exe,
            "--arg",
            "--stdio",
            "--json",
        ],
        0,
    );
    let docs = request(
        &f,
        &[
            "cc",
            "mcp",
            "add",
            "docs",
            "--url",
            "https://docs.example.com/mcp",
            "--header-ref",
            "Authorization=DOCS_TOKEN",
            "--json",
        ],
        0,
    );
    assert_eq!(docs["data"]["name"], "docs");
    let on = request(
        &f,
        &["cc", "mcp", "enable", "files", "codex-45", "--json"],
        0,
    );
    assert_eq!(
        on["data"],
        json!({"changed":true,"profile_id":"2","name":"files","warnings":[]})
    );
    assert!(same_shape(&on, &example("mcp.enable")));
    let all = request(&f, &["cc", "mcp", "enable", "docs", "--all", "--json"], 0);
    assert_eq!(
        all["data"],
        json!({"changed":true,"profile_id":null,"name":"docs","warnings":[]})
    );
    let off = request(
        &f,
        &["cc", "mcp", "disable", "docs", "claude-work", "--json"],
        0,
    );
    assert_eq!(
        off["data"],
        json!({"changed":true,"profile_id":"1","name":"docs","warnings":[]})
    );
    assert!(same_shape(&off, &example("mcp.disable")));
    let list = request(&f, &["cc", "mcp", "list", "--json"], 0);
    assert!(same_shape(&list, &example("mcp.list")));
    assert_eq!(
        list["data"]["servers"][0],
        json!({"name":"docs","transport":"http","secret_refs":[{"kind":"env","name":"DOCS_TOKEN"}],
               "switches":[{"profile_id":"1","enabled":false},{"profile_id":"2","enabled":true}],"state":"ok"})
    );
    assert_eq!(
        list["data"]["servers"][1]["secret_refs"],
        json!([{"kind":"env","name":"FILES_TOKEN"},{"kind":"env","name":"HOME_DIR"}])
    );
    let names: Vec<_> = list["data"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["docs", "files", "spaced", "tools"]);
    // List never exposes the command, args, URL or header names.
    let text = serde_json::to_string(&list).unwrap();
    for hidden in [exe.as_str(), "--stdio", "docs.example.com", "Authorization"] {
        assert!(!text.contains(hidden), "{hidden}");
    }
    let matrix = request(&f, &["cc", "mcp", "matrix", "--json"], 0);
    assert!(same_shape(&matrix, &example("mcp.matrix")));
    assert_eq!(
        matrix["data"]["profiles"],
        json!([{"id":"1","name":"claude-work","provider":"claude"},{"id":"2","name":"codex-45","provider":"codex"}])
    );
    assert_eq!(
        matrix["data"]["rows"][0],
        json!({"name":"docs","enabled":[false,true]})
    );
    assert_eq!(
        matrix["data"]["rows"][1],
        json!({"name":"files","enabled":[false,true]})
    );
    let sync_all = request(&f, &["cc", "mcp", "sync-all", "on", "--json"], 0);
    assert_eq!(
        sync_all["data"],
        json!({"changed":true,"profile_id":null,"name":null,"warnings":[]})
    );
    assert!(same_shape(&sync_all, &example("mcp.sync-all")));
    let sync = request(&f, &["cc", "mcp", "sync", "--json"], 0);
    assert_eq!(
        sync["data"],
        json!({"changed":false,"profile_id":null,"name":null,"warnings":[]})
    );
    assert!(same_shape(&sync, &example("mcp.sync")));
    // A stale GUI snapshot refuses before resolving the name.
    let stale = request(
        &f,
        &[
            "cc",
            "mcp",
            "remove",
            "absent",
            "--expected-revision",
            "1",
            "--json",
        ],
        1,
    );
    assert_eq!(
        stale,
        json!({"schema":1,"command":"mcp.remove","ok":false,"error":{"code":"conflict","message":"Registry revision or references conflict."}})
    );
    let missing = request(&f, &["cc", "mcp", "remove", "absent", "--json"], 1);
    assert_eq!(missing["error"]["code"], "not_found");
    let removed = request(&f, &["cc", "mcp", "remove", "files", "--json"], 0);
    assert_eq!(
        removed["data"],
        json!({"changed":true,"profile_id":null,"name":"files","warnings":[]})
    );
    assert!(same_shape(&removed, &example("mcp.remove")));
    // Renders exist only below profile-owned directories.
    assert!(
        f.root
            .join("profiles/2/cli/larp-mcp/codex-overrides.json")
            .is_file()
    );
    assert!(f.root.join("profiles/1/cli/larp-mcp/claude.json").is_file());
    // Human mode stays quiet on stderr and prints the same references.
    let human = common::capture(f.command(&["cc", "mcp", "list"])).unwrap();
    assert!(human.status.success() && human.stderr.is_empty());
    assert!(common::stdout(&human).contains("env:DOCS_TOKEN"));
    let help = request(&f, &["cc", "mcp", "add", "--help", "--json"], 0);
    assert_eq!(help["command"], "help");
    // Clean up every synthetic profile; the temp root is removed on drop.
    for server in ["docs", "spaced", "tools"] {
        request(&f, &["cc", "mcp", "remove", server, "--json"], 0);
    }
}

#[test]
fn stage07_matrix_cli_parse_shapes_exit_2_without_echo() {
    let f = common::Fixture::new();
    let exe = command_file(&f);
    for args in [
        vec!["cc", "mcp", "add", "files", "--json"],
        vec![
            "cc",
            "mcp",
            "add",
            "files",
            "--command",
            &exe,
            "--url",
            "https://h.example.com/mcp",
            "--json",
        ],
        vec!["cc", "mcp", "enable", "files", "--json"],
        vec![
            "cc", "mcp", "enable", "files", "codex-45", "--all", "--json",
        ],
        vec!["cc", "mcp", "sync-all", "maybe", "--json"],
    ] {
        let out = request(&f, &args, 2);
        assert_eq!(
            out,
            json!({"schema":1,"command":"parse","ok":false,"error":{"code":"usage","message":"Invalid command or arguments."}})
        );
    }
    // Transport-mismatched references and duplicates are refused by the MCP validator.
    for args in [
        vec![
            "cc",
            "mcp",
            "add",
            "files",
            "--command",
            &exe,
            "--arg=--stdio",
            "--arg=--stdio",
            "--json",
        ],
        vec![
            "cc",
            "mcp",
            "add",
            "files",
            "--url",
            "https://h.example.com/mcp",
            "--env-ref",
            "A",
            "--json",
        ],
        vec![
            "cc",
            "mcp",
            "add",
            "files",
            "--command",
            &exe,
            "--header-ref",
            "A=B",
            "--json",
        ],
        vec![
            "cc",
            "mcp",
            "add",
            "files",
            "--command",
            &exe,
            "--env-ref",
            "A",
            "--env-ref",
            "A",
            "--json",
        ],
    ] {
        assert_eq!(
            request(&f, &args, 2),
            json!({"schema":1,"command":"mcp.add","ok":false,"error":{"code":"usage","message":"MCP definition input is invalid."}})
        );
    }
    assert!(!f.root.exists());
}

#[test]
fn stage07_secrets_cli_refuses_literals_and_references_with_zero_secret_bytes() {
    let f = common::Fixture::new();
    let exe = command_file(&f);
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
    let secret = "InventedSecretValue9xQ7";
    let token = "ghp_InventedToken0123456789abcdef";
    let secret_dir = f.temp.path().join(token);
    common::create_directory(&secret_dir);
    let secret_exe = secret_dir.join("server");
    common::create_file(&secret_exe, b"synthetic\n");
    let secret_exe = secret_exe.to_str().unwrap().to_owned();
    let arg_eq = format!("--arg=--token={secret}");
    let env_eq = format!("TOKEN={secret}");
    let userinfo = format!("https://user:{secret}@docs.example.com/mcp");
    let query = format!("https://docs.example.com/mcp?key={secret}");
    let fragment = format!("https://docs.example.com/mcp#{secret}");
    let path = format!("https://docs.example.com/{secret}/mcp");
    let header = format!("Authorization=Bearer {secret}");
    let header_name = format!("{secret}=DOCS_TOKEN");
    let literal: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--command", &exe, &arg_eq], secret),
        (vec!["--command", &exe, "--arg", secret], secret),
        (vec!["--command", &exe, "--env-ref", &env_eq], secret),
        (vec!["--command", &exe, "--env-ref", token], token),
        (vec!["--command", &secret_exe], token),
        (vec!["--url", &userinfo], secret),
        (vec!["--url", &query], secret),
        (vec!["--url", &fragment], secret),
        (vec!["--url", &path], secret),
        (
            vec![
                "--url",
                "https://docs.example.com/mcp",
                "--header-ref",
                &header,
            ],
            secret,
        ),
        (
            vec![
                "--url",
                "https://docs.example.com/mcp",
                "--header-ref",
                &header_name,
            ],
            secret,
        ),
    ];
    for (extra, needle) in &literal {
        for json in [true, false] {
            let mut args = vec!["cc", "mcp", "add", "files"];
            args.extend(extra);
            if json {
                args.push("--json");
            }
            let out = common::capture(f.command(&args)).unwrap();
            assert_eq!(out.status.code(), Some(1), "{extra:?}");
            let all = [out.stdout.as_slice(), out.stderr.as_slice()].concat();
            assert!(
                !all.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "{extra:?} echoed"
            );
            if json {
                assert!(out.stderr.is_empty());
                assert_eq!(
                    serde_json::from_slice::<Value>(&out.stdout).unwrap(),
                    json!({"schema":1,"command":"mcp.add","ok":false,"error":{"code":"secret_literal","message":"Literal values are refused; use references only."}})
                );
            }
        }
    }
    // Name position too.
    let named = request(
        &f,
        &["cc", "mcp", "add", token, "--command", &exe, "--json"],
        1,
    );
    assert_eq!(named["error"]["code"], "secret_literal");
    for extra in [
        vec!["--command", exe.as_str(), "--env-ref", "1BAD"],
        vec![
            "--url",
            "https://docs.example.com/mcp",
            "--header-ref",
            "Authorization=",
        ],
        vec![
            "--url",
            "https://docs.example.com/mcp",
            "--header-ref",
            "Authorization",
        ],
    ] {
        let mut args = vec!["cc", "mcp", "add", "files"];
        args.extend(&extra);
        args.push("--json");
        assert_eq!(
            request(&f, &args, 1),
            json!({"schema":1,"command":"mcp.add","ok":false,"error":{"code":"secret_reference","message":"A reference is missing, invalid or unsupported."}})
        );
    }
    assert_eq!(
        request(
            &f,
            &[
                "cc",
                "mcp",
                "add",
                "files",
                "--url",
                "http://localhost/mcp",
                "--json"
            ],
            1
        )["error"]["code"],
        "unsupported"
    );
    let relative = request(
        &f,
        &[
            "cc",
            "mcp",
            "add",
            "files",
            "--command",
            "files-mcp",
            "--json",
        ],
        2,
    );
    assert_eq!(relative["error"]["code"], "usage");
    let missing = f.temp.path().join("bin").join("absent-mcp");
    assert_eq!(
        request(
            &f,
            &[
                "cc",
                "mcp",
                "add",
                "files",
                "--command",
                missing.to_str().unwrap(),
                "--json"
            ],
            1
        )["error"]["code"],
        "unsafe_path"
    );
    // Nothing reached the catalog, renders or any other file under the fixture.
    assert!(!f.root.join("mcp.json").exists());
    for needle in [secret, token] {
        let hits: Vec<_> = files_containing(&f.root, needle.as_bytes());
        assert!(hits.is_empty(), "{hits:?}");
    }
    // A valid reference definition stores and renders names only.
    request(
        &f,
        &[
            "cc",
            "mcp",
            "add",
            "files",
            "--command",
            &exe,
            "--env-ref",
            "FILES_TOKEN",
            "--json",
        ],
        0,
    );
    request(
        &f,
        &["cc", "mcp", "enable", "files", "claude-work", "--json"],
        0,
    );
    let render = fs::read_to_string(f.root.join("profiles/1/cli/larp-mcp/claude.json")).unwrap();
    assert!(render.contains("\"FILES_TOKEN\": \"${FILES_TOKEN}\""));
    request(&f, &["cc", "mcp", "remove", "files", "--json"], 0);
}
