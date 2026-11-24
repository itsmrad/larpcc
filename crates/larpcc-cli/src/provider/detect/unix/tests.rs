use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

pub(super) fn private_dir() -> tempfile::TempDir {
    let parent = std::env::var_os("LARPCC_TEST_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
        .unwrap()
}

fn private_executable(path: &Path) {
    #[cfg(target_os = "linux")]
    let bytes = b"\x7fELFsynthetic candidate";
    #[cfg(target_os = "macos")]
    let bytes = b"\xfe\xed\xfa\xcfsynthetic candidate";
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn scan(path: &Path) -> Scan {
    Scan {
        exclusions: Exclusions::new(Some(path)),
        deadline: Instant::now() + SCAN_TIME,
        limited: Cell::new(false),
        dirs: 0,
        entries: 0,
        checked: HashSet::new(),
        candidates: Vec::new(),
        skipped: BTreeSet::new(),
        managed_root: path.join("managed"),
    }
}

#[test]
fn stage19_guard_stops_before_lookup_inside_vendor_roots() {
    let temp = private_dir();
    let home = temp.path();
    let mut guard = Exclusions::new(Some(home));
    let configured = home.join("custom-provider-state");
    guard.insert(&configured);
    let protected = [
        home.join(".claude2"),
        home.join(".codex"),
        home.join("Library/Application Support/Claude"),
        configured,
    ];
    for (index, root) in protected.iter().enumerate() {
        fs::create_dir_all(root).unwrap();
        let alias = home.join(format!("alias-{index}"));
        symlink(root, &alias).unwrap();
        let allowed = std::cell::RefCell::new(Vec::new());
        let result = walk(
            &alias.join("codex"),
            |path, _| {
                if !guard.allows(path) {
                    assert_eq!(path, root);
                    return false;
                }
                allowed.borrow_mut().push(path.to_owned());
                true
            },
            true,
        );
        assert!(matches!(result, Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied));
        assert!(
            allowed
                .into_inner()
                .iter()
                .all(|path| !path.starts_with(root))
        );
    }
    let escaped = home.join("back-out");
    symlink(".codex/../elsewhere", &escaped).unwrap();
    assert!(matches!(
        walk(&escaped, |p, parent| guard.allows_child(p, parent), false),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied
    ));
    assert!(guard.allows(&home.join(".local/share/claude/versions/1.0")));
    assert!(!guard.allows(&home.join(".local/share/claude/account-data")));
    let mut scan = scan(home);
    scan.exclusions = guard;
    assert!(scan.locate(Provider::Codex, &escaped, true).is_none());
    assert!(
        scan.locate(Provider::Codex, &home.join(".codex/bin/codex"), true)
            .is_none()
    );
    let data = scan.results(Vec::new());
    let skipped = data["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2);
    assert!(
        skipped
            .iter()
            .all(|row| row["reason"] == "inside provider home")
    );
}

#[test]
fn stage19_configured_provider_root_is_excluded_before_walk() {
    let temp = private_dir();
    let path = temp.path();
    let actual = path.join("actual");
    let alias = path.join("alias");
    fs::create_dir(&actual).unwrap();
    symlink("actual", &alias).unwrap();
    let logical = alias.join("private-state");
    let physical = actual.join("private-state");
    let mut scan = scan(path);
    scan.exclusions
        .register(std::slice::from_ref(&logical), || true)
        .unwrap();
    // No protected leaf exists. A syscall on either spelling before denial
    // would return NotFound instead of PermissionDenied.
    for root in [logical, physical] {
        assert!(!scan.exclusions.allows(&root));
        assert!(matches!(
            walk(&root, |p, parent| scan.exclusions.allows_child(p, parent), false),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
        ));
    }
    let renamed = path.join("renamed");
    fs::rename(&actual, &renamed).unwrap();
    let sibling = renamed.join("codéx");
    private_executable(&sibling);
    let relocated = renamed.join("private-state");
    assert!(
        scan.exclusions.allows(&relocated),
        "spelling alone would allow it"
    );
    assert!(scan.locate(Provider::Codex, &relocated, true).is_none());
    assert!(
        scan.locate(Provider::Codex, &sibling, true)
            .unwrap()
            .object
            .unwrap()
            .is_ok()
    );
    assert!(scan.skipped.contains(&("codex", relocated)));
}

#[test]
fn stage19_nonmatching_ancestors_and_siblings_are_allowed() {
    let temp = private_dir();
    let home = temp.path().join("workspaces/Élodie");
    let config = home.join(".config");
    fs::create_dir_all(&config).unwrap();
    let root = config.join("claude");
    let sibling = config.join("claudé");
    private_executable(&sibling);
    let mut guard = Exclusions::new(Some(&home));
    guard
        .register(std::slice::from_ref(&root), || true)
        .unwrap();
    assert!(walk(&sibling, |p, parent| guard.allows_child(p, parent), false).is_ok());
    assert!(matches!(
        walk(&root, |p, parent| guard.allows_child(p, parent), false),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
    ));
}

#[test]
fn stage19_registers_conventional_home_and_xdg_through_symlinked_home() {
    let temp = private_dir();
    let actual = temp.path().join("home");
    let alias = temp.path().join("home-alias");
    let xdg = ["xdg/config", "xdg/data", "xdg/cache", "xdg/state"];
    for parent in xdg.into_iter().chain([
        ".config",
        "Library/Application Support",
        "Library/Containers",
        "Library/Caches",
        "settings",
        ".local/bin",
    ]) {
        fs::create_dir_all(actual.join(parent)).unwrap();
    }
    symlink("home", &alias).unwrap();
    private_executable(&actual.join(".local/bin/codex"));

    for home in [&actual, &alias] {
        let roots = protected_roots(
            Some(home),
            [
                home.join("settings/claude-data"),
                home.join("settings/codex-data"),
            ],
            xdg.map(|parent| home.join(parent)),
        );
        let expected: BTreeSet<_> = [
            "settings/claude-data",
            "settings/codex-data",
            ".config/Claude",
            ".config/ChatGPT",
            ".config/claude",
            ".config/chatgpt",
            "Library/Application Support/Claude",
            "Library/Application Support/ChatGPT",
            "Library/Containers/com.anthropic.claudefordesktop",
            "Library/Containers/com.openai.chat",
            "Library/Caches/com.anthropic.claudefordesktop",
            "Library/Caches/com.openai.chat",
        ]
        .map(|path| home.join(path))
        .into_iter()
        .chain(xdg.into_iter().flat_map(|parent| {
            [
                "Claude",
                "claude",
                "ChatGPT",
                "chatgpt",
                "Codex",
                "codex",
                "com.openai.chat",
                "com.openai.codex",
                "com.anthropic.claudefordesktop",
            ]
            .map(|app| home.join(parent).join(app))
        }))
        .collect();
        assert_eq!(roots.iter().cloned().collect::<BTreeSet<_>>(), expected);
        let mut guard = Exclusions::new(Some(home));
        guard.register(&roots, || true).unwrap();

        for root in &roots {
            let physical = actual.join(root.strip_prefix(home).unwrap());
            for path in [root, &physical] {
                assert!(
                    matches!(
                        walk(path, |p, parent| guard.allows_child(p, parent), false),
                        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
                    ),
                    "expected denial before looking up {}",
                    path.display()
                );
            }
        }
        let candidate = walk(
            &home.join(".local/bin/codex"),
            |p, parent| guard.allows_child(p, parent),
            false,
        )
        .unwrap();
        assert!(candidate.executable());
    }
}

#[test]
fn stage19_fails_closed_when_a_protected_parent_is_ambiguous() {
    let temp = private_dir();
    let path = temp.path();
    let cycle = path.join("cycle");
    symlink("cycle", &cycle).unwrap();
    let mut guard = Exclusions::new(Some(path));
    assert!(
        guard
            .register(&[cycle.join("private-state")], || true)
            .is_err()
    );
    let mut guard = Exclusions::new(Some(path));
    guard
        .register(&[path.join("absent/one"), path.join("absent/two")], || true)
        .unwrap();
    assert!(matches!(
        walk(&path.join("absent"), |p, parent| guard.allows_child(p, parent), false),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
    ));
}

#[test]
fn stage19_overlapping_configured_roots_close_setup_in_either_order() {
    let temp = private_dir();
    let path = temp.path();
    let actual = path.join("actual");
    let alias = path.join("alias");
    let state = actual.join("private-state");
    fs::create_dir_all(&state).unwrap();
    symlink("actual", &alias).unwrap();
    let mut roots = [alias.join("private-state"), state.join("nested")];
    for _ in 0..2 {
        let mut guard = Exclusions::new(Some(path));
        assert!(matches!(
            guard.register(&roots, || true),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
        ));
        roots.reverse();
    }
}

#[test]
fn stage19_overlap_denies_before_edge_metadata_in_either_order() {
    let temp = private_dir();
    let path = temp.path();
    let actual = path.join("actual");
    let alias = path.join("alias");
    fs::create_dir(&actual).unwrap();
    // A directory edge can be opened and rejected only after metadata. A file
    // makes that path NotADirectory; PermissionDenied is the allow callback
    // refusing before the lookup.
    let edge = actual.join("private-state");
    fs::write(&edge, b"sentinel").unwrap();
    symlink("actual", &alias).unwrap();
    let mut roots = [alias.join("private-state"), edge.join("nested")];
    for _ in 0..2 {
        let mut guard = Exclusions::new(Some(path));
        let error = guard.register(&roots, || true).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(&edge).unwrap(), b"sentinel");
        roots.reverse();
    }
}

#[test]
fn stage19_same_basename_elsewhere_stays_available() {
    let temp = private_dir();
    let path = temp.path();
    let home = path.join("claude/Élodie");
    let protected = home.join("notes/custom-state");
    let nested = path.join("projects/custom-state/state");
    let candidate = path.join("bin/custom-state");
    fs::create_dir_all(protected.parent().unwrap()).unwrap();
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::create_dir_all(candidate.parent().unwrap()).unwrap();
    let mut roots = [protected.clone(), nested.clone()];
    for _ in 0..2 {
        let mut guard = Exclusions::new(Some(&home));
        guard.register(&roots, || true).unwrap();
        for root in [&protected, &nested] {
            assert!(matches!(
                walk(root, |p, parent| guard.allows_child(p, parent), false),
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
            ));
        }
        // The leaf is absent. Denial would be PermissionDenied; NotFound means
        // the guard allowed the metadata lookup under the other parent.
        assert!(matches!(
            walk(&candidate, |p, parent| guard.allows_child(p, parent), false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
        roots.reverse();
    }
}

#[test]
fn stage19_skipped_path_matches_frozen_detection_fixture() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/contracts/cli-v1.examples.json"
    )))
    .unwrap();
    let expected = fixture
        .as_array()
        .unwrap()
        .iter()
        .find(|example| example["command"] == "provider.detect")
        .unwrap();
    let path = Path::new("/fixture/.codex/bin/codex");
    let mut scan = scan(Path::new("/fixture"));
    assert!(scan.locate(Provider::Codex, path, true).is_none());
    let data = scan.results(Vec::new());
    assert_eq!(&data, &expected["data"]);
}

#[test]
fn stage19_loops_and_unreadable_directories_have_no_candidates() {
    let temp = private_dir();
    let path = temp.path();
    let cycle = path.join("cycle");
    symlink("cycle", &cycle).unwrap();
    assert!(matches!(
        walk(&cycle, |_, _| true, false),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData
    ));
    let blocked = path.join("blocked");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("version"), b"sentinel").unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    // Root can enumerate through permission bits, so force the permission branch
    // only when the runner's OS actually enforces them for this fixture.
    if fs::read_dir(&blocked).is_err() {
        assert!(scan(path).names(&blocked).is_empty());
    }
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn stage19_deduplicates_bound_install_and_reports_untrusted_candidate() {
    let temp = private_dir();
    let path = temp.path();
    let bound = path.join("selected-claude");
    private_executable(&bound);
    let alias = path.join("claude");
    symlink(&bound, &alias).unwrap();
    let writable = path.join("writable");
    fs::create_dir(&writable).unwrap();
    fs::set_permissions(&writable, fs::Permissions::from_mode(0o777)).unwrap();
    let untrusted = writable.join("claude");
    private_executable(&untrusted);

    let mut scan = scan(path);
    scan.offer(Provider::Claude, alias, Kind::Homebrew, Some(&bound));
    scan.offer(
        Provider::Claude,
        untrusted.clone(),
        Kind::Unknown,
        Some(&bound),
    );
    scan.offer(Provider::Claude, bound.clone(), Kind::Unknown, Some(&bound));
    let result = scan.results(Vec::new());
    let results = result["executables"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["path"], display::path(&bound));
    assert_eq!(results[0]["bindable"], true);
    assert_eq!(results[0]["bound"], true);
    assert_eq!(results[0]["qualified"], false);
    assert_eq!(results[0]["version"], "unapproved");
    assert_eq!(results[1]["path"], display::path(&untrusted));
    assert_eq!(results[1]["bindable"], false);
    assert_eq!(results[1]["sha256"], Value::Null);
}

#[cfg(target_os = "macos")]
#[test]
fn stage19_bundle_plist_refuses_symlinked_contents() {
    let temp = private_dir();
    let home = temp.path();
    let root = home.join("ChatGPT.app");
    fs::create_dir(&root).unwrap();
    symlink(home.join(".codex"), root.join("Contents")).unwrap();
    let guard = Exclusions::new(Some(home));
    let directory = fs::File::open(&root).unwrap();
    let row = crate::desktop::detect_app(Provider::Codex, &root, &root, &directory, |p, parent| {
        guard.allows_child(p, Some(parent))
    });
    assert_eq!(row["bundle_id"], Value::Null);
    assert_eq!(row["code_requirement"], "unavailable");
}

#[test]
fn stage19_homebrew_cask_is_classified_and_retains_bind_refusal() {
    let temp = private_dir();
    let path = temp.path();
    let caskroom = path.join("Caskroom");
    let version = caskroom.join("claude-code/2.1.0");
    fs::create_dir_all(&version).unwrap();
    let executable = version.join("claude");
    private_executable(&executable);
    fs::create_dir(path.join("bin")).unwrap();
    symlink(
        "../Caskroom/claude-code/2.1.0/claude",
        path.join("bin/claude"),
    )
    .unwrap();
    fs::set_permissions(&caskroom, fs::Permissions::from_mode(0o777)).unwrap();

    let mut scan = scan(path);
    scan.homebrew_locations(path, Provider::Claude, None);
    assert_eq!(scan.candidates.len(), 2);
    let data = scan.results(Vec::new());
    let rows = data["executables"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["resolved_path"], display::path(&executable));
    assert_eq!(rows[0]["install_kind"], "homebrew");
    assert_eq!(rows[0]["bindable"], false);
    assert_eq!(rows[0]["bind_reason"], "has a foreign-writable ancestor");
}

#[test]
fn stage19_recheck_refuses_ancestor_retarget_into_denied_subtree() {
    let temp = private_dir();
    let home = temp.path();
    let bin = home.join("bin");
    fs::create_dir(&bin).unwrap();
    let candidate = bin.join("codex");
    private_executable(&candidate);
    let mut scan = scan(home);
    scan.offer(Provider::Codex, candidate.clone(), Kind::Unknown, None);
    assert!(scan.candidates[0].object.as_ref().unwrap().is_ok());

    fs::rename(&bin, home.join("original")).unwrap();
    // The target does not exist. A lookup before the guard would instead
    // return NotFound and produce no skipped row.
    symlink(home.join(".codex-denied"), &bin).unwrap();
    let observed = scan.results(Vec::new());
    assert_eq!(observed["executables"], json!([]));
    assert_eq!(observed["skipped"][0]["path"], display::path(&candidate));
    assert_eq!(observed["skipped"][0]["reason"], "inside provider home");
}

#[cfg(feature = "synthetic-launch")]
#[test]
fn stage19_matches_exact_descriptor_and_keeps_execution_closed() {
    use sha2::{Digest, Sha256};
    use version::{VersionDescriptor, synthetic_descriptors};

    let temp = private_dir();
    let path = temp.path().join("codex");
    private_executable(&path);
    let descriptor = VersionDescriptor {
        provider: Provider::Codex,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        sha256: Sha256::digest(fs::read(&path).unwrap()).into(),
        argv: &["--version"],
        home_env: version::home_env(Provider::Codex),
        prefix: "codex ",
        suffix: "",
        evidence: "synthetic Stage 19 fixture",
    };
    let result = |descriptor| {
        synthetic_descriptors(vec![descriptor]);
        let mut scan = scan(temp.path());
        scan.offer(Provider::Codex, path.clone(), Kind::Native, None);
        scan.results(Vec::new())["executables"][0].clone()
    };
    let matched = result(descriptor.clone());
    assert_eq!(matched["version"], Value::Null);
    assert_eq!(matched["version_diagnostic"], "version_probe_unavailable");
    for descriptor in [
        VersionDescriptor {
            os: "other-os",
            ..descriptor.clone()
        },
        VersionDescriptor {
            arch: "other-arch",
            ..descriptor.clone()
        },
        VersionDescriptor {
            sha256: [0; 32],
            ..descriptor.clone()
        },
        VersionDescriptor {
            provider: Provider::Claude,
            home_env: version::home_env(Provider::Claude),
            ..descriptor
        },
    ] {
        let unmatched = result(descriptor);
        assert_eq!(unmatched["version"], "unapproved");
        assert_eq!(unmatched["version_diagnostic"], "version_unapproved");
    }
    assert!(!temp.path().join("managed").exists());
}
