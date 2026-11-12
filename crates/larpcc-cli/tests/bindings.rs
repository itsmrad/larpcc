#![cfg(unix)]
mod common;
use common::*;
use std::{fs, os::unix::fs::PermissionsExt, process::Stdio};
#[test]
fn binders_serialize_and_noop_does_not_revalidate_or_increment() {
    let f = Fixture::new();
    let native = f.temp.path().join("native");
    #[cfg(target_os = "macos")]
    let magic = [0xcf, 0xfa, 0xed, 0xfe];
    #[cfg(not(target_os = "macos"))]
    let magic = *b"\x7fELF";
    fs::write(&native, magic).unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
    let path = native.to_str().unwrap();
    let children = (0..8)
        .map(|_| {
            f.command(&["cc", "provider", "bind", "claude", path])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let mut bound = 0;
    for child in children {
        let output = wait_output(child).unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        if stdout(&output).starts_with("Bound ") {
            bound += 1;
        }
    }
    assert_eq!(bound, 1);
    let before = fs::read(f.root.join("registry.json")).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(data["revision"], 1);
    assert_eq!(data["version"], 2);
    fs::remove_file(&native).unwrap();
    assert_eq!(
        stdout(&f.run(&["cc", "provider", "bind", "claude", path], 0)),
        format!("claude is already bound to {path}.\n")
    );
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
    let output = f.run(&["cc", "provider", "bind", "claude", "/missing-native"], 2);
    assert!(stderr(&output).contains("without --replace"));
    let rows = stdout(&f.run(&["cc", "provider", "list"], 0));
    assert!(rows.contains("invalid: does not exist"));
    assert!(rows.contains("codex"));
    assert!(rows.contains("unbound"));
}
