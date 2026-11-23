use super::*;
use crate::registry::Provider;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture() -> (tempfile::TempDir, Registry) {
    let parent =
        std::env::var_os("LARPCC_TEST_PARENT").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
        .unwrap();
    let registry = Registry::at(temp.path().join("managed")).unwrap();
    (temp, registry)
}

fn name(value: &str) -> ProfileName {
    ProfileName::parse_new(value).unwrap()
}

fn interrupted<T>(step: &'static str, run: impl FnOnce() -> Result<T, Error>) {
    FAULT.set(Some(step));
    let result = run();
    FAULT.set(None);
    assert!(matches!(result, Err(Error::RecoveryRequired)));
}

#[test]
fn stage04_delete_recovery() {
    for phase in [
        "delete journal",
        "delete tombstone",
        "delete aliases",
        "delete skills",
        "delete references",
        "delete folder",
        "delete registry",
    ] {
        let (_temp, registry) = fixture();
        let label = name("claude-work");
        let profile = registry.add(label.clone(), Provider::Claude).unwrap();
        let path = profile.state_dir.parent().unwrap();
        interrupted(phase, || registry.delete_profile(label.clone(), false));
        assert!(crate::share::pending(registry.root()));
        assert!(matches!(registry.get(&label), Err(Error::RecoveryRequired)));
        assert!(matches!(
            registry.forget(&label),
            Err(Error::RecoveryRequired)
        ));
        let data = store::load_unchecked(registry.root()).unwrap();
        assert_eq!(
            data.profiles.first().and_then(|p| p.state),
            match phase {
                "delete journal" => Some(ProfileState::Active),
                "delete registry" => None,
                _ => Some(ProfileState::Deleting),
            }
        );
        assert_eq!(
            registry.delete_profile(label, true).unwrap(),
            profile.profile.id
        );
        assert!(!path.exists());
        assert!(!crate::share::pending(registry.root()));
        assert_eq!(store::load(registry.root()).unwrap().next_id, 2);
    }
}

#[test]
fn stage04_delete_escape_refuses_forged_journal_paths() {
    let (temp, registry) = fixture();
    let label = name("codex-45");
    let profile = registry.add(label.clone(), Provider::Codex).unwrap();
    let sentinel = temp.path().join("outside");
    fs::write(&sentinel, b"outside sentinel").unwrap();
    interrupted("delete tombstone", || {
        registry.delete_profile(label.clone(), false)
    });
    let path = registry.root().join(JOURNAL);
    let valid = fs::read(&path).unwrap();
    let mut forged: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    for bad in ["../../outside", "/outside", "C:\\outside", "inside:stream"] {
        forged["transaction"]["path"] = serde_json::json!(bad);
        fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(matches!(
            registry.delete_profile(label.clone(), true),
            Err(Error::RecoveryRequired)
        ));
        assert!(profile.state_dir.is_dir());
        assert_eq!(fs::read(&sentinel).unwrap(), b"outside sentinel");
    }
    fs::write(&path, valid).unwrap();
    registry.delete_profile(label, true).unwrap();
}

#[test]
fn stage04_delete_escape() {
    let (temp, registry) = fixture();
    let label = name("codex-45");
    let profile = registry.add(label.clone(), Provider::Codex).unwrap();
    let sentinel = temp.path().join("outside");
    fs::write(&sentinel, b"outside sentinel").unwrap();
    fs::hard_link(&sentinel, profile.state_dir.join("hard")).unwrap();
    symlink(temp.path(), profile.state_dir.join("outside-link")).unwrap();
    interrupted("delete tombstone", || {
        registry.delete_profile(label.clone(), false)
    });
    let home = profile.state_dir.parent().unwrap();
    let parked = home.with_file_name("parked");
    fs::rename(home, &parked).unwrap();
    symlink(temp.path(), home).unwrap();
    assert!(registry.delete_profile(label.clone(), true).is_err());
    assert!(crate::share::pending(registry.root()));
    fs::remove_file(home).unwrap();
    fs::rename(parked, home).unwrap();
    registry.delete_profile(label, true).unwrap();
    assert_eq!(fs::read(sentinel).unwrap(), b"outside sentinel");
}

#[test]
fn stage04_delete_recovery_guard_busy() {
    let (_temp, registry) = fixture();
    let label = name("claude-work");
    let p = registry.add(label.clone(), Provider::Claude).unwrap();
    let guard = acquire_auth_guard(registry.root(), p.profile.id).unwrap();
    assert!(matches!(
        registry.delete_profile(label.clone(), false),
        Err(Error::AuthBusy)
    ));
    assert!(!crate::share::pending(registry.root()));
    assert!(p.state_dir.exists());
    drop(guard);
    registry.delete_profile(label, false).unwrap();
}

