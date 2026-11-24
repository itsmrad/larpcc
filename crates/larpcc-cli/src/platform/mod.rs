use crate::registry::{DirStatus, Error};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Directory,
    File,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum FileIdentity {
    #[cfg(unix)]
    Unix { dev: u64, ino: u64 },
    #[cfg(windows)]
    Windows { volume: u64, id: [u8; 16] },
}
pub struct CheckedObject {
    pub file: File,
    pub identity: FileIdentity,
}
pub struct AnchoredPath {
    pub ancestors: Vec<CheckedObject>,
}
pub(crate) mod removal;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(unix, test))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ancestry {
    Strict,
    ProviderExecutable,
}

#[cfg(any(unix, test))]
fn trust_reason(
    mode: u32,
    uid: u32,
    gid: u32,
    euid: u32,
    is_dir: bool,
    ancestry: Ancestry,
) -> Option<&'static str> {
    if uid != 0 && uid != euid {
        return Some("is owned by an untrusted principal");
    }
    let forbidden = if cfg!(target_os = "macos")
        && ancestry == Ancestry::ProviderExecutable
        && is_dir
        && gid == 80
    {
        0o002
    } else {
        0o022
    };
    (mode & forbidden != 0).then_some(if is_dir {
        "has a foreign-writable ancestor"
    } else {
        "is writable by group or other users"
    })
}

#[cfg(test)]
#[test]
fn executable_ancestry_trust_matrix() {
    for ancestry in [Ancestry::Strict, Ancestry::ProviderExecutable] {
        for is_dir in [false, true] {
            for uid in [0, 501, 502] {
                for gid in [20, 80] {
                    for mode in [0o755, 0o775, 0o757, 0o777, 0o1775, 0o1777] {
                        let expected = match (uid, mode & 0o022) {
                            (502, _) => Some("is owned by an untrusted principal"),
                            (_, 0) => None,
                            (_, 0o020)
                                if cfg!(target_os = "macos")
                                    && is_dir
                                    && gid == 80
                                    && ancestry == Ancestry::ProviderExecutable =>
                            {
                                None
                            }
                            _ if is_dir => Some("has a foreign-writable ancestor"),
                            _ => Some("is writable by group or other users"),
                        };
                        assert_eq!(
                            trust_reason(mode, uid, gid, 501, is_dir, ancestry),
                            expected,
                            "mode={mode:o} uid={uid} gid={gid} is_dir={is_dir} provider={}",
                            ancestry == Ancestry::ProviderExecutable,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(all(test, windows))]
#[path = "../../tests/fixtures/windows/private.rs"]
pub(crate) mod fixture_private;

pub(crate) fn fd_exhausted(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(error.raw_os_error(), Some(libc::EMFILE | libc::ENFILE))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            ERROR_NO_SYSTEM_RESOURCES, ERROR_TOO_MANY_OPEN_FILES,
        };
        error.raw_os_error().is_some_and(|code| {
            matches!(
                code as u32,
                ERROR_TOO_MANY_OPEN_FILES | ERROR_NO_SYSTEM_RESOURCES
            )
        })
    }
}

/// Directory metadata only: resolve an excluded vendor-home alias, then retain
/// its target/ancestor identities without trust checks or reading home contents.
/// These handles authorize exclusion only, never managed reads or writes.
pub(crate) fn skill_home(path: &Path) -> Result<Option<(CheckedObject, AnchoredPath)>, Error> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| Error::Io {
                action: "resolve skill home",
                path: path.into(),
                source,
            })?
            .join(path)
    };
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                action: "inspect skill home",
                path,
                source,
            });
        }
        Ok(_) => (),
    }
    let resolved = std::fs::canonicalize(&path).map_err(|source| Error::Io {
        action: "resolve skill home",
        path: path.clone(),
        source,
    })?;
    let object = skill_directory_identity(&resolved)?;
    let ancestors = AnchoredPath {
        ancestors: resolved
            .ancestors()
            .skip(1)
            .map(skill_directory_identity)
            .collect::<Result<_, _>>()?,
    };
    Ok(Some((object, ancestors)))
}

// Internal only: a namespace change is distinct from every validation or I/O failure.
#[derive(Debug)]
pub(crate) enum AcquisitionError {
    Changed,
    Failed(Error),
}
impl From<Error> for AcquisitionError {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}
impl AcquisitionError {
    fn diagnostic(self, path: &Path) -> Error {
        match self {
            Self::Failed(error) => error,
            Self::Changed => Error::Unsafe {
                path: path.into(),
                reason: "filesystem identity cannot be verified".into(),
            },
        }
    }
}
// Call only for the replaceable registry.json / registry.json.tmp namespace.
pub(crate) fn acquire_replaceable<T>(
    path: &Path,
    mut acquire: impl FnMut() -> Result<T, AcquisitionError>,
) -> Result<T, Error> {
    for _ in 0..32 {
        match acquire() {
            Ok(object) => return Ok(object),
            Err(AcquisitionError::Changed) => (),
            Err(AcquisitionError::Failed(error)) => return Err(error),
        }
    }
    Err(AcquisitionError::Changed.diagnostic(path))
}
#[cfg(windows)]
pub(crate) fn inspect_replaceable(path: &Path) -> Result<Option<CheckedObject>, AcquisitionError> {
    inspect(path, ObjectKind::File).map_err(Into::into)
}

#[cfg(windows)]
pub(crate) fn open_artifact(path: &Path, root: &Path) -> Result<(File, FileIdentity), Error> {
    windows::open_artifact_checked(path, root)
}

#[cfg(windows)]
pub(crate) use open_artifact as open_larp_artifact;

#[cfg(windows)]
pub(crate) fn open_current_larp(root: &Path) -> Result<(File, FileIdentity, PathBuf), Error> {
    // A module pathname reopened at startup alone cannot prove the loaded image.
    let _ = root;
    Err(Error::AliasImageUnverified)
}

#[cfg(windows)]
pub(crate) fn open_auth_lock(path: &Path) -> Result<CheckedObject, Error> {
    windows::open_auth_lock_checked(path)
}
