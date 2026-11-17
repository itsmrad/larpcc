//! Skills filesystem operations. Callers must hold the registry lock and journal
//! every mutation before calling these primitives; this module creates no locks.
use crate::{
    platform::{self, CheckedObject, FileIdentity, ObjectKind},
    registry::ProfileName,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub const MAX_FILES: usize = 1_000;
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_DEPTH: usize = 16;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Unsafe,
    TooLarge,
    Collision,
    Drift,
    Io,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
struct Entry {
    path: String,
    bytes: Vec<u8>,
}
/// Bounded snapshot; never rereads a source during installation.
#[derive(Debug)]
pub struct Package {
    files: Vec<Entry>,
    directories: Vec<String>,
    pub sha256: String,
}

fn checked(path: &Path, kind: ObjectKind) -> Result<CheckedObject> {
    platform::inspect(path, kind)
        .map_err(|_| Error::Unsafe)?
        .ok_or(Error::Unsafe)
}

/// Retain existing trusted ancestry and additionally refuse symlink spelling.
fn ancestry(path: &Path) -> Result<platform::AnchoredPath> {
    if !path.is_absolute()
        || path.components().any(|c| {
            !matches!(
                c,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
    {
        return Err(Error::Unsafe);
    }
    for parent in path.ancestors().skip(1) {
        let meta = fs::symlink_metadata(parent).map_err(|_| Error::Unsafe)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::Unsafe);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(Error::Unsafe);
            }
        }
    }
    platform::anchor(path).map_err(|_| Error::Unsafe)
}

fn single_link(file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        platform::single_link(file).map_err(|_| Error::Unsafe)
    }
    #[cfg(windows)]
    {
        // Integration will expose the existing checked-handle platform query.
        let _ = file;
        Err(Error::Unsafe)
    }
}

// Portable spelling prevents ADS/device paths and case-insensitive collisions.
fn component(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || !c.is_ascii() || "<>:\"/\\|?*".contains(c))
    {
        return Err(Error::Unsafe);
    }
    let stem = name.split('.').next().unwrap().to_ascii_lowercase();
    if matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}

impl Package {
    pub fn read(source: &Path) -> Result<Self> {
        let _ancestors = ancestry(source)?;
        let source: PathBuf = source.components().collect();
        let mut package = Self {
            files: Vec::new(),
            directories: Vec::new(),
            sha256: String::new(),
        };
        let mut total = 0;
        package.walk(&source, "", 0, &mut total)?;
        if !package.files.iter().any(|f| f.path == "SKILL.md") {
            return Err(Error::Unsafe);
        }
        package.files.sort_by(|a, b| a.path.cmp(&b.path));
        package.directories.sort();
        let mut digest = Sha256::new();
        for entry in &package.files {
            digest.update((entry.path.len() as u64).to_be_bytes());
            digest.update(entry.path.as_bytes());
            digest.update((entry.bytes.len() as u64).to_be_bytes());
            digest.update(&entry.bytes);
        }
        package.sha256 = format!("{:x}", digest.finalize());
        Ok(package)
    }

