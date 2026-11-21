//! Root-bound Larp copies. Registry mutation and the pending-journal gate own activation.
//! The only artifact paths are the fixed `<root>/bin/NAME[.exe]` launcher and its adjacent
//! `<root>/bin/NAME.larp-alias.json` receipt; nothing here accepts another path to remove.
use crate::{
    platform::{self, CheckedObject, FileIdentity, ObjectKind},
    registry::{Error, ProfileId, ProfileName, Surface},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const ABI: u32 = 1;
const RECEIPT_LIMIT: u64 = 16 * 1024;
const LAUNCHER_LIMIT: u64 = 512 * 1024 * 1024;

#[cfg(test)]
#[allow(
    dead_code,
    reason = "integration fixtures check the alias name grammar"
)]
pub(crate) fn name(value: &str, shadow: bool) -> Result<ProfileName, Error> {
    let name = ProfileName::parse_new(value).map_err(|_| Error::ArgConflict {
        label: "alias name",
    })?;
    if shadow_required(&name) && !shadow {
        return Err(Error::ArgConflict {
            label: "--shadow required",
        });
    }
    Ok(name)
}
/// Native command names are only installed with explicit `--shadow`.
pub(crate) fn shadow_required(name: &ProfileName) -> bool {
    matches!(name.as_str(), "claude" | "codex")
}

/// This classification must run before clap or global --json discovery.
/// Only the exact standard executable basename enters the normal parser.
pub(crate) fn invocation(argv0: &OsStr) -> Result<Option<ProfileName>, Error> {
    let path = Path::new(argv0);
    let basename = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or(Error::AliasInvalid)?;
    #[cfg(windows)]
    let normalized = basename.to_ascii_lowercase();
    #[cfg(windows)]
    let basename = normalized.strip_suffix(".exe").unwrap_or(&normalized);
    if basename == "larp" {
        return Ok(None);
    }
    ProfileName::parse_new(basename)
        .map(Some)
        .map_err(|_| Error::AliasInvalid)
}

pub(crate) fn launcher_path(root: &Path, name: &ProfileName) -> PathBuf {
    let path = root.join("bin").join(name.as_str());
    #[cfg(windows)]
    let path = path.with_extension("exe");
    path
}
pub(crate) fn receipt_path(root: &Path, name: &ProfileName) -> PathBuf {
    root.join("bin")
        .join(format!("{}.larp-alias.json", name.as_str()))
}

/// Metadata only: never execute or read an existing command to test collision.
pub(crate) fn check_collision(
    root: &Path,
    name: &ProfileName,
    search_path: Option<&OsStr>,
) -> Result<(), Error> {
    let mut candidates = vec![launcher_path(root, name), receipt_path(root, name)];
    if let Some(search_path) = search_path {
        for directory in std::env::split_paths(search_path) {
            #[cfg(not(windows))]
            candidates.push(directory.join(name.as_str()));
            #[cfg(windows)]
            for extension in ["", ".exe", ".com", ".cmd", ".bat"] {
                candidates.push(directory.join(format!("{}{extension}", name.as_str())));
            }
        }
    }
    for path in candidates {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(Error::Conflict),
        }
    }
    Ok(())
}

fn same_path(a: &Path, b: &Path) -> Result<bool, Error> {
    #[cfg(windows)]
    return Ok(platform::contains_path(a, b)? && platform::contains_path(b, a)?);
    #[cfg(unix)]
    Ok(a == b)
}

