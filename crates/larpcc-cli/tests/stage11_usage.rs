mod common;

use common::*;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn usage(fixture: &Fixture, args: &[&str], code: i32) -> Value {
    fixture.larp_json(&[&["cc", "--json"][..], args].concat(), code)
}

fn profiles(fixture: &Fixture) {
    fixture.profile(&["add", "claude-work", "--provider", "claude"], 0);
    fixture.profile(&["add", "codex-45", "--provider", "codex"], 0);
}

#[test]
fn stage11_json_framing_order_selection_and_revision() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let result = usage(&fixture, &["usage"], 0);
    assert_eq!(result["command"], "usage");
    assert_eq!(result["data"]["requested_mode"], "settings");
    let rows = result["data"]["profiles"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "claude-work");
    assert_eq!(
        rows[0]["windows"]["five_hour"]["reason"],
        "capture_not_installed"
    );
    assert_eq!(rows[1]["name"], "codex-45");
    assert_eq!(rows[1]["windows"]["five_hour"]["reason"], "not_captured");
    assert_eq!(
        usage(&fixture, &["usage", "codex-45", "--local"], 0)["data"]["profiles"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        usage(&fixture, &["usage", "absent"], 1)["error"]["code"],
        "not_found"
    );
    assert_eq!(
        usage(
            &fixture,
            &["usage", "absent", "--expected-revision", "0"],
            1
        )["error"]["code"],
        "conflict"
    );
    assert_eq!(
        usage(&fixture, &["usage", "--local", "--live"], 2)["error"]["code"],
        "usage"
    );
}

#[test]
fn stage11_settings_receipts_pruning_and_validation() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let settings = usage(&fixture, &["settings", "usage"], 0);
    assert_eq!(settings["data"]["live"]["state"], "off");
    assert_eq!(settings["data"]["profiles"][0]["claude_statusline"], false);
    assert_eq!(
        settings["data"]["profiles"][1]["claude_statusline"],
        Value::Null
    );

    let add = ["profile", "plan-label", "claude-work", "Max 5x"];
    let label = usage(&fixture, &add, 0);
    assert_eq!(
        label["data"],
        json!({"changed":true,"profile_id":"1","name":"claude-work","warnings":[]})
    );
    let same = usage(&fixture, &add, 0);
    assert_eq!(same["revision"], label["revision"]);
    assert_eq!(same["data"]["changed"], false);
    assert_eq!(
        usage(&fixture, &["usage", "claude-work"], 0)["data"]["profiles"][0]["plan"],
        json!({"label":"Max 5x","source":"user"})
    );
    let bad = usage(
        &fixture,
        &["profile", "plan-label", "claude-work", "private@label"],
        2,
    );
    assert_eq!(bad["error"]["code"], "usage");
    assert!(!bad.to_string().contains("private@label"));
    let bad_flag = fixture.run(
        &[
            "cc",
            "profile",
            "plan-label",
            "claude-work",
            "--private-label-sentinel",
        ],
        2,
    );
    assert!(!stderr(&bad_flag).contains("private-label-sentinel"));
    assert_eq!(
        usage(
            &fixture,
            &["profile", "plan-label", "claude-work", "--clear"],
            0
        )["data"]["changed"],
        true
    );

    let interval = usage(&fixture, &["settings", "usage", "interval", "300"], 0);
    assert_eq!(
        interval["data"],
        json!({"changed":true,"profile_id":null,"name":null,"warnings":[]})
    );
    assert_eq!(
        usage(&fixture, &["settings", "usage", "interval", "299"], 2)["error"]["code"],
        "usage"
    );
    assert_eq!(
        usage(&fixture, &["settings", "usage", "mode", "inherit"], 2)["error"]["code"],
        "usage"
    );
    assert_eq!(
        usage(
            &fixture,
            &["settings", "usage", "claude-statusline", "on"],
            2
        )["error"]["code"],
        "usage"
    );
    assert_eq!(
        usage(
            &fixture,
            &[
                "settings",
                "usage",
                "claude-statusline",
                "on",
                "--profile",
                "codex-45"
            ],
            1
        )["error"]["code"],
        "unsupported"
    );

    usage(&fixture, &add, 0);
    fixture.profile(&["forget", "claude-work", "--yes"], 0);
    usage(
        &fixture,
        &[
            "settings",
            "usage",
            "mode",
            "local",
            "--profile",
            "codex-45",
        ],
        0,
    );
    let persisted: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("usage.json")).unwrap()).unwrap();
    assert!(persisted["profiles"].get("1").is_none());
}