#[test]
fn stage04_delete_recovery_retains_tombstone_on_skill_drift() {
    let (temp, registry) = fixture();
    let label = name("claude-work");
    let profile = registry.add(label.clone(), Provider::Claude).unwrap();
    let source = temp.path().join("package");
    platform::create_directory(&source).unwrap();
    let mut specimen = platform::create_file(&source.join("SKILL.md"))
        .unwrap()
        .file;
    specimen.write_all(b"synthetic skill").unwrap();
    drop(specimen);
    skills::tests::private_skills(
        &registry,
        crate::registry::SkillsAction::Add {
            source,
            name: name("example"),
        },
    )
    .unwrap();
    let link = profile.state_dir.join("skills/example");
    let held = link.with_file_name("held");
    fs::rename(&link, &held).unwrap();
    let outside = temp.path().join("outside");
    fs::write(&outside, b"outside sentinel").unwrap();
    symlink(&outside, &link).unwrap();
    assert!(matches!(
        registry.delete_profile(label.clone(), false),
        Err(Error::Drift)
    ));
    assert!(!crate::share::pending(registry.root()));
    fs::remove_file(&link).unwrap();
    fs::rename(&held, &link).unwrap();
    interrupted("delete tombstone", || {
        registry.delete_profile(label.clone(), false)
    });
    fs::rename(&link, &held).unwrap();
    symlink(&outside, &link).unwrap();
    assert!(matches!(
        registry.delete_profile(label.clone(), true),
        Err(Error::Drift)
    ));
    assert_eq!(
        store::load_unchecked(registry.root()).unwrap().profiles[0].state,
        Some(ProfileState::Deleting)
    );
    fs::remove_file(&link).unwrap();
    fs::rename(&held, &link).unwrap();
    registry.delete_profile(label, true).unwrap();
    assert_eq!(fs::read(outside).unwrap(), b"outside sentinel");
}

fn legacy(registry: &Registry, version: u64) -> std::path::PathBuf {
    let _lock = store::lock(registry.root(), LOCK_TIMEOUT).unwrap();
    let data = store::Data {
        version,
        revision: 7,
        next_id: 3,
        profiles: vec![Profile {
            id: ProfileId::new(2),
            name: name("codex-45"),
            provider: Provider::Codex,
            surfaces: None,
            state: None,
            deletion_transaction: None,
        }],
        ..Default::default()
    };
    paths::mkdir(&registry.root().join("profiles")).unwrap();
    let home = registry.root().join("profiles/2");
    paths::mkdir(&home).unwrap();
    persist(registry.root(), &store::serialize(&data).unwrap()).unwrap();
    home
}

#[test]
fn stage04_migration_empty_only() {
    for version in [1, 2] {
        let (_temp, registry) = fixture();
        let home = legacy(&registry, version);
        let occupied = home.join("synthetic-sentinel");
        fs::write(&occupied, b"untouched").unwrap();
        assert!(matches!(registry.migrate_profiles(), Err(Error::Conflict)));
        assert!(!crate::share::pending(registry.root()));
        assert_eq!(store::load(registry.root()).unwrap().version, version);
        assert_eq!(fs::read(&occupied).unwrap(), b"untouched");
        fs::remove_file(occupied).unwrap();
        let identity = platform::inspect(&home, ObjectKind::Directory)
            .unwrap()
            .unwrap()
            .identity;
        assert!(registry.migrate_profiles().unwrap());
        assert!(!registry.migrate_profiles().unwrap());
        assert!(home.join("desktop").is_dir());
        assert_eq!(
            platform::inspect(&home.join("cli"), ObjectKind::Directory)
                .unwrap()
                .unwrap()
                .identity,
            identity
        );
        let after = store::load(registry.root()).unwrap();
        assert_eq!((after.version, after.revision, after.next_id), (3, 8, 3));
    }
}

#[test]
fn stage04_migration_crash() {
    for phase in [
        "migrate journal",
        "migrate legacy rename",
        "migrate container create",
        "migrate container journal",
        "migrate container rename",
        "migrate publish journal",
        "migrate cli rename",
        "migrate progress journal",
        "migrate registry",
    ] {
        let (_temp, registry) = fixture();
        let home = legacy(&registry, 2);
        interrupted(phase, || registry.migrate_profiles());
        assert!(matches!(
            store::load(registry.root()),
            Err(Error::RecoveryRequired)
        ));
        if phase == "migrate container create" {
            assert!(matches!(registry.migrate_profiles(), Err(Error::Drift)));
            assert!(home.with_file_name(".larp-migrate-2-legacy").is_dir());
        } else {
            assert!(registry.migrate_profiles().unwrap(), "{phase}");
            assert!(home.join("cli").is_dir());
            assert!(home.join("desktop").is_dir());
            assert!(!crate::share::pending(registry.root()));
        }
    }
}

#[test]
fn stage04_migration_concurrent_attempts_publish_once() {
    let (_temp, registry) = fixture();
    let home = legacy(&registry, 2);
    let root = registry.root();
    let a = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let migrate = || {
            let registry = Registry::at(root.to_owned()).unwrap().expecting(Some(7));
            a.wait();
            registry.migrate_profiles()
        };
        let first = scope.spawn(migrate);
        let second = scope.spawn(migrate);
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|r| matches!(r, Ok(true))).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(Error::AuthBusy | Error::Conflict)))
                .count(),
            1
        );
    });
    assert!(home.join("cli").is_dir());
    assert!(home.join("desktop").is_dir());
    assert_eq!(store::load(root).unwrap().revision, 8);
}
