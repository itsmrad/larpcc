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
    held: Vec<CheckedObject>,
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
    let snapshot = registry.admission_snapshot(name, |p| {
        rows.iter().any(|r| candidate(r, op) && r.provider == p)
    })?;
    let t = &snapshot.target;
    let artifact = checked_artifact(&t.resolved_executable, registry.root())?;
    let row = rows
        .iter()
        .find(|r| {
            candidate(r, op) && r.provider == t.profile.provider && r.sha256 == artifact.sha256
        })
        .ok_or(Error::QualificationPending)?;
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
        if env
            .iter()
            .any(|k| env_equal(k.as_encoded_bytes(), label.as_bytes()))
        {
            conflicts.push(label.to_string());
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
    native_paths(t, row)?;
    let plan = LaunchPlan {
        executable: t.resolved_executable.clone(),
        argv0: t.stored_executable.as_os_str().into(),
        argv: row.argv.iter().map(OsString::from).collect(),
        cwd: None,
        env_delta: vec![(variable.into(), t.state_dir.as_os_str().into())],
        identity: artifact.identity.clone(),
    };
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
fn native_paths(t: &LaunchTarget, row: &ArtifactRow) -> Result<(), Error> {
    for rule in row.paths {
        let relative = Path::new(rule.relative);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(Error::NamespaceUnverified);
        }
        let path = t.state_dir.join(relative);
        let _held = platform::anchor(&path)?;
        if platform::inspect(&path, rule.kind)?.is_none() && !rule.may_be_absent {
            return Err(Error::NamespaceUnverified);
        }
    }
    Ok(())
}
pub(crate) fn checked_artifact(path: &Path, root: &Path) -> Result<CheckedArtifact, Error> {
    let (file, identity) = platform::open_artifact(path, root)?;
    let before = file.metadata().map_err(|_| Error::ArtifactInvalid)?;
    let size = before.len();
    if size > MAX_BYTES {
        return Err(Error::ArtifactInvalid);
    }
    let sha256 = digest(&file, size)?;
    let after = file.metadata().map_err(|_| Error::ArtifactInvalid)?;
    let (_, current) = platform::executable(path, root)?;
    if current != identity || after.len() != size || before.modified().ok() != after.modified().ok()
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
    native_paths(&a.snapshot.target, a.row).map_err(|_| Error::AdmissionChanged)
}
pub(crate) fn confirm(op: Operation, input: &mut impl BufRead) -> Result<(), Error> {
    if !matches!(op, Operation::Login | Operation::Logout) {
        return Ok(());
    }
    let mut line = Vec::new();
    input
        .take(17)
        .read_until(b'\n', &mut line)
        .map_err(|_| Error::ConfirmationDeclined)?;
    if line.len() <= 16 && matches!(line.as_slice(), b"yes\n" | b"yes\r\n") {
        Ok(())
    } else {
        Err(Error::ConfirmationDeclined)
    }
}
pub(crate) fn acquire_auth_guard(root: &Path, id: ProfileId) -> Result<AuthGuard, Error> {
    let mut held = platform::anchor(&root.join("auth-locks").join("sentinel"))?.ancestors;
    let dir = root.join("auth-locks");
    if platform::inspect(&dir, ObjectKind::Directory)?.is_none() {
        match platform::create_directory(&dir) {
            Ok(object) => held.push(object),
            Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    held.push(platform::inspect(&dir, ObjectKind::Directory)?.ok_or(Error::AdmissionChanged)?);
    let path = dir.join(format!("{id}.lock"));
    let object = match platform::create_file(&path) {
        Ok(object) => object,
        Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {
            platform::inspect(&path, ObjectKind::File)?.ok_or(Error::AdmissionChanged)?
        }
        Err(e) => return Err(e),
    };
    object.file.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::AuthBusy,
        std::fs::TryLockError::Error(_) => Error::NamespaceUnverified,
    })?;
    Ok(AuthGuard {
        file: object.file,
        held,
    })
}
pub(crate) fn dispatch(a: Admitted<'_>, guard: Option<AuthGuard>) -> Result<u32, Error> {
    // S transitional boundary: auth spawn/wait cannot run until C/W lifecycle is reviewed.
    if a.row.operation != Operation::Run {
        if let Some(g) = &guard {
            let _ = g.file.metadata();
            let _ = g.held.len();
        }
        return Err(Error::NamespaceUnverified);
    }
    let _held = &a.snapshot.held;
    eprintln!("{ROUTING_NOTICE}");
    if a.row.operation != Operation::Run {
        eprintln!("{AUTH_NOTICE}");
    }
    #[cfg(unix)]
    {
        platform::execve(
            &a.plan.executable,
            &a.plan.argv0,
            &a.plan.argv,
            &a.plan.env_delta,
        )
    }
    #[cfg(windows)]
    {
        super::windows::spawn_wait(&a.plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
