//! Profile-tree operations relative to checked directory handles.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Entries, Profiles, same_mount};
#[cfg(all(test, unix))]
pub(crate) use unix::before_unlink;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::Profiles;
