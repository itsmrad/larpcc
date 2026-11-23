#![cfg(feature = "synthetic-launch")]
//! Stage 08 bound-executable evidence export: reads only the bound executable,
//! runs nothing and never touches profile homes.
mod common;
use common::*;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

fn bound() -> (Fixture, PathBuf) {
    let f = Fixture::new();
    let fake = f
        .temp
        .path()
        .join(format!("native-fake{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_larpcc-fake-provider"), &fake).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    }
    f.run(
        &["cc", "provider", "bind", "codex", fake.to_str().unwrap()],
        0,
    );
    f.profile(&["add", "codex-45", "--provider", "codex"], 0);
    (f, fake)
}

#[test]
fn stage08_evidence_json_is_exact_and_runs_nothing() {
    let (f, fake) = bound();
    let before = f.outside_snapshot();
    let value = f.larp_json(&["cc", "provider", "evidence", "codex", "--json"], 0);
    assert_eq!(value["command"], "provider.evidence");
    assert_eq!(value["revision"], "2");
    let data = &value["data"];
    let bytes = fs::read(&fake).unwrap();
    assert_eq!(data["provider"], "codex");
    assert_eq!(data["path"], fake.to_str().unwrap());
    assert_eq!(data["size"], bytes.len().to_string());
    assert_eq!(data["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(data["os"], std::env::consts::OS);
    assert_eq!(data["arch"], std::env::consts::ARCH);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::metadata(&fake).unwrap();
        assert_eq!(data["resolved_path"], fake.to_str().unwrap());
        assert_eq!(
            data["identity"],
            serde_json::json!({"kind":"unix","dev":meta.dev().to_string(),"ino":meta.ino().to_string()})
        );
        assert_eq!(data["owner_uid"], meta.uid().to_string());
        assert_eq!(data["mode"], "0700");
    }
    #[cfg(windows)]
    {
        assert_eq!(data["identity"]["kind"], "windows");
        assert_eq!(data["owner_uid"], serde_json::Value::Null);
        assert_eq!(data["mode"], serde_json::Value::Null);
    }
    #[cfg(not(target_os = "macos"))]
    assert_eq!(data["cdhash"], serde_json::Value::Null);
    // Nothing ran: the fake leaves a record next to itself whenever it is executed.
    assert!(fake.parent().unwrap().read_dir().unwrap().all(|e| {
        !e.unwrap().file_name().to_string_lossy().contains(".probe-")
    }));
    assert_eq!(before, f.outside_snapshot(), "evidence export changed files");
}

#[test]
fn stage08_evidence_human_output() {
    let (f, fake) = bound();
    let output = f.run(&["cc", "provider", "evidence", "codex"], 0);
    let text = stdout(&output);
    let digest = format!("{:x}", Sha256::digest(fs::read(&fake).unwrap()));
    assert!(text.starts_with("Provider: codex\nPath: "), "{text}");
    assert!(text.contains(&format!("\nSHA-256: {digest}\n")), "{text}");
    assert!(text.ends_with("Nothing was executed and no profile or account data was read.\n"));
}

#[test]
fn stage08_evidence_refuses_unbound_and_stale_revision() {
    let (f, _fake) = bound();
    let value = f.larp_json(&["cc", "provider", "evidence", "claude", "--json"], 1);
    assert_eq!(
        value["error"],
        serde_json::json!({"code":"not_found","message":"Provider is not bound."})
    );
    let value = f.larp_json(
        &["cc", "provider", "evidence", "codex", "--json", "--expected-revision", "1"],
        1,
    );
    assert_eq!(value["error"]["code"], "conflict");
    let output = f.run(&["cc", "provider", "evidence", "claude"], 1);
    assert!(stderr(&output).starts_with("error: no executable bound for claude\n"));
}

#[cfg(unix)]
#[test]
fn stage08_evidence_refuses_untrusted_bound_path() {
    let (f, fake) = bound();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o722)).unwrap();
    let value = f.larp_json(&["cc", "provider", "evidence", "codex", "--json"], 1);
    assert_eq!(value["error"]["code"], "unsafe_path");
    let text = serde_json::to_string(&value).unwrap();
    assert!(!text.contains(fake.to_str().unwrap()));
}
