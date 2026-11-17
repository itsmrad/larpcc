//! Skills filesystem operations. Callers must hold the registry lock and journal
//! every mutation before calling these primitives; this module creates no locks.
use crate::{
    platform::{self, CheckedObject, FileIdentity, ObjectKind},
    registry::ProfileName,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    bytes: Vec<u8>,
}
/// Bounded snapshot; never rereads a source during installation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    platform::single_link(file).map_err(|_| Error::Unsafe)
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
        Self::read_with(source, false)
    }
    pub fn source(source: &Path) -> Result<Self> {
        Self::read_with(source, true)
    }
    fn read_with(source: &Path, trusted_source: bool) -> Result<Self> {
        let _ancestors = ancestry(source)?;
        let source: PathBuf = source.components().collect();
        let _root = platform::read_skill_object(&source, ObjectKind::Directory, trusted_source)
            .map_err(|_| Error::Unsafe)?;
        let entry =
            platform::read_skill_object(&source.join("SKILL.md"), ObjectKind::File, trusted_source)
                .map_err(|_| Error::Unsafe)?;
        single_link(&entry.file)?;
        let mut package = Self {
            files: Vec::new(),
            directories: Vec::new(),
            sha256: String::new(),
        };
        let mut total = 0;
        package.walk(&source, "", 0, &mut total, trusted_source)?;
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

    fn walk(
        &mut self,
        root: &Path,
        relative: &str,
        depth: usize,
        total: &mut u64,
        trusted_source: bool,
    ) -> Result<()> {
        let path = if relative.is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        };
        let _directory = platform::read_skill_object(&path, ObjectKind::Directory, trusted_source)
            .map_err(|_| Error::Unsafe)?;
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
                self.walk(root, &relative, depth + 1, total, trusted_source)?;
            } else {
                if self.files.len() >= MAX_FILES {
                    return Err(Error::TooLarge);
                }
                let object = platform::read_skill_object(&path, ObjectKind::File, trusted_source)
                    .map_err(|_| Error::Unsafe)?;
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
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn copy_new(&self, destination: &Path) -> Result<FileIdentity> {
        use std::io::Write;
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

#[cfg(any(unix, test))]
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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OwnedCopy {
    identity: FileIdentity,
    files: Vec<(String, FileIdentity, String, u64)>,
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
                format!("{:x}", Sha256::digest(&entry.bytes)),
                entry.bytes.len() as u64,
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
        self.verify_contents(path, false).map_err(|_| Error::Drift)
    }

    /// Unlink exact proven leaves; never recursive deletion or following a target.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn remove(&self, path: &Path) -> Result<()> {
        self.verify(path)?;
        #[cfg(unix)]
        {
            self.remove_unix(path)
        }
        #[cfg(windows)]
        {
            self.remove_windows(path)
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
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::NotFound
            {
                return Err(Error::Io);
            }
            io_phase("unlink")?;
            Ok(())
        }
        let _ancestors = ancestry(path)?;
        let parent = checked(path.parent().ok_or(Error::Unsafe)?, ObjectKind::Directory)?.file;
        let leaf = path.file_name().ok_or(Error::Unsafe)?;
        if !path.try_exists().map_err(|_| Error::Io)? {
            return Ok(());
        }
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
            if !path.join(relative).try_exists().map_err(|_| Error::Io)? {
                continue;
            }
            let directory = open_directory(
                held.get(parent_name).ok_or(Error::Unsafe)?,
                relative_path.file_name().ok_or(Error::Unsafe)?,
            )?;
            if &identity(&directory)? != expected {
                return Err(Error::Drift);
            }
            held.insert(relative.clone(), directory);
        }
        for (relative, _, _, _) in &self.files {
            if !path.join(relative).try_exists().map_err(|_| Error::Io)? {
                continue;
            }
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
            if !path.join(relative).try_exists().map_err(|_| Error::Io)? {
                continue;
            }
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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OwnedLink {
    identity: FileIdentity,
    #[serde(with = "native_path")]
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

impl Package {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.files.len() > MAX_FILES || self.directories.len() > MAX_FILES {
            return Err(Error::TooLarge);
        }
        let mut total = 0u64;
        let mut paths = BTreeSet::new();
        for path in self
            .directories
            .iter()
            .map(String::as_str)
            .chain(self.files.iter().map(|f| f.path.as_str()))
        {
            relative(path)?;
            if !paths.insert(path.to_ascii_lowercase()) {
                return Err(Error::Collision);
            }
        }
        let mut digest = Sha256::new();
        let mut previous = None;
        for f in &self.files {
            if previous.is_some_and(|p: &str| p >= f.path.as_str()) {
                return Err(Error::Unsafe);
            }
            previous = Some(&f.path);
            total = total
                .checked_add(f.bytes.len() as u64)
                .ok_or(Error::TooLarge)?;
            if total > MAX_BYTES {
                return Err(Error::TooLarge);
            }
            digest.update((f.path.len() as u64).to_be_bytes());
            digest.update(f.path.as_bytes());
            digest.update((f.bytes.len() as u64).to_be_bytes());
            digest.update(&f.bytes);
        }
        if !self.files.iter().any(|f| f.path == "SKILL.md")
            || format!("{:x}", digest.finalize()) != self.sha256
        {
            return Err(Error::Unsafe);
        }
        Ok(())
    }
    /// Only for a journaled, preflight-absent destination. Resume exact prefixes;
    /// unrelated entries or changed bytes are never overwritten.
    pub(crate) fn resume_copy(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let _ancestors = ancestry(path)?;
        if fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            platform::create_directory(path).map_err(|_| Error::Io)?;
        }
        io_phase("directory")?;
        let mut held = vec![checked(path, ObjectKind::Directory)?];
        for relative in &self.directories {
            let child = path.join(relative);
            if fs::symlink_metadata(&child).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                platform::create_directory(&child).map_err(|_| Error::Io)?;
            }
            held.push(checked(&child, ObjectKind::Directory)?);
        }
        // Enumerate names only; never read an unplanned file's contents.
        for dir in std::iter::once("").chain(self.directories.iter().map(String::as_str)) {
            for child in fs::read_dir(path.join(dir)).map_err(|_| Error::Io)? {
                let child = child.map_err(|_| Error::Io)?;
                let relative = child
                    .path()
                    .strip_prefix(path)
                    .map_err(|_| Error::Unsafe)?
                    .to_str()
                    .ok_or(Error::Unsafe)?
                    .replace('\\', "/");
                if !self.files.iter().any(|f| f.path == relative)
                    && !self.directories.contains(&relative)
                {
                    return Err(Error::Drift);
                }
            }
        }
        for entry in &self.files {
            let child = path.join(&entry.path);
            if fs::symlink_metadata(&child).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                platform::create_file(&child).map_err(|_| Error::Io)?;
            }
            io_phase("file_created")?;
            let object = platform::read_skill_object(&child, ObjectKind::File, false)
                .map_err(|_| Error::Unsafe)?;
            single_link(&object.file)?;
            let mut bytes = Vec::new();
            (&object.file)
                .take(entry.bytes.len() as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Io)?;
            if !entry.bytes.starts_with(&bytes) {
                return Err(Error::Drift);
            }
            let identity = object.identity;
            drop(object.file);
            platform::append_skill_file(&child, &identity, &entry.bytes[bytes.len()..])
                .map_err(|_| Error::Unsafe)?;
            io_phase("file_written")?;
        }
        #[cfg(unix)]
        {
            for object in held.iter().rev() {
                object.file.sync_all().map_err(|_| Error::Io)?;
            }
        }
        Ok(())
    }
}

