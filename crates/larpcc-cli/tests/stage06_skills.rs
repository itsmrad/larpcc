// Temporary path harness until CORE registers share after the v3 seed handoff.
#![allow(dead_code)]
#[path = "../src/launch/mod.rs"]
mod launch;
#[path = "../src/platform/mod.rs"]
mod platform;
#[path = "../src/provider/mod.rs"]
mod provider;
#[path = "../src/registry/mod.rs"]
mod registry;
#[path = "../src/share/skills/mod.rs"]
mod skills;

#[cfg(unix)]
mod cases {
    use super::*;
    use std::{
        fs,
        io::Write,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    fn fixture() -> tempfile::TempDir {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        tempfile::Builder::new()
            .prefix("stage06-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap()
    }
    fn directory(path: &Path) {
        platform::create_directory(path).unwrap();
    }
    fn file(path: &Path, bytes: &[u8]) {
        platform::create_file(path)
            .unwrap()
            .file
            .write_all(bytes)
            .unwrap();
    }
    fn package(root: &Path) -> PathBuf {
        let source = root.join("source");
        directory(&source);
        file(&source.join("SKILL.md"), b"example\n");
        source
    }
    #[test]
    fn stage06_store_snapshot_digest_and_immutable_copy() {
        let temp = fixture();
        let source = package(temp.path());
        directory(&source.join("notes"));
        file(&source.join("notes/a.txt"), b"hello");
        let snapshot = skills::Package::read(&source).unwrap();
        assert_eq!(
            snapshot.sha256,
            "4227b8dec26c602e18d429988a2dd7daa17275ebe98e286414267f4f0492e282"
        );
        fs::write(source.join("notes/a.txt"), b"changed").unwrap();
        let destination = temp.path().join("store");
        snapshot.copy_new(&destination).unwrap();
        assert_eq!(fs::read(destination.join("notes/a.txt")).unwrap(), b"hello");
        assert_eq!(
            skills::Package::read(&destination).unwrap().sha256,
            snapshot.sha256
        );
        assert_eq!(
            snapshot.copy_new(&destination),
            Err(skills::Error::Collision)
        );
    }
    #[test]
    fn stage06_store_limits_files_bytes_depth() {
        let temp = fixture();
        let source = package(temp.path());
        let large = platform::create_file(&source.join("large")).unwrap().file;
        large.set_len(skills::MAX_BYTES + 1).unwrap();
        assert!(matches!(
            skills::Package::read(&source),
            Err(skills::Error::TooLarge)
        ));
        fs::remove_file(source.join("large")).unwrap();
        let mut nested = source.clone();
        for _ in 0..skills::MAX_DEPTH + 1 {
            nested = nested.join("d");
            directory(&nested);
        }
        assert!(matches!(
            skills::Package::read(&source),
            Err(skills::Error::TooLarge)
        ));
        // Remove only the known synthetic empty directory chain.
        for _ in 0..skills::MAX_DEPTH + 1 {
            fs::remove_dir(&nested).unwrap();
            nested.pop();
        }
        for i in 0..skills::MAX_FILES {
            file(&source.join(format!("f{i}")), b"");
        }
        assert!(matches!(
            skills::Package::read(&source),
            Err(skills::Error::TooLarge)
        ));
    }
    #[test]
    fn stage06_drift_traversal_hardlink_symlink_special_outside_sentinel() {
        let temp = fixture();
        let source = package(temp.path());
        let outside = temp.path().join("outside");
        file(&outside, b"sentinel");
        fs::hard_link(&outside, source.join("hardlink")).unwrap();
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("hardlink")).unwrap();
        std::os::unix::fs::symlink(&outside, source.join("link")).unwrap();
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("link")).unwrap();
        let fifo =
            std::ffi::CString::new(source.join("socket").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: NUL-terminated synthetic path; no descriptor or contents IO.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("socket")).unwrap();
        assert!(skills::Package::read(&source.join("../source")).is_err());
        std::os::unix::fs::symlink(&source, temp.path().join("alias")).unwrap();
        assert!(skills::Package::read(&temp.path().join("alias")).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
    }
    #[test]
    fn stage06_store_portable_collisions_and_unsafe_ancestry() {
        let temp = fixture();
        let source = package(temp.path());
        file(&source.join("skill.MD"), b"collision");
        assert!(matches!(
            skills::Package::read(&source),
            Err(skills::Error::Collision)
        ));
        fs::remove_file(source.join("skill.MD")).unwrap();
        file(&source.join("a:b"), b"ads");
        assert!(skills::Package::read(&source).is_err());
        fs::remove_file(source.join("a:b")).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(skills::Package::read(&source).is_err());
        fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn stage06_links_remove_only_owned_link_and_preserve_store() {
        let temp = fixture();
        let source = package(temp.path());
        let snapshot = skills::Package::read(&source).unwrap();
        directory(&temp.path().join("shared"));
        directory(&temp.path().join("shared/skills"));
        snapshot
            .copy_new(&temp.path().join("shared/skills/example"))
            .unwrap();
        let link = temp.path().join("claude-work");
        let receipt = skills::OwnedLink::create(
            temp.path(),
            &registry::ProfileName::parse("example").unwrap(),
            &link,
            &snapshot.sha256,
        )
        .unwrap();
        assert!(matches!(
            skills::OwnedLink::create(
                temp.path(),
                &registry::ProfileName::parse("example").unwrap(),
                &link,
                &snapshot.sha256
            ),
            Err(skills::Error::Collision)
        ));
        receipt.verify(&link).unwrap();
        receipt.remove(&link).unwrap();
        assert!(source.join("SKILL.md").is_file());
        assert!(!link.exists());
    }
    #[test]
    fn stage06_drift_changed_link_occupied_directory_and_copy() {
        let temp = fixture();
        let source = package(temp.path());
        let snapshot = skills::Package::read(&source).unwrap();
        directory(&temp.path().join("shared"));
        directory(&temp.path().join("shared/skills"));
        snapshot
            .copy_new(&temp.path().join("shared/skills/example"))
            .unwrap();
        let link = temp.path().join("claude-work");
        let receipt = skills::OwnedLink::create(
            temp.path(),
            &registry::ProfileName::parse("example").unwrap(),
            &link,
            &snapshot.sha256,
        )
        .unwrap();
        fs::remove_file(&link).unwrap();
        directory(&link);
        file(&link.join("native"), b"keep");
        assert_eq!(receipt.remove(&link), Err(skills::Error::Drift));
        assert_eq!(fs::read(link.join("native")).unwrap(), b"keep");
        let copy = temp.path().join("codex-45");
        snapshot.copy_new(&copy).unwrap();
        let receipt = skills::OwnedCopy::capture(&copy, &snapshot.sha256).unwrap();
        fs::write(copy.join("SKILL.md"), b"changed").unwrap();
        assert_eq!(receipt.remove(&copy), Err(skills::Error::Drift));
        fs::write(copy.join("SKILL.md"), b"example\n").unwrap();
        receipt.remove(&copy).unwrap();
        assert!(!copy.exists());
        assert!(source.exists());
    }
    #[test]
    fn stage06_drift_nested_copy_outside_target_untouched() {
        let temp = fixture();
        let source = package(temp.path());
        directory(&source.join("nested"));
        file(&source.join("nested/sentinel"), b"inside");
        let snapshot = skills::Package::read(&source).unwrap();
        let copy = temp.path().join("codex-45");
        snapshot.copy_new(&copy).unwrap();
        let receipt = skills::OwnedCopy::capture(&copy, &snapshot.sha256).unwrap();
        let outside = temp.path().join("outside");
        directory(&outside);
        file(&outside.join("sentinel"), b"outside");
        fs::rename(copy.join("nested"), copy.join("saved")).unwrap();
        std::os::unix::fs::symlink(&outside, copy.join("nested")).unwrap();
        assert_eq!(receipt.remove(&copy), Err(skills::Error::Drift));
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
        fs::remove_file(copy.join("nested")).unwrap();
        fs::rename(copy.join("saved"), copy.join("nested")).unwrap();
        receipt.remove(&copy).unwrap();
        assert!(!copy.exists());
    }
    #[test]
    fn stage06_store_missing_skill_and_symlink_trailing_separator() {
        let temp = fixture();
        let source = package(temp.path());
        std::os::unix::fs::symlink(&source, temp.path().join("alias")).unwrap();
        assert!(skills::Package::read(&temp.path().join("alias/")).is_err());
        fs::remove_file(source.join("SKILL.md")).unwrap();
        assert!(skills::Package::read(&source).is_err());
    }
    #[test]
    fn stage06_drift_copy_replaced_file_same_bytes() {
        let temp = fixture();
        let source = package(temp.path());
        let snapshot = skills::Package::read(&source).unwrap();
        let receipt = skills::OwnedCopy::capture(&source, &snapshot.sha256).unwrap();
        let original = fs::File::open(source.join("SKILL.md")).unwrap();
        fs::remove_file(source.join("SKILL.md")).unwrap();
        file(&source.join("SKILL.md"), b"example\n");
        assert_eq!(receipt.remove(&source), Err(skills::Error::Drift));
        drop(original);
    }
}
