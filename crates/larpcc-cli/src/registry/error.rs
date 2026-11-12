#![allow(dead_code)] // Seed errors are frozen before implementation.
use super::{DirStatus, ProfileName, Provider};
use std::{fmt, io, path::PathBuf};
#[derive(Debug)]
pub enum Error {
    ManagedMode {
        path: PathBuf,
        mode: u32,
        directory: bool,
    },
    QualificationPending,
    AlreadyBound {
        provider: Provider,
        old: PathBuf,
    },
    Unbound {
        provider: Provider,
    },
    InvalidExecutable {
        provider: Provider,
        path: PathBuf,
        reason: String,
    },
    HomeUnavailable {
        name: ProfileName,
        status: DirStatus,
    },
    EnvConflict {
        names: Vec<String>,
    },
    ArgConflict {
        label: &'static str,
    },
    Launch {
        path: PathBuf,
        source: io::Error,
    },
    Console {
        action: &'static str,
        source: io::Error,
    },
    Wait {
        source: io::Error,
    },
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
            Self::ManagedMode { path, mode, .. } => write!(
                f,
                "unsafe data path {}: has mode {mode:04o}; group and other access must be removed",
                crate::registry::display::path(path)
            ),
            Self::QualificationPending => {
                f.write_str("provider launch is disabled pending stage 3 qualification")
            }
            Self::AlreadyBound { provider, .. } => {
                write!(f, "refusing to rebind {provider} without --replace")
            }
            Self::Unbound { provider } => write!(f, "no executable bound for {provider}"),
            Self::InvalidExecutable {
                provider,
                path,
                reason,
            } => write!(
                f,
                "invalid {provider} executable {}: {reason}",
                crate::registry::display::path(path)
            ),
            Self::HomeUnavailable { name, status } => write!(
                f,
                "state directory for profile {name} is {}",
                match status {
                    DirStatus::Missing => "missing".into(),
                    DirStatus::Unsafe(r) => format!("unsafe: {r}"),
                    DirStatus::Ok => "ok".into(),
                }
            ),
            Self::EnvConflict { names } => {
                write!(f, "conflicting environment variables: {}", names.join(", "))
            }
            Self::ArgConflict { label } => write!(f, "unsupported launch argument: {label}"),
            Self::Launch { path, source } => {
                write!(
                    f,
                    "cannot launch {}: {source}",
                    crate::registry::display::path(path)
                )
            }
            Self::Console { action, source } => write!(f, "cannot {action}: {source}"),
            Self::Wait { source } => write!(f, "cannot wait for provider: {source}"),
            Self::NoDataDir => f.write_str("cannot determine the data directory"),
            Self::InvalidRoot { path, reason } => {
                write!(
                    f,
                    "invalid data directory {}: {reason}",
                    crate::registry::display::path(path)
                )
            }
            Self::Unsafe { path, reason } => {
                write!(
                    f,
                    "unsafe data path {}: {reason}",
                    crate::registry::display::path(path)
                )
            }
            Self::Corrupt { path, reason } => {
                write!(
                    f,
                    "registry {} is invalid: {reason}",
                    crate::registry::display::path(path)
                )
            }
            Self::NewerSchema { path, found } => write!(
                f,
                "registry {} uses schema version {found}; this Larp CC supports version 1",
                crate::registry::display::path(path)
            ),
            Self::NotFound { name } => write!(f, "no profile named {name}"),
            Self::NameTaken { name } => write!(f, "a profile named {name} already exists"),
            Self::Busy { path } => write!(
                f,
                "registry is locked by another Larp CC process ({})",
                crate::registry::display::path(path)
            ),
            Self::TooLarge => f.write_str("prospective registry exceeds 1 MiB"),
            Self::Exhausted => f.write_str("registry counters are exhausted"),
            Self::Io {
                action,
                path,
                source,
            } => write!(
                f,
                "cannot {action} {}: {source}",
                crate::registry::display::path(path)
            ),
        }
    }
}
impl std::error::Error for Error {}
impl Error {
    pub fn hint(&self) -> Option<String> {
        Some(match self {
            Self::QualificationPending => {
                "binding and profile metadata commands remain available".into()
            }
            Self::AlreadyBound { old, .. } => {
                format!("currently bound to {}", crate::registry::display::path(old))
            }
            Self::Unbound { provider } => {
                format!("run 'larp cc provider bind {provider} <ABSOLUTE_PATH>'")
            }
            Self::InvalidExecutable { .. } => {
                "bind a supported absolute native executable path".into()
            }
            Self::HomeUnavailable { name, .. } => format!(
                "inspect it with 'larp cc profile show {name}'; Larp CC will not recreate it"
            ),
            Self::EnvConflict { .. } => "remove these overrides before launch".into(),
            Self::ArgConflict { .. } => {
                "the argument form has not been admitted by the launch policy".into()
            }
            Self::Launch { .. } => "check the native executable and its access permissions".into(),
            Self::Wait { .. } => "the direct child may require manual cleanup".into(),
            Self::NoDataDir | Self::InvalidRoot { .. } => {
                "set LARPCC_HOME to an absolute path".into()
            }
            Self::ManagedMode {
                path, directory, ..
            } => format!(
                "run: chmod {} {}",
                if *directory { "700" } else { "600" },
                crate::registry::display::path(path)
            ),
            Self::Unsafe { .. } => "fix or move it, or set LARPCC_HOME to another directory".into(),
            Self::Corrupt { path, .. } => format!(
                "Larp CC did not modify it; move it aside to start an empty registry (state directories under {} are not touched)",
                crate::registry::display::path(&path.parent()?.join("profiles"))
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

#[cfg(test)]
mod stage2_fixtures {
    use super::*;
    #[test]
    fn fixed_launch_errors_do_not_echo_supplied_values() {
        let cases = [
            (
                Error::QualificationPending,
                "provider launch is disabled pending stage 3 qualification",
                Some("binding and profile metadata commands remain available"),
            ),
            (
                Error::AlreadyBound {
                    provider: Provider::Codex,
                    old: "/native/codex".into(),
                },
                "refusing to rebind codex without --replace",
                Some("currently bound to /native/codex"),
            ),
            (
                Error::Unbound {
                    provider: Provider::Claude,
                },
                "no executable bound for claude",
                Some("run 'larp cc provider bind claude <ABSOLUTE_PATH>'"),
            ),
            (
                Error::EnvConflict {
                    names: vec!["CODEX_HOME".into()],
                },
                "conflicting environment variables: CODEX_HOME",
                Some("remove these overrides before launch"),
            ),
            (
                Error::ArgConflict {
                    label: "argument syntax",
                },
                "unsupported launch argument: argument syntax",
                Some("the argument form has not been admitted by the launch policy"),
            ),
        ];
        for (error, message, hint) in cases {
            assert_eq!(error.to_string(), message);
            assert_eq!(error.hint().as_deref(), hint);
        }
    }
}