fn relative(path: &str) -> Result<()> {
    if path.is_empty() || path.split('/').count() > MAX_DEPTH {
        return Err(Error::Unsafe);
    }
    for part in path.split('/') {
        component(part)?;
    }
    Ok(())
}
impl OwnedCopy {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.files.len() > MAX_FILES || self.directories.len() > MAX_FILES {
            return Err(Error::TooLarge);
        }
        let mut names = BTreeSet::new();
        for path in self
            .files
            .iter()
            .map(|f| &f.0)
            .chain(self.directories.iter().map(|d| &d.0))
        {
            relative(path)?;
            if !names.insert(path.to_ascii_lowercase()) {
                return Err(Error::Unsafe);
            }
        }
        if self
            .files
            .iter()
            .map(|f| f.3)
            .try_fold(0u64, u64::checked_add)
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(Error::TooLarge);
        }
        if !self.files.iter().any(|f| f.0 == "SKILL.md")
            || self.files.windows(2).any(|w| w[0].0 >= w[1].0)
            || self.directories.windows(2).any(|w| w[0].0 >= w[1].0)
        {
            return Err(Error::Unsafe);
        }
        for path in self
            .files
            .iter()
            .map(|f| &f.0)
            .chain(self.directories.iter().map(|d| &d.0))
        {
            if let Some(parent) = path.rsplit_once('/').map(|p| p.0)
                && !self.directories.iter().any(|d| d.0 == parent)
            {
                return Err(Error::Unsafe);
            }
        }
        Ok(())
    }
    /// Verify remaining entries against the journaled manifest; missing leaves
    /// represent completed unlinks. No unknown entry is read or removed.
    fn verify_contents(&self, path: &Path, allow_missing: bool) -> Result<()> {
        self.validate()?;
        let _ancestors = ancestry(path)?;
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && allow_missing => return Ok(()),
            Err(_) => return Err(Error::Drift),
            Ok(_) => (),
        }
        if checked(path, ObjectKind::Directory)?.identity != self.identity {
            return Err(Error::Drift);
        }
        for (relative, id) in &self.directories {
            let exists = fs::symlink_metadata(path.join(relative)).is_ok();
            if (!exists && !allow_missing)
                || (exists && checked(&path.join(relative), ObjectKind::Directory)?.identity != *id)
            {
                return Err(Error::Drift);
            }
        }
        for dir in std::iter::once("").chain(self.directories.iter().map(|d| d.0.as_str())) {
            let directory = path.join(dir);
            if !directory.try_exists().map_err(|_| Error::Io)? {
                continue;
            }
            for child in fs::read_dir(directory).map_err(|_| Error::Io)? {
                let child = child.map_err(|_| Error::Io)?;
                let relative = child
                    .path()
                    .strip_prefix(path)
                    .map_err(|_| Error::Unsafe)?
                    .to_str()
                    .ok_or(Error::Unsafe)?
                    .replace('\\', "/");
                if !self.files.iter().any(|f| f.0 == relative)
                    && !self.directories.iter().any(|d| d.0 == relative)
                {
                    return Err(Error::Drift);
                }
            }
        }
        for (relative, id, digest, len) in &self.files {
            let child = path.join(relative);
            if fs::symlink_metadata(&child).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                if allow_missing {
                    continue;
                } else {
                    return Err(Error::Drift);
                }
            }
            let object = platform::read_skill_object(&child, ObjectKind::File, false)
                .map_err(|_| Error::Unsafe)?;
            single_link(&object.file)?;
            if object.identity != *id {
                return Err(Error::Drift);
            }
            let mut bytes = Vec::new();
            (&object.file)
                .take(*len + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Io)?;
            if bytes.len() as u64 != *len || format!("{:x}", Sha256::digest(bytes)) != *digest {
                return Err(Error::Drift);
            }
        }
        Ok(())
    }
    pub(crate) fn resume_remove(&self, path: &Path) -> Result<()> {
        self.verify_contents(path, true)?;
        if fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            return Ok(());
        }
        #[cfg(unix)]
        {
            self.remove_unix(path)
        }
        #[cfg(windows)]
        {
            self.remove_windows(path)
        }
    }
    #[cfg(windows)]
    fn remove_windows(&self, path: &Path) -> Result<()> {
        let _ancestors = ancestry(path)?;
        let root = checked(path, ObjectKind::Directory)?;
        if root.identity != self.identity {
            return Err(Error::Drift);
        }
        let mut held = Vec::new();
        for (relative, identity) in &self.directories {
            let child = path.join(relative);
            if fs::symlink_metadata(&child).is_ok() {
                let object = checked(&child, ObjectKind::Directory)?;
                if object.identity != *identity {
                    return Err(Error::Drift);
                }
                held.push((relative, object));
            }
        }
        for (relative, identity, _, _) in &self.files {
            let child = path.join(relative);
            if fs::symlink_metadata(&child).is_ok() {
                platform::remove_skill_leaf(&child, identity, ObjectKind::File)
                    .map_err(|_| Error::Unsafe)?;
                io_phase("unlink")?;
            }
        }
        while let Some((relative, object)) = held.pop() {
            let identity = object.identity;
            drop(object.file);
            platform::remove_skill_leaf(&path.join(relative), &identity, ObjectKind::Directory)
                .map_err(|_| Error::Unsafe)?;
        }
        drop(root.file);
        platform::remove_skill_leaf(path, &self.identity, ObjectKind::Directory)
            .map_err(|_| Error::Unsafe)
    }
}

