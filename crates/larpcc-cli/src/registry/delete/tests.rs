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

fn references(base: &Path, registry: &Registry, label: &ProfileName) {
    let executable = base.join("larp");
    let mut file = platform::create_file(&executable).unwrap().file;
    let magic: &[u8] = if cfg!(target_os = "macos") {
        &[0xcf, 0xfa, 0xed, 0xfe]
    } else {
        b"\x7fELF"
    };
    file.write_all(magic).unwrap();
    file.write_all(b"synthetic launcher").unwrap();
    file.set_permissions(fs::Permissions::from_mode(0o700))
        .unwrap();
    drop(file);
    registry
        .alias_add_with(
            name("codex-45"),
            label,
            super::super::Surface::Cli,
            None,
            |root| Ok(crate::aliases::test_source(&executable, root)),
        )
        .unwrap();
    let source = base.join("package");
    platform::create_directory(&source).unwrap();
    platform::create_file(&source.join("SKILL.md"))
        .unwrap()
        .file
        .write_all(b"synthetic")
        .unwrap();
    for skill in ["example", "sample"] {
        skills::tests::private_skills(
            registry,
            crate::registry::SkillsAction::Add {
                source: source.clone(),
                name: name(skill),
            },
        )
        .unwrap();
    }
    registry
        .skills(crate::registry::SkillsAction::Switch {
            name: name("sample"),
            profile: label.clone(),
            enabled: false,
        })
        .unwrap();
}

fn enable_mcp(registry: &Registry) {
    use crate::share::mcp::cli::{McpAction, Target};

    registry
        .mcp(McpAction::Add {
            name: "files".into(),
            command: None,
            args: vec![],
            env_refs: vec![],
            url: Some("https://docs.example.com/mcp".into()),
            header_refs: vec![],
        })
        .unwrap();
    registry
        .mcp(McpAction::Switch {
            name: name("files"),
            target: Target::All,
            enabled: true,
        })
        .unwrap();
}

#[test]
fn stage04_delete_recovery_stage07_mcp_cleanup() {
    use crate::share::mcp::cli::McpAction;
    use serde_json::{Value, json};

    for phase in [
        None,
        Some("delete mcp render"),
        Some("delete mcp manifest"),
        Some("delete mcp"),
        Some("delete registry"),
    ] {
        let (_temp, registry) = fixture();
        let label = name("claude-work");
        let profile = registry.add(label.clone(), Provider::Claude).unwrap();
        let keep = registry.add(name("codex-45"), Provider::Codex).unwrap();
        enable_mcp(&registry);
        let root = registry.root();
        let catalog = root.join("mcp.json");
        let before: Value = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
        let render = mcp::render_path(root, keep.profile.id, Provider::Codex);
        let keep_bytes = fs::read(&render).unwrap();
        if let Some(phase) = phase {
            interrupted(phase, || registry.delete_profile(label.clone(), false));
            assert!(matches!(
                registry.mcp(McpAction::List),
                Err(Error::RecoveryRequired)
            ));
            assert!(matches!(
                registry.mcp(McpAction::Sync),
                Err(Error::RecoveryRequired)
            ));
            assert_eq!(
                registry.delete_profile(label, true).unwrap(),
                profile.profile.id
            );
        } else {
            assert_eq!(
                registry.delete_profile(label, false).unwrap(),
                profile.profile.id
            );
        }
        assert!(!crate::share::pending(root));
        assert!(!profile.state_dir.parent().unwrap().exists());
        assert_eq!(fs::read(&render).unwrap(), keep_bytes);
        let after: Value = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
        assert_eq!(after["revision"], before["revision"].as_u64().unwrap() + 1);
        assert_eq!(
            after["servers"][0]["switches"],
            json!([{"profile_id":2,"enabled":true}])
        );
        let matrix = registry.mcp(McpAction::Matrix).unwrap();
        assert_eq!(matrix["profiles"].as_array().unwrap().len(), 1);
        assert_eq!(matrix["profiles"][0]["name"], "codex-45");
        assert_eq!(matrix["rows"][0]["enabled"], json!([true]));
        let list = registry.mcp(McpAction::List).unwrap();
        assert_eq!(
            list["servers"][0]["switches"],
            json!([{"profile_id":"2","enabled":true}])
        );
    }
}

