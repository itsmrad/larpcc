use crate::{
    platform::{
        CheckedObject, detection,
        removal::{open_dir, stat_at},
    },
    registry::Error,
};
use std::{
    collections::VecDeque,
    ffi::{CStr, CString, OsStr, OsString},
    fs::File,
    io,
    os::{
        fd::AsRawFd,
        unix::ffi::{OsStrExt, OsStringExt},
    },
    path::{Component, Path, PathBuf},
};

pub(super) struct Entry {
    pub path: PathBuf,
    pub stat: libc::stat,
    pub directory: Option<File>,
    pub object: Option<Result<CheckedObject, Error>>,
}

impl Entry {
    pub fn executable(&self) -> bool {
        self.stat.st_mode & libc::S_IFMT == libc::S_IFREG && self.stat.st_mode & 0o111 != 0
    }
}

fn target_at(parent: &File, name: &CStr) -> io::Result<PathBuf> {
    let mut target = [0u8; 4096];
    // The live directory and single-component name anchor the link read.
    let len = unsafe {
        libc::readlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            target.as_mut_ptr().cast(),
            target.len(),
        )
    };
    let len = usize::try_from(len).map_err(|_| io::Error::last_os_error())?;
    if len == target.len() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(OsStr::from_bytes(&target[..len]).into())
}

pub(super) fn walk(
    path: &Path,
    allow: impl Fn(&Path) -> bool,
    open_leaf_directory: bool,
) -> io::Result<Entry> {
    walk_inner(path, allow, open_leaf_directory, None)
}

pub(super) fn walk_executable(
    path: &Path,
    allow: impl Fn(&Path) -> bool,
    managed_root: &Path,
) -> io::Result<Entry> {
    walk_inner(path, allow, false, Some(managed_root))
}

fn walk_inner(
    path: &Path,
    allow: impl Fn(&Path) -> bool,
    open_leaf_directory: bool,
    managed_root: Option<&Path>,
) -> io::Result<Entry> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 || !allow(Path::new("/")) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut directories = vec![File::open("/")?];
    let mut resolved = PathBuf::from("/");
    let mut remaining = components(path);
    let mut hops = 0;
    let mut steps = 0;
    let mut trust = Ok(());
    loop {
        let dir = directories.last().expect("root retained");
        let Some(part) = remaining.pop_front() else {
            let stat = stat_at(dir, c".")?.ok_or(io::ErrorKind::NotFound)?;
            return Ok(Entry {
                path: resolved,
                stat,
                directory: open_leaf_directory.then(|| directories.pop().expect("root retained")),
                object: None,
            });
        };
        steps += 1;
        if steps > 256 || directories.len() > 64 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if part == ".." {
            if resolved.pop() {
                directories.pop();
            }
            continue;
        }
        resolved.push(&part);
        if resolved.as_os_str().len() > 4096 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if !allow(&resolved) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let name = CString::new(part.into_vec()).map_err(|_| io::ErrorKind::InvalidInput)?;
        let stat = stat_at(dir, &name)?.ok_or(io::ErrorKind::NotFound)?;
        match stat.st_mode & libc::S_IFMT {
            libc::S_IFLNK => {
                if managed_root.is_some() && trust.is_ok() {
                    trust = detection::check_link(&resolved, &stat);
                }
                hops += 1;
                if hops > 40 {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                let target = target_at(dir, &name)?;
                resolved.pop();
                if target.is_absolute() {
                    resolved = PathBuf::from("/");
                    directories.truncate(1);
                }
                for part in components(&target).into_iter().rev() {
                    remaining.push_front(part);
                }
            }
            libc::S_IFDIR if !remaining.is_empty() || open_leaf_directory => {
                let opened = open_dir(dir, &name)?;
                let confirmed = stat_at(&opened, c".")?.ok_or(io::ErrorKind::NotFound)?;
                if (stat.st_dev, stat.st_ino) != (confirmed.st_dev, confirmed.st_ino) {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                if managed_root.is_some() && trust.is_ok() {
                    trust = detection::check_directory(&resolved, &opened);
                }
                directories.push(opened);
            }
            _ if remaining.is_empty() => {
                let object = managed_root
                    .filter(|_| {
                        stat.st_mode & libc::S_IFMT == libc::S_IFREG && stat.st_mode & 0o111 != 0
                    })
                    .map(|root| {
                        trust?;
                        if path.starts_with(root) || resolved.starts_with(root) {
                            return Err(Error::Unsafe {
                                path: path.to_owned(),
                                reason: "is inside the Larp CC data directory".into(),
                            });
                        }
                        let file =
                            detection::open_regular(dir, &name).map_err(|source| Error::Io {
                                action: "open executable",
                                path: path.to_owned(),
                                source,
                            })?;
                        detection::check_executable(&resolved, file, &stat)
                    });
                return Ok(Entry {
                    path: resolved,
                    stat,
                    directory: None,
                    object,
                });
            }
            _ => return Err(io::ErrorKind::NotADirectory.into()),
        }
    }
}

fn components(path: &Path) -> VecDeque<OsString> {
    path.components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_owned()),
            Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect()
}