#[test]
fn stage11_closed_ignores_any_credential_input() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let fake = ["fake", "token", "input-sentinel"].join("-");
    let profiles = fixture.root.join("profiles");
    create_file(&profiles.join("1/cli/.credentials.json"), fake.as_bytes());
    create_file(&profiles.join("2/cli/auth.json"), fake.as_bytes());
    fixture.install_larp();
    let before = fixture.outside_snapshot();
    for command in [
        &["usage", "--live"][..],
        &["settings", "usage", "live", "on", "--acknowledge-risk"],
        &["settings", "usage", "live", "off"],
        &["settings", "usage", "mode", "live"],
    ] {
        let result = usage(&fixture, command, 1);
        assert_eq!(result["error"]["code"], "live_disabled");
        assert!(!result.to_string().contains(&fake));
    }
    assert_eq!(
        usage(&fixture, &["settings", "usage", "live", "on"], 2)["error"]["code"],
        "confirmation_required"
    );
    assert_eq!(
        usage(&fixture, &["usage", "--local"], 0)["data"]["profiles"][0]["mode"],
        "local"
    );
    assert_eq!(
        usage(&fixture, &["usage"], 0)["data"]["profiles"][1]["mode"],
        "local"
    );
    assert!(
        fixture.outside_snapshot() == before,
        "usage changed a file outside its root"
    );
    assert!(!fixture.root.join("usage.json").exists());
}

#[test]
fn stage11_closed_remote_reads_force_local_and_mutations_refuse() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let stored = br#"{"version":1,"mode":"live","live":{"enabled":true,"acknowledged_warning":1},"refresh_interval_seconds":null,"profiles":{}}"#;
    let settings = fixture.root.join("usage.json");
    create_file(&settings, stored);
    let fake = ["fake", "token", "persisted-live-sentinel"].join("-");
    let claude = fixture.root.join("profiles/1/cli/.credentials.json");
    let codex = fixture.root.join("profiles/2/cli/auth.json");
    create_file(&claude, fake.as_bytes());
    create_file(&codex, fake.as_bytes());
    let read = usage(&fixture, &["settings", "usage"], 0);
    assert_eq!(read["data"]["live"]["state"], "needs_acknowledgement");
    assert!(read["data"]["live"]["acknowledged_warning"].is_null());
    assert_eq!(fs::read(&settings).unwrap(), stored);
    let denied = usage(&fixture, &["usage", "--live"], 1);
    assert_eq!(denied["error"]["code"], "live_disabled");
    assert!(!read.to_string().contains(&fake) && !denied.to_string().contains(&fake));
    let remote = usage(&fixture, &["usage", "--remote-request"], 0);
    assert_eq!(remote["data"]["requested_mode"], "local");
    assert!(
        remote["data"]["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["mode"] == "local")
    );
    for command in [
        &["usage", "--live"][..],
        &["settings", "usage", "live", "on", "--acknowledge-risk"],
        &["settings", "usage", "live", "off"],
        &["settings", "usage", "mode", "local"],
        &["settings", "usage", "interval", "off"],
        &[
            "settings",
            "usage",
            "claude-statusline",
            "on",
            "--profile",
            "claude-work",
        ],
        &["profile", "plan-label", "codex-45", "Plus"],
    ] {
        let result = usage(&fixture, &[command, &["--remote-request"]].concat(), 2);
        assert_eq!(result["error"]["code"], "unsupported");
    }
    assert_eq!(fs::read(&settings).unwrap(), stored);
    assert_eq!(fs::read(claude).unwrap(), fake.as_bytes());
    assert_eq!(fs::read(codex).unwrap(), fake.as_bytes());
    assert!(!fixture.root.join("bin/.larp-usage-hook").exists());
}

