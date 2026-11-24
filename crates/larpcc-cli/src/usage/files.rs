use crate::platform::{self, CheckedObject, ObjectKind};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone)]
pub(super) struct Directory {
    object: Rc<CheckedObject>,
    path: PathBuf,
    parent: Option<Rc<Directory>>,
    _anchor: Option<Rc<platform::AnchoredPath>>,
}

impl Directory {
    #[cfg(unix)]
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn root(path: &Path) -> Result<Option<Self>, ()> {
        let anchor = platform::anchor(path).map_err(|_| ())?;
        let object = platform::inspect(path, ObjectKind::Directory).map_err(|_| ())?;
        Ok(object.map(|object| Self {
            object: Rc::new(object),
            path: path.to_owned(),
            parent: None,
            _anchor: Some(Rc::new(anchor)),
        }))
    }

    pub(super) fn directory(&self, name: &OsStr, managed: bool) -> Result<Option<Self>, ()> {
        let child = self.open(name, ObjectKind::Directory, managed)?;
        Ok(child.map(|object| Self {
            object: Rc::new(object),
            path: self.path.join(name),
            parent: Some(Rc::new(self.clone())),
            _anchor: None,
        }))
    }

    pub(super) fn file(&self, name: &OsStr, managed: bool) -> Result<Option<CheckedObject>, ()> {
        self.open(name, ObjectKind::File, managed)
    }

