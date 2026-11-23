use super::{
    Error, ProfileName, Registry,
    catalog_sharing::{self, Kind, Sharing},
    mcp, paths, skills, store,
};
use crate::{
    platform::{self, FileIdentity, ObjectKind},
    remote::{mcp_frame, skill_frame},
    share::{self, mcp::Transport},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(crate) fn list(registry: &Registry, kind: Kind) -> Result<(u64, Value), Error> {
    let (data, _guard) = store::snapshot(registry.root(), registry.expected)?;
    let flags = Sharing::load(registry.root())?;
    let (catalog_revision, rows) = match kind {
        Kind::Mcp => {
            let (revision, servers) = mcp::catalog_entries(registry.root(), &data)?;
            (
                revision,
                servers
                    .into_iter()
                    .map(|server| {
                        let digest = mcp_frame::encode(&server.name, &server.transport)
                            .ok()
                            .map(|(_, digest)| digest);
                        (server.name, digest)
                    })
                    .collect::<Vec<_>>(),
            )
        }
        Kind::Skills => skills::catalog_entries(registry.root(), &data)?,
    };
    if rows.len() > 256 {
        return Err(Error::TooLarge);
    }
    let entries: Vec<_> = rows.into_iter().map(|(name, digest)| {
        json!({"name":name,"all_machines":flags.opted_in(kind,catalog_revision,&name),"content_sha256":digest})
    }).collect();
    Ok((data.revision, json!({"kind":kind,"entries":entries})))
}

pub(crate) fn share(
    registry: &Registry,
    kind: Kind,
    entry: &ProfileName,
    enabled: bool,
) -> Result<(bool, u64), Error> {
    let root = registry.root();
    let _guard = store::lock(root, Duration::from_secs(10))?;
    let data = store::load(root)?;
    store::precondition(&data, registry.expected)?;
    store::require_v3(&data)?;
    let not_found = || Error::NotFound {
        name: entry.clone(),
    };
    let revision = match kind {
        Kind::Mcp => {
            let (revision, servers) = mcp::catalog_entries(root, &data)?;
            if !servers.iter().any(|server| &server.name == entry) {
                return Err(not_found());
            }
            revision
        }
        Kind::Skills => {
            let (revision, skills) = skills::catalog_entries(root, &data)?;
            if !skills.iter().any(|(name, _)| name == entry) {
                return Err(not_found());
            }
            revision
        }
    };
    journal_outcome(
        root,
        catalog_sharing::share(root, &data, kind, revision, entry, enabled),
    )
}

pub(crate) fn export(
    registry: &Registry,
    kind: Kind,
    entry: &ProfileName,
) -> Result<(Vec<u8>, String), Error> {
    let root = registry.root();
    let (data, _guard) = store::snapshot(root, registry.expected)?;
    match kind {
        Kind::Mcp => {
            let (_, servers) = mcp::catalog_entries(root, &data)?;
            let server = servers
                .into_iter()
                .find(|s| &s.name == entry)
                .ok_or_else(|| Error::NotFound {
                    name: entry.clone(),
                })?;
            Ok(mcp_frame::encode(entry, &server.transport)?)
        }
        Kind::Skills => {
            let package = skills::catalog_export(root, &data, entry)?;
            let frame = skill_frame::encode(entry, &package.sha256, package.catalog_files())
                .map_err(frame_error)?;
            Ok((frame, package.sha256))
        }
    }
}

pub(crate) fn import_preflight(
    registry: &Registry,
    entry: &ProfileName,
    sha256: &str,
) -> Result<(), Error> {
    let (data, _guard) = store::snapshot(registry.root(), registry.expected)?;
    store::require_v3(&data)?;
    skills::catalog_import_preflight(registry.root(), &data, entry, sha256)
}

pub(crate) fn import_skills(
    registry: &Registry,
    entry: &ProfileName,
    sha256: &str,
    files: Vec<(String, Vec<u8>)>,
    deadline: Option<Instant>,
) -> Result<(bool, u64), Error> {
    before_commit(deadline)?;
    import_preflight(registry, entry, sha256)?;
    let stage = ImportStage::new(registry.root())?;
    let package = stage.stage(files, sha256, deadline)?;
    journal_outcome(
        registry.root(),
        skills::catalog_import(registry, entry, package, deadline),
    )
}

pub(crate) fn apply_mcp(
    registry: &Registry,
    entry: &ProfileName,
    definition: Transport,
    deadline: Option<Instant>,
) -> Result<(bool, u64), Error> {
    before_commit(deadline)?;
    journal_outcome(
        registry.root(),
        mcp::catalog_apply(registry, entry, definition, deadline),
    )
}

pub(super) fn before_commit(deadline: Option<Instant>) -> Result<(), Error> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Error::CatalogDeadline);
    }
    Ok(())
}