#[test]
fn stage11_json_tombstones_are_excluded_and_refused_by_name() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let path = fixture.root.join("registry.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["profiles"][0]["state"] = json!("deleting");
    state["profiles"][0]["deletion_transaction"] = json!("delete-1");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let list = usage(&fixture, &["usage"], 0);
    assert_eq!(list["data"]["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(list["data"]["profiles"][0]["name"], "codex-45");
    assert_eq!(
        usage(&fixture, &["usage", "claude-work"], 1)["error"]["code"],
        "recovery_required"
    );
}

#[test]
fn stage11_statusline_check_refuses_untrusted_settings() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let file = fixture.root.join("profiles/1/cli/settings.json");
    let check = [
        "settings",
        "usage",
        "claude-statusline",
        "on",
        "--profile",
        "claude-work",
    ];
    for input in [
        "{",
        "[1]",
        "{/* comment */}",
        "{\"key\":1,}",
        "{\"key\":1,\"key\":2}",
        "{} {}",
    ] {
        create_file(&file, input.as_bytes());
        let result = usage(&fixture, &check, 1);
        assert_eq!(
            result["error"]["code"], "statusline_unverifiable",
            "{input}"
        );
        fs::remove_file(&file).unwrap();
    }
    create_file(&file, &vec![b' '; 1024 * 1024 + 1]);
    assert_eq!(
        usage(&fixture, &check, 1)["error"]["code"],
        "statusline_unverifiable"
    );
    fs::remove_file(&file).unwrap();
    create_file(&file, br#"{"statusLine":null}"#);
    assert_eq!(
        usage(&fixture, &check, 1)["error"]["code"],
        "statusline_exists"
    );
    assert!(!fixture.root.join("usage.json").exists());
    fs::remove_file(&file).unwrap();
    assert_eq!(usage(&fixture, &check, 0)["data"]["changed"], true);
    assert!(!fixture.root.join("profiles/1/usage").exists());
}

fn rollout(directory: &Path, name: &str, contents: &[u8]) {
    create_file(&directory.join(name), contents);
}

const THREAD: &str = "01234567-89ab-cdef-0123-456789abcdef";
const REVERT: &str = "89abcdef-0123-4567-89ab-cdef01234567";

fn codex_directory(fixture: &Fixture) -> std::path::PathBuf {
    let mut path = fixture.root.join("profiles/2/cli");
    for component in ["sessions", "2026", "11", "21"] {
        path.push(component);
        create_directory(&path);
    }
    path
}

fn event(at: &str, bucket: Option<&str>, primary: Value, secondary: Value, plan: &str) -> String {
    json!({
        "timestamp": at, "type":"event_msg", "payload":{
            "type":"token_count",
            "info":{"conversation":"private conversation sentinel"},
            "rate_limits":{"limit_id":bucket,"limit_name":"private bucket label sentinel",
                "primary":primary,"secondary":secondary,"plan_type":plan}
        }
    })
    .to_string()
}

fn window(used: f64, minutes: i64, reset: Option<i64>) -> Value {
    json!({"used_percent": used, "window_minutes": minutes, "resets_at":reset})
}

fn log(version: &str, lines: &[String]) -> String {
    let mut output = format!(
        "{}\n",
        json!({"type":"session_meta","payload":{"cli_version":version,
        "creator":"private creator sentinel"}})
    );
    for line in lines {
        output.push_str(line);
        output.push('\n');
    }
    output
}

#[test]
fn stage11_local_codex_filename_buckets_versions_and_redaction() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let directory = codex_directory(&fixture);
    let stamp = "2026-11-21T12:00:00.280Z";
    let old = event(
        "2026-11-21T10:00:00Z",
        Some("private-special-bucket"),
        window(12.0, 300, None),
        Value::Null,
        "unknown",
    );
    let stale = event(
        "2026-11-21T10:30:00Z",
        Some("other-special-bucket"),
        window(22.0, 300, Some(1_600_000_000)),
        Value::Null,
        "unknown",
    );
    let recent = event(
        stamp,
        None,
        window(75.0, 10080, None),
        window(29.0, 300, None),
        "pro",
    );
    rollout(
        &directory,
        &format!("rollout-2026-11-21T11-00-00-{THREAD}.jsonl"),
        log("0.169.0", &[old, stale]).as_bytes(),
    );
    let mut newest = log("0.169.0", &[recent]);
    newest.push_str(&event(
        "2026-11-21T13:00:00Z",
        None,
        window(99.0, 300, None),
        Value::Null,
        "unknown",
    ));
    rollout(
        &directory,
        &format!("rollout-2026-11-21T12-00-00-{THREAD}_{REVERT}.jsonl"),
        newest.as_bytes(),
    );
    for name in [
        "rollout-2026-11-21T23-59-59-noncanonical.jsonl",
        "rollout-2026-11-21T23-59-59-01234567-89ab-cdef-0123-456789abcdef.jsonl.zst",
    ] {
        rollout(&directory, name, b"malformed, must not read");
    }
    let result = usage(&fixture, &["usage", "codex-45"], 0);
    let row = &result["data"]["profiles"][0];
    assert_eq!(row["source"], "codex_session_log");
    assert_eq!(row["client_version"], "0.169.0");
    assert_eq!(row["plan"], json!({"label":"pro","source":"client_local"}));
    let windows = &row["windows"];
    assert_eq!(windows["five_hour"]["used_percent"], 29.0);
    assert_eq!(windows["five_hour"]["captured_at"], "2026-11-21T12:00:00Z");
    assert_eq!(windows["weekly"]["used_percent"], 75.0);
    assert_eq!(windows["extra"][0]["label"], "Other limit 1 5h");
    assert_eq!(windows["extra"][0]["reason"], "reset_since_capture");
    assert!(windows["extra"][0]["used_percent"].is_null());
    assert_eq!(windows["extra"][1]["captured_at"], "2026-11-21T10:00:00Z");
    assert_eq!(windows["extra"][1]["label"], "Other limit 2 5h");
    for sentinel in [
        "private conversation sentinel",
        "private creator sentinel",
        "private bucket label sentinel",
        "private-special-bucket",
        "other-special-bucket",
    ] {
        assert!(!result.to_string().contains(sentinel));
    }
    assert!(!fixture.root.join("usage.json").exists());
    usage(
        &fixture,
        &["profile", "plan-label", "codex-45", "Codex 45"],
        0,
    );
    assert_eq!(
        usage(&fixture, &["usage", "codex-45"], 0)["data"]["profiles"][0]["plan"]["source"],
        "user"
    );
    usage(
        &fixture,
        &["profile", "plan-label", "codex-45", "--clear"],
        0,
    );
    rollout(
        &directory,
        &format!("rollout-2026-11-21T13-00-00-{THREAD}.jsonl"),
        log(
            "0.169.0",
            &[event(
                "2026-11-21T13:00:00Z",
                None,
                window(33.0, 300, None),
                Value::Null,
                "free_workspace",
            )],
        )
        .as_bytes(),
    );
    assert!(usage(&fixture, &["usage", "codex-45"], 0)["data"]["profiles"][0]["plan"].is_null());
    rollout(
        &directory,
        &format!("rollout-2026-11-21T14-00-00-{THREAD}.jsonl"),
        log(
            "0.170.0",
            &[event(
                "2026-11-21T14:00:00Z",
                None,
                window(35.0, 300, None),
                Value::Null,
                "pro",
            )],
        )
        .as_bytes(),
    );
    let row = &usage(&fixture, &["usage", "codex-45"], 0)["data"]["profiles"][0];
    assert_eq!(row["windows"]["five_hour"]["reason"], "unverified_version");
    assert!(row["plan"].is_null());
}

#[test]
fn stage11_json_status_reads_the_selected_profile_locally() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let directory = codex_directory(&fixture);
    rollout(
        &directory,
        &format!("rollout-2026-11-21T11-00-00-{THREAD}.jsonl"),
        log(
            "0.169.0",
            &[event(
                "2026-11-21T11:00:00Z",
                None,
                window(21.0, 300, Some(1_600_000_000)),
                window(15.0, 10080, None),
                "plus",
            )],
        )
        .as_bytes(),
    );
    let stored = br#"{"version":1,"mode":"live","live":{"enabled":true,"acknowledged_warning":1},"refresh_interval_seconds":null,"profiles":{}}"#;
    create_file(&fixture.root.join("usage.json"), stored);
    let usage_result = usage(&fixture, &["usage", "codex-45", "--local"], 0);
    let status = usage(
        &fixture,
        &["status", "codex-45", "--expected-profile-id", "2"],
        0,
    );
    assert_eq!(status["command"], "status.account");
    assert_eq!(status["data"]["usage"], usage_result["data"]["profiles"][0]);
    assert_eq!(
        status["data"]["usage"]["windows"]["five_hour"]["reason"],
        "reset_since_capture"
    );
    assert!(status["data"]["account"]["plan"].is_null());
    assert_eq!(
        usage(
            &fixture,
            &["status", "codex-45", "--expected-profile-id", "1"],
            1
        )["error"]["code"],
        "conflict"
    );
    let metadata = usage(&fixture, &["status", "codex-45", "--no-account"], 0);
    assert_eq!(metadata["command"], "status");
    assert!(metadata["data"].get("usage").is_none());
}

