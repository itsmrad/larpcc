#![cfg(all(feature = "synthetic-launch", unix))]
//! Stage 08 doctor and version-only probe. Every descriptor here is INVENTED and
//! injected only through the nonpackaged synthetic driver; the fake provider picks
//! its behavior from the descriptor's exact argv because the probe env is clean.
mod common;
use common::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

fn setup() -> (Fixture, PathBuf, String) {
    let f = Fixture::new();
    let fake = f.temp.path().join("native-fake");
    fs::copy(env!("CARGO_BIN_EXE_larpcc-fake-provider"), &fake).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    f.run(
        &["cc", "provider", "bind", "codex", fake.to_str().unwrap()],
        0,
    );
    f.profile(&["add", "codex-45", "--provider", "codex"], 0);
    let digest = format!("{:x}", Sha256::digest(fs::read(&fake).unwrap()));
    (f, fake, digest)
}

fn driver(f: &Fixture, descriptors: &str, args: &[&str]) -> Command {
    let mut command = f.command_for(
        Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
        &[&["cc", "doctor"][..], args].concat(),
    );
    // Ambient values that must never reach the probe child.
    let key = ["sk", "ant", "invented", "ambient"].join("-"); // assembled: not a credential literal
    command
        .env("LARPCC_SYNTHETIC_VERSION_DESCRIPTORS", descriptors)
        .env("LARPCC_TEST_VALUE", "ambient-sentinel")
        .env("ANTHROPIC_API_KEY", &key)
        .env("OPENAI_API_KEY", &key)
        .env("PATH", "/ambient/bin");
    command
}

fn descriptor(digest: &str, mode: &str) -> String {
    format!("codex|{digest}|invented-version-{mode}|invented-cli | (fixture)")
}

fn doctor_json(f: &Fixture, descriptors: &str, args: &[&str], code: i32) -> serde_json::Value {
    let args = [args, &["--json"]].concat();
    let value = envelope(&exec(driver(f, descriptors, &args), code));
    assert_eq!(value["command"], "doctor");
    value
}

fn record(fake: &Path, mode: &str) -> Option<serde_json::Value> {
    let path = fake.with_extension(format!("probe-{mode}"));
    fs::read(path)
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
}

fn assert_clean(f: &Fixture) {
    assert!(
        !f.root.join("probes").exists(),
        "temporary probe home was not removed"
    );
}

fn cli_binding(value: &serde_json::Value) -> &serde_json::Value {
    let detections = value["data"]["detected_versions"].as_array().unwrap();
    &detections
        .iter()
        .find(|d| d["provider"] == "codex" && d["surface"] == "cli")
        .unwrap()["binding"]
}

#[test]
fn stage08_doctor_matching_descriptor_detects_version_with_clean_env() {
    let (f, fake, digest) = setup();
    let value = doctor_json(
        &f,
        &descriptor(&digest, "ok"),
        &["codex-45", "--detect-versions"],
        0,
    );
    assert_eq!(cli_binding(&value)["detected_version"], "1.2.3");
    assert_eq!(cli_binding(&value)["qualified"], false);
    assert_eq!(value["data"]["diagnostics"], serde_json::json!([]));
    assert_eq!(value["data"]["native_rows"], serde_json::json!([]));
    assert_eq!(value["data"]["native_auth"], "not_observed");
    let record = record(&fake, "ok").expect("probe child did not run");
    let env = record["env"].as_object().unwrap();
    let mut keys: Vec<_> = env.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "APPDATA",
            "CODEX_HOME",
            "HOME",
            "LOCALAPPDATA",
            "PATH",
            "TMPDIR",
            "USERPROFILE",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME"
        ],
        "child env must be exactly the explicit clean set"
    );
    let home = PathBuf::from(env["HOME"].as_str().unwrap());
    assert!(home.starts_with(f.root.join("probes")), "{home:?}");
    assert!(home.ends_with("profiles/codex-45/home"), "{home:?}");
    assert_eq!(
        PathBuf::from(env["CODEX_HOME"].as_str().unwrap()),
        home.parent().unwrap().join("cli")
    );
    assert_eq!(env["PATH"], "/usr/bin:/bin");
    assert_eq!(record["cwd"], env["HOME"]);
    assert_eq!(record["args"], serde_json::json!(["invented-version-ok"]));
    assert_eq!(record["stdin_eof"], true);
    // The child wrote into both homes; the whole temporary LARPCC_HOME is gone.
    assert!(!home.exists());
    assert_clean(&f);
}

