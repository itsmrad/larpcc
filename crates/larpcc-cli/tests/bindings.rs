mod common;
use common::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, process::Stdio};

#[cfg(target_os = "macos")]
#[test]
fn d41_admin_write_is_limited_to_provider_executable_ancestry() {
    use std::{
        os::unix::fs::{MetadataExt, chown, symlink},
        path::Path,
        process::Command,
    };

    let groups = Command::new("/usr/bin/id").arg("-G").output().unwrap();
    assert!(groups.status.success(), "id -G failed: {}", stderr(&groups));
    let groups = stdout(&groups);
    let groups: Vec<u32> = groups
        .split_whitespace()
        .map(|gid| gid.parse().unwrap())
        .collect();
    assert!(
        groups.contains(&80),
        "D41 requires membership in admin (gid 80); id -G: {groups:?}"
    );
    let non_admin = groups
        .iter()
        .copied()
        .find(|&gid| gid != 80)
        .expect("D41 needs a non-admin group for the refusal fixture");
    let set_mode = |path: &Path, gid, mode| {
        chown(path, None, Some(gid))
            .unwrap_or_else(|error| panic!("D41 could not set gid {gid} on {path:?}: {error}"));
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        let meta = fs::symlink_metadata(path).unwrap();
        assert_eq!(meta.gid(), gid);
        assert_eq!(meta.mode() & 0o7777, mode);
    };

    let mut f = Fixture::new();
    let bin = f.temp.path().join("bin");
    let cask = f.temp.path().join("Caskroom");
    let native = cask.join("native");
    let link = bin.join("provider");
    let directory_link = bin.join("cask");
    for dir in [&bin, &cask] {
        create_directory(dir);
        set_mode(dir, 80, 0o775);
    }
    create_file(&native, &[0xcf, 0xfa, 0xed, 0xfe]);
    set_mode(&native, 80, 0o755);
    symlink("../Caskroom/native", &link).unwrap();
    symlink("../Caskroom", &directory_link).unwrap();

    let bind = |path: &Path, code| {
        f.run(
            &[
                "cc",
                "provider",
                "bind",
                "claude",
                path.to_str().unwrap(),
                "--replace",
            ],
            code,
        )
    };
    bind(&native, 0);
    bind(&link, 0);
    bind(&directory_link.join("native"), 0);

    set_mode(&bin, non_admin, 0o775);
    assert!(stderr(&bind(&link, 1)).contains("has a foreign-writable ancestor"));
    set_mode(&bin, 80, 0o775);
    set_mode(&cask, 80, 0o1777);
    assert!(stderr(&bind(&link, 1)).contains("has a foreign-writable ancestor"));
    set_mode(&cask, 80, 0o775);
    set_mode(&native, 80, 0o775);
    assert!(stderr(&bind(&link, 1)).contains("is writable by group or other users"));
    set_mode(&native, 80, 0o755);

    let grant = Command::new("/bin/chmod")
        .args(["+a", "everyone allow list"])
        .arg(&cask)
        .output()
        .unwrap();
    assert!(grant.status.success(), "{}", stderr(&grant));
    assert!(stderr(&bind(&link, 1)).contains("has an extended ACL that grants access"));
    let clear = Command::new("/bin/chmod")
        .arg("-N")
        .arg(&cask)
        .output()
        .unwrap();
    assert!(clear.status.success(), "{}", stderr(&clear));

    set_mode(&f.root, 80, 0o770);
    assert!(stderr(&f.profile(&["list"], 1)).contains("group and other access must be removed"));
    f.root = cask.join("managed");
    assert!(stderr(&f.profile(&["list"], 1)).contains("has a foreign-writable ancestor"));
}

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
    assert_eq!(data["version"], 3);
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
    // Reuse the built inode so the identity test never needs a writable executable copy.
    let own = std::path::Path::new(env!("CARGO_BIN_EXE_larp"));
    let alias = f.temp.path().join("larp-hardlink");
    fs::hard_link(own, &alias).unwrap();
    let out = capture(f.command_for(
        own,
        &["cc", "provider", "bind", "claude", alias.to_str().unwrap()],
    ))
    .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("is Larp CC itself"),
        "{}",
        stderr(&out)
    );
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
