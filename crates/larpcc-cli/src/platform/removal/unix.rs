use crate::{
    platform::{self, CheckedObject, FileIdentity, ObjectKind},
    registry::Error,
};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) struct Profiles {
    path: PathBuf,
    ancestors: Vec<CheckedObject>,
    root: CheckedObject,
    dir: File,
}

fn stat_at(parent: &File, name: &CStr) -> io::Result<Option<libc::stat>> {
    let mut stat = std::mem::MaybeUninit::uninit();
    // fstatat initializes the buffer on success and never follows the final component.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(Some(unsafe { stat.assume_init() }));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(error)
    }
}

fn open_dir(parent: &File, name: &CStr) -> io::Result<File> {
    // All names are single components from a trusted derivation or readdir.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_RDONLY,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[allow(
    clippy::unnecessary_cast,
    reason = "stat field widths vary across Unix targets"
)]
fn identity(stat: &libc::stat) -> FileIdentity {
    FileIdentity::Unix {
        dev: stat.st_dev as u64,
        ino: stat.st_ino as u64,
    }
}

fn file_identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    Ok(FileIdentity::Unix {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

fn same_mount(parent: &File, child: &File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    if parent.metadata()?.dev() != child.metadata()?.dev() {
        return Ok(false);
    }
    #[cfg(target_os = "linux")]
    {
        // A bind mount can share st_dev with its parent.
        fn mount(file: &File) -> io::Result<u64> {
            let mut stat = std::mem::MaybeUninit::<libc::statx>::zeroed();
            // AT_EMPTY_PATH queries the retained descriptor, including its mount ID.
            if unsafe {
                libc::statx(
                    file.as_raw_fd(),
                    c"".as_ptr(),
                    libc::AT_EMPTY_PATH,
                    libc::STATX_MNT_ID,
                    stat.as_mut_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let stat = unsafe { stat.assume_init() };
            if stat.stx_mask & libc::STATX_MNT_ID == 0 {
                return Err(io::ErrorKind::Unsupported.into());
            }
            Ok(stat.stx_mnt_id)
        }
        Ok(mount(parent)? == mount(child)?)
    }
    #[cfg(not(target_os = "linux"))]
    Ok(true)
}

struct Entries(*mut libc::DIR);
impl Entries {
    fn open(dir: &File) -> io::Result<Self> {
        // Reopen to give the directory stream its own offset.
        let fd = open_dir(dir, c".")?.into_raw_fd();
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            drop(unsafe { File::from_raw_fd(fd) });
            return Err(error);
        }
        Ok(Self(stream))
    }

    fn next(&mut self) -> io::Result<Option<CString>> {
        loop {
            // readdir returns null at EOF or on error. Reset errno to tell them apart.
            #[cfg(target_os = "linux")]
            let errno = unsafe { libc::__errno_location() };
            #[cfg(target_os = "macos")]
            let errno = unsafe { libc::__error() };
            unsafe { *errno = 0 };
            let entry = unsafe { libc::readdir(self.0) };
            if entry.is_null() {
                return match unsafe { *errno } {
                    0 => Ok(None),
                    code => Err(io::Error::from_raw_os_error(code)),
                };
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name != c"." && name != c".." {
                return Ok(Some(name.to_owned()));
            }
        }
    }
}
impl Drop for Entries {
    fn drop(&mut self) {
        unsafe { libc::closedir(self.0) };
    }
}

fn unlink_at(parent: &File, name: &CStr, dir: bool) -> io::Result<()> {
    let flags = if dir { libc::AT_REMOVEDIR } else { 0 };
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn name(component: &str) -> Result<CString, Error> {
    if component.is_empty() || matches!(component, "." | "..") || component.contains('/') {
        return Err(Error::RecoveryRequired);
    }
    CString::new(component).map_err(|_| Error::RecoveryRequired)
}

impl Profiles {
    pub(crate) fn open(root: &Path) -> Result<Self, Error> {
        let ancestors = platform::anchor(root)?.ancestors;
        let root_pin =
            platform::inspect(root, ObjectKind::Directory)?.ok_or(Error::RecoveryRequired)?;
        let path = root.join("profiles");
        let checked =
            platform::inspect(&path, ObjectKind::Directory)?.ok_or(Error::RecoveryRequired)?;
        let dir = open_dir(&root_pin.file, c"profiles").map_err(|source| Error::Io {
            action: "open profiles directory",
            path: path.clone(),
            source,
        })?;
        let profiles = Self {
            path,
            ancestors,
            root: root_pin,
            dir,
        };
        if profiles.io(file_identity(&profiles.dir))? != checked.identity
            || !profiles.io(same_mount(&profiles.root.file, &profiles.dir))?
        {
            return Err(Error::Drift);
        }
        profiles.recheck()?;
        Ok(profiles)
    }

    pub(crate) fn identity(&self) -> Result<FileIdentity, Error> {
        self.io(file_identity(&self.dir))
    }

    fn io<T>(&self, result: io::Result<T>) -> Result<T, Error> {
        result.map_err(|source| Error::Io {
            action: "access profile tree",
            path: self.path.clone(),
            source,
        })
    }

    fn recheck(&self) -> Result<(), Error> {
        let root = self.path.parent().ok_or(Error::RecoveryRequired)?;
        let current = platform::anchor(root)?;
        if !current
            .ancestors
            .iter()
            .map(|a| &a.identity)
            .eq(self.ancestors.iter().map(|a| &a.identity))
        {
            return Err(Error::Drift);
        }
        if platform::inspect(root, ObjectKind::Directory)?.map(|p| p.identity)
            != Some(self.root.identity.clone())
        {
            return Err(Error::Drift);
        }
        self.check_link(&self.root.file, c"profiles", &self.dir)
    }

    fn check_link(&self, parent: &File, name: &CStr, dir: &File) -> Result<(), Error> {
        let stat = self.io(stat_at(parent, name))?.ok_or(Error::Drift)?;
        if stat.st_mode & libc::S_IFMT != libc::S_IFDIR
            || identity(&stat) != self.io(file_identity(dir))?
        {
            return Err(Error::Drift);
        }
        Ok(())
    }

    fn checked_dir(&self, parent: &File, name: &str, path: &Path) -> Result<Option<File>, Error> {
        let component = self::name(name)?;
        let found = self.io(stat_at(parent, &component))?;
        let Some(found) = found else { return Ok(None) };
        let checked = platform::inspect(path, ObjectKind::Directory)?.ok_or(Error::Drift)?;
        let file = self.io(open_dir(parent, &component))?;
        if self.io(file_identity(&file))? != checked.identity
            || identity(&found) != checked.identity
            || !self.io(same_mount(parent, &file))?
        {
            return Err(Error::Drift);
        }
        Ok(Some(file))
    }

    pub(crate) fn home(
        &self,
        role: &str,
        child: Option<&str>,
        empty: bool,
    ) -> Result<Option<FileIdentity>, Error> {
        self.recheck()?;
        let path = self.path.join(role);
        let Some(container) = self.checked_dir(&self.dir, role, &path)? else {
            return Ok(None);
        };
        let dir = match child {
            Some(child) => {
                let Some(dir) = self.checked_dir(&container, child, &path.join(child))? else {
                    return Ok(None);
                };
                dir
            }
            None => container,
        };
        if empty
            && self
                .io(Entries::open(&dir).and_then(|mut items| items.next()))?
                .is_some()
        {
            return Err(Error::Conflict);
        }
        self.io(file_identity(&dir)).map(Some)
    }

    pub(crate) fn has_only(&self, role: &str, allowed: &[&CStr]) -> Result<bool, Error> {
        self.recheck()?;
        let dir = self
            .checked_dir(&self.dir, role, &self.path.join(role))?
            .ok_or(Error::Drift)?;
        let mut stream = self.io(Entries::open(&dir))?;
        let mut found = 0;
        while let Some(name) = self.io(stream.next())? {
            if !allowed.contains(&name.as_c_str()) {
                return Ok(false);
            }
            found += 1;
        }
        Ok(found == allowed.len())
    }

    pub(crate) fn remove(&self, role: &str, expected: &FileIdentity) -> Result<(), Error> {
        let start = Instant::now();
        self.recheck()?;
        let Some(dir) = self.checked_dir(&self.dir, role, &self.path.join(role))? else {
            // The previous attempt may have unlinked the root without syncing its parent.
            return self.io(self.dir.sync_all());
        };
        if self.io(file_identity(&dir))? != *expected {
            return Err(Error::Drift);
        }
        self.walk(&mut vec![(name(role)?, dir)], start, &mut 0)
    }

    pub(crate) fn make_container(&self, role: &str) -> Result<(FileIdentity, FileIdentity), Error> {
        self.recheck()?;
        let component = name(role)?;
        let mkdir = |parent: &File, component: &CStr| {
            if unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
                return self.io(Err(io::Error::last_os_error()));
            }
            self.io(parent.sync_all())
        };
        mkdir(&self.dir, &component)?;
        let path = self.path.join(role);
        let dir = self
            .checked_dir(&self.dir, role, &path)?
            .ok_or(Error::Drift)?;
        mkdir(&dir, c"desktop")?;
        let desktop = self
            .checked_dir(&dir, "desktop", &path.join("desktop"))?
            .ok_or(Error::Drift)?;
        self.io(desktop.sync_all())?;
        Ok((
            self.io(file_identity(&dir))?,
            self.io(file_identity(&desktop))?,
        ))
    }

    pub(crate) fn rename_home(
        &self,
        from: &str,
        to: &str,
        container: Option<(&str, &FileIdentity)>,
        expected: &FileIdentity,
        empty: bool,
    ) -> Result<(), Error> {
        self.recheck()?;
        let file = self
            .checked_dir(&self.dir, from, &self.path.join(from))?
            .ok_or(Error::Drift)?;
        if self.io(file_identity(&file))? != *expected {
            return Err(Error::Drift);
        }
        if empty
            && self
                .io(Entries::open(&file).and_then(|mut items| items.next()))?
                .is_some()
        {
            return Err(Error::Conflict);
        }
        let parent = container
            .map(|(role, expected)| {
                let dir = self
                    .checked_dir(&self.dir, role, &self.path.join(role))?
                    .ok_or(Error::Drift)?;
                if self.io(file_identity(&dir))? != *expected {
                    return Err(Error::Drift);
                }
                self.check_link(&self.dir, &name(role)?, &dir)?;
                Ok(dir)
            })
            .transpose()?;
        let destination = parent.as_ref().unwrap_or(&self.dir);
        let from = name(from)?;
        let to = name(to)?;
        self.check_link(&self.dir, &from, &file)?;
        #[cfg(target_os = "linux")]
        let status = unsafe {
            libc::renameat2(
                self.dir.as_raw_fd(),
                from.as_ptr(),
                destination.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let status = unsafe {
            libc::renameatx_np(
                self.dir.as_raw_fd(),
                from.as_ptr(),
                destination.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if status != 0 {
            return self.io(Err(io::Error::last_os_error()));
        }
        self.check_link(destination, &to, &file)?;
        self.io(destination.sync_all())?;
        self.io(self.dir.sync_all())
    }

    pub(crate) fn sync(&self, role: Option<&str>) -> Result<(), Error> {
        self.recheck()?;
        if let Some(role) = role {
            let dir = self
                .checked_dir(&self.dir, role, &self.path.join(role))?
                .ok_or(Error::Drift)?;
            self.io(dir.sync_all())?;
        }
        self.io(self.dir.sync_all())
    }

    fn verify_chain(&self, nodes: &[(CString, File)]) -> Result<(), Error> {
        self.recheck()?;
        let mut parent = &self.dir;
        for (name, dir) in nodes {
            self.check_link(parent, name, dir)?;
            parent = dir;
        }
        Ok(())
    }

    fn walk(
        &self,
        nodes: &mut Vec<(CString, File)>,
        start: Instant,
        entries: &mut usize,
    ) -> Result<(), Error> {
        let mut stream = self.io(Entries::open(&nodes.last().ok_or(Error::Drift)?.1))?;
        loop {
            if start.elapsed() >= Duration::from_secs(30) {
                return Err(Error::TooLarge);
            }
            let Some(name) = self.io(stream.next())? else {
                break;
            };
            *entries += 1;
            if *entries > 100_000 {
                return Err(Error::TooLarge);
            }
            self.verify_chain(nodes)?;
            let dir = &nodes.last().ok_or(Error::Drift)?.1;
            let stat = self.io(stat_at(dir, &name))?.ok_or(Error::Drift)?;
            if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
                if nodes.len() >= 64 {
                    return Err(Error::TooLarge);
                }
                let child = self.io(open_dir(dir, &name))?;
                if identity(&stat) != self.io(file_identity(&child))?
                    || !self.io(same_mount(dir, &child))?
                {
                    return Err(Error::Drift);
                }
                nodes.push((name, child));
                self.walk(nodes, start, entries)?;
            } else {
                // Unlink the leaf itself. Hardlinks, symlinks and special files need no content IO.
                if self.io(stat_at(dir, &name))?.as_ref().map(identity) != Some(identity(&stat)) {
                    return Err(Error::Drift);
                }
                self.io(unlink_at(dir, &name, false))?;
            }
        }
        self.verify_chain(nodes)?;
        let (name, dir) = nodes.pop().ok_or(Error::Drift)?;
        self.io(dir.sync_all())?;
        let parent = nodes.last().map_or(&self.dir, |(_, dir)| dir);
        self.check_link(parent, &name, &dir)?;
        self.io(unlink_at(parent, &name, true))?;
        self.io(parent.sync_all())
    }
}
