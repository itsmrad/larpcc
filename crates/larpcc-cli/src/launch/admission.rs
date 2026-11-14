use super::LaunchPlan;
use crate::{
    platform::{self, CheckedObject, FileIdentity, ObjectKind},
    registry::{Error, LaunchTarget, ProfileId, ProfileName, Provider, Registry},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
};

pub(crate) const ROUTING_NOTICE: &str = "Profiles select native state, not a verified account, model or endpoint. Native project, configuration and administrator policy still apply. Provider restrictions remain possible.";
pub(crate) const AUTH_NOTICE: &str = "Quiesce relevant sessions before authentication changes. Refresh and logout may affect grants and integrations.";
pub(crate) const AUTH_HELP: &str = "Profiles select native state, not a verified account, model or endpoint. Native project, configuration and administrator policy still apply. Provider restrictions remain possible.\nQuiesce relevant sessions before authentication changes. Refresh and logout may affect grants and integrations.";
pub(crate) const ROWS: &[ArtifactRow] = &[];
const MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Run,
    Login,
    Status,
    Logout,
}
impl Operation {
    fn label(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Login => "login",
            Self::Status => "status",
            Self::Logout => "logout",
        }
    }
}
// Every value is reviewed row data, never inferred from an installed provider.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ArtifactRow {
    pub provider: Provider,
    pub release: &'static str,
    pub sha256: [u8; 32],
    pub install_kind: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub operation: Operation,
    pub interface: &'static str,
    pub backend: &'static str,
    pub policy_revision: u64,
    pub argv: &'static [&'static str],
    pub denied_env: &'static [&'static str],
    pub paths: &'static [NativePath],
    pub evidence: &'static str,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NativePath {
    pub relative: &'static str,
    pub kind: ObjectKind,
    pub may_be_absent: bool,
}
pub(crate) struct AdmissionSnapshot {
    pub revision: u64,
    pub target: LaunchTarget,
    pub homes: Vec<(ProfileId, FileIdentity)>,
    pub held: Vec<CheckedObject>,
}
pub(crate) struct CheckedArtifact {
    pub file: File,
    pub identity: FileIdentity,
    pub size: u64,
    pub sha256: [u8; 32],
}
pub(crate) struct Admitted<'a> {
    pub root: PathBuf,
    pub snapshot: AdmissionSnapshot,
    pub row: &'a ArtifactRow,
    pub artifact: CheckedArtifact,
    pub plan: LaunchPlan,
}
pub(crate) struct AuthGuard {
    file: File,
    // Own ancestry pins through direct-child reap, including newly created directories.
    _held: Vec<CheckedObject>,
}

