use super::ProfileName;
use std::{fmt, io, path::PathBuf};
#[derive(Debug)]
pub enum Error {
    NoDataDir,
    InvalidRoot {
        path: PathBuf,
        reason: String,
    },
    Unsafe {
        path: PathBuf,
        reason: String,
    },
    Corrupt {
        path: PathBuf,
        reason: String,
    },
    NewerSchema {
        path: PathBuf,
        found: u64,
    },
    NotFound {
        name: ProfileName,
    },
    NameTaken {
        name: ProfileName,
    },
    Busy {
        path: PathBuf,
    },
    TooLarge,
    Exhausted,
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDataDir => f.write_str("cannot determine the data directory"),
            Self::InvalidRoot { path, reason } => {
                write!(f, "invalid data directory {}: {reason}", path.display())
            }
            Self::Unsafe { path, reason } => {
                write!(f, "unsafe data path {}: {reason}", path.display())
            }
            Self::Corrupt { path, reason } => {
                write!(f, "registry {} is invalid: {reason}", path.display())
            }
            Self::NewerSchema { path, found } => write!(
                f,
                "registry {} uses schema version {found}; this Larp CC supports version 1",
                path.display()
            ),
            Self::NotFound { name } => write!(f, "no profile named {name}"),
            Self::NameTaken { name } => write!(f, "a profile named {name} already exists"),
            Self::Busy { path } => write!(
                f,
                "registry is locked by another Larp CC process ({})",
                path.display()
            ),
            Self::TooLarge => f.write_str("prospective registry exceeds 1 MiB"),
            Self::Exhausted => f.write_str("registry counters are exhausted"),
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "cannot {action} {}: {source}", path.display()),
        }
    }
}
impl std::error::Error for Error {}
impl Error {
    pub fn hint(&self) -> Option<String> {
        Some(match self {
            Self::NoDataDir | Self::InvalidRoot { .. } => {
                "set LARPCC_HOME to an absolute path".into()
            }
            Self::Unsafe { path, reason } if reason.starts_with("has mode ") => format!(
                "run: chmod {} {}",
                if !path.is_dir() { "600" } else { "700" },
                path.display()
            ),
            Self::Unsafe { .. } => "fix or move it, or set LARPCC_HOME to another directory".into(),
            Self::Corrupt { path, .. } => format!(
                "Larp CC did not modify it; move it aside to start an empty registry (state directories under {} are not touched)",
                path.parent()?.join("profiles").display()
            ),
            Self::NewerSchema { .. } => "upgrade Larp CC; the file was not modified".into(),
            Self::NotFound { .. } => "run 'larp cc profile list'".into(),
            Self::Busy { .. } => "retry when the other command finishes".into(),
            Self::TooLarge => "forget unused profiles or use a separate data root".into(),
            Self::Io {
                action: "create directory",
                source,
                ..
            } if source.kind() == io::ErrorKind::NotFound => {
                "choose a data root under an existing parent directory".into()
            }
            Self::Io {
                action: "sync",
                source,
                ..
            } if source.to_string().starts_with("the change was applied") => {
                "the change was applied but may not be durable; run 'larp cc profile list'".into()
            }
            _ => return None,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    Invalid { input: String },
    Reserved { input: String },
}
impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { input } => write!(
                f,
                "invalid profile name {input:?}: use 1-32 characters from a-z, 0-9, '-' and '_', starting with a letter"
            ),
            Self::Reserved { input } => {
                write!(f, "invalid profile name {input:?}: reserved on Windows")
            }
        }
    }
}
impl std::error::Error for NameError {}
