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
    let basename = basename.strip_suffix(".exe").unwrap_or(basename);
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
    fn open(path: &Path, root: &Path) -> Result<Self, Error> {
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

/// Exclusive and empty: the caller journals the identity before writing any byte.
pub(crate) fn create_launcher(root: &Path, name: &ProfileName) -> Result<CheckedObject, Error> {
    let path = launcher_path(root, name);
    let _anchor = platform::anchor(&path)?;
    platform::create_file(&path)
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
/// Exclusive receipt creation; the journal records these exact bytes first.
pub(crate) fn write_receipt(root: &Path, name: &ProfileName, bytes: &[u8]) -> Result<(), Error> {
    let path = receipt_path(root, name);
    let _anchor = platform::anchor(&path)?;
    let mut object = platform::create_file(&path)?;
    object
        .file
        .write_all(bytes)
        .and_then(|()| object.file.sync_all())
        .map_err(|_| Error::AliasInvalid)
}

impl Receipt {
    /// Construct after exclusive launcher creation, while holding the registry lock.
    /// The journal/manifest must record the returned receipt hash before activation.
    pub(crate) fn prepare(
        root: &Path,
        name: ProfileName,
        profile_id: ProfileId,
        surface: Surface,
        source: &Source,
    ) -> Result<Self, Error> {
        if profile_id.get() == 0 || ProfileName::parse_new(name.as_str()).is_err() {
            return Err(Error::AliasInvalid);
        }
        let _anchor = platform::anchor(root)?;
        let root_object = required(root, ObjectKind::Directory)?;
        let bin = required(&root.join("bin"), ObjectKind::Directory)?;
        let mut launcher = read_required(&launcher_path(root, &name))?;
        if hash(&mut launcher.file, LAUNCHER_LIMIT)? != source.sha256 {
            return Err(Error::AliasInvalid);
        }
        let fresh_source = source.reopen(root)?;
        if fresh_source.identity != source.identity || fresh_source.sha256 != source.sha256 {
            return Err(Error::AdmissionChanged);
        }
        Ok(Self {
            version: 1,
            abi: ABI,
            root: root.into(),
            root_identity: (&root_object.identity).into(),
            bin_identity: (&bin.identity).into(),
            name,
            profile_id,
            surface,
            source: source.path.clone(),
            source_identity: (&source.identity).into(),
            launcher_identity: (&launcher.identity).into(),
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
        if executable != launcher_path(root, name) {
            return Err(Error::AliasInvalid);
        }
        let _anchor = platform::anchor(executable)?;
        let mut receipt = read_required(&receipt_path(root, name))?;
        let bytes = read_bounded(&mut receipt.file)?;
        let parsed: Self = serde_json::from_slice(&bytes).map_err(|_| Error::AliasInvalid)?;
        if parsed.root != root || &parsed.name != name {
            return Err(Error::AliasInvalid);
        }
        Ok((parsed, Sha256::digest(&bytes).into()))
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

    /// Call under the registry lock, after checking pending journal, mapping
    /// and independently owned receipt digest; keep returned handles to dispatch.
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
    fn check(&self, executable: &Path, with_source: bool) -> Result<Vec<CheckedObject>, Error> {
        if self.version != 1
            || self.abi != ABI
            || self.profile_id.get() == 0
            || executable != launcher_path(&self.root, &self.name)
            || !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Err(Error::AliasInvalid);
        }
        let mut held = platform::anchor(executable)?.ancestors;
        for (path, identity) in [
            (self.root.clone(), &self.root_identity),
            (self.root.join("bin"), &self.bin_identity),
        ] {
            let object = required(&path, ObjectKind::Directory)?;
            if Identity::from(&object.identity) != *identity {
                return Err(Error::AliasInvalid);
            }
            held.push(object);
        }
        let mut launcher = read_required(executable)?;
        if Identity::from(&launcher.identity) != self.launcher_identity
            || hash(&mut launcher.file, LAUNCHER_LIMIT)? != self.sha256
        {
            return Err(Error::AliasInvalid);
        }
        held.push(launcher);
        if !with_source {
            return Ok(held);
        }
        let source = self.source_image()?;
        if Identity::from(&source.identity) != self.source_identity || source.sha256 != self.sha256
        {
            return Err(Error::AliasInvalid);
        }
        held.push(CheckedObject {
            file: source.file,
            identity: source.identity,
        });
        Ok(held)
    }
    // Listing from the installed Larp sees its own image; a launcher sees another image.
    fn source_image(&self) -> Result<Source, Error> {
        if std::env::current_exe().ok().as_deref() == Some(self.source.as_path()) {
            let current = Source::current(&self.root)?;
            if current.path != self.source {
                return Err(Error::AliasInvalid);
            }
            return Ok(current);
        }
        Source::open(&self.source, &self.root)
    }
}

/// Every present artifact of a registered alias, proven ours for removal.
pub(crate) struct Owned {
    pub launcher: Option<(FileIdentity, [u8; 32])>,
    pub receipt: Option<[u8; 32]>,
}
/// Removal ownership: receipt bytes must match the registry-owned digest, and a present
/// launcher must match the receipt identity and hash. A launcher without a receipt is drift.
pub(crate) fn owned(
    root: &Path,
    name: &ProfileName,
    profile_id: ProfileId,
    surface: Surface,
    receipt_digest: &[u8; 32],
) -> Result<Owned, Error> {
    let executable = launcher_path(root, name);
    let _anchor = platform::anchor(&executable)?;
    let receipt = match read_optional(&receipt_path(root, name))? {
        None => None,
        Some(mut object) => {
            let bytes = read_bounded(&mut object.file)?;
            if Sha256::digest(&bytes)[..] != receipt_digest[..] {
                return Err(Error::AliasInvalid);
            }
            Some(bytes)
        }
    };
    let launcher = read_optional(&executable)?;
    match (receipt, launcher) {
        (None, None) => Ok(Owned {
            launcher: None,
            receipt: None,
        }),
        (None, Some(_)) => Err(Error::AliasInvalid),
        (Some(_), None) => Ok(Owned {
            launcher: None,
            receipt: Some(*receipt_digest),
        }),
        (Some(bytes), Some(mut object)) => {
            let r: Receipt = serde_json::from_slice(&bytes).map_err(|_| Error::AliasInvalid)?;
            if &r.name != name
                || r.profile_id != profile_id
                || r.surface != surface
                || r.root != root
                || Identity::from(&object.identity) != r.launcher_identity
                || hash(&mut object.file, LAUNCHER_LIMIT)? != r.sha256
            {
                return Err(Error::AliasInvalid);
            }
            Ok(Owned {
                launcher: Some((object.identity, r.sha256)),
                receipt: Some(*receipt_digest),
            })
        }
    }
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
    let (Ok(receipt), Ok(launcher)) = (
        read_optional(&receipt_path(root, name)),
        read_optional(&executable),
    ) else {
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
    if &r.name != name || r.profile_id != profile_id || r.surface != surface || r.root != root {
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

pub(crate) enum Removal {
    Absent,
    Removed,
    /// Present but not proven ours; left untouched.
    Foreign,
}
/// Unlink one fixed artifact only when `owns` accepts its checked handle.
pub(crate) fn remove_owned(
    path: &Path,
    owns: impl FnOnce(&mut CheckedObject) -> Result<bool, Error>,
) -> Result<Removal, Error> {
    let _anchor = platform::anchor(path)?;
    let Some(mut object) = read_optional(path)? else {
        return Ok(Removal::Absent);
    };
    if !owns(&mut object)? {
        return Ok(Removal::Foreign);
    }
    let identity = object.identity.clone();
    drop(object);
    #[cfg(windows)]
    platform::remove_skill_leaf(path, &identity, ObjectKind::File)?;
    #[cfg(unix)]
    {
        match platform::inspect(path, ObjectKind::File)? {
            Some(current) if current.identity == identity => (),
            _ => return Err(Error::AliasInvalid),
        }
        std::fs::remove_file(path).map_err(|_| Error::AliasInvalid)?;
    }
    Ok(Removal::Removed)
}
pub(crate) fn launcher_owner(
    identity: &FileIdentity,
    sha256: Option<&[u8; 32]>,
) -> impl FnOnce(&mut CheckedObject) -> Result<bool, Error> {
    move |object| {
        Ok(&object.identity == identity
            && match sha256 {
                None => true,
                Some(expected) => hash(&mut object.file, LAUNCHER_LIMIT)? == *expected,
            })
    }
}
/// A receipt is ours when its bytes are exactly (or, for an interrupted write, a prefix of)
/// the journaled receipt bytes.
pub(crate) fn receipt_owner(
    expected: &[u8],
    allow_prefix: bool,
) -> impl FnOnce(&mut CheckedObject) -> Result<bool, Error> + '_ {
    move |object| {
        let bytes = read_bounded(&mut object.file)?;
        Ok(bytes == expected || (allow_prefix && expected.starts_with(&bytes)))
    }
}
pub(crate) fn receipt_digest_owner(
    expected: &[u8; 32],
) -> impl FnOnce(&mut CheckedObject) -> Result<bool, Error> + '_ {
    move |object| Ok(digest(&read_bounded(&mut object.file)?) == *expected)
}
pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn required(path: &Path, kind: ObjectKind) -> Result<CheckedObject, Error> {
    platform::inspect(path, kind)?.ok_or(Error::AliasInvalid)
}
// Managed files are reopened with data access (Windows inspection is metadata-only).
fn read_optional(path: &Path) -> Result<Option<CheckedObject>, Error> {
    if platform::inspect(path, ObjectKind::File)?.is_none() {
        return Ok(None);
    }
    let object = platform::read_skill_object(path, ObjectKind::File, false)?;
    platform::single_link(&object.file).map_err(|_| Error::AliasInvalid)?;
    Ok(Some(object))
}
fn read_required(path: &Path) -> Result<CheckedObject, Error> {
    read_optional(path)?.ok_or(Error::AliasInvalid)
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