fn journal_outcome<T>(root: &Path, result: Result<T, Error>) -> Result<T, Error> {
    result.map_err(|error| {
        if matches!(error, Error::Io { .. }) && share::pending(root) {
            Error::RecoveryRequired
        } else {
            error
        }
    })
}

pub(crate) fn frame_error(error: skill_frame::Error) -> Error {
    let path = PathBuf::from("catalog frame");
    match error {
        skill_frame::Error::Corrupt => Error::Corrupt {
            path,
            reason: "invalid catalog frame".into(),
        },
        skill_frame::Error::Unsafe => Error::Unsafe {
            path,
            reason: "unsafe skill manifest path".into(),
        },
        skill_frame::Error::TooLarge => Error::TooLarge,
        skill_frame::Error::Io => paths::io(
            "read catalog frame",
            &path,
            std::io::Error::other("frame I/O failed"),
        ),
    }
}

static STAGE_ID: AtomicU64 = AtomicU64::new(0);
struct ImportStage {
    root: PathBuf,
    objects: Vec<(PathBuf, FileIdentity, ObjectKind)>,
    _ancestors: platform::AnchoredPath,
}

impl ImportStage {
    fn new(root: &Path) -> Result<Self, Error> {
        let _guard = store::lock(root, Duration::from_secs(10))?;
        let ancestors = platform::anchor(&root.join("import"))?;
        for _ in 0..32 {
            let sequence = STAGE_ID.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("skill-import-{}-{sequence}", std::process::id()));
            match platform::create_directory(&path) {
                Ok(object) => {
                    return Ok(Self {
                        root: path.clone(),
                        objects: vec![(path, object.identity, ObjectKind::Directory)],
                        _ancestors: ancestors,
                    });
                }
                Err(Error::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::AlreadyExists =>
                {
                    ()
                }
                Err(error) => return Err(error),
            }
        }
        Err(Error::RecoveryRequired)
    }

    fn stage(
        mut self,
        files: Vec<(String, Vec<u8>)>,
        sha256: &str,
        deadline: Option<Instant>,
    ) -> Result<share::skills::Package, Error> {
        let mut directories = BTreeSet::new();
        for (relative, bytes) in files {
            before_commit(deadline)?;
            let mut parts = relative.split('/').peekable();
            let mut path = self.root.clone();
            while let Some(part) = parts.next() {
                path.push(part);
                if parts.peek().is_some() && directories.insert(path.clone()) {
                    let directory = platform::create_directory(&path)?;
                    self.objects
                        .push((path.clone(), directory.identity, ObjectKind::Directory));
                }
            }
            let mut object = platform::create_file(&path)?;
            self.objects
                .push((path.clone(), object.identity, ObjectKind::File));
            object
                .file
                .write_all(&bytes)
                .map_err(|e| paths::io("stage skill file", &path, e))?;
        }
        let package = share::skills::Package::read(&self.root)?;
        if package.sha256 != sha256 {
            return Err(frame_error(skill_frame::Error::Corrupt));
        }
        before_commit(deadline)?;
        Ok(package)
    }
}

impl Drop for ImportStage {
    fn drop(&mut self) {
        for (path, identity, kind) in self.objects.iter().rev() {
            if !platform::inspect(path, *kind).is_ok_and(|result| {
                result.is_some_and(|object| {
                    object.identity == *identity
                        && (*kind != ObjectKind::File
                            || platform::single_link(&object.file).is_ok())
                })
            }) {
                continue;
            }
            #[cfg(windows)]
            let _ = platform::remove_skill_leaf(path, identity, *kind);
            #[cfg(unix)]
            let _ = if *kind == ObjectKind::Directory {
                std::fs::remove_dir(path)
            } else {
                std::fs::remove_file(path)
            };
        }
    }
}
