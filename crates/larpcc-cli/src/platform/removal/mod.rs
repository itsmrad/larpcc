//! Profile-tree operations relative to checked directory handles.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::Profiles;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::Profiles;