#[test]
fn stage04_delete_recovery_stage07_mcp_drift_keeps_tombstone() {
    for edit_catalog in [false, true] {
        let (_temp, registry) = fixture();
        let label = name("claude-work");
        let profile = registry.add(label.clone(), Provider::Claude).unwrap();
        enable_mcp(&registry);
        let root = registry.root();
        interrupted("delete tombstone", || {
            registry.delete_profile(label.clone(), false)
        });
        let path = if edit_catalog {
            root.join("mcp.json")
        } else {
            mcp::render_path(root, profile.profile.id, Provider::Claude)
        };
        let original = fs::read(&path).unwrap();
        let mut edited: serde_json::Value = serde_json::from_slice(&original).unwrap();
        let edited = if edit_catalog {
            // A formatting-only edit must also survive recovery untouched.
            serde_json::to_vec_pretty(&edited).unwrap()
        } else {
            edited["version"] = serde_json::json!(99);
            serde_json::to_vec(&edited).unwrap()
        };
        fs::write(&path, &edited).unwrap();
        assert!(matches!(
            registry.delete_profile(label.clone(), true),
            Err(Error::Drift)
        ));
        assert!(crate::share::pending(root));
        assert!(profile.state_dir.exists());
        assert_eq!(fs::read(&path).unwrap(), edited);
        let data = store::load_unchecked(root).unwrap();
        assert_eq!(data.profiles[0].state, Some(ProfileState::Deleting));
        fs::write(&path, original).unwrap();
        registry.delete_profile(label, true).unwrap();
    }
}

#[test]
fn stage04_delete_stage07_without_mcp_entries_keeps_catalog() {
    use crate::share::mcp::cli::McpAction;

    let (_temp, registry) = fixture();
    let label = name("claude-work");
    registry.add(label.clone(), Provider::Claude).unwrap();
    enable_mcp(&registry);
    registry
        .mcp(McpAction::Remove {
            name: name("files"),
        })
        .unwrap();
    let catalog = registry.root().join("mcp.json");
    let bytes = fs::read(&catalog).unwrap();
    let identity = platform::inspect(&catalog, ObjectKind::File)
        .unwrap()
        .unwrap()
        .identity;
    registry.delete_profile(label, false).unwrap();
    assert_eq!(fs::read(&catalog).unwrap(), bytes);
    assert_eq!(
        platform::inspect(&catalog, ObjectKind::File)
            .unwrap()
            .unwrap()
            .identity,
        identity
    );
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
        let (temp, registry) = fixture();
        let label = name("claude-work");
        let profile = registry.add(label.clone(), Provider::Claude).unwrap();
        let path = profile.state_dir.parent().unwrap();
        references(temp.path(), &registry, &label);
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
        assert!(!crate::aliases::launcher_path(registry.root(), &name("codex-45")).exists());
        assert!(!crate::aliases::receipt_path(registry.root(), &name("codex-45")).exists());
        assert!(
            registry
                .root()
                .join("shared/skills/example/SKILL.md")
                .exists()
        );
        assert!(!crate::share::pending(registry.root()));
        assert!(!registry.root().join("mcp.json").exists());
        assert_eq!(store::load(registry.root()).unwrap().next_id, 2);
    }
}

#[test]
fn stage04_delete_recovery_between_reference_writes() {
    for phase in ["alias leaf", "skill index"] {
        let (temp, registry) = fixture();
        let label = name("claude-work");
        registry.add(label.clone(), Provider::Claude).unwrap();
        references(temp.path(), &registry, &label);
        let launcher = crate::aliases::launcher_path(registry.root(), &name("codex-45"));
        let receipt = crate::aliases::receipt_path(registry.root(), &name("codex-45"));
        let manifest = registry.root().join("owned-files.json");
        let index = registry.root().join("skills.json");
        let original_manifest = fs::read(&manifest).unwrap();
        let original_index = fs::read(&index).unwrap();
        if phase == "alias leaf" {
            alias::set_fault(Some("artifact leaf"));
        } else {
            skills::set_fault(Some("delete index"));
        }
        let result = registry.delete_profile(label.clone(), false);
        alias::set_fault(None);
        skills::set_fault(None);
        assert!(matches!(result, Err(Error::RecoveryRequired)));
        assert!(!launcher.exists());
        assert_eq!(receipt.exists(), phase == "alias leaf");
        assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
        assert_eq!(
            fs::read(&index).unwrap() == original_index,
            phase == "alias leaf"
        );
        registry.delete_profile(label, true).unwrap();
        assert!(!receipt.exists());
        assert_ne!(fs::read(&manifest).unwrap(), original_manifest);
        assert_ne!(fs::read(&index).unwrap(), original_index);
    }
}

