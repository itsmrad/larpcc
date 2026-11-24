//! Profile-tree operations relative to checked directory handles.

#[cfg(unix)]
mod unix;
#[cfg(all(test, unix))]
pub(crate) use unix::before_unlink;
#[cfg(unix)]
pub(crate) use unix::{Entries, Profiles, open_dir, same_mount, stat_at};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::Profiles;
