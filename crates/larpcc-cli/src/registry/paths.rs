use super::{DirStatus, Error, Profile, ProfileEntry};
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    ffi::OsString,
    fs, io,
    path::{Component, Path, PathBuf},
};

pub(super) fn io(action: &'static str, path: &Path, source: io::Error) -> Error {
    Error::Io {
        action,
        path: path.into(),
        source,
    }
}
pub(super) fn resolve(env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, Error> {
    let nonempty = |key| env(key).filter(|s| !s.is_empty());
    if let Some(root) = nonempty("LARPCC_HOME") {
        return Ok(root.into());
    }
    #[cfg(windows)]
    if let Some(root) = nonempty("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(root.join("larpcc"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(root) = nonempty("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(root.join("larpcc"));
    }
    #[cfg(unix)]
    if let Some(home) = nonempty("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        #[cfg(target_os = "macos")]
        return Ok(home.join("Library/Application Support/larpcc"));
        #[cfg(not(target_os = "macos"))]
        return Ok(home.join(".local/share/larpcc"));
    }
    Err(Error::NoDataDir)
}
pub(super) fn validate(root: &Path) -> Result<(), Error> {
    let reason = if !root.is_absolute() {
        "must be an absolute path"
    } else if root.components().any(|c| c == Component::ParentDir) {
        "must not contain '..'"
    } else {
        return Ok(());
    };
    Err(Error::InvalidRoot {
        path: root.into(),
        reason: reason.into(),
    })
}
#[cfg(all(test, unix))]
pub(super) fn euid() -> u32 {
    // SAFETY: geteuid takes no arguments and has no memory safety preconditions.
    unsafe { libc::geteuid() }
}
pub(super) fn inspect(path: &Path, dir: bool) -> Result<Option<fs::Metadata>, Error> {
    let kind = if dir {
        crate::platform::ObjectKind::Directory
    } else {
        crate::platform::ObjectKind::File
    };
    crate::platform::inspect(path, kind)?
        .map(|o| o.file.metadata().map_err(|e| io("inspect", path, e)))
        .transpose()
}
pub(super) fn mkdir(path: &Path) -> Result<bool, Error> {
    match crate::platform::create_directory(path) {
        Ok(_) => Ok(true),
        Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {
            inspect(path, true)?;
            Ok(false)
        }
        Err(e) => Err(e),
    }
}
pub(super) fn options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    options
}
pub(super) fn sync(path: &Path) -> Result<(), Error> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        #[cfg(test)]
        super::store::phase("sync", path)?;
        fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| io("sync", path, e))?;
    }
    Ok(())
}
pub(super) fn entries(root: &Path, profiles: &[Profile]) -> Result<Vec<ProfileEntry>, Error> {
    let mut identities = HashMap::new();
    let mut result = Vec::with_capacity(profiles.len());
    for profile in profiles {
        let state_dir = root.join("profiles").join(profile.id.to_string());
        let mut status = crate::platform::inspect_home(&state_dir)?;
        if status == DirStatus::Ok
            && let Some(object) =
                crate::platform::inspect(&state_dir, crate::platform::ObjectKind::Directory)?
            && let Some(other) = identities.insert(object.identity, profile.id.get())
        {
            status =
                DirStatus::Unsafe(format!("shares a filesystem identity with profile {other}"));
        }
        result.push(ProfileEntry {
            profile: profile.clone(),
            state_dir,
            status,
        });
    }
    Ok(result)
}
#[cfg(all(test, unix))]
pub(super) fn duplicate(
    identities: &mut HashMap<(u64, u64), u64>,
    identity: (u64, u64),
    id: u64,
) -> Option<String> {
    if let Some(other) = identities.get(&identity) {
        Some(format!("shares a filesystem identity with profile {other}"))
    } else {
        identities.insert(identity, id);
        None
    }
}

pub(super) fn read_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    options
}
