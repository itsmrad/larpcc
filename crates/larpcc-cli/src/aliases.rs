//! Root-bound Larp copies. Registry mutation and the pending-journal gate own activation.
use crate::{
    platform::{self, CheckedObject, FileIdentity, ObjectKind},
    registry::{Error, ProfileId, ProfileName, Surface},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const ABI: u32 = 1;
const RECEIPT_LIMIT: u64 = 16 * 1024;
const LAUNCHER_LIMIT: u64 = 512 * 1024 * 1024;

pub(crate) fn name(value: &str, shadow: bool) -> Result<ProfileName, Error> {
    let name = ProfileName::parse_new(value).map_err(|_| Error::ArgConflict {
        label: "alias name",
    })?;
    if matches!(value, "claude" | "codex") && !shadow {
        return Err(Error::ArgConflict {
            label: "--shadow required",
        });
    }
    Ok(name)
}

/// This classification must run before clap or global --json discovery.
/// Only the exact standard executable basename enters the normal parser.
pub(crate) fn invocation(argv0: &OsStr) -> Result<Option<ProfileName>, Error> {
    let path = Path::new(argv0);
    let basename = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or(Error::ArtifactInvalid)?;
    #[cfg(windows)]
    let basename = basename.strip_suffix(".exe").unwrap_or(basename);
    if basename == "larp" {
        return Ok(None);
    }
    ProfileName::parse_new(basename)
        .map(Some)
        .map_err(|_| Error::ArtifactInvalid)
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
}
impl Source {
    /// Call with current_exe of standard Larp, never a vendor binding.
    pub(crate) fn current(root: &Path) -> Result<Self, Error> {
        let path = std::env::current_exe().map_err(|_| Error::ArtifactInvalid)?;
        if invocation(path.as_os_str())?.is_some() {
            return Err(Error::ArtifactInvalid);
        }
        Self::open(&path, root)
    }
    fn open(path: &Path, root: &Path) -> Result<Self, Error> {
        let (mut file, identity) = platform::open_artifact(path, root)?;
        let sha256 = hash(&mut file, LAUNCHER_LIMIT)?;
        file.rewind().map_err(|_| Error::ArtifactInvalid)?;
        Ok(Self {
            file,
            identity,
            path: path.into(),
            sha256,
        })
    }
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
            return Err(Error::ArtifactInvalid);
        }
        let _anchor = platform::anchor(root)?;
        let root_object = required(root, ObjectKind::Directory)?;
        let bin = required(&root.join("bin"), ObjectKind::Directory)?;
        let mut launcher = required(&launcher_path(root, &name), ObjectKind::File)?;
        if hash(&mut launcher.file, LAUNCHER_LIMIT)? != source.sha256 {
            return Err(Error::ArtifactInvalid);
        }
        let fresh_source = Source::open(&source.path, root)?;
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
        let bytes = serde_json::to_vec(self).map_err(|_| Error::ArtifactInvalid)?;
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
        let bin = executable.parent().ok_or(Error::ArtifactInvalid)?;
        let root = bin.parent().ok_or(Error::ArtifactInvalid)?;
        if executable != launcher_path(root, name) {
            return Err(Error::ArtifactInvalid);
        }
        let _anchor = platform::anchor(executable)?;
        let mut receipt = required(&receipt_path(root, name), ObjectKind::File)?;
        let mut bytes = Vec::new();
        (&mut receipt.file)
            .take(RECEIPT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::ArtifactInvalid)?;
        if bytes.len() as u64 > RECEIPT_LIMIT {
            return Err(Error::TooLarge);
        }
        let parsed: Self = serde_json::from_slice(&bytes).map_err(|_| Error::ArtifactInvalid)?;
        if parsed.root != root || &parsed.name != name {
            return Err(Error::ArtifactInvalid);
        }
        Ok((parsed, Sha256::digest(&bytes).into()))
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
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
        if self.version != 1
            || self.abi != ABI
            || self.profile_id.get() == 0
            || invocation(argv0)?.as_ref() != Some(&self.name)
            || executable != launcher_path(&self.root, &self.name)
            || !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Err(Error::ArtifactInvalid);
        }
        let mut held = platform::anchor(executable)?.ancestors;
        for (path, kind, identity) in [
            (
                self.root.clone(),
                ObjectKind::Directory,
                &self.root_identity,
            ),
            (
                self.root.join("bin"),
                ObjectKind::Directory,
                &self.bin_identity,
            ),
            (executable.into(), ObjectKind::File, &self.launcher_identity),
        ] {
            let mut object = required(&path, kind)?;
            if Identity::from(&object.identity) != *identity {
                return Err(Error::ArtifactInvalid);
            }
            if kind == ObjectKind::File && hash(&mut object.file, LAUNCHER_LIMIT)? != self.sha256 {
                return Err(Error::ArtifactInvalid);
            }
            held.push(object);
        }
        let source = Source::open(&self.source, &self.root)?;
        if Identity::from(&source.identity) != self.source_identity || source.sha256 != self.sha256
        {
            return Err(Error::ArtifactInvalid);
        }
        held.push(CheckedObject {
            file: source.file,
            identity: source.identity,
        });
        Ok(held)
    }
}

fn required(path: &Path, kind: ObjectKind) -> Result<CheckedObject, Error> {
    platform::inspect(path, kind)?.ok_or(Error::ArtifactInvalid)
}
fn hash(file: &mut File, limit: u64) -> Result<[u8; 32], Error> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Error::ArtifactInvalid)?;
    let mut digest = Sha256::new();
    let mut read = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| Error::ArtifactInvalid)?;
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

#[cfg(test)]
pub(crate) fn test_source(path: &Path, root: &Path) -> Source {
    Source::open(path, root).unwrap()
}
