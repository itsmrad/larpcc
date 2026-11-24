use super::{
    Ancestry, CheckedObject, Error, acl_file, identity, running_identity, trust_reason, uid,
    unsafe_path,
};
use std::{
    ffi::CStr,
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{FileExt, MetadataExt},
    },
    path::Path,
};

pub(crate) fn check_link(path: &Path, stat: &libc::stat) -> Result<(), Error> {
    if stat.st_uid != 0 && stat.st_uid != uid() {
        return Err(unsafe_path(path, "is owned by an untrusted principal"));
    }
    Ok(())
}

pub(crate) fn check_directory(path: &Path, file: &File) -> Result<(), Error> {
    let meta = file
        .metadata()
        .map_err(|e| super::io_error("inspect", path, e))?;
    check_trust(path, file, &meta, Ancestry::ProviderExecutable)
}

fn check_trust(
    path: &Path,
    file: &File,
    meta: &std::fs::Metadata,
    ancestry: Ancestry,
) -> Result<(), Error> {
    if let Some(reason) = trust_reason(
        meta.mode(),
        meta.uid(),
        meta.gid(),
        uid(),
        meta.is_dir(),
        ancestry,
    ) {
        return Err(unsafe_path(path, reason));
    }
    acl_file(file, path)
}

pub(crate) fn open_regular(parent: &File, name: &CStr) -> io::Result<File> {
    // SAFETY: the live descriptor holds the parent and name is a single component.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new, owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}

#[allow(clippy::unnecessary_cast, reason = "stat field widths vary by OS")]
pub(crate) fn check_executable(
    path: &Path,
    file: File,
    observed: &libc::stat,
) -> Result<CheckedObject, Error> {
    let meta = file
        .metadata()
        .map_err(|e| super::io_error("inspect", path, e))?;
    if (meta.dev(), meta.ino()) != (observed.st_dev as u64, observed.st_ino as u64) {
        return Err(unsafe_path(path, "changed since validation"));
    }
    if !meta.is_file() || meta.mode() & 0o111 == 0 {
        return Err(unsafe_path(path, "is not executable"));
    }
    check_trust(path, &file, &meta, Ancestry::Strict)?;
    let mut magic = [0; 4];
    let count = file
        .read_at(&mut magic, 0)
        .map_err(|e| super::io_error("read", path, e))?;
    #[cfg(target_os = "linux")]
    let native = magic == *b"\x7fELF";
    #[cfg(target_os = "macos")]
    let native = matches!(
        u32::from_be_bytes(magic),
        0xfeedface
            | 0xcefaedfe
            | 0xfeedfacf
            | 0xcffaedfe
            | 0xcafebabe
            | 0xbebafeca
            | 0xcafebabf
            | 0xbfbafeca
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let native = false;
    if count != 4 || !native {
        return Err(unsafe_path(path, "is not a native executable"));
    }
    let identity = identity(&meta);
    if running_identity(path)? == identity {
        return Err(unsafe_path(path, "is Larp CC itself"));
    }
    Ok(CheckedObject { file, identity })
}
