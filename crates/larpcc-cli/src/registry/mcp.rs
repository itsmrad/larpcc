//! MCP registry transactions (`registry::mcp`). They share the
//! registry lock, snapshot revision and share-journal gate. Journal kind `mcp`.
//!
//! Files: `mcp.json` (catalog), `mcp-owned.json` (render manifest) and one
//! render per profile below `profiles/<ID>/cli/larp-mcp/`. Paths are derived
//! from validated IDs only; no persisted path is ever trusted for effects.
use super::{Error, ProfileId, ProfileState, Provider, Registry, paths, store};
use crate::{
    platform::{self, FileIdentity, ObjectKind},
    share::mcp::{
        self as model, Catalog, InputError, RenderError, Server,
        cli::{McpAction, Target},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const CATALOG: &str = "mcp.json";
const MANIFEST: &str = "mcp-owned.json";
const JOURNAL: &str = "share-journal.json";
const JOURNAL_LIMIT: usize = 16 * 1024 * 1024;
const RENDER_DIR: &str = "larp-mcp";

impl From<InputError> for Error {
    fn from(e: InputError) -> Self {
        match e {
            InputError::Usage => Error::McpInvalid,
            InputError::SecretLiteral => Error::SecretLiteral,
            InputError::SecretReference => Error::SecretReference,
            InputError::Unsupported => Error::Unsupported,
            InputError::TooLarge => Error::TooLarge,
            InputError::UnsafePath => Error::Unsafe {
                path: PathBuf::from("mcp command"),
                reason: "must be an existing absolute local file".into(),
            },
        }
    }
}
impl From<RenderError> for Error {
    fn from(e: RenderError) -> Self {
        match e {
            RenderError::Unsupported => Error::Unsupported,
            RenderError::SecretReference => Error::SecretReference,
            RenderError::TooLarge => Error::TooLarge,
            RenderError::Collision => Error::Conflict,
        }
    }
}

/// One owned render. Identity and digest are both required to touch it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owned {
    profile_id: ProfileId,
    provider: Provider,
    directory: FileIdentity,
    identity: FileIdentity,
    sha256: String,
    len: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    version: u64,
    entries: Vec<Owned>,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            entries: vec![],
        }
    }
}
impl Manifest {
    fn find(&self, id: ProfileId) -> Option<&Owned> {
        self.entries.iter().find(|o| o.profile_id == id)
    }
    fn upsert(&mut self, owned: Owned) {
        self.entries.retain(|o| o.profile_id != owned.profile_id);
        self.entries.push(owned);
        self.entries.sort_by_key(|o| o.profile_id);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    /// Bytes are re-rendered from the journaled catalog and must match.
    Render {
        profile_id: ProfileId,
        provider: Provider,
        sha256: String,
        len: u64,
    },
    Remove {
        profile_id: ProfileId,
        provider: Provider,
    },
}
impl Step {
    fn id(&self) -> ProfileId {
        match self {
            Self::Render { profile_id, .. } | Self::Remove { profile_id, .. } => *profile_id,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    version: u64,
    kind: String,
    before_registry: String,
    after_registry: String,
    before_catalog: Catalog,
    after_catalog: Catalog,
    before_manifest: Manifest,
    manifest: Manifest,
    steps: Vec<Step>,
    next: usize,
}
#[derive(Deserialize)]
struct JournalKind {
    kind: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn digest_valid(d: &str) -> bool {
    d.len() == 64
        && d.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn render_dir(root: &Path, id: ProfileId) -> PathBuf {
    root.join("profiles")
        .join(id.to_string())
        .join("cli")
        .join(RENDER_DIR)
}
/// The profile-owned render path, derived only from the validated ID. A future
/// reviewed launch adapter revalidates this exact file; nothing passes it yet.
#[allow(
    dead_code,
    reason = "launch stays closed until a reviewed MCP adapter amendment"
)]
pub(crate) fn render_path(root: &Path, id: ProfileId, provider: Provider) -> PathBuf {
    render_dir(root, id).join(model::file_name(provider))
}
fn tmp_name(provider: Provider) -> String {
    format!("{}.tmp", model::file_name(provider))
}
fn active(p: &super::Profile) -> bool {
    p.state == Some(ProfileState::Active) && p.surfaces.is_some_and(|s| s.cli)
}
/// Raw switch; a missing ID inherits sync_all where the adapter supports it.
fn raw(catalog: &Catalog, server: &Server, p: &super::Profile) -> bool {
    server
        .switch(p.id)
        .unwrap_or(catalog.sync_all && model::supported(p.provider, server).is_ok())
}
fn effective(catalog: &Catalog, server: &Server, p: &super::Profile) -> bool {
    active(p) && raw(catalog, server, p)
}

fn read_file(path: &Path, limit: usize) -> Result<Option<(Vec<u8>, FileIdentity)>, Error> {
    if platform::inspect(path, ObjectKind::File)?.is_none() {
        return Ok(None);
    }
    // Inspection handles are metadata-only on Windows; reopen for data with checks.
    let object = platform::read_skill_object(path, ObjectKind::File, false)?;
    platform::single_link(&object.file)?;
    let mut bytes = vec![];
    (&object.file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| paths::io("read MCP metadata", path, e))?;
    if bytes.len() > limit {
        return Err(Error::TooLarge);
    }
    Ok(Some((bytes, object.identity)))
}
fn decode<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|_| Error::RecoveryRequired)
}
fn bounded<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::RecoveryRequired)?;
    if bytes.len() > limit {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}
fn write_atomic(root: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let path = root.join(name);
    let tmp = root.join(format!("{name}.tmp"));
    if let Some(object) = platform::inspect(&tmp, ObjectKind::File)? {
        platform::single_link(&object.file)?;
        drop(object);
        fs::remove_file(&tmp).map_err(|e| paths::io("remove metadata temporary", &tmp, e))?;
    }
    let mut file = platform::create_file(&tmp)?.file;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| paths::io("write metadata", &tmp, e))?;
    drop(file);
    store::replace(&tmp, &path)?;
    paths::sync(root)
}

fn validate(catalog: &Catalog, manifest: &Manifest, data: &store::Data) -> Result<(), Error> {
    catalog.validate().map_err(|_| Error::RecoveryRequired)?;
    if catalog.revision > data.revision
        || manifest.version != 1
        || manifest.entries.len() > model::MAX_PROFILES
        || manifest
            .entries
            .windows(2)
            .any(|w| w[0].profile_id >= w[1].profile_id)
    {
        return Err(Error::RecoveryRequired);
    }
    let known = |id: ProfileId| data.profiles.iter().any(|p| p.id == id);
    if catalog
        .servers
        .iter()
        .any(|s| s.switches.iter().any(|w| !known(w.profile_id)))
    {
        return Err(Error::RecoveryRequired);
    }
    for o in &manifest.entries {
        if !data
            .profiles
            .iter()
            .any(|p| p.id == o.profile_id && p.provider == o.provider)
            || !digest_valid(&o.sha256)
            || o.len > model::MAX_RENDER_BYTES as u64
        {
            return Err(Error::RecoveryRequired);
        }
    }
    Ok(())
}
fn load(root: &Path, data: &store::Data) -> Result<(Catalog, Manifest), Error> {
    let catalog = read_file(&root.join(CATALOG), model::MAX_CATALOG_BYTES)?
        .map(|(b, _)| decode(&b))
        .transpose()?
        .unwrap_or_default();
    let manifest = read_file(&root.join(MANIFEST), store::LIMIT)?
        .map(|(b, _)| decode(&b))
        .transpose()?
        .unwrap_or_default();
    validate(&catalog, &manifest, data)?;
    Ok((catalog, manifest))
}
/// Materialize every registry ID explicitly and drop IDs that left it.
fn normalized(mut catalog: Catalog, data: &store::Data) -> Catalog {
    let copy = catalog.clone();
    for (server, original) in catalog.servers.iter_mut().zip(&copy.servers) {
        server
            .switches
            .retain(|w| data.profiles.iter().any(|p| p.id == w.profile_id));
        for p in &data.profiles {
            server.set(p.id, raw(&copy, original, p));
        }
    }
    catalog
}
fn same_catalog(a: &Catalog, b: &Catalog) -> bool {
    a.sync_all == b.sync_all && a.servers == b.servers
}

/// Desired render for one profile under a catalog, or None when nothing is enabled.
fn desired(catalog: &Catalog, p: &super::Profile) -> Result<Option<Vec<u8>>, Error> {
    if !active(p) {
        return Ok(None);
    }
    let servers: Vec<_> = catalog
        .servers
        .iter()
        .filter(|s| raw(catalog, s, p))
        .collect();
    if servers.is_empty() {
        return Ok(None);
    }
    Ok(Some(model::render(p.provider, p.id, &servers)?))
}