#[test]
fn stage11_local_codex_scan_caps() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let directory = codex_directory(&fixture);
    let request = || usage(&fixture, &["usage", "codex-45"], 0);
    let capped = |result: Value| {
        assert!(
            result["data"]["profiles"][0]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "scan_limit")
        )
    };
    let name = format!("rollout-2026-11-21T23-59-59-{THREAD}.jsonl");
    rollout(&directory, &name, &vec![b' '; 1024 * 1024 + 1]);
    capped(request());
    fs::remove_file(directory.join(&name)).unwrap();
    let mut lines = log("0.169.0", &[]);
    lines.push_str(&"{\"type\":\"ignored\"}\n".repeat(20_000));
    rollout(&directory, &name, lines.as_bytes());
    capped(request());
    fs::remove_file(directory.join(&name)).unwrap();
    for second in 1..=9 {
        rollout(
            &directory,
            &format!("rollout-2026-11-21T11-00-{second:02}-{THREAD}.jsonl"),
            log("0.169.0", &[]).as_bytes(),
        );
    }
    capped(request());
}

#[test]
fn stage11_local_codex_refuses_hardlink_and_symlink() {
    let fixture = Fixture::new();
    profiles(&fixture);
    let directory = codex_directory(&fixture);
    let leaf = directory.join(format!("rollout-2026-11-21T11-00-00-{THREAD}.jsonl"));
    let outside = fixture.temp.path().join("outside-rollout");
    create_file(
        &outside,
        log(
            "0.169.0",
            &[event(
                "2026-11-21T09:00:00Z",
                None,
                window(25.0, 300, None),
                Value::Null,
                "pro",
            )],
        )
        .as_bytes(),
    );
    let refused = || {
        assert_eq!(
            usage(&fixture, &["usage", "codex-45"], 0)["data"]["profiles"][0]["windows"]["five_hour"]
                ["reason"],
            "invalid_record"
        )
    };
    fs::hard_link(&outside, &leaf).unwrap();
    refused();
    fs::remove_file(&leaf).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, &leaf).unwrap();
        refused();
    }
}

