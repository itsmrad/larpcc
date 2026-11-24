use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

fn private_dir() -> tempfile::TempDir {
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
            |path| {
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
        walk(&escaped, |p| guard.allows(p), false),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied
    ));
    assert!(guard.allows(&home.join(".local/share/claude/versions/1.0")));
    assert!(!guard.allows(&home.join(".local/share/claude/account-data")));
    let mut scan = scan(home);
    scan.exclusions = guard;
    assert!(scan.locate(Provider::Codex, &escaped).is_none());
    assert!(
        scan.locate(Provider::Codex, &home.join(".codex/bin/codex"))
            .is_none()
    );
    let data = scan.results(&home.join("managed"), Vec::new()).unwrap();
    let skipped = data["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2);
    assert!(
        skipped
            .iter()
            .all(|row| row["reason"] == "inside provider home")
    );
}

#[test]
fn stage19_symlinked_provider_root_excludes_its_target_spelling() {
    let temp = private_dir();
    let path = temp.path();
    let real = path.join("private-state");
    fs::create_dir(&real).unwrap();
    let alias = path.join(".codex-45");
    symlink(&real, &alias).unwrap();
    let mut scan = scan(path);
    scan.exclude_alias(&alias);
    assert!(!scan.exclusions.allows(&real));
    assert!(!scan.exclusions.allows(&real.join("bin/codex")));
}

#[test]
fn stage19_loops_and_unreadable_directories_have_no_candidates() {
    let temp = private_dir();
    let path = temp.path();
    let cycle = path.join("cycle");
    symlink("cycle", &cycle).unwrap();
    assert!(matches!(
        walk(&cycle, |_| true, false),
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
    let result = scan.results(&path.join("managed"), Vec::new()).unwrap();
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
fn stage19_bundle_signature_preflight_rejects_external_resource_links() {
    let temp = private_dir();
    let home = temp.path();
    let root = home.join("ChatGPT.app");
    fs::create_dir_all(root.join("Contents/Resources")).unwrap();
    let file = root.join("Contents/Resources/real-resource");
    fs::write(&file, b"bundle file").unwrap();
    let link = root.join("Contents/Resources/link");
    symlink("real-resource", &link).unwrap();
    let guard = Exclusions::new(Some(home));
    assert!(super::fs::confined_bundle(&root, |p| guard.allows(p)));
    fs::remove_file(&link).unwrap();
    symlink(home.join(".codex"), &link).unwrap();
    assert!(!super::fs::confined_bundle(&root, |p| guard.allows(p)));
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
    let data = scan.results(&path.join("managed"), Vec::new()).unwrap();
    let rows = data["executables"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["resolved_path"], display::path(&executable));
    assert_eq!(rows[0]["install_kind"], "homebrew");
    assert_eq!(rows[0]["bindable"], false);
    assert_eq!(rows[0]["bind_reason"], "has a foreign-writable ancestor");
}