fn candidate(row: &ArtifactRow, op: Operation) -> bool {
    row.operation == op && row.os == std::env::consts::OS && row.arch == std::env::consts::ARCH
}
pub(crate) fn invoke(
    op: Operation,
    name: &ProfileName,
    args: Vec<OsString>,
    yes: bool,
    rows: &[ArtifactRow],
) -> Result<u32, Error> {
    // This check precedes even root/environment discovery.
    if !rows.iter().any(|r| candidate(r, op)) {
        return Err(Error::QualificationPending);
    }
    let registry = Registry::locate()?;
    let admitted = admit(
        &registry,
        op,
        name,
        args,
        rows,
        &std::env::vars_os().map(|(k, _)| k).collect::<Vec<_>>(),
    )?;
    let guard = if op == Operation::Run {
        None
    } else {
        Some(acquire_auth_guard(
            registry.root(),
            admitted.snapshot.target.profile.id,
        )?)
    };
    if matches!(op, Operation::Login | Operation::Logout) && !yes {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            return Err(Error::ConfirmationRequired);
        }
        let t = &admitted.snapshot.target;
        eprint!(
            "{} {} (ID {}, {}, {})\nNative state and grants may change. Continue? Type yes: ",
            op.label(),
            t.profile.name,
            t.profile.id,
            t.profile.provider,
            crate::registry::display::path(&t.state_dir)
        );
        io::stderr()
            .flush()
            .map_err(|_| Error::ConfirmationDeclined)?;
        confirm(op, &mut io::stdin().lock())?;
    }
    revalidate(&admitted)?;
    if let Some(guard) = &guard {
        revalidate_guard(registry.root(), admitted.snapshot.target.profile.id, guard)?;
    }
    dispatch(admitted, guard)
}
pub(crate) fn admit<'a>(
    registry: &Registry,
    op: Operation,
    name: &ProfileName,
    args: Vec<OsString>,
    rows: &'a [ArtifactRow],
    env: &[OsString],
) -> Result<Admitted<'a>, Error> {
    if !rows.iter().any(|r| candidate(r, op)) {
        return Err(Error::QualificationPending);
    }
    let mut snapshot = registry.admission_snapshot(name, |p| {
        rows.iter().any(|r| candidate(r, op) && r.provider == p)
    })?;
    let t = &snapshot.target;
    let artifact = checked_artifact(&t.resolved_executable, registry.root())?;
    let row = rows
        .iter()
        .find(|r| {
            candidate(r, op) && r.provider == t.profile.provider && r.sha256 == artifact.sha256
        })
        .ok_or(Error::ArtifactInvalid)?;
    if row.release.is_empty()
        || row.install_kind.is_empty()
        || row.interface.is_empty()
        || row.backend.is_empty()
        || row.policy_revision == 0
        || row.evidence.is_empty()
    {
        return Err(Error::ArtifactInvalid);
    }
    let variable = match t.profile.provider {
        Provider::Claude => "CLAUDE_CONFIG_DIR",
        Provider::Codex => "CODEX_HOME",
    };
    let mut conflicts = Vec::new();
    for label in std::iter::once(variable).chain(row.denied_env.iter().copied()) {
        if env.iter().any(|k| {
            let key = k.as_encoded_bytes();
            if let Some(prefix) = label.strip_suffix('*') {
                key.get(..prefix.len())
                    .is_some_and(|bytes| env_equal(bytes, prefix.as_bytes()))
            } else {
                env_equal(key, label.as_bytes())
            }
        }) {
            conflicts.push(label.trim_end_matches('*').to_string());
        }
    }
    conflicts.sort();
    conflicts.dedup();
    if !conflicts.is_empty() {
        return Err(Error::EnvConflict { names: conflicts });
    }
    if !args.is_empty() {
        return Err(Error::ArgConflict {
            label: "argument syntax",
        });
    }
    let native_held = native_paths(t, row)?;
    let plan = LaunchPlan {
        executable: t.resolved_executable.clone(),
        argv0: t.stored_executable.as_os_str().into(),
        argv: row.argv.iter().map(OsString::from).collect(),
        cwd: None,
        env_delta: vec![(variable.into(), t.state_dir.as_os_str().into())],
        identity: artifact.identity.clone(),
    };
    snapshot.held.extend(native_held);
    Ok(Admitted {
        root: registry.root().into(),
        snapshot,
        row,
        artifact,
        plan,
    })
}
fn env_equal(a: &[u8], b: &[u8]) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}
fn native_paths(t: &LaunchTarget, row: &ArtifactRow) -> Result<Vec<CheckedObject>, Error> {
    let mut held = Vec::new();
    for rule in row.paths {
        let relative = Path::new(rule.relative);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(Error::NamespaceUnverified);
        }
        let mut path = t.state_dir.clone();
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            path.push(component);
            let kind = if components.peek().is_some() {
                ObjectKind::Directory
            } else {
                rule.kind
            };
            match platform::inspect(&path, kind).map_err(|_| Error::NamespaceUnverified)? {
                Some(object) => {
                    #[cfg(unix)]
                    if kind == ObjectKind::File {
                        platform::single_link(&object.file)?;
                    }
                    held.push(object);
                }
                None if rule.may_be_absent => break,
                None => return Err(Error::NamespaceUnverified),
            }
        }
    }
    Ok(held)
}
pub(crate) fn checked_artifact(path: &Path, root: &Path) -> Result<CheckedArtifact, Error> {
    checked_artifact_with(path, root, |_| {})
}
fn checked_artifact_with(
    path: &Path,
    root: &Path,
    before_read: impl FnOnce(&File),
) -> Result<CheckedArtifact, Error> {
    let (file, identity) = platform::open_artifact(path, root)?;
    let before = file.metadata().map_err(|_| Error::ArtifactInvalid)?;
    let size = before.len();
    if size > MAX_BYTES {
        return Err(Error::ArtifactInvalid);
    }
    before_read(&file);
    let sha256 = digest(&file, size)?;
    let after = file.metadata().map_err(|_| Error::ArtifactInvalid)?;
    let (verified, current) =
        platform::open_artifact(path, root).map_err(|_| Error::ArtifactInvalid)?;
    let current_size = verified
        .metadata()
        .map_err(|_| Error::ArtifactInvalid)?
        .len();
    if current != identity
        || current_size != size
        || after.len() != size
        || before.modified().map_err(|_| Error::ArtifactInvalid)?
            != after.modified().map_err(|_| Error::ArtifactInvalid)?
    {
        return Err(Error::ArtifactInvalid);
    }
    Ok(CheckedArtifact {
        file,
        identity,
        size,
        sha256,
    })
}
fn digest(mut file: &File, size: u64) -> Result<[u8; 32], Error> {
    if size > MAX_BYTES {
        return Err(Error::ArtifactInvalid);
    }
    let mut hash = Sha256::new();
    let mut remaining = size;
    let mut buffer = [0u8; 64 * 1024];
    while remaining != 0 {
        let length = buffer.len().min(remaining as usize);
        let count = file
            .read(&mut buffer[..length])
            .map_err(|_| Error::ArtifactInvalid)?;
        if count == 0 {
            return Err(Error::ArtifactInvalid);
        }
        remaining -= count as u64;
        hash.update(&buffer[..count]);
    }
    // Never read beyond the actual-byte cap, even to detect a growing stream.
    if file.metadata().map_err(|_| Error::ArtifactInvalid)?.len() != size {
        return Err(Error::ArtifactInvalid);
    }
    Ok(hash.finalize().into())
}
pub(crate) fn revalidate(a: &Admitted<'_>) -> Result<(), Error> {
    if a.plan.identity != a.artifact.identity {
        return Err(Error::AdmissionChanged);
    }
    let registry = Registry::at(a.root.clone())?;
    let _lock = registry
        .admission_lock()
        .map_err(|_| Error::AdmissionChanged)?;
    let fresh = registry
        .launch_target(&a.snapshot.target.profile.name)
        .map_err(|_| Error::AdmissionChanged)?;
    if fresh.revision != a.snapshot.revision
        || fresh.target != a.snapshot.target
        || fresh.homes != a.snapshot.homes
    {
        return Err(Error::AdmissionChanged);
    }
    let fresh =
        checked_artifact(&a.plan.executable, &a.root).map_err(|_| Error::AdmissionChanged)?;
    if fresh.identity != a.artifact.identity
        || fresh.size != a.artifact.size
        || fresh.sha256 != a.artifact.sha256
        || a.artifact
            .file
            .metadata()
            .map_err(|_| Error::AdmissionChanged)?
            .len()
            != a.artifact.size
    {
        return Err(Error::AdmissionChanged);
    }
    native_paths(&a.snapshot.target, a.row)
        .map(|_| ())
        .map_err(|_| Error::AdmissionChanged)
}
pub(crate) fn confirm(op: Operation, input: &mut impl BufRead) -> Result<(), Error> {
    if !matches!(op, Operation::Login | Operation::Logout) {
        return Ok(());
    }
    let mut line = Vec::new();
    input
        .take(16)
        .read_until(b'\n', &mut line)
        .map_err(|_| Error::ConfirmationDeclined)?;
    if line.len() <= 16 && matches!(line.as_slice(), b"yes\n" | b"yes\r\n") {
        Ok(())
    } else {
        Err(Error::ConfirmationDeclined)
    }
}
pub(crate) fn acquire_auth_guard(root: &Path, id: ProfileId) -> Result<AuthGuard, Error> {
    let root_object =
        platform::inspect(root, ObjectKind::Directory)?.ok_or(Error::AdmissionChanged)?;
    let mut held = platform::anchor(root)?.ancestors;
    held.push(root_object);
    let dir = root.join("auth-locks");
    if platform::inspect(&dir, ObjectKind::Directory)?.is_none() {
        match platform::create_directory(&dir) {
            Ok(object) => held.push(object),
            Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    held.push(platform::inspect(&dir, ObjectKind::Directory)?.ok_or(Error::AdmissionChanged)?);
    // The sentinel is never opened/created; anchor retains the new directory itself
    // with the platform's ancestry handle semantics, including Windows rename pins.
    held.extend(platform::anchor(&dir.join("sentinel"))?.ancestors);
    let path = dir.join(format!("{id}.lock"));
    let object = match platform::create_file(&path) {
        Ok(object) => object,
        Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {
            platform::open_auth_lock(&path)?
        }
        Err(e) => return Err(e),
    };
    #[cfg(unix)]
    platform::single_link(&object.file)?;
    if object
        .file
        .metadata()
        .map_err(|_| Error::NamespaceUnverified)?
        .len()
        != 0
    {
        return Err(Error::NamespaceUnverified);
    }
    object.file.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::AuthBusy,
        std::fs::TryLockError::Error(_) => Error::NamespaceUnverified,
    })?;
    Ok(AuthGuard {
        file: object.file,
        _held: held,
    })
}
fn revalidate_guard(root: &Path, id: ProfileId, guard: &AuthGuard) -> Result<(), Error> {
    let path = root.join("auth-locks").join(format!("{id}.lock"));
    let current = platform::open_auth_lock(&path).map_err(|_| Error::AdmissionChanged)?;
    let owned = platform::verify_opened(
        guard
            .file
            .try_clone()
            .map_err(|_| Error::AdmissionChanged)?,
        &path,
        ObjectKind::File,
    )
    .map_err(|_| Error::AdmissionChanged)?;
    #[cfg(unix)]
    platform::single_link(&owned.file).map_err(|_| Error::AdmissionChanged)?;
    if current.identity != owned.identity
        || owned
            .file
            .metadata()
            .map_err(|_| Error::AdmissionChanged)?
            .len()
            != 0
    {
        return Err(Error::AdmissionChanged);
    }
    Ok(())
}
pub(crate) fn dispatch(a: Admitted<'_>, _guard: Option<AuthGuard>) -> Result<u32, Error> {
    // Admitted and AuthGuard own their path handles until the explicit drops after reap.
    eprintln!("{ROUTING_NOTICE}");
    if a.row.operation != Operation::Run {
        eprintln!("{AUTH_NOTICE}");
    }
    #[cfg(unix)]
    {
        if a.row.operation == Operation::Run {
            return platform::execve(
                &a.plan.executable,
                &a.plan.argv0,
                &a.plan.argv,
                &a.plan.env_delta,
            );
        }
        let exit = platform::auth_spawn_wait(&a.plan)?;
        drop(_guard);
        drop(a);
        match exit {
            platform::AuthExit::Code(code) => Ok(code),
            platform::AuthExit::Signal(signal) => platform::terminate_by_signal(signal),
        }
    }
    #[cfg(windows)]
    {
        super::windows::spawn_wait(&a.plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn fixture() -> (tempfile::TempDir, Registry, ArtifactRow) {
        use std::os::unix::fs::PermissionsExt;
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let temp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let registry = Registry::at(temp.path().join("managed")).unwrap();
        registry
            .add("work".parse().unwrap(), Provider::Codex)
            .unwrap();
        let path = temp.path().join("fake");
        #[cfg(target_os = "linux")]
        let magic = b"\x7fELF";
        #[cfg(target_os = "macos")]
        let magic = b"\xcf\xfa\xed\xfe";
        std::fs::write(&path, [magic.as_slice(), b"AAAA"].concat()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        registry.bind(Provider::Codex, path.clone(), false).unwrap();
        let digest = checked_artifact(&path, registry.root()).unwrap().sha256;
        let row = ArtifactRow {
            provider: Provider::Codex,
            release: "invented",
            sha256: digest,
            install_kind: "native-fake",
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            operation: Operation::Run,
            interface: "invented",
            backend: "invented",
            policy_revision: 1,
            argv: &[],
            denied_env: &[],
            paths: &[],
            evidence: "fixture-only",
        };
        (temp, registry, row)
    }
    #[cfg(unix)]
    #[test]
    fn replacement_rebind_home_and_revision_changes_invalidate_snapshot() {
        use std::os::unix::fs::PermissionsExt;
        for change in [
            "bytes",
            "identity",
            "home",
            "rename",
            "rebind",
            "forget",
            "vanished-root",
        ] {
            let (temp, registry, row) = fixture();
            let rows = [row];
            let admitted = admit(
                &registry,
                Operation::Run,
                &"work".parse().unwrap(),
                vec![],
                &rows,
                &[],
            )
            .unwrap();
            let path = temp.path().join("fake");
            match change {
                "bytes" => {
                    use std::io::{Seek, SeekFrom};
                    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
                    file.seek(SeekFrom::Start(4)).unwrap();
                    file.write_all(b"BBBB").unwrap();
                }
                "identity" | "rebind" => {
                    let replacement = temp.path().join("replacement");
                    std::fs::copy(&path, &replacement).unwrap();
                    std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o700))
                        .unwrap();
                    if change == "identity" {
                        std::fs::rename(replacement, &path).unwrap();
                    } else {
                        registry.bind(Provider::Codex, replacement, true).unwrap();
                    }
                }
                "home" => {
                    let home = registry.root().join("profiles/1");
                    std::fs::rename(&home, temp.path().join("old-home")).unwrap();
                    platform::create_directory(&home).unwrap();
                }
                "rename" => {
                    registry
                        .rename(&"work".parse().unwrap(), "renamed".parse().unwrap())
                        .unwrap();
                }
                "vanished-root" => {
                    std::fs::rename(registry.root(), temp.path().join("old-root")).unwrap();
                }
                "forget" => {
                    registry.forget(&"work".parse().unwrap()).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                matches!(revalidate(&admitted), Err(Error::AdmissionChanged)),
                "{change}"
            );
            if change == "vanished-root" {
                assert!(!registry.root().exists());
            } else if change == "identity" {
                assert!(
                    admit(
                        &registry,
                        Operation::Run,
                        &"work".parse().unwrap(),
                        vec![],
                        &rows,
                        &[]
                    )
                    .is_ok()
                );
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn native_metadata_rules_check_each_component_without_following_links() {
        use std::os::unix::fs::symlink;
        let (temp, registry, mut row) = fixture();
        row.paths = &[NativePath {
            relative: "private/commit",
            kind: ObjectKind::File,
            may_be_absent: true,
        }];
        let private = registry.root().join("profiles/1/private");
        let name = "work".parse().unwrap();
        assert!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec![],
                std::slice::from_ref(&row),
                &[]
            )
            .is_ok()
        );
        platform::create_directory(&private).unwrap();
        platform::create_file(&private.join("commit")).unwrap();
        assert!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec![],
                std::slice::from_ref(&row),
                &[]
            )
            .is_ok()
        );
        std::fs::hard_link(private.join("commit"), temp.path().join("alias")).unwrap();
        assert!(matches!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec![],
                std::slice::from_ref(&row),
                &[]
            ),
            Err(Error::NamespaceUnverified)
        ));
        std::fs::remove_file(temp.path().join("alias")).unwrap();
        std::fs::rename(&private, temp.path().join("saved")).unwrap();
        symlink(temp.path().join("saved"), &private).unwrap();
        assert!(matches!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec![],
                std::slice::from_ref(&row),
                &[]
            ),
            Err(Error::NamespaceUnverified)
        ));
    }
    #[cfg(unix)]
    #[test]
    fn unrelated_rows_close_before_selected_home_and_artifact_access() {
        use std::os::unix::fs::symlink;
        let (temp, registry, mut row) = fixture();
        let home = registry.root().join("profiles/1");
        std::fs::remove_dir(&home).unwrap();
        symlink(temp.path().join("missing"), &home).unwrap();
        row.provider = Provider::Claude;
        assert!(matches!(
            admit(
                &registry,
                Operation::Run,
                &"work".parse().unwrap(),
                vec![],
                &[row],
                &[]
            ),
            Err(Error::QualificationPending)
        ));
    }
    #[cfg(unix)]
    #[test]
    fn artifact_read_limit_refuses_oversize_without_consuming_bytes() {
        use std::io::{Seek, SeekFrom};
        let (temp, registry, _) = fixture();
        let path = temp.path().join("fake");
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(matches!(
            checked_artifact(&path, registry.root()),
            Err(Error::ArtifactInvalid)
        ));
        file.seek(SeekFrom::Start(0)).unwrap();
        assert!(matches!(
            digest(&file, MAX_BYTES + 1),
            Err(Error::ArtifactInvalid)
        ));
        assert_eq!(file.stream_position().unwrap(), 0);
    }
    #[cfg(unix)]
    #[test]
    fn changing_checked_stream_and_late_security_changes_refuse() {
        use std::os::unix::fs::PermissionsExt;
        for change in ["grow", "shrink", "mode", "replace"] {
            let (temp, registry, _) = fixture();
            let path = temp.path().join("fake");
            let result = checked_artifact_with(&path, registry.root(), |_| match change {
                "grow" => {
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .unwrap()
                        .write_all(b"growth")
                        .unwrap();
                }
                "shrink" => {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .unwrap()
                        .set_len(4)
                        .unwrap();
                }
                "mode" => {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777)).unwrap()
                }
                _ => {
                    let new = temp.path().join("replacement");
                    std::fs::copy(&path, &new).unwrap();
                    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o700)).unwrap();
                    std::fs::rename(new, &path).unwrap();
                }
            });
            assert!(matches!(result, Err(Error::ArtifactInvalid)), "{change}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn row_policy_and_early_artifact_precedence_are_closed() {
        let (_temp, registry, mut row) = fixture();
        let name = "work".parse().unwrap();
        row.os = "invented-other-os";
        assert!(matches!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec![],
                std::slice::from_ref(&row),
                &[]
            ),
            Err(Error::QualificationPending)
        ));
        row.os = std::env::consts::OS;
        row.operation = Operation::Status;
        assert!(matches!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec![],
                std::slice::from_ref(&row),
                &[]
            ),
            Err(Error::QualificationPending)
        ));
        row.operation = Operation::Run;
        row.policy_revision = 0;
        assert!(matches!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec![],
                std::slice::from_ref(&row),
                &[]
            ),
            Err(Error::ArtifactInvalid)
        ));
        row.policy_revision = 1;
        row.sha256 = [0; 32];
        row.paths = &[NativePath {
            relative: "../forbidden",
            kind: ObjectKind::File,
            may_be_absent: false,
        }];
        assert!(matches!(
            admit(
                &registry,
                Operation::Run,
                &name,
                vec!["secret".into()],
                std::slice::from_ref(&row),
                &["CODEX_HOME".into()]
            ),
            Err(Error::ArtifactInvalid)
        ));
    }
    #[test]
    fn confirmation_is_one_bounded_exact_line() {
        for op in [Operation::Login, Operation::Logout] {
            for value in [b"yes\n".as_slice(), b"yes\r\n"] {
                assert!(confirm(op, &mut io::Cursor::new(value)).is_ok());
            }
            for value in [
                b"yes".as_slice(),
                b"no\n",
                b"YES\n",
                b"yes \n",
                b"",
                b"yesxxxxxxxxxxxxxxx\n",
            ] {
                assert!(matches!(
                    confirm(op, &mut io::Cursor::new(value)),
                    Err(Error::ConfirmationDeclined)
                ));
            }
        }
    }
    #[test]
    fn streaming_digest_uses_actual_checked_bytes() {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let mut file = tempfile::tempfile_in(parent).unwrap();
        file.write_all(b"abc").unwrap();
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(
            digest(&file, 3)
                .unwrap()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        file.seek(SeekFrom::Start(0)).unwrap();
        assert!(matches!(digest(&file, 4), Err(Error::ArtifactInvalid)));
    }
}
