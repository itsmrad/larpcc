use super::{DirStatus, Error, Profile, ProfileEntry};
#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
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
#[cfg(unix)]
pub(super) fn euid() -> u32 {
    // SAFETY: geteuid takes no arguments and has no memory safety preconditions.
    unsafe { libc::geteuid() }
}
pub(super) fn reason(meta: &fs::Metadata, dir: bool) -> Option<String> {
    if meta.file_type().is_symlink() {
        return Some("is a symbolic link or reparse point".into());
    }
    if dir && !meta.is_dir() {
        return Some("is not a directory".into());
    }
    if !dir && !meta.is_file() {
        return Some("is not a regular file".into());
    }
    #[cfg(unix)]
    {
        if meta.uid() != euid() {
            return Some(format!(
                "is owned by uid {}, expected {}",
                meta.uid(),
                euid()
            ));
        }
        if meta.mode() & 0o077 != 0 {
            return Some(format!(
                "has mode {:04o}; group and other access must be removed",
                meta.mode() & 0o7777
            ));
        }
    }
    None
}
pub(super) fn inspect(path: &Path, dir: bool) -> Result<Option<fs::Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if let Some(reason) = reason(&meta, dir) {
                return Err(Error::Unsafe {
                    path: path.into(),
                    reason,
                });
            }
            Ok(Some(meta))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io("inspect", path, e)),
    }
}
pub(super) fn mkdir(path: &Path) -> Result<bool, Error> {
    #[cfg(unix)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            inspect(path, true)?;
            Ok(false)
        }
        Err(e) => Err(io("create directory", path, e)),
    }
}
pub(super) fn options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
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
    #[cfg(unix)]
    let mut identities = HashMap::new();
    let mut result = Vec::with_capacity(profiles.len());
    for profile in profiles {
        let state_dir = root.join("profiles").join(profile.id.to_string());
        let status = match fs::symlink_metadata(&state_dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => DirStatus::Missing,
            Err(e) => return Err(io("inspect", &state_dir, e)),
            Ok(meta) => {
                if let Some(reason) = reason(&meta, true) {
                    DirStatus::Unsafe(reason)
                } else {
                    #[cfg(unix)]
                    if let Some(reason) =
                        duplicate(&mut identities, (meta.dev(), meta.ino()), profile.id.get())
                    {
                        result.push(ProfileEntry {
                            profile: profile.clone(),
                            state_dir,
                            status: DirStatus::Unsafe(reason),
                        });
                        continue;
                    }
                    DirStatus::Ok
                }
            }
        };
        result.push(ProfileEntry {
            profile: profile.clone(),
            state_dir,
            status,
        });
    }
    Ok(result)
}
#[cfg(unix)]
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