#[test]
fn stage08_doctor_no_flag_never_probes() {
    let (f, fake, digest) = setup();
    for args in [&["codex-45"][..], &[]] {
        let value = doctor_json(&f, &descriptor(&digest, "ok"), args, 0);
        assert_eq!(
            cli_binding(&value)["detected_version"],
            serde_json::Value::Null
        );
        assert_eq!(value["data"]["diagnostics"], serde_json::json!([]));
    }
    assert!(
        record(&fake, "ok").is_none(),
        "doctor probed without the flag"
    );
    assert!(!f.root.join("probes").exists());
}

#[test]
fn stage08_doctor_without_profile_reports_all_bindings() {
    let (f, _fake, digest) = setup();
    let value = doctor_json(&f, &descriptor(&digest, "ok"), &["--detect-versions"], 0);
    assert_eq!(value["data"]["profile"], serde_json::Value::Null);
    let order: Vec<_> = value["data"]["detected_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            format!(
                "{}/{}",
                d["provider"].as_str().unwrap(),
                d["surface"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        order,
        ["claude/cli", "claude/desktop", "codex/cli", "codex/desktop"]
    );
    assert_eq!(cli_binding(&value)["detected_version"], "1.2.3");
    assert_clean(&f);
}

#[test]
fn stage08_doctor_unknown_digest_is_null_without_spawn() {
    let (f, fake, _digest) = setup();
    let zero = "0".repeat(64);
    let value = doctor_json(
        &f,
        &descriptor(&zero, "ok"),
        &["codex-45", "--detect-versions"],
        0,
    );
    assert_eq!(
        cli_binding(&value)["detected_version"],
        serde_json::Value::Null
    );
    assert_eq!(
        value["data"]["diagnostics"],
        serde_json::json!([{"code":"version_unapproved","message":"No reviewed version probe matches the codex executable."}])
    );
    assert!(record(&fake, "ok").is_none());
    assert_clean(&f);
}

#[test]
fn stage08_doctor_shipped_table_is_unapproved() {
    let (f, fake, _digest) = setup();
    let value = f.larp_json(
        &["cc", "doctor", "codex-45", "--detect-versions", "--json"],
        0,
    );
    assert_eq!(
        cli_binding(&value)["detected_version"],
        serde_json::Value::Null
    );
    assert_eq!(
        value["data"]["diagnostics"][0]["code"],
        "version_unapproved"
    );
    assert!(record(&fake, "ok").is_none());
}

#[test]
fn stage08_doctor_rejected_output_is_null_with_fixed_diagnostic() {
    let (f, fake, digest) = setup();
    for mode in ["stderr", "nonzero", "oversize", "grammar", "lines"] {
        let value = doctor_json(
            &f,
            &descriptor(&digest, mode),
            &["codex-45", "--detect-versions"],
            0,
        );
        assert_eq!(
            cli_binding(&value)["detected_version"],
            serde_json::Value::Null,
            "{mode}"
        );
        assert_eq!(
            value["data"]["diagnostics"],
            serde_json::json!([{"code":"version_probe_failed","message":"The codex version probe returned no accepted version."}]),
            "{mode}"
        );
        let text = serde_json::to_string(&value).unwrap();
        assert!(!text.contains("noise") && !text.contains("xxxx"), "{mode}");
        assert!(record(&fake, mode).is_some(), "{mode} did not run");
        assert_clean(&f);
    }
}

#[test]
fn stage08_doctor_timeout_kills_and_reaps_direct_child() {
    let (f, fake, digest) = setup();
    let start = Instant::now();
    let value = doctor_json(
        &f,
        &descriptor(&digest, "sleep"),
        &["codex-45", "--detect-versions"],
        0,
    );
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_secs(5), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(9), "{elapsed:?}");
    assert_eq!(
        cli_binding(&value)["detected_version"],
        serde_json::Value::Null
    );
    assert_eq!(
        value["data"]["diagnostics"][0]["code"],
        "version_probe_failed"
    );
    let pid = record(&fake, "sleep").unwrap()["pid"].as_i64().unwrap() as libc::pid_t;
    // SAFETY: signal 0 only checks existence of the reaped child's former PID.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    assert!(!alive, "timed-out probe child is still running");
    assert_clean(&f);
}

