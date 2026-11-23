//! Explicit desktop dispatch; native account state is never inspected or asserted.
use crate::{
    platform::{self, AnchoredPath, CheckedObject, ObjectKind},
    registry::{Error, ProfileId, Provider},
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignatureCache {
    version: u8,
    executable: [i64; 7],
    info: [i64; 7],
    cdhash: Vec<u8>,
}
#[cfg(any(target_os = "macos", test))]
fn signature_check_needed(
    bind: bool,
    cached: Option<&SignatureCache>,
    current: &SignatureCache,
) -> bool {
    bind || cached != Some(current)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DesktopReceipt {
    pub profile_id: ProfileId,
    pub provider: Provider,
    pub surface: &'static str,
    pub outcome: &'static str,
    pub native_auth: &'static str,
}

pub(crate) struct CheckedApp {
    path: PathBuf,
    ancestry: AnchoredPath,
    executable: CheckedObject,
    info: CheckedObject,
    executable_stamp: AppFileStamp,
    info_stamp: AppFileStamp,
}
#[derive(PartialEq, Eq)]
struct AppFileStamp {
    size: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
}
impl AppFileStamp {
    fn of(object: &CheckedObject) -> Result<Self, Error> {
        let meta = object
            .file
            .metadata()
            .map_err(|_| Error::AdmissionChanged)?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            #[cfg(unix)]
            changed: (meta.ctime(), meta.ctime_nsec()),
            size: meta.len(),
            modified: meta.modified().map_err(|_| Error::AdmissionChanged)?,
        })
    }
}
fn app_anchor(path: &Path) -> Result<AnchoredPath, Error> {
    #[cfg(unix)]
    return platform::vendor_app_anchor(path);
    #[cfg(windows)]
    platform::anchor(path)
}
fn app_file(path: &Path) -> Result<CheckedObject, Error> {
    #[cfg(unix)]
    return platform::vendor_app_object(path, ObjectKind::File);
    #[cfg(windows)]
    platform::inspect(path, ObjectKind::File)?.ok_or(Error::ArtifactInvalid)
}
// Shared fixed argument vector for the native launcher and invented-target tests.
pub(crate) fn open_arguments(
    target: &ValidatedDesktopTarget,
) -> Result<Vec<std::ffi::OsString>, Error> {
    use std::ffi::OsString;
    let cli = launch_path(&target.snapshot.cli.path)?;
    let desktop = launch_path(&target.snapshot.desktop.path)?;
    let mut environment = OsString::from(match target.snapshot.provider {
        Provider::Codex => "CODEX_HOME=",
        Provider::Claude => "CLAUDE_CONFIG_DIR=",
    });
    environment.push(cli);
    let mut data = OsString::from("--user-data-dir=");
    data.push(desktop);
    Ok(vec![
        "-n".into(),
        "-a".into(),
        target.app.path.clone().into_os_string(),
        "--env".into(),
        environment,
        "--args".into(),
        data,
    ])
}
fn launch_path(path: &Path) -> Result<&str, Error> {
    let value = path.to_str().ok_or(Error::QualificationPending)?;
    if value.bytes().any(|b| b == b'=' || b < 0x20 || b == 0x7f) {
        return Err(Error::QualificationPending);
    }
    Ok(value)
}
fn provider_environment_name(name: &[u8]) -> bool {
    [b"CLAUDE".as_slice(), b"ANTHROPIC_", b"CODEX_", b"OPENAI_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}
#[cfg(any(target_os = "macos", test))]
pub(crate) fn open_command(
    args: Vec<std::ffi::OsString>,
    names: impl IntoIterator<Item = std::ffi::OsString>,
) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    command.args(args);
    for name in names {
        if provider_environment_name(name.as_encoded_bytes()) {
            command.env_remove(name);
        }
    }
    command
}
fn supports_open_environment(version: &[u8]) -> bool {
    let Ok(version) = std::str::from_utf8(version) else {
        return false;
    };
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !parts
        .clone()
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    major.parse::<u32>().is_ok_and(|major| major >= 13)
}

/// Path spelling is necessary but never sufficient evidence of stock identity.
/// Only static code validation is performed; no vendor process is executed.
pub(crate) fn validate_binding(
    provider: Provider,
    path: &Path,
    cached: Option<&SignatureCache>,
    bind: bool,
) -> Result<(CheckedApp, Option<SignatureCache>), Error> {
    if provider == Provider::Claude {
        return Err(Error::QualificationPending);
    }
    let expected = match provider {
        Provider::Claude => Path::new("/Applications/Claude.app"),
        Provider::Codex => Path::new("/Applications/ChatGPT.app"),
    };
    if path.as_os_str() != expected.as_os_str() {
        return Err(Error::ArtifactInvalid);
    }
    #[cfg(target_os = "macos")]
    if provider == Provider::Codex {
        return macos::validate(path, cached, bind);
    }
    let _ = (cached, bind);
    Err(Error::ArtifactInvalid)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InspectionCode {
    Unavailable,
    Missing,
    ForeignHost,
    Stale,
    Denied,
    Malformed,
    Truncated,
    Timeout,
    IdentityChanged,
    #[allow(dead_code)]
    // Synthetic process-enumeration bound; native inspection uses the singleton PID.
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Inactive,
    KnownActive,
    Unknown(InspectionCode),
}

/// CORE supplies these paths and handles from ONE locked, checked v3 snapshot.
/// No display JSON, mutable profile name lookup, or parent stripping is involved.
pub(crate) struct CheckedDirectory {
    pub path: PathBuf,
    pub object: CheckedObject,
    pub ancestry: AnchoredPath,
}
impl CheckedDirectory {
    fn recheck(&self) -> Result<(), Error> {
        let current =
            platform::inspect(&self.path, ObjectKind::Directory)?.ok_or(Error::AdmissionChanged)?;
        let retained = platform::verify_opened(
            self.object
                .file
                .try_clone()
                .map_err(|_| Error::AdmissionChanged)?,
            &self.path,
            ObjectKind::Directory,
        )?;
        if current.identity != self.object.identity || retained.identity != self.object.identity {
            return Err(Error::AdmissionChanged);
        }
        // Recheck trusted ancestry; retained ancestors stay alive through dispatch.
        let now = platform::anchor(&self.path)?;
        if now.ancestors.len() != self.ancestry.ancestors.len()
            || now
                .ancestors
                .iter()
                .zip(&self.ancestry.ancestors)
                .any(|(a, b)| a.identity != b.identity)
        {
            return Err(Error::AdmissionChanged);
        }
        Ok(())
    }
}

pub(crate) struct CheckedSnapshot {
    pub profile_id: ProfileId,
    pub provider: Provider,
    pub revision: u64,
    pub recorded_version: Option<String>,
    pub root: CheckedDirectory,
    pub container: CheckedDirectory,
    pub cli: CheckedDirectory,
    pub desktop: CheckedDirectory,
}

/// Retains every checked handle until dispatch finishes. Construction requires a
/// coherent CORE snapshot and an independently validated app, never raw path JSON.
pub(crate) struct ValidatedDesktopTarget {
    snapshot: CheckedSnapshot,
    app: CheckedApp,
}
impl ValidatedDesktopTarget {
    pub(crate) fn snapshot(&self) -> &CheckedSnapshot {
        &self.snapshot
    }
    pub(crate) fn binding_path(&self) -> &Path {
        self.app.path()
    }
    pub(crate) fn from_checked(snapshot: CheckedSnapshot, app: CheckedApp) -> Result<Self, Error> {
        let target = Self { snapshot, app };
        target.recheck()?;
        Ok(target)
    }
    fn recheck(&self) -> Result<(), Error> {
        launch_path(&self.snapshot.cli.path)?;
        launch_path(&self.snapshot.desktop.path)?;
        self.app.recheck()?;
        let s = &self.snapshot;
        if !s.root.path.is_absolute()
            || s.container.path != s.root.path.join("profiles").join(s.profile_id.to_string())
            || s.cli.path != s.container.path.join("cli")
            || s.desktop.path != s.container.path.join("desktop")
        {
            return Err(Error::AdmissionChanged);
        }
        for dir in [&s.root, &s.container, &s.cli, &s.desktop] {
            dir.recheck()?;
        }
        let dirs = [&s.root, &s.container, &s.cli, &s.desktop];
        for (i, a) in dirs.iter().enumerate() {
            if dirs[..i]
                .iter()
                .any(|b| a.object.identity == b.object.identity)
            {
                return Err(Error::AdmissionChanged);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod synthetic;

impl CheckedApp {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn recheck(&self) -> Result<(), Error> {
        let exe = self.path.join("Contents/MacOS/ChatGPT");
        let now = app_anchor(&exe)?;
        if now.ancestors.len() != self.ancestry.ancestors.len()
            || now
                .ancestors
                .iter()
                .zip(&self.ancestry.ancestors)
                .any(|(a, b)| a.identity != b.identity)
        {
            return Err(Error::AdmissionChanged);
        }
        for (path, held, expected) in [
            (&exe, &self.executable, &self.executable_stamp),
            (
                &self.path.join("Contents/Info.plist"),
                &self.info,
                &self.info_stamp,
            ),
        ] {
            let current = app_file(path)?;
            if current.identity != held.identity
                || AppFileStamp::of(&current)? != *expected
                || AppFileStamp::of(held)? != *expected
            {
                return Err(Error::AdmissionChanged);
            }
        }

        Ok(())
    }
}
#[cfg(target_os = "macos")]
mod macos;

#[cfg(unix)]
mod singleton;

/// Initial OS gate before registry or environment access. Claude remains closed at target admission.
pub(crate) fn preflight() -> Result<(), Error> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(Error::QualificationPending)
    }
}
// Reopen only after the ChatGPT ~/.codex attribution re-test is resolved.
const CHATGPT_DISPATCH_QUALIFIED: bool = false;

pub(crate) fn open_preflight() -> Result<(), Error> {
    preflight()?;
    if !CHATGPT_DISPATCH_QUALIFIED {
        return Err(Error::QualificationPending);
    }
    Ok(())
}

pub(crate) fn activity_for(target: &ValidatedDesktopTarget) -> Activity {
    #[cfg(target_os = "macos")]
    return macos::dispatch::activity(target);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        Activity::Unknown(InspectionCode::Unavailable)
    }
}
/// CORE retains the per-ID lifecycle guard across this entire call.
pub(crate) fn open_target(
    target: &ValidatedDesktopTarget,
    revalidate: impl FnMut() -> Result<(), Error>,
    notice: impl FnMut(&'static str),
) -> Result<DesktopReceipt, Error> {
    #[cfg(target_os = "macos")]
    return macos::dispatch::open(target, revalidate, notice);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (target, revalidate, notice);
        Err(Error::QualificationPending)
    }
}

/// Metadata display checks presence/type only. It does not certify a signature or qualification.
pub(crate) fn binding_path_present(provider: Provider, path: &Path) -> bool {
    if provider != Provider::Codex
        || path.as_os_str() != Path::new("/Applications/ChatGPT.app").as_os_str()
    {
        return false;
    }
    [
        path.to_path_buf(),
        path.join("Contents"),
        path.join("Contents/MacOS"),
    ]
    .iter()
    .all(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir()))
        && [
            path.join("Contents/MacOS/ChatGPT"),
            path.join("Contents/Info.plist"),
        ]
        .iter()
        .all(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file()))
}

#[cfg(test)]
mod qualification_tests {
    #[test]
    fn signature_cache_requires_exact_key_and_bind_always_checks() {
        let key = super::SignatureCache {
            version: 1,
            executable: [1; 7],
            info: [2; 7],
            cdhash: vec![3; 20],
        };
        assert!(!super::signature_check_needed(false, Some(&key), &key));
        assert!(super::signature_check_needed(true, Some(&key), &key));
        assert!(super::signature_check_needed(false, None, &key));
        for index in 0..7 {
            let mut changed = key.clone();
            changed.executable[index] += 1;
            assert!(super::signature_check_needed(false, Some(&key), &changed));
            let mut changed = key.clone();
            changed.info[index] += 1;
            assert!(super::signature_check_needed(false, Some(&key), &changed));
        }
        let mut changed = key.clone();
        changed.cdhash[0] += 1;
        assert!(super::signature_check_needed(false, Some(&key), &changed));
    }
    #[test]
    fn chatgpt_dispatch_stays_closed_pending_attribution() {
        assert!(!std::hint::black_box(super::CHATGPT_DISPATCH_QUALIFIED));
        assert!(matches!(
            super::open_preflight(),
            Err(crate::registry::Error::QualificationPending)
        ));
    }
}
