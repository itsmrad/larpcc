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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

#[cfg(all(test, windows))]
#[path = "../../tests/fixtures/windows/private.rs"]
pub(crate) mod fixture_private;

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
pub(crate) fn open_auth_lock(path: &Path) -> Result<CheckedObject, Error> {
    windows::open_auth_lock_checked(path)
}