    #[cfg(unix)]
    pub(super) fn project_chain(path: &Path) -> Result<Vec<Self>, ()> {
        use std::{
            fs::OpenOptions,
            os::unix::fs::{MetadataExt, OpenOptionsExt},
            path::Component,
        };
        if !path.is_absolute() {
            return Err(());
        }
        let root = PathBuf::from("/");
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&root)
            .map_err(|_| ())?;
        let info = file.metadata().map_err(|_| ())?;
        let identity = platform::FileIdentity::Unix {
            dev: info.dev(),
            ino: info.ino(),
        };
        let mut chain = vec![Self {
            object: Rc::new(CheckedObject { file, identity }),
            path: root,
            parent: None,
            _anchor: None,
        }];
        let components = path.components().skip(1);
        if components.clone().count() > 64 {
            return Err(());
        }
        for component in components {
            let Component::Normal(name) = component else {
                return Err(());
            };
            let parent = chain.last().ok_or(())?;
            let object =
                open_native(&parent.object, name, ObjectKind::Directory, false)?.ok_or(())?;
            chain.push(Self {
                object: Rc::new(object),
                path: parent.path.join(name),
                parent: Some(Rc::new(parent.clone())),
                _anchor: None,
            });
        }
        Ok(chain)
    }

    fn open(
        &self,
        name: &OsStr,
        kind: ObjectKind,
        managed: bool,
    ) -> Result<Option<CheckedObject>, ()> {
        self.recheck()?;
        let path = self.path.join(name);
        #[cfg(unix)]
        let object = if managed {
            platform::alias_open_at(&self.object, &path, kind, false).map_err(|_| ())?
        } else {
            open_native(&self.object, name, kind, true)?
        };
        #[cfg(windows)]
        let object = {
            let _ = managed;
            platform::alias_open_at(&self.object, &path, kind, false).map_err(|_| ())?
        };
        if let Some(object) = &object {
            #[cfg(unix)]
            if !platform::removal::same_mount(&self.object.file, &object.file).map_err(|_| ())? {
                return Err(());
            }
            if kind == ObjectKind::File {
                platform::single_link(&object.file).map_err(|_| ())?;
            }
        }
        Ok(object)
    }

    pub(super) fn recheck(&self) -> Result<(), ()> {
        if let Some(parent) = &self.parent {
            parent.recheck()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let info = std::fs::symlink_metadata(&self.path).map_err(|_| ())?;
            let identity = platform::FileIdentity::Unix {
                dev: info.dev(),
                ino: info.ino(),
            };
            if !info.is_dir() || identity != self.object.identity {
                return Err(());
            }
        }
        Ok(())
    }

    pub(super) fn names(&self, mut visit: impl FnMut(&OsStr) -> bool) -> Result<(), ()> {
        self.recheck()?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let mut entries =
                platform::removal::Entries::open(&self.object.file).map_err(|_| ())?;
            while let Some(name) = entries.next().map_err(|_| ())? {
                if !visit(OsStr::from_bytes(name.as_bytes())) {
                    break;
                }
            }
        }
        #[cfg(windows)]
        for entry in std::fs::read_dir(&self.path).map_err(|_| ())? {
            if !visit(&entry.map_err(|_| ())?.file_name()) {
                break;
            }
        }
        self.recheck()
    }

    #[cfg(unix)]
    pub(super) fn ensure_directory(&self, name: &std::ffi::CStr) -> Result<Self, ()> {
        use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
        let leaf = OsStr::from_bytes(name.to_bytes());
        if let Some(directory) = self.directory(leaf, true)? {
            return Ok(directory);
        }
        // The name is a fixed single component beneath the retained profile.
        if unsafe { libc::mkdirat(self.object.file.as_raw_fd(), name.as_ptr(), 0o700) } != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(());
        }
        self.object.file.sync_all().map_err(|_| ())?;
        self.directory(leaf, true)?.ok_or(())
    }

    #[cfg(unix)]
    pub(super) fn install(&self, leaf: &str, bytes: &[u8]) -> Result<platform::FileIdentity, ()> {
        use std::io::Write;
        let target = self.path.join(leaf);
        let mut file = platform::alias_open_at(&self.object, &target, ObjectKind::File, true)
            .map_err(|_| ())?
            .ok_or(())?;
        let written = file
            .file
            .write_all(bytes)
            .and_then(|_| file.file.sync_all());
        if written.is_err() {
            let _ = platform::alias_unlink_at(&self.object, &target, &file.identity);
            return Err(());
        }
        self.object.file.sync_all().map_err(|_| ())?;
        Ok(file.identity)
    }

    #[cfg(unix)]
    pub(super) fn replace_capture(&self, bytes: &[u8], captured: i64) -> Result<(), ()> {
        use std::{
            ffi::CString,
            io::Write,
            os::{fd::AsRawFd, unix::ffi::OsStrExt},
        };
        const PREFIX: &str = ".claude-statusline.next-";
        let parent = self.object.file.as_raw_fd();
        self.names(|name| {
            if name
                .to_str()
                .and_then(|name| name.strip_prefix(PREFIX))
                .and_then(|suffix| {
                    let (stamp, pid) = suffix.split_once('-')?;
                    let process = pid.parse::<u32>().ok()?;
                    let time = stamp.parse::<i64>().ok()?;
                    (process != 0 && process.to_string() == pid && time.to_string() == stamp)
                        .then_some(time)
                })
                .is_some_and(|stamp| stamp >= 0 && captured.saturating_sub(stamp) > 60)
                && let Ok(Some(file)) = self.file(name, true)
            {
                let _ =
                    platform::alias_unlink_at(&self.object, &self.path.join(name), &file.identity);
            }
            true
        })?;
        let name = format!("{PREFIX}{captured}-{}", std::process::id());
        let temp = self.path.join(&name);
        let mut file = platform::alias_open_at(&self.object, &temp, ObjectKind::File, true)
            .map_err(|_| ())?
            .ok_or(())?;
        let written = (|| {
            file.file.write_all(bytes).map_err(|_| ())?;
            file.file.sync_all().map_err(|_| ())?;
            self.recheck()?;
            self.file(
                OsStr::from_bytes(c"claude-statusline.json".to_bytes()),
                true,
            )?;
            let name = CString::new(name).map_err(|_| ())?;
            // Both checked leaf names are relative to the same retained directory.
            if unsafe {
                libc::renameat(
                    parent,
                    name.as_ptr(),
                    parent,
                    c"claude-statusline.json".as_ptr(),
                )
            } != 0
            {
                return Err(());
            }
            self.object.file.sync_all().map_err(|_| ())
        })();
        if written.is_err() {
            let _ = platform::alias_unlink_at(&self.object, &temp, &file.identity);
        }
        written
    }
}

#[cfg(unix)]
fn open_native(
    parent: &CheckedObject,
    name: &OsStr,
    kind: ObjectKind,
    owner_only: bool,
) -> Result<Option<CheckedObject>, ()> {
    use std::{
        ffi::CString,
        fs::File,
        io,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
    };

    let name = CString::new(name.as_bytes()).map_err(|_| ())?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | match kind {
            ObjectKind::Directory => libc::O_DIRECTORY,
            ObjectKind::File => libc::O_NONBLOCK,
        };
    // The caller passes one component beneath a retained directory descriptor.
    let fd = unsafe { libc::openat(parent.file.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return match io::Error::last_os_error().kind() {
            io::ErrorKind::NotFound => Ok(None),
            _ => Err(()),
        };
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let info = file.metadata().map_err(|_| ())?;
    if (owner_only && info.uid() != unsafe { libc::geteuid() })
        || (kind == ObjectKind::File && (!info.is_file() || info.mode() & 0o400 == 0))
        || (kind == ObjectKind::Directory && !info.is_dir())
    {
        return Err(());
    }
    let identity = platform::FileIdentity::Unix {
        dev: info.dev(),
        ino: info.ino(),
    };
    Ok(Some(CheckedObject { file, identity }))
}