#[test]
fn stage04_delete_staging_ownership() {
    let (_temp, registry) = fixture();
    let label = name("codex-45");
    let p = registry.add(label.clone(), Provider::Codex).unwrap();
    let staging = registry.root().join(STAGING);
    platform::create_file(&staging)
        .unwrap()
        .file
        .write_all(b"unowned")
        .unwrap();
    assert!(matches!(
        registry.delete_profile(label.clone(), false),
        Err(Error::Drift)
    ));
    assert_eq!(fs::read(&staging).unwrap(), b"unowned");
    assert!(p.state_dir.exists());
    assert!(!crate::share::pending(registry.root()));
    fs::remove_file(&staging).unwrap();
    interrupted("delete tombstone", || {
        registry.delete_profile(label.clone(), false)
    });
    interrupted("lifecycle staging", || {
        registry.delete_profile(label.clone(), true)
    });
    assert!(staging.is_file());
    assert!(p.state_dir.exists());
    registry.delete_profile(label, true).unwrap();
    assert!(!staging.exists());
    assert!(!p.state_dir.exists());
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
fn stage04_delete_escape_rechecks_chain_after_directory_sync() {
    let (temp, registry) = fixture();
    let label = name("codex-45");
    let p = registry.add(label.clone(), Provider::Codex).unwrap();
    let home = p.state_dir.parent().unwrap().to_owned();
    let moved = temp.path().join("moved");
    let outside = temp.path().join("outside");
    platform::create_directory(&outside).unwrap();
    let sentinel = outside.join("sentinel");
    fs::write(&sentinel, b"outside sentinel").unwrap();
    let (from, to, target) = (home.clone(), moved.clone(), outside.clone());
    crate::platform::removal::before_unlink(move || {
        fs::rename(&from, to).unwrap();
        symlink(target, from).unwrap();
    });
    assert!(matches!(
        registry.delete_profile(label.clone(), false),
        Err(Error::Drift)
    ));
    assert!(moved.join("cli").is_dir());
    assert!(crate::share::pending(registry.root()));
    assert_eq!(fs::read(sentinel).unwrap(), b"outside sentinel");
    fs::remove_file(&home).unwrap();
    fs::rename(moved, home).unwrap();
    registry.delete_profile(label, true).unwrap();
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
fn stage04_delete_recovery_surviving_child() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    fn child(root: &Path, mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "registry::delete::tests::stage04_delete_recovery_surviving_child",
            ])
            .env("LARPCC_STAGE04_CHILD_ROOT", root)
            .env("LARPCC_STAGE04_CHILD_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    if let Some(root) = std::env::var_os("LARPCC_STAGE04_CHILD_ROOT") {
        let root = std::path::PathBuf::from(root);
        if std::env::var_os("LARPCC_STAGE04_CHILD_MODE").as_deref()
            == Some(std::ffi::OsStr::new("broker"))
        {
            // The synthetic session outlives its launcher.
            #[allow(
                clippy::zombie_processes,
                reason = "The test deliberately exits the broker while its bounded child holds the lease"
            )]
            let _session = child(&root, "session").spawn().unwrap();
            return;
        }
        let _guard = acquire_auth_guard(&root, ProfileId::new(1)).unwrap();
        fs::write(root.join("child-ready"), []).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while root.exists() && !root.join("child-stop").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    let (_temp, registry) = fixture();
    let root = registry.root();
    let label = name("claude-work");
    registry.add(label.clone(), Provider::Claude).unwrap();
    interrupted("delete tombstone", || {
        registry.delete_profile(label.clone(), false)
    });
    assert!(child(root, "broker").status().unwrap().success());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("child-ready").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(root.join("child-ready").exists());
    let busy = registry.delete_profile(label.clone(), true);
    fs::write(root.join("child-stop"), []).unwrap();
    assert!(matches!(busy, Err(Error::AuthBusy)));
    loop {
        match registry.delete_profile(label.clone(), true) {
            Ok(_) => break,
            Err(Error::AuthBusy) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            other => panic!("guard did not release: {other:?}"),
        }
    }
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
    let manifest = registry.root().join("owned-files.json");
    let original = fs::read(&manifest).unwrap();
    let mut forged: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let owned = forged["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["profile"] == 1)
        .unwrap();
    owned["artifact"]["receipt"]["target"] =
        serde_json::json!(outside.as_os_str().as_encoded_bytes());
    fs::write(&manifest, serde_json::to_vec(&forged).unwrap()).unwrap();
    assert!(registry.delete_profile(label.clone(), false).is_err());
    assert!(!crate::share::pending(registry.root()));
    assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
    fs::write(manifest, original).unwrap();
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
