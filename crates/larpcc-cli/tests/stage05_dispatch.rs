#![cfg(any(unix, feature = "synthetic-launch"))]
mod common;
use common::*;
#[cfg(unix)]
use serde_json::{Value, json};
#[cfg(unix)]
use std::ffi::OsString;
use std::fs;
#[cfg(any(unix, feature = "synthetic-launch"))]
use std::path::Path;

#[cfg(unix)]
const PENDING: &str = "error: provider operation is disabled for this combination\nhint: review the documented qualification matrix\n";
#[cfg(unix)]
const INVALID: &str = "error: alias launcher, receipt or mapping cannot be verified\n";

#[cfg(unix)]
fn example(command: &str) -> Value {
    let all: Vec<Value> =
        serde_json::from_str(include_str!("../../../docs/contracts/cli-v1.examples.json")).unwrap();
    all.into_iter().find(|e| e["command"] == command).unwrap()
}
// Same keys and JSON kinds at every object level; values are fixture specific.
#[cfg(unix)]
fn same_shape(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (k, v) in a {
                same_shape(v, &b[k]);
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if let (Some(a), Some(b)) = (a.first(), b.first()) {
                same_shape(a, b);
            }
        }
        _ => assert_eq!(
            std::mem::discriminant(a),
            std::mem::discriminant(b),
            "{a} vs {b}"
        ),
    }
}
#[cfg(unix)]
fn alias(f: &Fixture, name: &str, id: &str, surface: &str, state: &str) -> Value {
    json!({"name":name,"profile_id":id,"surface":surface,
        "path":f.launcher(name).to_str().unwrap(),"state":state})
}
#[cfg(any(unix, feature = "synthetic-launch"))]
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
    f.larp_json(
        &[
            "cc",
            "profile",
            "add",
            "codex-45",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    f
}
// The launcher never consults ambient LARPCC_HOME: point it at an unrelated root.
#[cfg(unix)]
fn launch(f: &Fixture, name: &str, args: &[OsString], code: i32) -> std::process::Output {
    let mut command = f.command_for(&f.launcher(name), &[]);
    command
        .args(args)
        .env("LARPCC_HOME", f.temp.path().join("ambient"));
    let output = exec(command, code);
    assert!(output.stdout.is_empty(), "{}", stdout(&output));
    assert!(!f.temp.path().join("ambient").exists());
    output
}

#[cfg(unix)]
#[test]
fn stage05_dispatch_arbitrary_names_providers_and_contract_shapes() {
    let f = setup();
    // Names are not provider-shaped: either name maps to either provider by immutable ID.
    let added = f.larp_json(
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
        0,
    );
    assert_eq!(
        added,
        json!({"schema":1,"command":"alias.add","ok":true,"revision":"3",
            "data":{"changed":true,"profile_id":"1","name":"codex-45","warnings":[]}})
    );
    same_shape(&added, &example("alias.add"));
    f.larp_json(
        &["cc", "alias", "add", "claude-work", "codex-45", "--json"],
        0,
    );
    f.larp_json(
        &[
            "cc",
            "alias",
            "add",
            "a-first",
            "codex-45",
            "--desktop",
            "--json",
        ],
        0,
    );
    let list = f.larp_json(&["cc", "alias", "list", "--json"], 0);
    same_shape(&list, &example("alias.list"));
    assert_eq!(list["revision"], "5");
    assert_eq!(
        list["data"]["aliases"],
        json!([
            alias(&f, "a-first", "2", "desktop", "ok"),
            alias(&f, "claude-work", "2", "cli", "ok"),
            alias(&f, "codex-45", "1", "cli", "ok")
        ])
    );
    let show = f.larp_json(&["cc", "profile", "show", "codex-45", "--json"], 0);
    same_shape(&show, &example("profile.show"));
    assert_eq!(
        show["data"]["aliases"],
        json!([
            alias(&f, "a-first", "2", "desktop", "ok"),
            alias(&f, "claude-work", "2", "cli", "ok")
        ])
    );
    let listed = f.larp_json(&["cc", "profile", "list", "--json"], 0);
    assert_eq!(
        listed["data"]["profiles"][0]["aliases"],
        json!([alias(&f, "codex-45", "1", "cli", "ok")])
    );
    let removed = f.larp_json(&["cc", "alias", "remove", "claude-work", "--json"], 0);
    assert_eq!(
        removed,
        json!({"schema":1,"command":"alias.remove","ok":true,"revision":"6",
            "data":{"changed":true,"profile_id":"2","name":"claude-work","warnings":[]}})
    );
    same_shape(&removed, &example("alias.remove"));
    assert!(!f.launcher("claude-work").exists());
    assert!(!f.alias_receipt("claude-work").exists());
    let human = exec(
        f.command_for(&f.install_larp(), &["cc", "alias", "list"]),
        0,
    );
    assert!(human.stderr.is_empty());
    assert!(stdout(&human).starts_with("a-first\tprofile id 2\tdesktop\tok\t"));
    for name in ["a-first", "codex-45"] {
        f.larp_json(&["cc", "alias", "remove", name, "--json"], 0);
    }
    let human = exec(
        f.command_for(&f.install_larp(), &["cc", "alias", "list"]),
        0,
    );
    assert_eq!(stdout(&human), "No aliases.\n");
}

#[cfg(unix)]
#[test]
fn stage05_dispatch_root_pinned_argv_forwarding_rename_and_exit() {
    let f = setup();
    f.larp_json(
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
        0,
    );
    let mut args: Vec<OsString> = ["--json", "a b", "cc", "profile", "list", "--", "\"q\" \\"]
        .iter()
        .map(OsString::from)
        .collect();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        args.push(OsString::from_vec(b"non-\xff-unicode".to_vec()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        args.push(OsString::from_wide(&[0x61, 0xd800, 0x62]));
    }
    // Literal --json and cc grammar are vendor argv: still the human qualification refusal.
    assert_eq!(stderr(&launch(&f, "codex-45", &args, 1)), PENDING);
    assert_eq!(stderr(&launch(&f, "codex-45", &[], 1)), PENDING);
    // Rename keeps the ID mapping; reuse of the old name cannot capture the alias.
    f.larp_json(
        &[
            "cc",
            "profile",
            "rename",
            "claude-work",
            "renamed",
            "--json",
        ],
        0,
    );
    f.larp_json(
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
    assert_eq!(stderr(&launch(&f, "codex-45", &args, 1)), PENDING);
    let list = f.larp_json(&["cc", "alias", "list", "--json"], 0);
    assert_eq!(list["data"]["aliases"][0]["profile_id"], "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Exact argv[0]: another alias name for the same image refuses.
        let mut command = f.command_for(&f.launcher("codex-45"), &[]);
        command.arg0("renamed");
        assert!(stderr(&exec(command, 1)).starts_with(INVALID));
        // A launcher started as `larp` never reaches cc grammar or JSON mode.
        let mut command = f.command_for(
            &f.launcher("codex-45"),
            &["cc", "profile", "list", "--json"],
        );
        command.arg0("larp");
        let output = exec(command, 1);
        assert!(output.stdout.is_empty());
        assert!(stderr(&output).starts_with(INVALID));
    }
}

#[cfg(unix)]
#[test]
fn stage05_dispatch_copies_and_moves_refuse() {
    let f = setup();
    f.larp_json(
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
        0,
    );
    // Copying a launcher elsewhere (same basename) cannot select a registry.
    let elsewhere = f.temp.path().join("elsewhere");
    create_directory(&elsewhere);
    create_directory(&elsewhere.join("bin"));
    let copy = elsewhere
        .join("bin")
        .join(format!("codex-45{}", std::env::consts::EXE_SUFFIX));
    copy_executable(&f.launcher("codex-45"), &copy);
    create_file(
        &elsewhere.join("bin/codex-45.larp-alias.json"),
        &fs::read(f.alias_receipt("codex-45")).unwrap(),
    );
    let output = exec(f.command_for(&copy, &[]), 1);
    assert!(output.stdout.is_empty() && stderr(&output).starts_with(INVALID));
    // Moved root: the receipt pins the absolute root, so the launcher refuses there.
    let moved = f.temp.path().join("moved");
    fs::rename(&f.root, &moved).unwrap();
    let launcher = moved
        .join("bin")
        .join(format!("codex-45{}", std::env::consts::EXE_SUFFIX));
    let output = exec(f.command_for(&launcher, &[]), 1);
    assert!(output.stdout.is_empty() && stderr(&output).starts_with(INVALID));
    fs::rename(&moved, &f.root).unwrap();
    assert_eq!(stderr(&launch(&f, "codex-45", &[], 1)), PENDING);
    assert!(Path::new(&f.launcher("codex-45")).exists());
}

#[cfg(unix)]
#[test]
fn stage05_dispatch_desktop_alias_forwards_to_app_open() {
    let f = setup();
    f.larp_json(
        &[
            "cc",
            "alias",
            "add",
            "codex-app",
            "codex-45",
            "--desktop",
            "--json",
        ],
        0,
    );
    let output = launch(&f, "codex-app", &[OsString::from("--json")], 1);
    assert!(stderr(&output).starts_with("error: unsupported launch argument: argument syntax\n"));
    // Claude desktop (and every non-macOS desktop) stays closed: alias is no unlock.
    assert_eq!(stderr(&launch(&f, "codex-app", &[], 1)), PENDING);
}

#[cfg(feature = "synthetic-launch")]
#[path = "support/alias_args.rs"]
mod alias_args;

#[cfg(feature = "synthetic-launch")]
#[test]
fn stage05_dispatch_alias_reaches_fake_with_exact_args_home_and_exit() {
    use sha2::{Digest, Sha256};
    use std::ffi::OsStr;

    fn encode(value: &OsStr) -> String {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            value
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            value
                .encode_wide()
                .map(|unit| format!("{unit:04x}"))
                .collect()
        }
    }

    let f = setup();
    let fake = f
        .temp
        .path()
        .join(format!("fake provider{}", std::env::consts::EXE_SUFFIX));
    copy_executable(Path::new(env!("CARGO_BIN_EXE_larpcc-fake-provider")), &fake);
    f.run(
        &["cc", "provider", "bind", "codex", fake.to_str().unwrap()],
        0,
    );
    let digest: String = Sha256::digest(fs::read(&fake).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let driver = Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic"));
    let source = f.temp.path().join(driver.file_name().unwrap());
    copy_executable(driver, &source);
    exec(
        f.command_for(driver, &["stage05-install", source.to_str().unwrap()]),
        0,
    );
    f.profile(&["rename", "claude-work", "renamed"], 0);
    let ambient = f.temp.path().join("ambient");
    let record = f.temp.path().join("alias-record");
    let status: u32 = if cfg!(windows) { 0x9234_5678 } else { 37 };
    let mut command = f.command_for(&f.launcher("codex-45"), alias_args::ARGS);
    command
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("LARPCC_HOME", &ambient)
        .env("LARPCC_SYNTHETIC_ALIAS_SHA256", &digest)
        .env("LARPCC_FAKE_EXIT", status.to_string())
        .env("LARPCC_FAKE_RECORD", &record);
    let output = exec(command, status as i32);
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("Profiles select native state"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    #[cfg(windows)]
    let fake = fs::canonicalize(fake).unwrap();
    let expected: Vec<_> = std::iter::once(fake.as_os_str())
        .chain(alias_args::ARGS.iter().map(OsStr::new))
        .map(encode)
        .collect();
    assert_eq!(value["args"], serde_json::json!(expected));
    assert_eq!(
        value["CODEX_HOME"],
        encode(f.root.join("profiles").join("1").join("cli").as_os_str())
    );
    assert!(value["CLAUDE_CONFIG_DIR"].is_null() && !ambient.exists());
}
