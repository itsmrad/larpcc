// Seed-only allowances: implementations replace explicit refusal stubs after S.
#![allow(dead_code, unused_imports)]
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
pub fn unavailable(path: &Path) -> Error {
    Error::Unsafe {
        path: path.into(),
        reason: "platform safety validation is unavailable".into(),
    }
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