#[test]
fn stage11_settings_recovers_each_journal_boundary() {
    use sha2::{Digest, Sha256};
    for phase in 0..4 {
        let fixture = Fixture::new();
        profiles(&fixture);
        let settings = fixture.root.join("usage.json");
        let next = fixture.root.join("usage.json.next");
        let journal = fixture.root.join("share-journal.json");
        let registry = fixture.root.join("registry.json");
        let expected = br#"{"version":1,"mode":"local","live":{"enabled":false,"acknowledged_warning":null},"refresh_interval_seconds":300,"profiles":{}}"#;
        let mut state: Value = serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
        let base = state["revision"].as_u64().unwrap();
        let target = base + 1;
        create_file(&next, expected);
        if phase >= 1 {
            create_file(
                &journal,
                json!({"kind":"usage_settings","base":base,
                "target":target,"sha256":format!("{:x}", Sha256::digest(expected))})
                .to_string()
                .as_bytes(),
            );
        }
        if phase >= 2 {
            state["revision"] = json!(target);
            fs::write(&registry, serde_json::to_vec(&state).unwrap()).unwrap();
        }
        if phase == 3 {
            fs::rename(&next, &settings).unwrap();
        }
        let stale = if phase >= 2 { base } else { target }.to_string();
        let command: &[&str] = if phase % 2 == 0 {
            &["profile", "list"]
        } else {
            &["settings", "usage"]
        };
        assert_eq!(
            usage(
                &fixture,
                &[command, &["--expected-revision", &stale]].concat(),
                1
            )["error"]["code"],
            "conflict"
        );
        assert!(!next.exists());
        assert!(!journal.exists());
        let current = usage(&fixture, &["settings", "usage"], 0);
        assert_eq!(
            current["revision"],
            if phase >= 2 { target } else { base }.to_string()
        );
        assert_eq!(
            current["data"]["refresh_interval_seconds"],
            if phase >= 2 { json!(300) } else { Value::Null }
        );
    }
}