    fn walk(&mut self, root: &Path, relative: &str, depth: usize, total: &mut u64) -> Result<()> {
        let path = if relative.is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        };
        let _directory = checked(&path, ObjectKind::Directory)?;
        let mut names = BTreeSet::new();
        for entry in fs::read_dir(&path).map_err(|_| Error::Io)? {
            let entry = entry.map_err(|_| Error::Io)?;
            let name = entry.file_name().into_string().map_err(|_| Error::Unsafe)?;
            component(&name)?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(Error::Collision);
            }
            if depth + 1 > MAX_DEPTH {
                return Err(Error::TooLarge);
            }
            let relative = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let path = root.join(&relative);
            let meta = fs::symlink_metadata(&path).map_err(|_| Error::Io)?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                // Bound empty directories too, so enumeration cannot exhaust memory.
                if self.directories.len() >= MAX_FILES {
                    return Err(Error::TooLarge);
                }
                self.directories.push(relative.clone());
                self.walk(root, &relative, depth + 1, total)?;
            } else {
                if self.files.len() >= MAX_FILES {
                    return Err(Error::TooLarge);
                }
                let object = checked(&path, ObjectKind::File)?;
                single_link(&object.file)?;
                let len = object.file.metadata().map_err(|_| Error::Io)?.len();
                if len > MAX_BYTES - *total {
                    return Err(Error::TooLarge);
                }
                let mut bytes = Vec::new();
                (&object.file)
                    .take(MAX_BYTES - *total + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| Error::Io)?;
                if bytes.len() as u64 != len {
                    return Err(Error::Unsafe);
                }
                single_link(&object.file)?;
                *total += len;
                self.files.push(Entry {
                    path: relative,
                    bytes,
                });
            }
        }
        Ok(())
    }

    /// Destination must be absent and its parent prepared by the transaction.
    /// Failure leaves only journal-owned partial output for explicit recovery.
    pub fn copy_new(&self, destination: &Path) -> Result<FileIdentity> {
        let _ancestors = ancestry(destination)?;
        absent(destination)?;
        let directory = platform::create_directory(destination).map_err(|_| Error::Io)?;
        for relative in &self.directories {
            platform::create_directory(&destination.join(relative)).map_err(|_| Error::Io)?;
        }
        for entry in &self.files {
            let mut file = platform::create_file(&destination.join(&entry.path))
                .map_err(|_| Error::Io)?
                .file;
            file.write_all(&entry.bytes).map_err(|_| Error::Io)?;
            file.sync_all().map_err(|_| Error::Io)?;
        }
        Ok(directory.identity)
    }
}

fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(Error::Collision),
        Err(_) => Err(Error::Io),
    }
}

pub fn store_path(root: &Path, name: &ProfileName) -> PathBuf {
    root.join("shared/skills").join(name.as_str())
}

/// Copy ownership includes each regular file identity as well as its digest.
#[derive(Debug)]
pub struct OwnedCopy {
    identity: FileIdentity,
    files: Vec<(String, FileIdentity)>,
    directories: Vec<(String, FileIdentity)>,
    sha256: String,
}
impl OwnedCopy {
    pub fn capture(path: &Path, sha256: &str) -> Result<Self> {
        let package = Package::read(path)?;
        if package.sha256 != sha256 {
            return Err(Error::Drift);
        }
        let mut files = Vec::new();
        for entry in package.files {
            files.push((
                entry.path.clone(),
                checked(&path.join(&entry.path), ObjectKind::File)?.identity,
            ));
        }
        let mut directories = Vec::new();
        for relative in package.directories {
            directories.push((
                relative.clone(),
                checked(&path.join(&relative), ObjectKind::Directory)?.identity,
            ));
        }
        Ok(Self {
            identity: checked(path, ObjectKind::Directory)?.identity,
            files,
            directories,
            sha256: sha256.into(),
        })
    }
    pub fn verify(&self, path: &Path) -> Result<()> {
        let current = Self::capture(path, &self.sha256).map_err(|_| Error::Drift)?;
        if current.identity != self.identity
            || current.files != self.files
            || current.directories != self.directories
        {
            return Err(Error::Drift);
        }
        Ok(())
    }
    /// Unlink exact proven leaves; never recursive deletion or following a target.
    pub fn remove(&self, path: &Path) -> Result<()> {
        self.verify(path)?;
        #[cfg(unix)]
        {
            self.remove_unix(path)
        }
        #[cfg(windows)]
        {
            Err(Error::Unsafe)
        }
    }
}

