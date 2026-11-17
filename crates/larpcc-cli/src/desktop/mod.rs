//! Desktop admission is deliberately closed. Fixture machinery never qualifies a vendor app.
use crate::{
    platform::{self, AnchoredPath, CheckedObject, ObjectKind},
    registry::{Error, ProfileId, Provider},
};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DesktopReceipt {
    pub profile_id: ProfileId,
    pub provider: Provider,
    pub surface: &'static str,
    pub outcome: &'static str,
    pub native_auth: &'static str,
}

/// Call before root lookup, environment enumeration, or binding inspection.
pub(crate) fn open() -> Result<DesktopReceipt, Error> {
    Err(Error::QualificationPending)
}

pub(crate) struct CheckedApp {
    path: PathBuf,
    ancestry: AnchoredPath,
    executable: CheckedObject,
}

/// Path spelling is necessary but never sufficient evidence of stock identity.
/// Only static code validation is performed; no vendor process is executed.
pub(crate) fn validate_binding(provider: Provider, path: &Path) -> Result<CheckedApp, Error> {
    let expected = match provider {
        Provider::Claude => Path::new("/Applications/Claude.app"),
        Provider::Codex => Path::new("/Applications/ChatGPT.app"),
    };
    if path.as_os_str() != expected.as_os_str() {
        return Err(Error::ArtifactInvalid);
    }
    #[cfg(target_os = "macos")]
    if provider == Provider::Codex {
        return macos::validate(path);
    }
    Err(Error::ArtifactInvalid)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InspectionCode {
    Unavailable,
    Denied,
    Malformed,
    Truncated,
    Timeout,
    IdentityChanged,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Inactive,
    KnownActive,
    Unknown(InspectionCode),
}

/// Use under the per-ID lifecycle guard. Unknown must block deletion/recovery.
/// No argv/environment/auth data is inspected by this conservative native boundary.
pub(crate) fn activity_check() -> Activity {
    // KERN_PROCARGS2 also returns environment bytes; do not read it then discard secrets.
    Activity::Unknown(InspectionCode::Unavailable)
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
    pub(crate) fn from_checked(snapshot: CheckedSnapshot, app: CheckedApp) -> Result<Self, Error> {
        let target = Self { snapshot, app };
        target.recheck()?;
        Ok(target)
    }
    fn recheck(&self) -> Result<(), Error> {
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
    fn recheck(&self) -> Result<(), Error> {
        let exe = self.path.join("Contents/MacOS/ChatGPT");
        let now = platform::anchor(&exe)?;
        if now.ancestors.len() != self.ancestry.ancestors.len()
            || now
                .ancestors
                .iter()
                .zip(&self.ancestry.ancestors)
                .any(|(a, b)| a.identity != b.identity)
        {
            return Err(Error::AdmissionChanged);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = self
                .executable
                .file
                .metadata()
                .map_err(|_| Error::AdmissionChanged)?;
            let current = std::fs::symlink_metadata(&exe).map_err(|_| Error::AdmissionChanged)?;
            if current.file_type().is_symlink()
                || held.dev() != current.dev()
                || held.ino() != current.ino()
            {
                return Err(Error::AdmissionChanged);
            }
        }
        #[cfg(windows)]
        {
            let current =
                platform::inspect(&exe, ObjectKind::File)?.ok_or(Error::AdmissionChanged)?;
            if current.identity != self.executable.identity {
                return Err(Error::AdmissionChanged);
            }
        }
        #[cfg(target_os = "macos")]
        {
            #[cfg(test)]
            let fixture = self.path != Path::new("/Applications/ChatGPT.app");
            #[cfg(not(test))]
            let fixture = false;
            if !fixture {
                macos::verify_signature(&self.path)?;
            }
        }
        Ok(())
    }
}
#[cfg(target_os = "macos")]
mod macos;
