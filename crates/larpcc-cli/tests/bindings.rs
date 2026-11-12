mod common;
use common::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, process::Stdio};
#[test]
fn binders_serialize_and_noop_does_not_revalidate_or_increment() {
    let f = Fixture::new();
    let native = f.temp.path().join(if cfg!(windows) {
        "native.exe"
    } else {
        "native"
    });
    #[cfg(target_os = "macos")]
    let magic = [0xcf, 0xfa, 0xed, 0xfe];
    #[cfg(all(unix, not(target_os = "macos")))]
    let magic = *b"\x7fELF";
    #[cfg(windows)]
    let magic = *b"MZ\0\0";
    create_file(&native, &magic);
    #[cfg(unix)]
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
    let missing = f.temp.path().join(if cfg!(windows) {
        "missing-native.exe"
    } else {
        "missing-native"
    });
    let output = f.run(
        &[
            "cc",
            "provider",
            "bind",
            "claude",
            missing.to_str().unwrap(),
        ],
        2,
    );
    assert!(stderr(&output).contains("without --replace"));
    let rows = stdout(&f.run(&["cc", "provider", "list"], 0));
    assert!(rows.contains("invalid: does not exist"));
    assert!(rows.contains("codex"));
    assert!(rows.contains("unbound"));
}

#[cfg(unix)]
#[test]
fn control_paths_are_escaped_and_lexical_failures_do_not_create_root() {
    let f = Fixture::new();
    let native = f.temp.path().join("native\n\u{202e}");
    #[cfg(target_os = "macos")]
    let magic = [0xcf, 0xfa, 0xed, 0xfe];
    #[cfg(not(target_os = "macos"))]
    let magic = *b"\x7fELF";
    fs::write(&native, magic).unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
    let out = f.run(
        &["cc", "provider", "bind", "claude", native.to_str().unwrap()],
        0,
    );
    let text = stdout(&out);
    assert!(text.contains("native\\n\\u{202e}"));
    assert!(!text.contains('\u{202e}'));
    assert_eq!(text.lines().count(), 1);
    for path in ["relative", "/bad/../x", "/bad//x", "/bad/./x", "/bad/"] {
        let f = Fixture::new();
        f.run(&["cc", "provider", "bind", "codex", path], 1);
        assert!(!f.root.exists());
    }
}

#[cfg(unix)]
#[test]
fn self_hardlink_and_bounded_installation_link_chain() {
    let f = Fixture::new();
    let own = f.temp.path().join("larp-copy");
    let alias = f.temp.path().join("larp-hardlink");
    fs::copy(env!("CARGO_BIN_EXE_larp"), &own).unwrap();
    fs::set_permissions(&own, fs::Permissions::from_mode(0o700)).unwrap();
    fs::hard_link(&own, &alias).unwrap();
    let out = capture(f.command_for(
        &own,
        &["cc", "provider", "bind", "claude", alias.to_str().unwrap()],
    ))
    .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("is Larp CC itself"));
    assert!(!f.root.join("registry.json").exists());
    let native = f.temp.path().join("native");
    #[cfg(target_os = "macos")]
    let magic = [0xcf, 0xfa, 0xed, 0xfe];
    #[cfg(not(target_os = "macos"))]
    let magic = *b"\x7fELF";
    fs::write(&native, magic).unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
    let mut previous = native;
    for index in 0..40 {
        let next = f.temp.path().join(format!("link-{index}"));
        std::os::unix::fs::symlink(&previous, &next).unwrap();
        previous = next;
    }
    f.run(
        &[
            "cc",
            "provider",
            "bind",
            "claude",
            previous.to_str().unwrap(),
        ],
        0,
    );
    let extra = f.temp.path().join("link-40");
    std::os::unix::fs::symlink(previous, &extra).unwrap();
    let out = f.run(
        &["cc", "provider", "bind", "codex", extra.to_str().unwrap()],
        1,
    );
    assert!(stderr(&out).contains("cannot resolve a trusted executable path"));
}