#[cfg(unix)]
#[test]
fn stage11_local_claude_hook_dispatch_is_silent_and_confined() {
    use std::{
        io::Write,
        process::Stdio,
        time::{SystemTime, UNIX_EPOCH},
    };
    let fixture = Fixture::new();
    profiles(&fixture);
    usage(
        &fixture,
        &[
            "settings",
            "usage",
            "claude-statusline",
            "on",
            "--profile",
            "claude-work",
        ],
        0,
    );
    let hook = fixture.root.join("bin/.larp-usage-hook");
    assert!(hook.is_file());
    let closed = fixture.run(&["cc", "run", "claude-work"], 1);
    assert!(stderr(&closed).contains("provider operation is disabled for this combination"));
    assert!(
        !fixture
            .root
            .join("profiles/1/usage/claude-settings.json")
            .exists()
    );
    let home = fixture.root.join("profiles/1/cli");
    let capture = fixture.root.join("profiles/1/usage/claude-statusline.json");
    let run = |args: &[&str], input: &[u8], set_expected: bool| {
        let mut command = fixture.command_for(&hook, args);
        if set_expected {
            command.env("CLAUDE_CONFIG_DIR", &home);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn(&mut command).unwrap();
        if !input.is_empty() {
            // Refusals may close the pipe before the fixture finishes sending.
            let _ = child.stdin.take().unwrap().write_all(input);
        }
        drop(child.stdin.take());
        let result = wait_output(child).unwrap();
        assert_eq!(result.status.code(), Some(0));
        assert!(result.stdout.is_empty() && result.stderr.is_empty());
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let data = json!({"version":"2.1.325", "session_id":"private session sentinel",
        "rate_limits":{"five_hour":{"used_percentage":42.8,"resets_at":now+7200}}});
    let bytes = serde_json::to_vec(&data).unwrap();
    run(&["--help"], &[], true);
    run(&["claude-statusline", "01"], &bytes, true);
    run(&["claude-statusline", "1"], &bytes, false);
    let mut oversized = bytes.clone();
    oversized.resize(256 * 1024 + 1, b' ');
    run(&["claude-statusline", "1"], &oversized, true);
    assert!(!capture.exists());
    run(&["claude-statusline", "1"], &bytes, true);
    let before = fs::read(&capture).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains("sentinel"));
    run(
        &["claude-statusline", "1"],
        br#"{"version":"2.1.325"}"#,
        true,
    );
    assert_eq!(fs::read(&capture).unwrap(), before);
    let row = &usage(&fixture, &["usage", "claude-work", "--local"], 0)["data"]["profiles"][0];
    assert_eq!(row["source"], "claude_statusline");
    assert_eq!(row["windows"]["five_hour"]["used_percent"], 42.8);
    assert!(row["windows"]["extra"].is_null());
}