/// Directory entries must be exactly the owned render (and, during an
/// interrupted render step only, its journal-owned temporary).
fn entries_ok(dir: &Path, provider: Provider, allow_tmp: bool) -> Result<bool, Error> {
    let file = model::file_name(provider);
    let tmp = tmp_name(provider);
    for entry in fs::read_dir(dir).map_err(|e| paths::io("read MCP render directory", dir, e))? {
        let entry = entry.map_err(|e| paths::io("read MCP render directory", dir, e))?;
        let name = entry.file_name();
        let ok = name
            .to_str()
            .is_some_and(|n| n == file || (allow_tmp && n == tmp));
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Verify an owned render; Ok(false) when the file (or directory) is missing.
fn verify_owned(root: &Path, o: &Owned, allow_tmp: bool) -> Result<bool, Error> {
    let dir = render_dir(root, o.profile_id);
    let _ancestors = platform::anchor(&dir)?;
    let Some(directory) = platform::inspect(&dir, ObjectKind::Directory)? else {
        return Ok(false);
    };
    if directory.identity != o.directory || !entries_ok(&dir, o.provider, allow_tmp)? {
        return Err(Error::Drift);
    }
    drop(directory);
    match read_file(
        &dir.join(model::file_name(o.provider)),
        model::MAX_RENDER_BYTES,
    )? {
        None => Ok(false),
        Some((bytes, identity)) => {
            if identity != o.identity || bytes.len() as u64 != o.len || digest(&bytes) != o.sha256 {
                return Err(Error::Drift);
            }
            Ok(true)
        }
    }
}
/// Every owned render must be intact or missing; nothing unowned may exist
/// where a new render would be placed. Never overwrites changed bytes.
fn preflight(root: &Path, data: &store::Data, manifest: &Manifest) -> Result<(), Error> {
    for o in &manifest.entries {
        verify_owned(root, o, false)?;
    }
    for p in &data.profiles {
        if manifest.find(p.id).is_some() {
            continue;
        }
        let dir = render_dir(root, p.id);
        let _ancestors = platform::anchor(&dir)?;
        if platform::inspect(&dir, ObjectKind::Directory)?.is_some()
            && fs::read_dir(&dir)
                .map_err(|e| paths::io("read MCP render directory", &dir, e))?
                .next()
                .is_some()
        {
            return Err(Error::Drift);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Repair {
    None,
    All,
    Profile(ProfileId),
}
impl Repair {
    fn includes(self, id: ProfileId) -> bool {
        matches!(self, Self::All) || matches!(self, Self::Profile(s) if s == id)
    }
}
fn plan(
    root: &Path,
    data: &store::Data,
    catalog: &Catalog,
    manifest: &Manifest,
    repair: Repair,
) -> Result<Vec<Step>, Error> {
    if data.profiles.len() > model::MAX_PROFILES {
        return Err(Error::TooLarge);
    }
    let mut steps = vec![];
    let mut total = 0usize;
    let mut profiles: Vec<_> = data.profiles.iter().collect();
    profiles.sort_by_key(|p| p.id);
    for p in profiles {
        let owned = manifest.find(p.id);
        match desired(catalog, p)? {
            Some(bytes) => {
                let sha = digest(&bytes);
                let current = owned.is_some_and(|o| o.sha256 == sha && o.provider == p.provider);
                let missing = current
                    && repair.includes(p.id)
                    && !verify_owned(root, owned.ok_or(Error::Drift)?, false)?;
                if !current || missing {
                    let cli = root.join("profiles").join(p.id.to_string()).join("cli");
                    let _ancestors = platform::anchor(&cli)?;
                    platform::inspect(&cli, ObjectKind::Directory)?.ok_or(Error::Drift)?;
                    total = total.checked_add(bytes.len()).ok_or(Error::TooLarge)?;
                    steps.push(Step::Render {
                        profile_id: p.id,
                        provider: p.provider,
                        sha256: sha,
                        len: bytes.len() as u64,
                    });
                }
            }
            None => {
                if let Some(o) = owned {
                    steps.push(Step::Remove {
                        profile_id: o.profile_id,
                        provider: o.provider,
                    });
                }
            }
        }
    }
    for o in &manifest.entries {
        if !data.profiles.iter().any(|p| p.id == o.profile_id) {
            steps.push(Step::Remove {
                profile_id: o.profile_id,
                provider: o.provider,
            });
        }
    }
    if total > model::MAX_TRANSACTION_BYTES {
        return Err(Error::TooLarge);
    }
    Ok(steps)
}

// Release the registry lock before lifecycle guards; reacquire and compare.
fn guarded_relock(
    root: &Path,
    revision: u64,
    expected: Option<u64>,
    steps: &[Step],
    lock: store::RegistryLock,
) -> Result<
    (
        store::RegistryLock,
        Vec<crate::launch::admission::AuthGuard>,
    ),
    Error,
> {
    let ids: BTreeSet<_> = steps.iter().map(Step::id).collect();
    if ids.is_empty() {
        return Ok((lock, vec![]));
    }
    drop(lock);
    let mut guards = Vec::new();
    for id in ids {
        guards.push(crate::launch::admission::acquire_auth_guard(root, id)?);
    }
    let lock = store::lock(root, Duration::from_secs(10))?;
    let current = store::load_unchecked(root)?;
    store::precondition(&current, expected)?;
    if current.revision != revision {
        return Err(Error::Conflict);
    }
    Ok((lock, guards))
}

fn mkdir(path: &Path) -> Result<bool, Error> {
    let created = paths::mkdir(path)?;
    if created {
        paths::sync(path.parent().ok_or(Error::RecoveryRequired)?)?;
    }
    Ok(created)
}
fn remove_leaf(path: &Path, identity: &FileIdentity, kind: ObjectKind) -> Result<(), Error> {
    #[cfg(unix)]
    {
        let _ = identity;
        let result = if kind == ObjectKind::File {
            fs::remove_file(path)
        } else {
            fs::remove_dir(path)
        };
        match result {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(paths::io("remove MCP render", path, e))
            }
            _ => Ok(()),
        }
    }
    #[cfg(windows)]
    {
        platform::remove_skill_leaf(path, identity, kind)
    }
}

fn write_render(
    root: &Path,
    manifest: &mut Manifest,
    id: ProfileId,
    provider: Provider,
    bytes: &[u8],
) -> Result<(), Error> {
    let cli = root.join("profiles").join(id.to_string()).join("cli");
    let dir = render_dir(root, id);
    let _ancestors = platform::anchor(&dir)?;
    platform::inspect(&cli, ObjectKind::Directory)?.ok_or(Error::Drift)?;
    let created = platform::inspect(&dir, ObjectKind::Directory)?.is_none() && mkdir(&dir)?;
    let directory = platform::inspect(&dir, ObjectKind::Directory)?
        .ok_or(Error::Drift)?
        .identity;
    let owned = manifest.find(id).cloned();
    if owned
        .as_ref()
        .is_some_and(|o| !created && o.directory != directory)
        || !entries_ok(&dir, provider, true)?
    {
        return Err(Error::Drift);
    }
    let path = dir.join(model::file_name(provider));
    let tmp = dir.join(tmp_name(provider));
    let sha = digest(bytes);
    if let Some((current, identity)) = read_file(&path, model::MAX_RENDER_BYTES)? {
        // A completed-but-unrecorded step is accepted only on exact bytes.
        if digest(&current) != sha
            && !owned.as_ref().is_some_and(|o| {
                o.identity == identity
                    && o.sha256 == digest(&current)
                    && o.len == current.len() as u64
            })
        {
            return Err(Error::Drift);
        }
    }
    if let Some(object) = platform::inspect(&tmp, ObjectKind::File)? {
        platform::single_link(&object.file)?;
        let identity = object.identity.clone();
        drop(object);
        remove_leaf(&tmp, &identity, ObjectKind::File)?;
    }
    let unchanged =
        read_file(&path, model::MAX_RENDER_BYTES)?.is_some_and(|(b, _)| digest(&b) == sha);
    if !unchanged {
        let mut file = platform::create_file(&tmp)?.file;
        io_phase("render_create")?;
        file.write_all(bytes)
            .map_err(|e| paths::io("write MCP render", &tmp, e))?;
        io_phase("render_write")?;
        file.sync_all()
            .map_err(|e| paths::io("sync MCP render", &tmp, e))?;
        drop(file);
        io_phase("render_fsync")?;
        store::replace(&tmp, &path)?;
        io_phase("render_replace")?;
    }
    paths::sync(&dir)?;
    let (written, identity) = read_file(&path, model::MAX_RENDER_BYTES)?.ok_or(Error::Drift)?;
    if digest(&written) != sha {
        return Err(Error::Drift);
    }
    manifest.upsert(Owned {
        profile_id: id,
        provider,
        directory,
        identity,
        sha256: sha,
        len: bytes.len() as u64,
    });
    Ok(())
}
fn remove_render(
    root: &Path,
    manifest: &mut Manifest,
    id: ProfileId,
    provider: Provider,
) -> Result<(), Error> {
    let Some(owned) = manifest.find(id).cloned() else {
        return Ok(());
    };
    if owned.provider != provider {
        return Err(Error::RecoveryRequired);
    }
    let dir = render_dir(root, id);
    let present = verify_owned(root, &owned, false)?;
    if present {
        remove_leaf(
            &dir.join(model::file_name(provider)),
            &owned.identity,
            ObjectKind::File,
        )?;
        io_phase("render_unlink")?;
    }
    if platform::inspect(&dir, ObjectKind::Directory)?.is_some() {
        paths::sync(&dir)?;
        // Non-recursive: only the now-empty owned directory is removed.
        if fs::read_dir(&dir)
            .map_err(|e| paths::io("read MCP render directory", &dir, e))?
            .next()
            .is_none()
        {
            remove_leaf(&dir, &owned.directory, ObjectKind::Directory)?;
        }
        if let Some(parent) = dir.parent()
            && platform::inspect(parent, ObjectKind::Directory)?.is_some()
        {
            paths::sync(parent)?;
        }
    }
    manifest.entries.retain(|o| o.profile_id != id);
    Ok(())
}

fn verify_progress(root: &Path, data: &store::Data, j: &Journal) -> Result<(), Error> {
    let mut expected = j.before_manifest.clone();
    for step in &j.steps[..j.next] {
        match step {
            Step::Render {
                profile_id,
                provider,
                sha256,
                len,
            } => {
                let owned = j
                    .manifest
                    .find(*profile_id)
                    .ok_or(Error::RecoveryRequired)?;
                if owned.provider != *provider || owned.sha256 != *sha256 || owned.len != *len {
                    return Err(Error::RecoveryRequired);
                }
                expected.upsert(owned.clone());
            }
            Step::Remove { profile_id, .. } => {
                expected.entries.retain(|o| o.profile_id != *profile_id);
                let dir = render_dir(root, *profile_id);
                let _ancestors = platform::anchor(&dir)?;
                if platform::inspect(&dir, ObjectKind::Directory)?.is_some() {
                    return Err(Error::Drift);
                }
            }
        }
    }
    if expected != j.manifest {
        return Err(Error::RecoveryRequired);
    }
    let pending: BTreeSet<_> = j.steps[j.next..].iter().map(Step::id).collect();
    for o in &j.manifest.entries {
        if !pending.contains(&o.profile_id) && !verify_owned(root, o, false)? {
            return Err(Error::Drift);
        }
    }
    for p in &data.profiles {
        if !pending.contains(&p.id) && j.manifest.find(p.id).is_none() {
            let dir = render_dir(root, p.id);
            let _ancestors = platform::anchor(&dir)?;
            if platform::inspect(&dir, ObjectKind::Directory)?.is_some()
                && fs::read_dir(&dir)
                    .map_err(|e| paths::io("read MCP render directory", &dir, e))?
                    .next()
                    .is_some()
            {
                return Err(Error::Drift);
            }
        }
    }
    Ok(())
}

fn apply(root: &Path, j: &mut Journal) -> Result<(), Error> {
    let after = store::parse(Path::new("registry.json"), j.after_registry.as_bytes())?;
    while j.next < j.steps.len() {
        match j.steps[j.next].clone() {
            Step::Render {
                profile_id,
                provider,
                sha256,
                len,
            } => {
                let p = after
                    .profiles
                    .iter()
                    .find(|p| p.id == profile_id && p.provider == provider)
                    .ok_or(Error::RecoveryRequired)?;
                let bytes = desired(&j.after_catalog, p)?.ok_or(Error::RecoveryRequired)?;
                if digest(&bytes) != sha256 || bytes.len() as u64 != len {
                    return Err(Error::RecoveryRequired);
                }
                write_render(root, &mut j.manifest, profile_id, provider, &bytes)?;
            }
            Step::Remove {
                profile_id,
                provider,
            } => remove_render(root, &mut j.manifest, profile_id, provider)?,
        }
        j.next += 1;
        write_atomic(root, JOURNAL, &bounded(j, JOURNAL_LIMIT)?)?;
        fault("step")?;
    }
    validate(&j.after_catalog, &j.manifest, &after)?;
    verify_progress(root, &after, j)?;
    for p in &after.profiles {
        let want = desired(&j.after_catalog, p)?.map(|b| digest(&b));
        if want.as_deref() != j.manifest.find(p.id).map(|o| o.sha256.as_str()) {
            return Err(Error::RecoveryRequired);
        }
    }
    write_atomic(root, JOURNAL, &bounded(j, JOURNAL_LIMIT)?)?;
    write_atomic(
        root,
        CATALOG,
        &bounded(&j.after_catalog, model::MAX_CATALOG_BYTES)?,
    )?;
    fault("catalog")?;
    write_atomic(root, MANIFEST, &bounded(&j.manifest, store::LIMIT)?)?;
    fault("manifest")?;
    let registry = store::serialize(&store::load_unchecked(root)?)?;
    if registry != j.after_registry.as_bytes() {
        if registry != j.before_registry.as_bytes() {
            return Err(Error::Conflict);
        }
        let mut applied = false;
        let mut tmp = false;
        store::persist(root, j.after_registry.as_bytes(), &mut applied, &mut tmp)?;
        fault("registry")?;
    }
    fs::remove_file(root.join(JOURNAL))
        .map_err(|e| paths::io("finish share transaction", root, e))?;
    paths::sync(root)
}

#[allow(clippy::too_many_arguments)]
fn begin(
    root: &Path,
    before: &store::Data,
    after: &store::Data,
    before_catalog: Catalog,
    catalog: Catalog,
    manifest: Manifest,
    steps: Vec<Step>,
) -> Result<(), Error> {
    let mut j = prepare_journal(
        root,
        before,
        after,
        before_catalog,
        catalog,
        manifest,
        steps,
    )?;
    publish(root, &j)?;
    apply(root, &mut j)
}

#[allow(clippy::too_many_arguments)]
fn prepare_journal(
    root: &Path,
    before: &store::Data,
    after: &store::Data,
    before_catalog: Catalog,
    mut catalog: Catalog,
    manifest: Manifest,
    steps: Vec<Step>,
) -> Result<Journal, Error> {
    catalog.revision = after.revision;
    // Every persisted snapshot and public view must fit before intent is published.
    view(root, false, after, &catalog, &manifest)?;
    view(root, true, after, &catalog, &manifest)?;
    bounded(&catalog, model::MAX_CATALOG_BYTES)?;
    bounded(&manifest, store::LIMIT)?;
    let j = Journal {
        version: 1,
        kind: "mcp".into(),
        before_registry: String::from_utf8(store::serialize(before)?)
            .map_err(|_| Error::RecoveryRequired)?,
        after_registry: String::from_utf8(store::serialize(after)?)
            .map_err(|_| Error::RecoveryRequired)?,
        before_catalog,
        after_catalog: catalog,
        before_manifest: manifest.clone(),
        manifest,
        steps,
        next: 0,
    };
    verify_progress(root, after, &j)?;
    bounded(&j, JOURNAL_LIMIT)?;
    Ok(j)
}

pub(super) fn publish(root: &Path, j: &Journal) -> Result<(), Error> {
    write_atomic(root, JOURNAL, &bounded(j, JOURNAL_LIMIT)?)?;
    fault("journal")
}

fn validate_journal(j: &Journal, current: &store::Data) -> Result<store::Data, Error> {
    if j.version != 1
        || j.kind != "mcp"
        || j.next > j.steps.len()
        || j.steps.len() > 2 * model::MAX_PROFILES
    {
        return Err(Error::RecoveryRequired);
    }
    let before = store::parse(Path::new("registry.json"), j.before_registry.as_bytes())?;
    let after = store::parse(Path::new("registry.json"), j.after_registry.as_bytes())?;
    let current = store::serialize(current)?;
    if (current != j.before_registry.as_bytes() && current != j.after_registry.as_bytes())
        || (j.before_registry != j.after_registry
            && after.revision != before.revision.checked_add(1).ok_or(Error::Exhausted)?)
    {
        return Err(Error::Conflict);
    }
    validate(&j.before_catalog, &j.before_manifest, &before)?;
    // Partial manifests may reference profiles from either registry side.
    let mut union = store::parse(Path::new("registry.json"), j.after_registry.as_bytes())?;
    for p in &before.profiles {
        if !union.profiles.iter().any(|v| v.id == p.id) {
            union.profiles.push(p.clone());
        }
    }
    validate(&j.after_catalog, &j.manifest, &union)?;
    let mut ids = BTreeSet::new();
    for step in &j.steps {
        let ok = match step {
            Step::Render {
                profile_id,
                provider,
                sha256,
                len,
            } => {
                digest_valid(sha256)
                    && *len <= model::MAX_RENDER_BYTES as u64
                    && after
                        .profiles
                        .iter()
                        .any(|p| p.id == *profile_id && p.provider == *provider)
            }
            Step::Remove {
                profile_id,
                provider,
            } => j
                .before_manifest
                .find(*profile_id)
                .is_some_and(|o| o.provider == *provider),
        };
        if !ok || !ids.insert(step.id()) {
            return Err(Error::RecoveryRequired);
        }
    }
    Ok(after)
}

pub(super) fn validate_continuation(
    root: &Path,
    j: &Journal,
    before: &store::Data,
    after: &store::Data,
) -> Result<(), Error> {
    if j.next != 0
        || j.before_registry.as_bytes() != store::serialize(before)?
        || j.after_registry.as_bytes() != store::serialize(after)?
    {
        return Err(Error::RecoveryRequired);
    }
    validate_journal(j, before)?;
    preflight(root, before, &j.before_manifest)?;
    verify_progress(root, after, j)
}

pub(super) fn resume(
    root: &Path,
    revision: u64,
    expected: Option<u64>,
    lock: store::RegistryLock,
) -> Result<store::RegistryLock, Error> {
    let bytes = read_file(&root.join(JOURNAL), JOURNAL_LIMIT)?
        .ok_or(Error::RecoveryRequired)?
        .0;
    if decode::<JournalKind>(&bytes)?.kind != "mcp" {
        return Err(Error::RecoveryRequired);
    }
    let mut j: Journal = decode(&bytes)?;
    let current = store::load_unchecked(root)?;
    let after = validate_journal(&j, &current)?;
    let (lock, _guards) = guarded_relock(root, revision, expected, &j.steps, lock)?;
    if read_file(&root.join(JOURNAL), JOURNAL_LIMIT)?.map(|(b, _)| b) != Some(bytes) {
        return Err(Error::Conflict);
    }
    verify_progress(root, &after, &j)?;
    for (name, old, new, limit) in [
        (
            CATALOG,
            bounded(&j.before_catalog, model::MAX_CATALOG_BYTES)?,
            bounded(&j.after_catalog, model::MAX_CATALOG_BYTES)?,
            model::MAX_CATALOG_BYTES,
        ),
        (
            MANIFEST,
            bounded(&j.before_manifest, store::LIMIT)?,
            bounded(&j.manifest, store::LIMIT)?,
            store::LIMIT,
        ),
    ] {
        if let Some((bytes, _)) = read_file(&root.join(name), limit)? {
            let value: Value = decode(&bytes)?;
            if value != decode::<Value>(&old)? && value != decode::<Value>(&new)? {
                return Err(Error::Drift);
            }
        }
    }
    apply(root, &mut j)?;
    Ok(lock)
}

#[cfg(test)]
thread_local! {static FAULT: std::cell::RefCell<Option<&'static str>> = const {std::cell::RefCell::new(None)};}
#[cfg(test)]
pub(crate) fn set_fault(phase: Option<&'static str>) {
    FAULT.with(|f| *f.borrow_mut() = phase);
}
fn fault(_phase: &str) -> Result<(), Error> {
    #[cfg(test)]
    if FAULT.with(|f| f.borrow().is_some_and(|v| v == _phase)) {
        return Err(Error::RecoveryRequired);
    }
    Ok(())
}
fn io_phase(phase: &str) -> Result<(), Error> {
    fault(phase).map_err(|_| Error::Io {
        action: "write MCP render",
        path: PathBuf::from("mcp"),
        source: std::io::Error::other("injected MCP render fault"),
    })
}

fn receipt(changed: bool, id: Option<ProfileId>, name: Option<&str>, warnings: &[Value]) -> Value {
    json!({"changed":changed,"profile_id":id.map(|id| id.to_string()),"name":name,"warnings":warnings})
}
fn unsupported_warning() -> Value {
    json!({"code":"unsupported","message":"Some profiles cannot render this server and stay disabled."})
}

/// Validate add input without registry IO; refusals carry no value.
fn add_input(
    name: &str,
    command: Option<&std::ffi::OsStr>,
    args: &[String],
    env_refs: &[String],
    url: Option<&str>,
    header_refs: &[String],
) -> Result<(super::ProfileName, model::Transport), Error> {
    let name = model::server_name(name)?;
    let command = command
        .map(|c| c.to_str().ok_or(Error::McpInvalid))
        .transpose()?;
    let transport = model::transport(command, args, env_refs, url, header_refs)?;
    Ok((name, transport))
}

impl Registry {
    pub fn mcp(&self, action: McpAction) -> Result<Value, Error> {
        let _ancestors = platform::anchor(self.root())?;
        // Pure input validation precedes any registry access.
        let input = match &action {
            McpAction::Add {
                name,
                command,
                args,
                env_refs,
                url,
                header_refs,
            } => Some(add_input(
                name,
                command.as_deref(),
                args,
                env_refs,
                url.as_deref(),
                header_refs,
            )?),
            _ => None,
        };
        let read_only = matches!(action, McpAction::List | McpAction::Matrix);
        if read_only
            && platform::inspect(&self.root().join("registry.lock"), ObjectKind::File)?.is_none()
        {
            let data = store::load(self.root())?;
            store::precondition(&data, self.expected)?;
            let (catalog, manifest) = load(self.root(), &data)?;
            self.observed.set(Some(data.revision));
            return view(
                self.root(),
                matches!(action, McpAction::Matrix),
                &data,
                &catalog,
                &manifest,
            );
        }
        let lock = if read_only {
            store::admission_lock(self.root())?
        } else {
            store::lock(self.root(), Duration::from_secs(10))?
        };
        let mut data = store::load_unchecked(self.root())?;
        store::precondition(&data, self.expected)?;
        if crate::share::pending(self.root()) {
            if !matches!(action, McpAction::Sync) {
                return Err(Error::RecoveryRequired);
            }
            let _lock = resume(self.root(), data.revision, self.expected, lock)?;
            data = store::load_unchecked(self.root())?;
            self.observed.set(Some(data.revision));
            return Ok(receipt(true, None, None, &[]));
        }
        store::require_v3(&data)?;
        if let Some((_, model::Transport::Stdio { command, .. })) = &input {
            model::command_exists(command)?;
        }
        if data
            .profiles
            .iter()
            .any(|p| p.state != Some(ProfileState::Active))
        {
            return Err(Error::RecoveryRequired);
        }
        let (before_catalog, manifest) = load(self.root(), &data)?;
        if read_only {
            self.observed.set(Some(data.revision));
            return view(
                self.root(),
                matches!(action, McpAction::Matrix),
                &data,
                &before_catalog,
                &manifest,
            );
        }
        if data.profiles.len() > model::MAX_PROFILES {
            return Err(Error::TooLarge);
        }
        preflight(self.root(), &data, &manifest)?;
        let mut catalog = normalized(before_catalog.clone(), &data);
        let mut target_id = None;
        let mut target_name = None;
        let mut warnings = vec![];
        let mut repair = Repair::None;
        let mut profiles: Vec<_> = data.profiles.iter().collect();
        profiles.sort_by_key(|p| p.id);
        match action {
            McpAction::Add { .. } => {
                let (name, transport) = input.ok_or(Error::RecoveryRequired)?;
                if catalog.find(&name).is_some() {
                    return Err(Error::Conflict);
                }
                if catalog.servers.len() >= model::MAX_SERVERS {
                    return Err(Error::TooLarge);
                }
                let mut server = Server {
                    name: name.clone(),
                    transport,
                    switches: vec![],
                };
                for p in &profiles {
                    let supported = model::supported(p.provider, &server).is_ok();
                    if catalog.sync_all && !supported && warnings.is_empty() {
                        warnings.push(unsupported_warning());
                    }
                    server.set(p.id, catalog.sync_all && supported);
                }
                catalog.servers.push(server);
                catalog.servers.sort_by(|a, b| a.name.cmp(&b.name));
                target_name = Some(name);
            }
            McpAction::Remove { name } => {
                if catalog.find(&name).is_none() {
                    return Err(Error::McpNotFound);
                }
                catalog.servers.retain(|s| s.name != name);
                target_name = Some(name);
            }
            McpAction::Switch {
                name,
                target,
                enabled,
            } => {
                let server = catalog
                    .servers
                    .iter_mut()
                    .find(|s| s.name == name)
                    .ok_or(Error::McpNotFound)?;
                match target {
                    Target::Profile(profile) => {
                        let p = data
                            .profiles
                            .iter()
                            .find(|p| p.name == profile)
                            .ok_or(Error::NotFound { name: profile })?;
                        if enabled {
                            // Unsupported adapters refuse; never silently mapped.
                            model::supported(p.provider, server)?;
                            repair = Repair::Profile(p.id);
                        }
                        server.set(p.id, enabled);
                        target_id = Some(p.id);
                    }
                    Target::All => {
                        for p in &profiles {
                            let supported = model::supported(p.provider, server).is_ok();
                            if enabled && !supported && warnings.is_empty() {
                                warnings.push(unsupported_warning());
                            }
                            server.set(p.id, enabled && supported);
                        }
                    }
                }
                target_name = Some(name);
            }
            McpAction::SyncAll(on) => {
                if on {
                    for server in &mut catalog.servers {
                        for p in &profiles {
                            let supported = model::supported(p.provider, server).is_ok();
                            if !supported && warnings.is_empty() {
                                warnings.push(unsupported_warning());
                            }
                            server.set(p.id, supported);
                        }
                    }
                }
                // Off keeps every (already materialized) switch; only inheritance stops.
                catalog.sync_all = on;
            }
            McpAction::Sync => repair = Repair::All,
            McpAction::List | McpAction::Matrix => unreachable!(),
        }
        let steps = plan(self.root(), &data, &catalog, &manifest, repair)?;
        let changed = !same_catalog(&catalog, &before_catalog) || !steps.is_empty();
        let (_lock, _guards) =
            guarded_relock(self.root(), data.revision, self.expected, &steps, lock)?;
        store::load(self.root())?;
        preflight(self.root(), &data, &manifest)?;
        if changed {
            let mut after = store::parse(Path::new("registry.json"), &store::serialize(&data)?)?;
            after.revision = after.revision.checked_add(1).ok_or(Error::Exhausted)?;
            begin(
                self.root(),
                &data,
                &after,
                before_catalog,
                catalog,
                manifest,
                steps,
            )?;
            data = after;
        }
        self.observed.set(Some(data.revision));
        Ok(receipt(
            changed,
            target_id,
            target_name.as_ref().map(|n| n.as_str()),
            &warnings,
        ))
    }
}

/// List (`matrix == false`) or matrix view: reference names only.
fn view(
    root: &Path,
    matrix: bool,
    data: &store::Data,
    catalog: &Catalog,
    manifest: &Manifest,
) -> Result<Value, Error> {
    let mut profiles: Vec<_> = data.profiles.iter().collect();
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    let value = if matrix {
        json!({
            "profiles": profiles.iter().map(|p| json!({"id":p.id.to_string(),"name":p.name,"provider":p.provider})).collect::<Vec<_>>(),
            "rows": catalog.servers.iter().map(|s| json!({"name":s.name,"enabled":profiles.iter().map(|p| effective(catalog,s,p)).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        })
    } else {
        // Read-only render status per profile: never repairs anything.
        let mut status = std::collections::BTreeMap::new();
        for p in &data.profiles {
            let state = match desired(catalog, p) {
                Err(_) => "invalid",
                Ok(want) => {
                    let owned = manifest.find(p.id);
                    match (want, owned) {
                        (None, None) => "ok",
                        (Some(bytes), Some(o))
                            if o.sha256 == digest(&bytes)
                                && verify_owned(root, o, false).unwrap_or(false) =>
                        {
                            "ok"
                        }
                        _ => "drift",
                    }
                }
            };
            status.insert(p.id, state);
        }
        let mut by_id: Vec<_> = data.profiles.iter().collect();
        by_id.sort_by_key(|p| p.id);
        json!({
            "sync_all": catalog.sync_all,
            "servers": catalog.servers.iter().map(|s| {
                let enabled: Vec<_> = by_id.iter().filter(|p| effective(catalog, s, p)).collect();
                let state = if enabled.iter().any(|p| model::supported(p.provider, s).is_err() || status[&p.id] == "invalid") {
                    "invalid"
                } else if enabled.iter().any(|p| status[&p.id] != "ok") {
                    "drift"
                } else {
                    "ok"
                };
                json!({
                    "name": s.name,
                    "transport": s.transport.class(),
                    "secret_refs": s.transport.references().iter().map(|r| json!({"kind":r.kind(),"name":r.name()})).collect::<Vec<_>>(),
                    "switches": by_id.iter().map(|p| json!({"profile_id":p.id.to_string(),"enabled":effective(catalog,s,p)})).collect::<Vec<_>>(),
                    "state": state,
                })
            }).collect::<Vec<_>>(),
        })
    };
    // Leave framing space below the 1 MiB CLI envelope bound.
    bounded(&value, store::LIMIT - 1024)?;
    Ok(value)
}

// ---------------------------------------------------------------------------
// Profile lifecycle hooks (called by store.rs under the registry write lock).

pub(super) fn has_profile_references(
    root: &Path,
    data: &store::Data,
    id: ProfileId,
) -> Result<bool, Error> {
    let (catalog, manifest) = load(root, data)?;
    Ok(manifest.find(id).is_some()
        || data
            .profiles
            .iter()
            .any(|p| p.id == id && catalog.servers.iter().any(|s| raw(&catalog, s, p))))
}

/// Pure preparation for a profile add/rename/forget/surface commit `after`.
/// Call before any other hook commits so that drift refuses without effects.
pub(super) fn profile_prepare(root: &Path, after: &store::Data) -> Result<Option<Journal>, Error> {
    let before = store::load_unchecked(root)?;
    let (old, manifest) = load(root, &before)?;
    if old.servers.is_empty() && manifest.entries.is_empty() {
        return Ok(None);
    }
    preflight(root, &before, &manifest)?;
    let catalog = normalized(old.clone(), after);
    let steps = plan(root, after, &catalog, &manifest, Repair::None)?;
    if steps.is_empty() && same_catalog(&catalog, &old) {
        return Ok(None);
    }
    prepare_journal(root, &before, after, old, catalog, manifest, steps).map(Some)
}

pub(super) fn profile_apply(
    root: &Path,
    after: &store::Data,
    journal: Option<Journal>,
    lock: &mut Option<store::RegistryLock>,
    expected: Option<u64>,
) -> Result<bool, Error> {
    if let Some(mut j) = journal {
        let before = store::load(root)?;
        let (registry_lock, _guards) = guarded_relock(
            root,
            before.revision,
            expected,
            &j.steps,
            lock.take().ok_or(Error::RecoveryRequired)?,
        )?;
        *lock = Some(registry_lock);
        store::load(root)?;
        validate_continuation(root, &j, &before, after)?;
        publish(root, &j)?;
        apply(root, &mut j)?;
        return Ok(true);
    }
    // The skills hook has already committed the registry and published MCP intent.
    if crate::share::pending(root) {
        fault("handoff")?;
        *lock = Some(resume(
            root,
            after.revision,
            None,
            lock.take().ok_or(Error::RecoveryRequired)?,
        )?);
        return Ok(true);
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Typed cleanup entry for profile deletion (stage 04 delete owner).

/// Persistable MCP cleanup for one profile ID: remove its switches and its
/// owned render. `prepare` has no effects; `apply` is idempotent so that the
/// caller's own journal can replay it after a crash, under the same lock.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[allow(dead_code, reason = "typed seam for the stage 04 delete owner")]
pub(super) struct ProfileCleanup {
    profile_id: ProfileId,
    owned: Option<Owned>,
    before_catalog: Catalog,
    catalog: Catalog,
    before_manifest: Manifest,
    manifest: Manifest,
}
#[allow(dead_code, reason = "typed seam for the stage 04 delete owner")]
pub(super) fn prepare_profile_cleanup(
    root: &Path,
    data: &store::Data,
    id: ProfileId,
) -> Result<ProfileCleanup, Error> {
    let (before_catalog, before_manifest) = load(root, data)?;
    let owned = before_manifest.find(id).cloned();
    if let Some(o) = &owned {
        verify_owned(root, o, false)?;
    }
    let mut catalog = before_catalog.clone();
    for s in &mut catalog.servers {
        s.switches.retain(|w| w.profile_id != id);
    }
    let mut manifest = before_manifest.clone();
    manifest.entries.retain(|o| o.profile_id != id);
    Ok(ProfileCleanup {
        profile_id: id,
        owned,
        before_catalog,
        catalog,
        before_manifest,
        manifest,
    })
}
#[allow(dead_code, reason = "typed seam for the stage 04 delete owner")]
pub(super) fn apply_profile_cleanup(root: &Path, cleanup: &ProfileCleanup) -> Result<(), Error> {
    for (name, before, after, limit) in [
        (
            CATALOG,
            bounded(&cleanup.before_catalog, model::MAX_CATALOG_BYTES)?,
            bounded(&cleanup.catalog, model::MAX_CATALOG_BYTES)?,
            model::MAX_CATALOG_BYTES,
        ),
        (
            MANIFEST,
            bounded(&cleanup.before_manifest, store::LIMIT)?,
            bounded(&cleanup.manifest, store::LIMIT)?,
            store::LIMIT,
        ),
    ] {
        if let Some((bytes, _)) = read_file(&root.join(name), limit)?
            && bytes != before
            && bytes != after
        {
            return Err(Error::Drift);
        }
    }
    if let Some(owned) = &cleanup.owned {
        let mut manifest = Manifest {
            version: 1,
            entries: vec![owned.clone()],
        };
        remove_render(root, &mut manifest, owned.profile_id, owned.provider)?;
    }
    if cleanup.before_manifest == cleanup.manifest && cleanup.before_catalog == cleanup.catalog {
        return Ok(());
    }
    write_atomic(root, MANIFEST, &bounded(&cleanup.manifest, store::LIMIT)?)?;
    write_atomic(
        root,
        CATALOG,
        &bounded(&cleanup.catalog, model::MAX_CATALOG_BYTES)?,
    )
}

#[cfg(test)]
mod tests {
    // Stage 07 transaction tests: synthetic private roots, names claude-work and
    // codex-45, no MCP child, no network, no account, no environment values.
    use super::*;
    use crate::registry::{ProfileName, Surface};

    #[cfg(unix)]
    type Temp = tempfile::TempDir;
    #[cfg(windows)]
    type Temp = crate::platform::fixture_private::PrivateTempDir;

    fn name(s: &str) -> ProfileName {
        ProfileName::parse_new(s).unwrap()
    }
    struct Fixture {
        registry: Registry,
        command: String,
        _temp: Temp,
    }
    fn fixture() -> Fixture {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        #[cfg(unix)]
        let temp = {
            use std::os::unix::fs::PermissionsExt;
            tempfile::Builder::new()
                .prefix("stage07-")
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(&parent)
                .unwrap()
        };
        #[cfg(windows)]
        let temp = crate::platform::fixture_private::tempdir(&parent).unwrap();
        let bin = temp.path().join("bin");
        platform::create_directory(&bin).unwrap();
        let exe = bin.join(if cfg!(windows) {
            "files-mcp.exe"
        } else {
            "files-mcp"
        });
        platform::create_file(&exe)
            .unwrap()
            .file
            .write_all(b"synthetic server; never executed\n")
            .unwrap();
        Fixture {
            registry: Registry::at(temp.path().join("root")).unwrap(),
            command: exe.to_str().unwrap().to_owned(),
            _temp: temp,
        }
    }
    impl Fixture {
        fn add_stdio(&self, server: &str) -> Result<Value, Error> {
            self.registry.mcp(McpAction::Add {
                name: server.into(),
                command: Some(self.command.clone().into()),
                args: vec!["--stdio".into()],
                env_refs: vec!["FILES_TOKEN".into()],
                url: None,
                header_refs: vec![],
            })
        }
        fn add_url(&self, server: &str, url: &str) -> Result<Value, Error> {
            self.registry.mcp(McpAction::Add {
                name: server.into(),
                command: None,
                args: vec![],
                env_refs: vec![],
                url: Some(url.into()),
                header_refs: vec!["Authorization=DOCS_TOKEN".into()],
            })
        }
        fn switch(
            &self,
            server: &str,
            profile: Option<&str>,
            enabled: bool,
        ) -> Result<Value, Error> {
            self.registry.mcp(McpAction::Switch {
                name: name(server),
                target: profile.map_or(Target::All, |p| Target::Profile(name(p))),
                enabled,
            })
        }
        fn matrix(&self) -> Value {
            self.registry.mcp(McpAction::Matrix).unwrap()
        }
        fn list(&self) -> Value {
            self.registry.mcp(McpAction::List).unwrap()
        }
        fn render(&self, id: u64, provider: Provider) -> PathBuf {
            render_path(self.registry.root(), ProfileId::new(id), provider)
        }
        fn root(&self) -> &Path {
            self.registry.root()
        }
        fn catalog(&self) -> Value {
            serde_json::from_slice(&fs::read(self.root().join(CATALOG)).unwrap()).unwrap()
        }
    }

    #[test]
    fn stage07_matrix_defaults_overrides_and_receipts() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.registry.add(name("codex-45"), Provider::Codex).unwrap();
        let added = f.add_stdio("files").unwrap();
        assert_eq!(
            added,
            json!({"changed":true,"profile_id":null,"name":"files","warnings":[]})
        );
        // Default sync_all=false: new definitions are disabled everywhere.
        assert_eq!(f.matrix()["rows"][0]["enabled"], json!([false, false]));
        assert!(!f.render(1, Provider::Claude).exists());
        let revision = f.registry.revision();
        let enabled = f.switch("files", Some("codex-45"), true).unwrap();
        assert_eq!(
            enabled,
            json!({"changed":true,"profile_id":"2","name":"files","warnings":[]})
        );
        assert_eq!(f.registry.revision(), revision.map(|r| r + 1));
        assert!(f.render(2, Provider::Codex).is_file());
        // Columns follow profile name order: claude-work, codex-45.
        assert_eq!(f.matrix()["rows"][0]["enabled"], json!([false, true]));
        // A repeated toggle is a no-op that keeps the revision.
        let revision = f.registry.revision();
        assert_eq!(
            f.switch("files", Some("codex-45"), true).unwrap()["changed"],
            false
        );
        assert_eq!(f.registry.revision(), revision);
        let all = f.switch("files", None, true).unwrap();
        assert_eq!(
            all,
            json!({"changed":true,"profile_id":null,"name":"files","warnings":[]})
        );
        assert!(f.render(1, Provider::Claude).is_file());
        let list = f.list();
        assert_eq!(list["sync_all"], false);
        assert_eq!(
            list["servers"][0],
            json!({"name":"files","transport":"stdio","secret_refs":[{"kind":"env","name":"FILES_TOKEN"}],
                   "switches":[{"profile_id":"1","enabled":true},{"profile_id":"2","enabled":true}],"state":"ok"})
        );
        f.switch("files", None, false).unwrap();
        assert!(!f.render(1, Provider::Claude).exists());
        assert!(!f.render(2, Provider::Codex).exists());
        assert!(!render_dir(f.root(), ProfileId::new(1)).exists());
        // Expected revision is checked under the lock before name resolution.
        let stale = Registry::at(f.root().into()).unwrap().expecting(Some(1));
        assert!(matches!(
            stale.mcp(McpAction::Remove {
                name: name("missing")
            }),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            f.registry.mcp(McpAction::Remove {
                name: name("missing")
            }),
            Err(Error::McpNotFound)
        ));
        assert!(matches!(
            f.switch("files", Some("absent-profile"), true),
            Err(Error::NotFound { .. })
        ));
        assert!(matches!(f.add_stdio("files"), Err(Error::Conflict)));
        f.switch("files", Some("claude-work"), true).unwrap();
        assert_eq!(
            f.registry
                .mcp(McpAction::Remove {
                    name: name("files")
                })
                .unwrap(),
            json!({"changed":true,"profile_id":null,"name":"files","warnings":[]})
        );
        assert!(!f.render(1, Provider::Claude).exists());
        assert_eq!(f.list()["servers"], json!([]));
    }

    #[test]
    fn stage07_matrix_sync_all_new_profiles_rename_forget_surface() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.add_stdio("files").unwrap();
        assert_eq!(
            f.registry.mcp(McpAction::SyncAll(true)).unwrap(),
            json!({"changed":true,"profile_id":null,"name":null,"warnings":[]})
        );
        assert!(f.render(1, Provider::Claude).is_file());
        // Future profiles and future definitions inherit while on.
        f.registry.add(name("codex-45"), Provider::Codex).unwrap();
        assert!(f.render(2, Provider::Codex).is_file());
        f.add_stdio("tools").unwrap();
        assert_eq!(f.matrix()["rows"][1]["enabled"], json!([true, true]));
        // Per-profile override while on.
        f.switch("tools", Some("codex-45"), false).unwrap();
        assert_eq!(f.matrix()["rows"][1]["enabled"], json!([true, false]));
        // Off preserves current switches but stops inheritance.
        f.registry.mcp(McpAction::SyncAll(false)).unwrap();
        assert_eq!(f.matrix()["rows"][0]["enabled"], json!([true, true]));
        f.registry
            .add(name("claude-two"), Provider::Claude)
            .unwrap();
        assert_eq!(f.matrix()["rows"][0]["enabled"], json!([false, true, true]));
        assert!(!render_dir(f.root(), ProfileId::new(3)).exists());
        // Rename keeps the ID switch and render.
        f.registry
            .rename(&name("codex-45"), name("codex-46"))
            .unwrap();
        assert!(f.render(2, Provider::Codex).is_file());
        assert_eq!(f.list()["servers"][0]["switches"][1]["enabled"], true);
        // Forget refuses a profile with MCP switches; disabled profiles forget cleanly.
        assert!(matches!(
            f.registry.forget(&name("codex-46")),
            Err(Error::Conflict)
        ));
        f.registry.forget(&name("claude-two")).unwrap();
        assert!(
            f.catalog()["servers"][0]["switches"]
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w["profile_id"] != 3)
        );
        // CLI surface off removes the render; on restores it. Desktop is never rendered.
        f.registry
            .surface(name("claude-work"), Surface::Cli, false)
            .unwrap();
        assert!(!f.render(1, Provider::Claude).exists());
        f.registry
            .surface(name("claude-work"), Surface::Desktop, true)
            .unwrap();
        assert!(!render_dir(f.root(), ProfileId::new(1)).exists());
        f.registry
            .surface(name("claude-work"), Surface::Cli, true)
            .unwrap();
        assert!(f.render(1, Provider::Claude).is_file());
        assert_eq!(f.list()["servers"][0]["state"], "ok");
    }

    #[test]
    fn stage07_matrix_unsupported_adapter_refuses_or_warns() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.registry.add(name("codex-45"), Provider::Codex).unwrap();
        f.add_url("events", "https://docs.example.com/sse").unwrap();
        assert!(matches!(
            f.switch("events", Some("codex-45"), true),
            Err(Error::Unsupported)
        ));
        let all = f.switch("events", None, true).unwrap();
        assert_eq!(all["warnings"][0]["code"], "unsupported");
        assert_eq!(f.matrix()["rows"][0]["enabled"], json!([true, false]));
        assert_eq!(f.list()["servers"][0]["transport"], "http");
        assert_eq!(f.list()["servers"][0]["state"], "ok");
        assert!(!f.render(2, Provider::Codex).exists());
    }

    #[test]
    fn stage07_render_golden_claude_and_codex_reference_only() {
        let server = |n: &str, transport| Server {
            name: name(n),
            transport,
            switches: vec![],
        };
        let files = server(
            "files",
            model::Transport::Stdio {
                command: "/usr/local/bin/files-mcp".into(),
                args: vec!["--stdio".into()],
                env: vec![model::SecretRef::Env {
                    name: "FILES_TOKEN".into(),
                }],
            },
        );
        let docs = server(
            "docs",
            model::Transport::Http {
                url: "https://docs.example.com/mcp".into(),
                headers: vec![model::HeaderRef {
                    header: "Authorization".into(),
                    value: model::SecretRef::Env {
                        name: "DOCS_TOKEN".into(),
                    },
                }],
            },
        );
        let id = ProfileId::new(2);
        let claude = model::render(Provider::Claude, id, &[&files, &docs]).unwrap();
        assert_eq!(
            String::from_utf8(claude.clone()).unwrap(),
            r#"{
  "mcpServers": {
    "docs__p2": {
      "headers": {
        "Authorization": "${DOCS_TOKEN}"
      },
      "type": "http",
      "url": "https://docs.example.com/mcp"
    },
    "files__p2": {
      "args": [
        "--stdio"
      ],
      "command": "/usr/local/bin/files-mcp",
      "env": {
        "FILES_TOKEN": "${FILES_TOKEN}"
      },
      "type": "stdio"
    }
  }
}
"#
        );
        let codex = model::render(Provider::Codex, id, &[&docs, &files]).unwrap();
        assert_eq!(
            String::from_utf8(codex).unwrap(),
            r#"{
  "format": "codex-config-overrides",
  "overrides": [
    {
      "key": "mcp_servers.docs__p2.url",
      "value": "https://docs.example.com/mcp"
    },
    {
      "key": "mcp_servers.docs__p2.env_http_headers",
      "value": {
        "Authorization": "DOCS_TOKEN"
      }
    },
    {
      "key": "mcp_servers.files__p2.command",
      "value": "/usr/local/bin/files-mcp"
    },
    {
      "key": "mcp_servers.files__p2.args",
      "value": [
        "--stdio"
      ]
    },
    {
      "key": "mcp_servers.files__p2.env_vars",
      "value": [
        "FILES_TOKEN"
      ]
    }
  ],
  "version": 1
}
"#
        );
        // Deterministic regardless of input order.
        assert_eq!(
            model::render(Provider::Claude, id, &[&docs, &files]).unwrap(),
            claude
        );
        // Keychain references have no native syntax: refuse, never materialize.
        let keyed = server(
            "keyed",
            model::Transport::Stdio {
                command: "/usr/local/bin/files-mcp".into(),
                args: vec![],
                env: vec![model::SecretRef::Keychain {
                    name: "item".into(),
                }],
            },
        );
        for provider in Provider::ALL {
            assert_eq!(
                model::render(provider, id, &[&keyed]),
                Err(RenderError::SecretReference)
            );
        }
        let sse = server(
            "events",
            model::Transport::Http {
                url: "https://docs.example.com/sse".into(),
                headers: vec![],
            },
        );
        assert_eq!(
            model::render(Provider::Codex, id, &[&sse]),
            Err(RenderError::Unsupported)
        );
        assert!(
            String::from_utf8(model::render(Provider::Claude, id, &[&sse]).unwrap())
                .unwrap()
                .contains("\"type\": \"sse\"")
        );
    }

    #[test]
    fn stage07_render_files_match_pure_render_and_limits() {
        let f = fixture();
        f.registry.add(name("codex-45"), Provider::Codex).unwrap();
        f.add_stdio("files").unwrap();
        f.switch("files", Some("codex-45"), true).unwrap();
        let bytes = fs::read(f.render(1, Provider::Codex)).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("mcp_servers.files__p1.env_vars"));
        assert!(text.contains(&f.command.replace('\\', "\\\\")));
        // Bounded counts: the 257th definition refuses without a journal.
        for i in 1..model::MAX_SERVERS {
            f.registry
                .mcp(McpAction::Add {
                    name: format!("s{i}"),
                    command: Some(f.command.clone().into()),
                    args: vec![],
                    env_refs: vec![],
                    url: None,
                    header_refs: vec![],
                })
                .unwrap();
        }
        assert!(matches!(f.add_stdio("overflow"), Err(Error::TooLarge)));
        assert!(!crate::share::pending(f.root()));
    }

    #[test]
    fn stage07_drift_tampered_or_foreign_renders_refuse_and_keep_bytes() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.add_stdio("files").unwrap();
        f.switch("files", Some("claude-work"), true).unwrap();
        let path = f.render(1, Provider::Claude);
        let edited = b"{\"user\":\"edit\"}\n";
        #[cfg(unix)]
        fs::write(&path, edited).unwrap();
        #[cfg(windows)]
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            file.write_all(edited).unwrap();
        }
        assert_eq!(f.list()["servers"][0]["state"], "drift");
        for result in [
            f.switch("files", Some("claude-work"), false),
            f.registry.mcp(McpAction::Sync),
            f.add_stdio("tools"),
        ] {
            assert!(matches!(result, Err(Error::Drift)), "{result:?}");
        }
        // Profile hooks refuse too, before any registry commit.
        let revision = store::load(f.root()).unwrap().revision;
        assert!(matches!(
            f.registry.surface(name("claude-work"), Surface::Cli, false),
            Err(Error::Drift)
        ));
        assert_eq!(store::load(f.root()).unwrap().revision, revision);
        assert_eq!(fs::read(&path).unwrap(), edited);
        assert!(!crate::share::pending(f.root()));
    }

    #[test]
    fn stage07_drift_missing_render_repairs_only_on_sync() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.add_stdio("files").unwrap();
        f.switch("files", Some("claude-work"), true).unwrap();
        let path = f.render(1, Provider::Claude);
        let expected = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        // No implicit repair on list.
        assert_eq!(f.list()["servers"][0]["state"], "drift");
        assert!(!path.exists());
        assert_eq!(f.registry.mcp(McpAction::Sync).unwrap()["changed"], true);
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(f.list()["servers"][0]["state"], "ok");
        assert_eq!(f.registry.mcp(McpAction::Sync).unwrap()["changed"], false);
    }

    #[test]
    fn stage07_drift_foreign_entries_tmp_and_forged_metadata() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.registry.add(name("codex-45"), Provider::Codex).unwrap();
        f.add_stdio("files").unwrap();
        f.switch("files", Some("claude-work"), true).unwrap();
        let dir = render_dir(f.root(), ProfileId::new(1));
        // A leftover temporary outside a journal is not ours to delete.
        let tmp = dir.join(tmp_name(Provider::Claude));
        platform::create_file(&tmp).unwrap();
        assert!(matches!(f.registry.mcp(McpAction::Sync), Err(Error::Drift)));
        assert!(tmp.exists());
        fs::remove_file(&tmp).unwrap();
        // A foreign file where a new render would go refuses instead of overwriting.
        let other = render_dir(f.root(), ProfileId::new(2));
        platform::create_directory(&other).unwrap();
        platform::create_file(&other.join(model::file_name(Provider::Codex)))
            .unwrap()
            .file
            .write_all(b"foreign")
            .unwrap();
        assert!(matches!(
            f.switch("files", Some("codex-45"), true),
            Err(Error::Drift)
        ));
        assert_eq!(
            fs::read(other.join(model::file_name(Provider::Codex))).unwrap(),
            b"foreign"
        );
        fs::remove_file(other.join(model::file_name(Provider::Codex))).unwrap();
        fs::remove_dir(&other).unwrap();
        // Forged catalog fields, unknown IDs and traversal-shaped names refuse.
        let catalog = fs::read(f.root().join(CATALOG)).unwrap();
        for forged in [
            String::from_utf8(catalog.clone())
                .unwrap()
                .replace("\"sync_all\"", "\"extra\":1,\"sync_all\""),
            String::from_utf8(catalog.clone())
                .unwrap()
                .replace("\"profile_id\":2", "\"profile_id\":99"),
            String::from_utf8(catalog.clone())
                .unwrap()
                .replace("\"name\":\"files\"", "\"name\":\"../x\""),
            String::from_utf8(catalog.clone())
                .unwrap()
                .replace("--stdio", "--token=abc"),
        ] {
            assert_ne!(forged.as_bytes(), catalog.as_slice());
            write_atomic(f.root(), CATALOG, forged.as_bytes()).unwrap();
            assert!(
                matches!(
                    f.registry.mcp(McpAction::List),
                    Err(Error::RecoveryRequired)
                ),
                "{forged}"
            );
        }
        write_atomic(f.root(), CATALOG, &catalog).unwrap();
        assert_eq!(f.list()["servers"][0]["state"], "ok");
    }

    #[cfg(unix)]
    #[test]
    fn stage07_drift_symlink_and_directory_swap_keep_outside_sentinel() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.add_stdio("files").unwrap();
        f.switch("files", Some("claude-work"), true).unwrap();
        let outside = f.root().parent().unwrap().join("outside");
        platform::create_file(&outside)
            .unwrap()
            .file
            .write_all(b"sentinel")
            .unwrap();
        let path = f.render(1, Provider::Claude);
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(f.registry.mcp(McpAction::Sync).is_err());
        assert!(f.switch("files", Some("claude-work"), false).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
        fs::remove_file(&path).unwrap();
        // Rename race: the owned directory is swapped for another with equal bytes.
        let dir = render_dir(f.root(), ProfileId::new(1));
        let moved = dir.with_file_name("moved");
        fs::rename(&dir, &moved).unwrap();
        platform::create_directory(&dir).unwrap();
        platform::create_file(&path)
            .unwrap()
            .file
            .write_all(&original)
            .unwrap();
        assert!(matches!(
            f.switch("files", Some("claude-work"), false),
            Err(Error::Drift)
        ));
        assert_eq!(fs::read(&path).unwrap(), original);
        // Directory replaced by a symlink to the outside parent.
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&dir).unwrap();
        std::os::unix::fs::symlink(f.root().parent().unwrap(), &dir).unwrap();
        assert!(f.switch("files", Some("claude-work"), false).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
        fs::remove_file(&dir).unwrap();
        fs::remove_dir_all(&moved).unwrap();
    }

    #[test]
    fn stage07_recovery_every_phase_blocks_then_explicit_sync_resumes() {
        for phase in [
            "journal",
            "render_create",
            "render_write",
            "render_fsync",
            "render_replace",
            "step",
            "catalog",
            "manifest",
            "registry",
        ] {
            let f = fixture();
            f.registry
                .add(name("claude-work"), Provider::Claude)
                .unwrap();
            f.registry.add(name("codex-45"), Provider::Codex).unwrap();
            f.add_stdio("files").unwrap();
            set_fault(Some(phase));
            assert!(f.switch("files", None, true).is_err(), "{phase}");
            set_fault(None);
            assert!(crate::share::pending(f.root()), "{phase}");
            // Consumers are blocked until explicit recovery.
            assert!(matches!(
                f.registry.mcp(McpAction::List),
                Err(Error::RecoveryRequired)
            ));
            assert!(matches!(f.registry.list(), Err(Error::RecoveryRequired)));
            assert!(matches!(
                f.switch("files", None, false),
                Err(Error::RecoveryRequired)
            ));
            assert_eq!(
                f.registry.mcp(McpAction::Sync).unwrap(),
                json!({"changed":true,"profile_id":null,"name":null,"warnings":[]})
            );
            assert!(!crate::share::pending(f.root()), "{phase}");
            assert!(f.render(1, Provider::Claude).is_file());
            assert!(f.render(2, Provider::Codex).is_file());
            assert_eq!(f.list()["servers"][0]["state"], "ok", "{phase}");
            assert_eq!(f.matrix()["rows"][0]["enabled"], json!([true, true]));
            assert!(
                !render_dir(f.root(), ProfileId::new(1))
                    .join(tmp_name(Provider::Claude))
                    .exists()
            );
        }
    }

    #[test]
    fn stage07_recovery_remove_and_profile_hook_faults_resume() {
        for phase in ["render_unlink", "step", "catalog"] {
            let f = fixture();
            f.registry
                .add(name("claude-work"), Provider::Claude)
                .unwrap();
            f.add_stdio("files").unwrap();
            f.switch("files", Some("claude-work"), true).unwrap();
            set_fault(Some(phase));
            assert!(
                f.registry
                    .mcp(McpAction::Remove {
                        name: name("files")
                    })
                    .is_err()
            );
            set_fault(None);
            assert!(crate::share::pending(f.root()));
            f.registry.mcp(McpAction::Sync).unwrap();
            assert!(!f.render(1, Provider::Claude).exists());
            assert_eq!(f.list()["servers"], json!([]));
        }
        // Profile creation with inheritance: the MCP journal commits the registry.
        let f = fixture();
        f.add_stdio("files").unwrap();
        f.registry.mcp(McpAction::SyncAll(true)).unwrap();
        set_fault(Some("journal"));
        assert!(
            f.registry
                .add(name("claude-work"), Provider::Claude)
                .is_err()
        );
        set_fault(None);
        assert!(crate::share::pending(f.root()));
        f.registry.mcp(McpAction::Sync).unwrap();
        assert!(f.render(1, Provider::Claude).is_file());
        assert_eq!(f.list()["servers"][0]["switches"][0]["enabled"], true);
    }

    #[test]
    fn stage07_recovery_foreign_journal_kind_is_not_resumed() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        for kind in ["skills", "alias"] {
            let bytes = serde_json::to_vec(&json!({"version":1,"kind":kind,"steps":[]})).unwrap();
            write_atomic(f.root(), JOURNAL, &bytes).unwrap();
            assert!(matches!(
                f.registry.mcp(McpAction::Sync),
                Err(Error::RecoveryRequired)
            ));
            assert_eq!(fs::read(f.root().join(JOURNAL)).unwrap(), bytes);
        }
        fs::remove_file(f.root().join(JOURNAL)).unwrap();
    }

    #[test]
    fn stage07_recovery_concurrent_toggles_serialize() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.registry.add(name("codex-45"), Provider::Codex).unwrap();
        f.add_stdio("files").unwrap();
        let root = f.root().to_path_buf();
        let workers: Vec<_> = (0..6)
            .map(|i| {
                let root = root.clone();
                std::thread::spawn(move || {
                    let r = Registry::at(root).unwrap();
                    for _ in 0..4 {
                        let result = r.mcp(McpAction::Switch {
                            name: name("files"),
                            target: if i % 2 == 0 {
                                Target::Profile(name("claude-work"))
                            } else {
                                Target::All
                            },
                            enabled: i % 3 != 0,
                        });
                        assert!(
                            matches!(
                                result,
                                Ok(_) | Err(Error::Busy { .. } | Error::Conflict | Error::AuthBusy)
                            ),
                            "{result:?}"
                        );
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        assert!(!crate::share::pending(f.root()));
        let list = f.list();
        assert_eq!(list["servers"][0]["state"], "ok");
        for (id, provider) in [(1, Provider::Claude), (2, Provider::Codex)] {
            let on = list["servers"][0]["switches"][id - 1]["enabled"] == true;
            assert_eq!(f.render(id as u64, provider).is_file(), on);
        }
    }

    #[test]
    fn stage07_recovery_delete_cleanup_prepare_is_pure_and_apply_idempotent() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.registry.add(name("codex-45"), Provider::Codex).unwrap();
        f.add_stdio("files").unwrap();
        f.switch("files", None, true).unwrap();
        let data = store::load(f.root()).unwrap();
        let catalog = fs::read(f.root().join(CATALOG)).unwrap();
        let cleanup = prepare_profile_cleanup(f.root(), &data, ProfileId::new(1)).unwrap();
        assert_eq!(fs::read(f.root().join(CATALOG)).unwrap(), catalog);
        assert!(f.render(1, Provider::Claude).is_file());
        // Persistable for the delete owner's journal, then replayable.
        let cleanup: ProfileCleanup =
            serde_json::from_slice(&serde_json::to_vec(&cleanup).unwrap()).unwrap();
        apply_profile_cleanup(f.root(), &cleanup).unwrap();
        apply_profile_cleanup(f.root(), &cleanup).unwrap();
        assert!(!f.render(1, Provider::Claude).exists());
        assert!(f.render(2, Provider::Codex).is_file());
        let catalog = f.catalog();
        assert_eq!(
            catalog["servers"][0]["switches"],
            json!([{"profile_id":2,"enabled":true}])
        );
    }

    #[test]
    fn stage07_recovery_after_skills_commit_each_boundary_resumes() {
        // Skills commits with MCP intent already durable in the shared journal.
        for phase in [
            "skills-registry",
            "journal",
            "handoff",
            "render_create",
            "render_write",
            "render_fsync",
            "render_replace",
            "step",
            "catalog",
            "manifest",
        ] {
            let f = fixture();
            let source = f.root().parent().unwrap().join("skill-source");
            platform::create_directory(&source).unwrap();
            platform::create_file(&source.join("SKILL.md"))
                .unwrap()
                .file
                .write_all(b"example\n")
                .unwrap();
            super::super::skills::add_private(&f.registry, &source, name("example")).unwrap();
            f.add_stdio("files").unwrap();
            f.registry.mcp(McpAction::SyncAll(true)).unwrap();
            let before = store::load(f.root()).unwrap().revision;
            if phase == "skills-registry" {
                super::super::skills::set_fault(Some("registry"));
            } else {
                set_fault(Some(phase));
            }
            assert!(
                f.registry
                    .add(name("claude-work"), Provider::Claude)
                    .is_err(),
                "{phase}"
            );
            set_fault(None);
            super::super::skills::set_fault(None);
            // The skills transaction already committed the profile.
            let committed = store::load_unchecked(f.root()).unwrap();
            assert_eq!(committed.revision, before + 1, "{phase}");
            assert!(f.root().join("profiles/1/cli/skills/example").exists());
            assert!(crate::share::pending(f.root()), "{phase}");
            assert!(matches!(f.registry.list(), Err(Error::RecoveryRequired)));
            if phase == "skills-registry" {
                assert!(matches!(
                    f.registry.mcp(McpAction::Sync),
                    Err(Error::RecoveryRequired)
                ));
                f.registry
                    .skills(super::super::skills::SkillsAction::Sync)
                    .unwrap();
            }
            if matches!(phase, "skills-registry" | "journal" | "handoff") {
                let journal: Journal = decode(&fs::read(f.root().join(JOURNAL)).unwrap()).unwrap();
                assert_eq!(journal.kind, "mcp");
                assert_eq!(journal.next, 0);
                assert!(!f.render(1, Provider::Claude).exists());
                assert!(crate::share::pending(f.root()));
            }
            f.registry.mcp(McpAction::Sync).unwrap();
            assert!(!crate::share::pending(f.root()));
            assert_eq!(
                store::load(f.root()).unwrap().revision,
                before + 1,
                "{phase}"
            );
            assert!(f.render(1, Provider::Claude).is_file(), "{phase}");
            assert_eq!(f.list()["servers"][0]["state"], "ok", "{phase}");
            assert_eq!(f.list()["servers"][0]["switches"][0]["enabled"], true);
        }
    }

    #[test]
    fn stage07_recovery_completed_and_unchanged_render_drift_blocks_remaining_steps() {
        let f = fixture();
        for (profile, provider) in [
            ("claude-work", Provider::Claude),
            ("codex-45", Provider::Codex),
            ("claude-two", Provider::Claude),
        ] {
            f.registry.add(name(profile), provider).unwrap();
        }
        f.add_stdio("files").unwrap();
        f.switch("files", Some("claude-work"), true).unwrap();
        let before = store::load(f.root()).unwrap().revision;
        set_fault(Some("step"));
        assert!(matches!(
            f.switch("files", None, true),
            Err(Error::RecoveryRequired)
        ));
        set_fault(None);
        let journal_path = f.root().join(JOURNAL);
        let journal = fs::read(&journal_path).unwrap();
        let pending = f.render(3, Provider::Claude);
        for path in [f.render(1, Provider::Claude), f.render(2, Provider::Codex)] {
            let original = fs::read(&path).unwrap();
            fs::write(&path, b"synthetic drift").unwrap();
            assert!(matches!(f.registry.mcp(McpAction::Sync), Err(Error::Drift)));
            assert_eq!(fs::read(&journal_path).unwrap(), journal);
            assert_eq!(store::load_unchecked(f.root()).unwrap().revision, before);
            assert!(!pending.exists());
            fs::write(path, original).unwrap();
        }
        f.registry.mcp(McpAction::Sync).unwrap();
        assert!(pending.is_file());
        assert!(!crate::share::pending(f.root()));
    }

    #[test]
    fn stage07_secrets_catalog_and_journal_refuse_the_same_ambiguous_definitions() {
        let f = fixture();
        f.registry
            .add(name("claude-work"), Provider::Claude)
            .unwrap();
        f.add_url("docs", "https://docs.example.com/mcp").unwrap();
        let catalog_path = f.root().join(CATALOG);
        let catalog = f.catalog();
        let mutations = [
            ("/name", json!("sk-invented")),
            (
                "/transport/headers/0/header",
                json!("InventedSecretValue9xQ7"),
            ),
            (
                "/transport/headers/0/value/name",
                json!("InventedSecretValue9xQ7"),
            ),
            (
                "/transport",
                json!({
                    "kind": "stdio",
                    "command": Path::new(&f.command).with_file_name("InventedSecretValue9xQ7.exe"),
                    "args": [],
                    "env": []
                }),
            ),
        ];
        for (field, value) in &mutations {
            let mut forged = catalog.clone();
            *forged["servers"][0].pointer_mut(field).unwrap() = value.clone();
            fs::write(&catalog_path, serde_json::to_vec(&forged).unwrap()).unwrap();
            assert!(matches!(
                f.registry.mcp(McpAction::List),
                Err(Error::RecoveryRequired)
            ));
            assert!(!crate::share::pending(f.root()));
        }
        fs::write(&catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        set_fault(Some("journal"));
        assert!(matches!(
            f.switch("docs", None, true),
            Err(Error::RecoveryRequired)
        ));
        set_fault(None);
        let journal_path = f.root().join(JOURNAL);
        let original = fs::read(&journal_path).unwrap();
        for (field, value) in mutations {
            for side in ["before_catalog", "after_catalog"] {
                let mut forged: Value = decode(&original).unwrap();
                *forged[side]["servers"][0].pointer_mut(field).unwrap() = value.clone();
                let bytes = serde_json::to_vec(&forged).unwrap();
                fs::write(&journal_path, &bytes).unwrap();
                assert!(matches!(
                    f.registry.mcp(McpAction::Sync),
                    Err(Error::RecoveryRequired)
                ));
                assert_eq!(fs::read(&journal_path).unwrap(), bytes);
                assert!(!f.render(1, Provider::Claude).exists());
            }
        }
        fs::write(&journal_path, original).unwrap();
        f.registry.mcp(McpAction::Sync).unwrap();
        assert!(f.render(1, Provider::Claude).is_file());
    }
}