#[test]
fn stage08_doctor_cleanup_failure_is_operational() {
    let (f, fake, digest) = setup();
    let value = doctor_json(
        &f,
        &descriptor(&digest, "stuck"),
        &["codex-45", "--detect-versions"],
        1,
    );
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "io");
    // Restore the child's read-only directory, then remove the leftover ourselves.
    let record = record(&fake, "stuck").unwrap();
    let stuck = PathBuf::from(record["env"]["HOME"].as_str().unwrap()).join("stuck");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir_all(f.root.join("probes")).unwrap();
}

#[test]
fn stage08_doctor_human_output() {
    let (f, _fake, digest) = setup();
    let output = exec(
        driver(
            &f,
            &descriptor(&digest, "ok"),
            &["codex-45", "--detect-versions"],
        ),
        0,
    );
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.starts_with("Profile: codex-45 (ID 1, codex)\n"),
        "{text}"
    );
    assert!(text.contains("\n  detected version: 1.2.3\n"), "{text}");
    assert!(text.contains("Native auth: not observed\n"), "{text}");
    assert!(text.ends_with("Diagnostics: none\n"), "{text}");
    let output = exec(driver(&f, &descriptor(&digest, "ok"), &["codex-45"]), 0);
    assert!(stdout(&output).contains("\n  detected version: not probed\n"));
    assert_clean(&f);
}

#[test]
fn stage08_doctor_crash_leftover_never_wedges_registry_operations() {
    let (f, fake, digest) = setup();
    // A probe interrupted before its finally: an inert, unregistered tree under probes/.
    let leftover = f.root.join("probes").join("1-0-0");
    let home = leftover.join("profiles").join("codex-45").join("home");
    for dir in [
        f.root.join("probes"),
        leftover.clone(),
        leftover.join("profiles"),
        home.parent().unwrap().to_owned(),
        home.clone(),
    ] {
        create_directory(&dir);
    }
    create_file(&home.join("state"), b"synthetic leftover\n");
    f.profile(&["list"], 0);
    f.profile(&["add", "claude-work", "--provider", "claude"], 0);
    f.profile(&["show", "claude-work"], 0);
    // Delete succeeds with its fixed Keychain warning on stderr.
    let deleted = exec(
        f.command(&["cc", "profile", "delete", "claude-work", "--yes"]),
        0,
    );
    assert!(stderr(&deleted).starts_with("warning: Keychain items"));
    f.profile(&["migrate"], 0);
    f.run(&["cc", "provider", "list"], 0);
    let value = doctor_json(
        &f,
        &descriptor(&digest, "ok"),
        &["codex-45", "--detect-versions"],
        0,
    );
    assert_eq!(cli_binding(&value)["detected_version"], "1.2.3");
    assert!(record(&fake, "ok").is_some());
    // The new probe removed only its own tree; the leftover stays inert.
    let names: Vec<_> = fs::read_dir(f.root.join("probes"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["1-0-0"]);
    assert!(home.join("state").exists());
}

#[test]
fn stage08_doctor_expected_revision_is_checked_before_probe() {
    let (f, fake, digest) = setup();
    let value = doctor_json(
        &f,
        &descriptor(&digest, "ok"),
        &["codex-45", "--detect-versions", "--expected-revision", "99"],
        1,
    );
    assert_eq!(value["error"]["code"], "conflict");
    assert!(record(&fake, "ok").is_none());
    assert_clean(&f);
}