#[cfg(unix)]
impl OwnedLink {
    pub(crate) fn validate_internal(&self, root: &Path, name: &ProfileName) -> Result<()> {
        if self.target != store_path(root, name) {
            return Err(Error::Unsafe);
        }
        Ok(())
    }
    pub(crate) fn capture_internal(root: &Path, name: &ProfileName, path: &Path) -> Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let target = store_path(root, name);
        let meta = fs::symlink_metadata(path).map_err(|_| Error::Io)?;
        if !meta.file_type().is_symlink() || fs::read_link(path).map_err(|_| Error::Io)? != target {
            return Err(Error::Drift);
        }
        let receipt = Self {
            identity: FileIdentity::Unix {
                dev: meta.dev(),
                ino: meta.ino(),
            },
            target,
        };
        receipt.verify(path)?;
        Ok(receipt)
    }
}

impl Package {
    pub(crate) fn manifest_bound(&self) -> Result<usize> {
        let paths = serde_json::to_vec(
            &self
                .files
                .iter()
                .map(|f| &f.path)
                .chain(self.directories.iter())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| Error::Unsafe)?
        .len();
        // Each receipt adds bounded identity, hash and length fields to each path.
        Ok(paths + (self.files.len() + self.directories.len()) * 256 + 1024)
    }
}

// Persist the exact native link target, including non-Unicode managed roots.
#[cfg(unix)]
mod native_path {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::{os::unix::ffi::OsStringExt, path::PathBuf};
    pub fn serialize<S: Serializer>(
        path: &std::path::Path,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        path.as_os_str().as_encoded_bytes().serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<PathBuf, D::Error> {
        Ok(std::ffi::OsString::from_vec(Vec::<u8>::deserialize(deserializer)?).into())
    }
}

impl OwnedCopy {
    pub(crate) fn digest(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
thread_local! {static IO_FAULT:std::cell::RefCell<Option<&'static str>>=const{std::cell::RefCell::new(None)};}
#[cfg(test)]
pub(crate) fn set_io_fault(phase: Option<&'static str>) {
    IO_FAULT.with(|f| *f.borrow_mut() = phase);
}
fn io_phase(_phase: &str) -> Result<()> {
    #[cfg(test)]
    if IO_FAULT.with(|f| f.borrow().is_some_and(|v| v == _phase)) {
        return Err(Error::Io);
    }
    Ok(())
}
