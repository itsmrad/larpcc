//! Bounded vendor lock metadata only: never follow the symlink or touch IPC peers.
use super::{CheckedDirectory, InspectionCode};
use std::{ffi::CStr, os::fd::AsRawFd};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Lock {
    pub pid: i32,
    dev: u64,
    ino: u64,
    size: i64,
    changed: (i64, i64),
    pub modified: (i64, i64),
    pub created: (i64, i64),
}
const LIMIT: usize = 512;
pub(super) fn parse(target: &[u8], hostname: &[u8]) -> Result<i32, InspectionCode> {
    if target.len() >= LIMIT || target.contains(&0) || hostname.is_empty() || hostname.len() > 255 {
        return Err(InspectionCode::Malformed);
    }
    let dash = target
        .iter()
        .rposition(|b| *b == b'-')
        .ok_or(InspectionCode::Malformed)?;
    if &target[..dash] != hostname {
        return Err(InspectionCode::ForeignHost);
    }
    let digits = &target[dash + 1..];
    if digits.is_empty() || digits.len() > 10 || digits[0] == b'0' {
        return Err(InspectionCode::Malformed);
    }
    let mut pid = 0i32;
    for byte in digits {
        if !byte.is_ascii_digit() {
            return Err(InspectionCode::Malformed);
        }
        pid = pid
            .checked_mul(10)
            .and_then(|p| p.checked_add((byte - b'0') as i32))
            .ok_or(InspectionCode::Malformed)?;
    }
    if pid <= 0 {
        return Err(InspectionCode::Malformed);
    }
    Ok(pid)
}
fn metadata(directory: &CheckedDirectory) -> Result<libc::stat, InspectionCode> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: descriptor is retained; fixed NUL-terminated leaf, valid output pointer.
    let result = unsafe {
        libc::fstatat(
            directory.object.file.as_raw_fd(),
            c"SingletonLock".as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => InspectionCode::Missing,
            Some(libc::EACCES | libc::EPERM) => InspectionCode::Denied,
            _ => InspectionCode::Unavailable,
        });
    }
    // SAFETY: successful fstatat initialized the full structure.
    let stat = unsafe { stat.assume_init() };
    // SAFETY: geteuid has no arguments or memory preconditions.
    if stat.st_mode & libc::S_IFMT != libc::S_IFLNK
        || stat.st_uid != unsafe { libc::geteuid() }
        || stat.st_nlink != 1
        || stat.st_size <= 0
        || stat.st_size >= LIMIT as i64
    {
        return Err(InspectionCode::Malformed);
    }
    Ok(stat)
}
#[allow(
    clippy::unnecessary_cast,
    reason = "stat widths differ between Linux and macOS"
)]
fn record(stat: &libc::stat, pid: i32) -> Lock {
    Lock {
        pid,
        dev: stat.st_dev as u64,
        ino: stat.st_ino as u64,
        size: stat.st_size,
        changed: (stat.st_ctime, stat.st_ctime_nsec),
        modified: (stat.st_mtime, stat.st_mtime_nsec),
        #[cfg(target_os = "macos")]
        created: (stat.st_birthtime, stat.st_birthtime_nsec),
        #[cfg(not(target_os = "macos"))]
        created: (stat.st_ctime, stat.st_ctime_nsec),
    }
}
pub(super) fn read(directory: &CheckedDirectory, hostname: &[u8]) -> Result<Lock, InspectionCode> {
    directory
        .recheck()
        .map_err(|_| InspectionCode::IdentityChanged)?;
    let before = metadata(directory)?;
    let mut buffer = [0u8; LIMIT];
    // SAFETY: readlinkat writes at most buffer.len() bytes; target is never followed.
    let len = unsafe {
        libc::readlinkat(
            directory.object.file.as_raw_fd(),
            c"SingletonLock".as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if len < 0 {
        return Err(InspectionCode::Unavailable);
    }
    if len as usize >= LIMIT {
        return Err(InspectionCode::Truncated);
    }
    let pid = parse(&buffer[..len as usize], hostname)?;
    let before = record(&before, pid);
    if before.size != len as i64 || before != record(&metadata(directory)?, pid) {
        return Err(InspectionCode::IdentityChanged);
    }
    directory
        .recheck()
        .map_err(|_| InspectionCode::IdentityChanged)?;
    Ok(before)
}
pub(super) fn hostname() -> Result<Vec<u8>, InspectionCode> {
    let mut buffer = [0i8; 256];
    // SAFETY: buffer covers the provided length, and is checked for termination.
    if unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len()) } != 0 {
        return Err(InspectionCode::Unavailable);
    }
    if !buffer.contains(&0) {
        return Err(InspectionCode::Truncated);
    }
    // SAFETY: a NUL has been found within the buffer.
    let bytes = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_bytes();
    if bytes.is_empty() {
        return Err(InspectionCode::Malformed);
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_singleton_creation_stamp_uses_birthtime_on_macos() {
        // SAFETY: stat consists only of integer fields; no OS resource is represented.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        stat.st_ctime = 100;
        stat.st_ctime_nsec = 200;
        #[cfg(target_os = "macos")]
        {
            stat.st_birthtime = 10;
            stat.st_birthtime_nsec = 20;
            assert_eq!(record(&stat, 1).created, (10, 20));
        }
        #[cfg(not(target_os = "macos"))]
        assert_eq!(record(&stat, 1).created, (100, 200));
    }
}
