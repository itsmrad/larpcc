#![allow(dead_code)]
#[path = "../src/aliases.rs"]
mod aliases;
mod common;
#[path = "../src/launch/mod.rs"]
mod launch;
#[path = "../src/platform/mod.rs"]
mod platform;
#[path = "../src/provider/mod.rs"]
mod provider;
#[path = "../src/registry/mod.rs"]
mod registry;
use aliases::*;
use common::*;
use registry::{ProfileName, Provider, Registry, Surface};
use std::{ffi::OsStr, fs, path::Path};

#[test]
fn stage05_collisions_reserved_shadow_and_malformed_argv0() {
    for value in [
        "larp",
        "con",
        "com1",
        "lpt9",
        "aux",
        "nul",
        "../codex-45",
        "Codex",
        "a.exe",
        "a b",
        "a\0b",
        "",
    ] {
        assert!(name(value, true).is_err(), "{value:?}");
    }
    for value in ["claude", "codex"] {
        assert!(name(value, false).is_err());
        assert!(name(value, true).is_ok());
    }
    for value in ["claude-work", "codex-45", "other"] {
        assert_eq!(name(value, false).unwrap().as_str(), value);
        assert_eq!(
            invocation(OsStr::new(value)).unwrap().unwrap().as_str(),
            value
        );
    }
    assert!(invocation(OsStr::new("larp")).unwrap().is_none());
    for value in ["", "a b", "../", "CON", "a.exe.exe"] {
        assert!(invocation(OsStr::new(value)).is_err());
    }
    #[cfg(windows)]
    assert_eq!(
        invocation(OsStr::new(r"C:\space dir\codex-45.exe"))
            .unwrap()
            .unwrap()
            .as_str(),
        "codex-45"
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        assert!(invocation(OsStr::from_bytes(b"codex-\xff")).is_err());
    }
}

#[test]
fn stage05_dispatch_receipt_identity_hash_root_and_rename() {
    let f = Fixture::new();
    let registry = Registry::at(f.root.clone()).unwrap();
    let profile = registry
        .add(
            ProfileName::parse_new("claude-work").unwrap(),
            Provider::Codex,
        )
        .unwrap();
    let source_dir = f.temp.path().join("Larp source ü space");
    create_directory(&source_dir);
    let source_path = source_dir.join(if cfg!(windows) { "larp.exe" } else { "larp" });
    fs::copy(env!("CARGO_BIN_EXE_larp"), &source_path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&source_path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    // Source::current deliberately cannot accept an arbitrary binding; fixture
    // uses a test-only helper to retain exactly this checked Larp copy.
    let source = aliases::test_source(&source_path, &f.root);
    create_directory(&f.root.join("bin"));
    let alias = name("codex-45", false).unwrap();
    let executable = launcher_path(&f.root, &alias);
    fs::copy(&source_path, &executable).unwrap();
    let receipt = Receipt::prepare(
        &f.root,
        alias.clone(),
        profile.profile.id,
        Surface::Cli,
        &source,
    )
    .unwrap();
    let encoded = receipt.encode().unwrap();
    let receipt_file = receipt_path(&f.root, &alias);
    create_file(&receipt_file, &encoded);
    let (loaded, digest) = Receipt::adjacent(&executable, &alias).unwrap();
    assert_ne!(digest, [0; 32]);
    assert_eq!(loaded.root(), f.root);
    assert_eq!(loaded.surface(), Surface::Cli);
    assert_eq!(loaded.profile_id(), profile.profile.id);
    loaded
        .validate(&executable, OsStr::new("codex-45"))
        .unwrap();
    registry
        .rename(
            &profile.profile.name,
            ProfileName::parse_new("codex-45").unwrap(),
        )
        .unwrap();
    assert_eq!(registry.list().unwrap()[0].profile.id, loaded.profile_id());
    assert!(
        loaded
            .validate(&executable, OsStr::new("claude-work"))
            .is_err()
    );
    assert!(
        loaded
            .validate(Path::new("/elsewhere/codex-45"), OsStr::new("codex-45"))
            .is_err()
    );
    // Source drift independently refuses even when the launcher is untouched.
    let original_source = fs::read(&source_path).unwrap();
    let mut changed_source = original_source.clone();
    let last = changed_source.len() - 1;
    changed_source[last] ^= 1;
    fs::write(&source_path, changed_source).unwrap();
    assert!(
        loaded
            .validate(&executable, OsStr::new("codex-45"))
            .is_err()
    );
    fs::write(&source_path, original_source).unwrap();
    loaded
        .validate(&executable, OsStr::new("codex-45"))
        .unwrap();
    // Identical bytes at a new inode are still drift.
    let replacement = f.root.join("bin/replacement");
    fs::copy(&executable, &replacement).unwrap();
    fs::remove_file(&executable).unwrap();
    fs::rename(&replacement, &executable).unwrap();
    assert!(
        loaded
            .validate(&executable, OsStr::new("codex-45"))
            .is_err()
    );
    // A forged ABI or unknown receipt field cannot be accepted as this ABI.
    let mut forged: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    forged["abi"] = 999.into();
    fs::write(&receipt_file, serde_json::to_vec(&forged).unwrap()).unwrap();
    let (forged, _) = Receipt::adjacent(&executable, &alias).unwrap();
    assert!(
        forged
            .validate(&executable, OsStr::new("codex-45"))
            .is_err()
    );
    fs::write(&receipt_file, b"{}").unwrap();
    assert!(Receipt::adjacent(&executable, &alias).is_err());
}

#[test]
fn stage05_collisions_existing_path_and_receipt_never_overwrite() {
    let f = Fixture::new();
    create_directory(&f.root);
    create_directory(&f.root.join("bin"));
    let alias = name("claude-work", false).unwrap();
    check_collision(&f.root, &alias, None).unwrap();
    let directory = f.temp.path().join("commands ü space");
    create_directory(&directory);
    let collision = directory.join(if cfg!(windows) {
        "claude-work.cmd"
    } else {
        "claude-work"
    });
    create_file(&collision, b"unrelated command sentinel");
    let search_path = std::env::join_paths([&directory]).unwrap();
    assert!(check_collision(&f.root, &alias, Some(&search_path)).is_err());
    assert_eq!(fs::read(&collision).unwrap(), b"unrelated command sentinel");
    create_file(
        &receipt_path(&f.root, &alias),
        b"unrelated receipt sentinel",
    );
    assert!(check_collision(&f.root, &alias, None).is_err());
    assert_eq!(
        fs::read(receipt_path(&f.root, &alias)).unwrap(),
        b"unrelated receipt sentinel"
    );
}