#[cfg(unix)]
impl OwnedCopy {
    fn remove_unix(&self, path: &Path) -> Result<()> {
        use std::{
            ffi::CString,
            os::{
                fd::{AsRawFd, FromRawFd},
                unix::fs::MetadataExt,
            },
        };
        fn name(value: &std::ffi::OsStr) -> Result<CString> {
            CString::new(value.as_encoded_bytes()).map_err(|_| Error::Unsafe)
        }
        fn open_directory(parent: &File, leaf: &std::ffi::OsStr) -> Result<File> {
            let leaf = name(leaf)?;
            // SAFETY: retained directory fd, valid C string; no symlink following.
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    leaf.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(Error::Drift);
            }
            // SAFETY: openat returned a newly owned fd.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
        fn identity(file: &File) -> Result<FileIdentity> {
            let meta = file.metadata().map_err(|_| Error::Io)?;
            Ok(FileIdentity::Unix {
                dev: meta.dev(),
                ino: meta.ino(),
            })
        }
        fn unlink(parent: &File, leaf: &std::ffi::OsStr, directory: bool) -> Result<()> {
            let leaf = name(leaf)?;
            // SAFETY: retained directory fd; unlinkat never traverses the leaf.
            if unsafe {
                libc::unlinkat(
                    parent.as_raw_fd(),
                    leaf.as_ptr(),
                    if directory { libc::AT_REMOVEDIR } else { 0 },
                )
            } != 0
            {
                return Err(Error::Io);
            }
            Ok(())
        }
        let _ancestors = ancestry(path)?;
        let parent = checked(path.parent().ok_or(Error::Unsafe)?, ObjectKind::Directory)?.file;
        let leaf = path.file_name().ok_or(Error::Unsafe)?;
        let root = open_directory(&parent, leaf)?;
        if identity(&root)? != self.identity {
            return Err(Error::Drift);
        }
        let mut held = std::collections::BTreeMap::new();
        held.insert(String::new(), root);
        for (relative, expected) in &self.directories {
            let relative_path = Path::new(relative);
            let parent_name = relative_path
                .parent()
                .and_then(Path::to_str)
                .ok_or(Error::Unsafe)?;
            let directory = open_directory(
                held.get(parent_name).ok_or(Error::Unsafe)?,
                relative_path.file_name().ok_or(Error::Unsafe)?,
            )?;
            if &identity(&directory)? != expected {
                return Err(Error::Drift);
            }
            held.insert(relative.clone(), directory);
        }
        for (relative, _) in &self.files {
            let relative_path = Path::new(relative);
            let parent_name = relative_path
                .parent()
                .and_then(Path::to_str)
                .ok_or(Error::Unsafe)?;
            unlink(
                held.get(parent_name).ok_or(Error::Unsafe)?,
                relative_path.file_name().ok_or(Error::Unsafe)?,
                false,
            )?;
        }
        for (relative, _) in self.directories.iter().rev() {
            let relative_path = Path::new(relative);
            let parent_name = relative_path
                .parent()
                .and_then(Path::to_str)
                .ok_or(Error::Unsafe)?;
            unlink(
                held.get(parent_name).ok_or(Error::Unsafe)?,
                relative_path.file_name().ok_or(Error::Unsafe)?,
                true,
            )?;
        }
        unlink(&parent, leaf, true)
    }
}

#[cfg(unix)]
#[derive(Debug)]
pub struct OwnedLink {
    identity: FileIdentity,
    target: PathBuf,
}
#[cfg(unix)]
impl OwnedLink {
    pub fn create(root: &Path, name: &ProfileName, path: &Path, sha256: &str) -> Result<Self> {
        let target = store_path(root, name);
        use std::os::unix::fs::MetadataExt;
        let _ancestors = ancestry(path)?;
        if Package::read(&target)?.sha256 != sha256 {
            return Err(Error::Drift);
        }
        absent(path)?;
        std::os::unix::fs::symlink(&target, path).map_err(|_| Error::Io)?;
        let meta = fs::symlink_metadata(path).map_err(|_| Error::Io)?;
        Ok(Self {
            identity: FileIdentity::Unix {
                dev: meta.dev(),
                ino: meta.ino(),
            },
            target,
        })
    }
    pub fn verify(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let _ancestors = ancestry(path)?;
        let meta = fs::symlink_metadata(path).map_err(|_| Error::Drift)?;
        if !meta.file_type().is_symlink()
            || (FileIdentity::Unix {
                dev: meta.dev(),
                ino: meta.ino(),
            }) != self.identity
            || fs::read_link(path).map_err(|_| Error::Drift)? != self.target
        {
            return Err(Error::Drift);
        }
        Ok(())
    }
    pub fn remove(&self, path: &Path) -> Result<()> {
        self.verify(path)?;
        fs::remove_file(path).map_err(|_| Error::Io)
    }
}
