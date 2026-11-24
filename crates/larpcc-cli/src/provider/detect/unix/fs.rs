use crate::platform::removal::{open_dir, stat_at};
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
    if !path.is_absolute() || path.as_os_str().len() > 4096 || !allow(Path::new("/")) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut directories = vec![File::open("/")?];
    let mut resolved = PathBuf::from("/");
    let mut remaining = components(path);
    let mut hops = 0;
    let mut steps = 0;
    loop {
        let dir = directories.last().expect("root retained");
        let Some(part) = remaining.pop_front() else {
            let stat = stat_at(dir, c".")?.ok_or(io::ErrorKind::NotFound)?;
            return Ok(Entry {
                path: resolved,
                stat,
                directory: open_leaf_directory.then(|| directories.pop().expect("root retained")),
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
                directories.push(opened);
            }
            _ if remaining.is_empty() => {
                return Ok(Entry {
                    path: resolved,
                    stat,
                    directory: None,
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

#[cfg(target_os = "macos")]
pub(super) fn confined_bundle(root: &Path, allow: impl Fn(&Path) -> bool) -> bool {
    let guarded = |p: &Path| (p.starts_with(root) || root.starts_with(p)) && allow(p);
    let Ok(entry) = walk(root, guarded, true) else {
        return false;
    };
    let Some(directory) = entry.directory else {
        return false;
    };
    confined_directory(
        root,
        root,
        &directory,
        &guarded,
        0,
        &mut 8_192,
        &mut (2 * 1024 * 1024 * 1024),
    )
}

#[cfg(target_os = "macos")]
fn confined_directory(
    root: &Path,
    path: &Path,
    directory: &File,
    allowed: &impl Fn(&Path) -> bool,
    depth: usize,
    entries_left: &mut usize,
    bytes_left: &mut u64,
) -> bool {
    use crate::platform::removal::Entries;

    let Ok(mut entries) = Entries::open(directory) else {
        return false;
    };
    loop {
        let name = match entries.next() {
            Ok(Some(name)) => name,
            Ok(None) => return true,
            Err(_) => return false,
        };
        if *entries_left == 0 || depth == 32 {
            return false;
        }
        *entries_left -= 1;
        let child = path.join(OsStr::from_bytes(name.to_bytes()));
        if !allowed(&child) {
            return false;
        }
        let Ok(Some(stat)) = stat_at(directory, &name) else {
            return false;
        };
        match stat.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {
                let Ok(child_dir) = open_dir(directory, &name) else {
                    return false;
                };
                if !confined_directory(
                    root,
                    &child,
                    &child_dir,
                    allowed,
                    depth + 1,
                    entries_left,
                    bytes_left,
                ) {
                    return false;
                }
            }
            libc::S_IFLNK => {
                if !walk(&child, allowed, false).is_ok_and(|entry| entry.path.starts_with(root)) {
                    return false;
                }
            }
            libc::S_IFREG => {
                let Ok(size) = u64::try_from(stat.st_size) else {
                    return false;
                };
                let Some(remaining) = bytes_left.checked_sub(size) else {
                    return false;
                };
                *bytes_left = remaining;
            }
            _ => return false,
        }
    }
}