pub(crate) fn same_launcher(
    executable: &Path,
    root: &Path,
    name: &ProfileName,
) -> Result<bool, Error> {
    same_path(executable, &launcher_path(root, name))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectoryPin {
    root: FileIdentity,
    bin: Option<FileIdentity>,
}

pub(crate) struct Directories {
    root: CheckedObject,
    bin: Option<CheckedObject>,
    _ancestors: platform::AnchoredPath,
}
impl Directories {
    pub(crate) fn open(root: &Path) -> Result<Self, Error> {
        let bin = root.join("bin");
        let ancestors = platform::anchor(&bin.join(".alias-directory"))?;
        let root_object =
            platform::inspect(root, ObjectKind::Directory)?.ok_or(Error::AliasInvalid)?;
        let bin = platform::alias_open_at(&root_object, &bin, ObjectKind::Directory, false)?;
        Ok(Self {
            root: root_object,
            bin,
            _ancestors: ancestors,
        })
    }
    pub(crate) fn pin(&self) -> DirectoryPin {
        DirectoryPin {
            root: self.root.identity.clone(),
            bin: self.bin.as_ref().map(|bin| bin.identity.clone()),
        }
    }
    pub(crate) fn read(&self, path: &Path) -> Result<Option<CheckedObject>, Error> {
        let Some(bin) = &self.bin else {
            return Ok(None);
        };
        let object = platform::alias_open_at(bin, path, ObjectKind::File, false)?;
        if let Some(object) = &object {
            platform::single_link(&object.file).map_err(|_| Error::AliasInvalid)?;
        }
        Ok(object)
    }
    pub(crate) fn create(&self, path: &Path) -> Result<CheckedObject, Error> {
        platform::alias_open_at(self.bin()?, path, ObjectKind::File, true)?
            .ok_or(Error::AliasInvalid)
    }
    pub(crate) fn remove(&self, path: &Path, expected: &FileIdentity) -> Result<(), Error> {
        platform::alias_unlink_at(self.bin()?, path, expected)
    }
    pub(crate) fn publish(&self, from: &Path, to: &Path, expected: &Artifact) -> Result<(), Error> {
        if expected.present(self, from)? != Some(true) {
            return Err(Error::AliasInvalid);
        }
        platform::alias_publish_at(self.bin()?, from, to)
    }
    fn bin(&self) -> Result<&CheckedObject, Error> {
        self.bin.as_ref().ok_or(Error::AliasInvalid)
    }
    pub(crate) fn sync(&self) -> Result<(), Error> {
        #[cfg(unix)]
        if let Some(bin) = &self.bin {
            bin.file.sync_all().map_err(|_| Error::AliasInvalid)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub identity: FileIdentity,
    pub digest: [u8; 32],
}
impl Artifact {
    pub(crate) fn present(&self, dirs: &Directories, path: &Path) -> Result<Option<bool>, Error> {
        let Some(mut object) = dirs.read(path)? else {
            return Ok(None);
        };
        Ok(Some(
            object.identity == self.identity
                && hash(&mut object.file, LAUNCHER_LIMIT)? == self.digest,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "platform", rename_all = "lowercase")]
enum Identity {
    #[cfg(unix)]
    Unix { dev: u64, ino: u64 },
    #[cfg(windows)]
    Windows { volume: u64, id: [u8; 16] },
}
impl From<&FileIdentity> for Identity {
    fn from(value: &FileIdentity) -> Self {
        match value {
            #[cfg(unix)]
            FileIdentity::Unix { dev, ino } => Self::Unix {
                dev: *dev,
                ino: *ino,
            },
            #[cfg(windows)]
            FileIdentity::Windows { volume, id } => Self::Windows {
                volume: *volume,
                id: *id,
            },
        }
    }
}
impl From<&Identity> for FileIdentity {
    fn from(value: &Identity) -> Self {
        match value {
            #[cfg(unix)]
            Identity::Unix { dev, ino } => Self::Unix {
                dev: *dev,
                ino: *ino,
            },
            #[cfg(windows)]
            Identity::Windows { volume, id } => Self::Windows {
                volume: *volume,
                id: *id,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    version: u32,
    abi: u32,
    root: PathBuf,
    root_identity: Identity,
    bin_identity: Identity,
    name: ProfileName,
    profile_id: ProfileId,
    surface: Surface,
    source: PathBuf,
    source_identity: Identity,
    launcher_identity: Identity,
    sha256: [u8; 32],
}

pub(crate) struct Source {
    pub file: File,
    pub identity: FileIdentity,
    pub path: PathBuf,
    pub sha256: [u8; 32],
    running: bool,
}
impl Source {
    /// The running standard Larp, through the self-only executable trust policy.
    pub(crate) fn current(root: &Path) -> Result<Self, Error> {
        let (file, identity, path) = platform::open_current_larp(root)?;
        if invocation(path.as_os_str())?.is_some() {
            return Err(Error::AliasInvalid);
        }
        Self::hashed(file, identity, path, true)
    }
    // Another Larp image (an installed source seen from a launcher); never self.
    pub(crate) fn open(path: &Path, root: &Path) -> Result<Self, Error> {
        let (file, identity) = platform::open_artifact(path, root)?;
        Self::hashed(file, identity, path.into(), false)
    }
    fn hashed(
        mut file: File,
        identity: FileIdentity,
        path: PathBuf,
        running: bool,
    ) -> Result<Self, Error> {
        let sha256 = hash(&mut file, LAUNCHER_LIMIT)?;
        file.rewind().map_err(|_| Error::AliasInvalid)?;
        Ok(Self {
            file,
            identity,
            path,
            sha256,
            running,
        })
    }
    fn reopen(&self, root: &Path) -> Result<Self, Error> {
        if self.running {
            Self::current(root)
        } else {
            Self::open(&self.path, root)
        }
    }
}

/// Copy the checked source handle, make it owner-executable and prove the bytes landed.
pub(crate) fn fill_launcher(mut launcher: CheckedObject, source: &Source) -> Result<(), Error> {
    let mut input = &source.file;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|_| Error::AliasInvalid)?;
    std::io::copy(&mut input.take(LAUNCHER_LIMIT + 1), &mut launcher.file)
        .map_err(|_| Error::AliasInvalid)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        launcher
            .file
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::AliasInvalid)?;
    }
    launcher.file.flush().map_err(|_| Error::AliasInvalid)?;
    launcher.file.sync_all().map_err(|_| Error::AliasInvalid)?;
    if hash(&mut launcher.file, LAUNCHER_LIMIT)? != source.sha256 {
        return Err(Error::AliasInvalid);
    }
    Ok(())
}
impl Receipt {
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "receipt integration fixtures use the published launcher"
    )]
    pub(crate) fn prepare(
        root: &Path,
        name: ProfileName,
        profile_id: ProfileId,
        surface: Surface,
        source: &Source,
    ) -> Result<Self, Error> {
        let dirs = Directories::open(root)?;
        let mut launcher = dirs
            .read(&launcher_path(root, &name))?
            .ok_or(Error::AliasInvalid)?;
        if hash(&mut launcher.file, LAUNCHER_LIMIT)? != source.sha256 {
            return Err(Error::AliasInvalid);
        }
        Self::for_launcher(
            root,
            &dirs,
            name,
            profile_id,
            surface,
            source,
            &launcher.identity,
        )
    }

    pub(crate) fn for_launcher(
        root: &Path,
        dirs: &Directories,
        name: ProfileName,
        profile_id: ProfileId,
        surface: Surface,
        source: &Source,
        launcher: &FileIdentity,
    ) -> Result<Self, Error> {
        let fresh_source = source.reopen(root)?;
        if fresh_source.identity != source.identity || fresh_source.sha256 != source.sha256 {
            return Err(Error::AdmissionChanged);
        }
        Ok(Self {
            version: 1,
            abi: ABI,
            root: root.into(),
            root_identity: (&dirs.root.identity).into(),
            bin_identity: (&dirs.bin()?.identity).into(),
            name,
            profile_id,
            surface,
            source: source.path.clone(),
            source_identity: (&source.identity).into(),
            launcher_identity: launcher.into(),
            sha256: source.sha256,
        })
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, Error> {
        let bytes = serde_json::to_vec(self).map_err(|_| Error::AliasInvalid)?;
        if bytes.len() as u64 > RECEIPT_LIMIT {
            return Err(Error::TooLarge);
        }
        Ok(bytes)
    }

    /// Bootstrap only: the root is still untrusted until validate plus the
    /// same-lock owned-manifest hash and registered mapping checks succeed.
    pub(crate) fn adjacent(
        executable: &Path,
        name: &ProfileName,
    ) -> Result<(Self, [u8; 32]), Error> {
        let bin = executable.parent().ok_or(Error::AliasInvalid)?;
        let root = bin.parent().ok_or(Error::AliasInvalid)?;
        if !same_path(executable, &launcher_path(root, name))? {
            return Err(Error::AliasInvalid);
        }
        let dirs = Directories::open(root)?;
        let mut receipt = dirs
            .read(&receipt_path(root, name))?
            .ok_or(Error::AliasInvalid)?;
        let bytes = read_bounded(&mut receipt.file)?;
        let parsed: Self = serde_json::from_slice(&bytes).map_err(|_| Error::AliasInvalid)?;
        if !same_path(&parsed.root, root)? || &parsed.name != name {
            return Err(Error::AliasInvalid);
        }
        Ok((parsed, Sha256::digest(&bytes).into()))
    }

    pub(crate) fn staged(
        root: &Path,
        dirs: &Directories,
        path: &Path,
        name: &ProfileName,
        launcher: &Artifact,
        expected: &Artifact,
    ) -> Result<Self, Error> {
        let mut object = dirs.read(path)?.ok_or(Error::AliasInvalid)?;
        let bytes = read_bounded(&mut object.file)?;
        let receipt: Self = serde_json::from_slice(&bytes).map_err(|_| Error::AliasInvalid)?;
        if object.identity != expected.identity
            || self::digest(&bytes) != expected.digest
            || receipt.name() != name
            || receipt.version != 1
            || receipt.abi != ABI
            || !same_path(receipt.root(), root)?
            || receipt.directories() != dirs.pin()
            || FileIdentity::from(&receipt.launcher_identity) != launcher.identity
            || receipt.sha256 != launcher.digest
        {
            return Err(Error::AliasInvalid);
        }
        Ok(receipt)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn name(&self) -> &ProfileName {
        &self.name
    }
    pub(crate) fn profile_id(&self) -> ProfileId {
        self.profile_id
    }
    pub(crate) fn surface(&self) -> Surface {
        self.surface
    }

    fn directories(&self) -> DirectoryPin {
        DirectoryPin {
            root: (&self.root_identity).into(),
            bin: Some((&self.bin_identity).into()),
        }
    }

    /// Call under the registry lock, with the independently owned receipt digest.
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "receipt integration fixtures validate without a mapping"
    )]
    pub(crate) fn validate(
        &self,
        executable: &Path,
        argv0: &OsStr,
    ) -> Result<Vec<CheckedObject>, Error> {
        if invocation(argv0)?.as_ref() != Some(&self.name) {
            return Err(Error::AliasInvalid);
        }
        self.check(executable, true)
    }
    pub(crate) fn check_mapping(
        &self,
        root: &Path,
        digest: &[u8; 32],
    ) -> Result<Vec<CheckedObject>, Error> {
        if !same_path(root, &self.root)? {
            return Err(Error::AliasInvalid);
        }
        let dirs = Directories::open(root)?;
        if dirs.pin() != self.directories() {
            return Err(Error::AliasInvalid);
        }
        let mut object = dirs
            .read(&receipt_path(root, &self.name))?
            .ok_or(Error::AliasInvalid)?;
        if self::digest(&read_bounded(&mut object.file)?) != *digest {
            return Err(Error::AliasInvalid);
        }
        let mut held = self.check(&launcher_path(root, &self.name), true)?;
        held.push(object);
        Ok(held)
    }

    fn check(&self, executable: &Path, with_source: bool) -> Result<Vec<CheckedObject>, Error> {
        if self.version != 1
            || self.abi != ABI
            || self.profile_id.get() == 0
            || !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|c| c == std::path::Component::ParentDir)
            || !same_path(executable, &launcher_path(&self.root, &self.name))?
        {
            return Err(Error::AliasInvalid);
        }
        let dirs = Directories::open(&self.root)?;
        if dirs.pin() != self.directories() {
            return Err(Error::AliasInvalid);
        }
        let launcher = Artifact {
            identity: (&self.launcher_identity).into(),
            digest: self.sha256,
        };
        let mut object = dirs.read(executable)?.ok_or(Error::AliasInvalid)?;
        if object.identity != launcher.identity
            || hash(&mut object.file, LAUNCHER_LIMIT)? != launcher.digest
        {
            return Err(Error::AliasInvalid);
        }
        let mut held = vec![dirs.root, dirs.bin.ok_or(Error::AliasInvalid)?, object];
        held.extend(dirs._ancestors.ancestors);
        if with_source {
            let source = self.source_image()?;
            if Identity::from(&source.identity) != self.source_identity
                || source.sha256 != self.sha256
            {
                return Err(Error::AliasInvalid);
            }
            held.push(CheckedObject {
                file: source.file,
                identity: source.identity,
            });
        }
        Ok(held)
    }

    pub(crate) fn running(&self, executable: &Path) -> Result<(), Error> {
        if platform::running_identity(executable)? != FileIdentity::from(&self.launcher_identity) {
            return Err(Error::AliasInvalid);
        }
        Ok(())
    }
    // Listing from the installed Larp sees its own image; a launcher sees another image.
    fn source_image(&self) -> Result<Source, Error> {
        if let Ok(executable) = std::env::current_exe()
            && same_path(&executable, &self.source)?
        {
            let current = Source::current(&self.root)?;
            if !same_path(&current.path, &self.source)? {
                return Err(Error::AliasInvalid);
            }
            return Ok(current);
        }
        Source::open(&self.source, &self.root)
    }
}

/// Every present artifact of a registered alias, proven ours for removal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owned {
    pub launcher: Option<Artifact>,
    pub receipt: Option<Artifact>,
}
/// Removal and replay both derive ownership from the committed receipt digest.
pub(crate) fn owned(
    root: &Path,
    dirs: &Directories,
    name: &ProfileName,
    profile_id: ProfileId,
    surface: Surface,
    receipt_digest: &[u8; 32],
) -> Result<Owned, Error> {
    let receipt = dirs.read(&receipt_path(root, name))?;
    let launcher = dirs.read(&launcher_path(root, name))?;
    let Some(mut receipt) = receipt else {
        if launcher.is_some() {
            return Err(Error::AliasInvalid);
        }
        return Ok(Owned {
            launcher: None,
            receipt: None,
        });
    };
    let bytes = read_bounded(&mut receipt.file)?;
    if digest(&bytes) != *receipt_digest {
        return Err(Error::AliasInvalid);
    }
    let r: Receipt = serde_json::from_slice(&bytes).map_err(|_| Error::AliasInvalid)?;
    if &r.name != name
        || r.profile_id != profile_id
        || r.surface != surface
        || r.version != 1
        || !same_path(&r.root, root)?
        || dirs.pin() != r.directories()
    {
        return Err(Error::AliasInvalid);
    }
    let launcher = if let Some(mut object) = launcher {
        if Identity::from(&object.identity) != r.launcher_identity
            || hash(&mut object.file, LAUNCHER_LIMIT)? != r.sha256
        {
            return Err(Error::AliasInvalid);
        }
        Some(Artifact {
            identity: object.identity,
            digest: r.sha256,
        })
    } else {
        None
    };
    Ok(Owned {
        launcher,
        receipt: Some(Artifact {
            identity: receipt.identity,
            digest: *receipt_digest,
        }),
    })
}

/// Public listing state; never mutates and never trusts the receipt without the registry digest.
pub(crate) fn state(
    root: &Path,
    name: &ProfileName,
    profile_id: ProfileId,
    surface: Surface,
    receipt_digest: &[u8; 32],
) -> &'static str {
    let executable = launcher_path(root, name);
    let Ok(dirs) = Directories::open(root) else {
        return "drift";
    };
    let (Ok(receipt), Ok(launcher)) =
        (dirs.read(&receipt_path(root, name)), dirs.read(&executable))
    else {
        return "drift";
    };
    let receipt = match (receipt, launcher) {
        (_, None) => return "missing",
        (None, Some(_)) => return "drift",
        (Some(mut receipt), Some(_)) => match read_bounded(&mut receipt.file) {
            Ok(bytes) if Sha256::digest(&bytes)[..] == receipt_digest[..] => bytes,
            _ => return "drift",
        },
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&receipt) else {
        return "drift";
    };
    if value.get("version") != Some(&1.into()) || value.get("abi") != Some(&ABI.into()) {
        return "incompatible";
    }
    let Ok(r) = serde_json::from_value::<Receipt>(value) else {
        return "drift";
    };
    if &r.name != name
        || r.profile_id != profile_id
        || r.surface != surface
        || !same_path(&r.root, root).unwrap_or(false)
    {
        return "drift";
    }
    if r.check(&executable, false).is_err() {
        return "drift";
    }
    // The launcher is intact but no longer a copy of the installed Larp: reinstall.
    match r.source_image() {
        Ok(source)
            if Identity::from(&source.identity) == r.source_identity
                && source.sha256 == r.sha256 =>
        {
            "ok"
        }
        _ => "incompatible",
    }
}

pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn read_bounded(file: &mut File) -> Result<Vec<u8>, Error> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Error::AliasInvalid)?;
    let mut bytes = Vec::new();
    file.take(RECEIPT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::AliasInvalid)?;
    if bytes.len() as u64 > RECEIPT_LIMIT {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}
fn hash(file: &mut File, limit: u64) -> Result<[u8; 32], Error> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Error::AliasInvalid)?;
    let mut digest = Sha256::new();
    let mut read = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| Error::AliasInvalid)?;
        if count == 0 {
            return Ok(digest.finalize().into());
        }
        read += count as u64;
        if read > limit {
            return Err(Error::TooLarge);
        }
        digest.update(&buffer[..count]);
    }
}

/// Test fixtures only: retain an explicitly selected Larp copy (never the running image).
#[cfg(test)]
#[allow(dead_code, reason = "used by integration and registry unit fixtures")]
pub(crate) fn test_source(path: &Path, root: &Path) -> Source {
    Source::open(path, root).unwrap()
}
