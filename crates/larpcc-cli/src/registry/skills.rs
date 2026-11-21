use super::{Error, ProfileId, ProfileName, ProfileState, Registry, paths, store};
#[cfg(unix)]
use crate::share::skills::OwnedLink;
use crate::{
    platform::{self, ObjectKind},
    share::skills::{self, OwnedCopy, Package},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const INDEX: &str = "skills.json";
const MANIFEST: &str = "owned-files.json";
const JOURNAL: &str = "share-journal.json";
const JOURNAL_LIMIT: usize = 80 * 1024 * 1024;
const MAX_SKILLS: usize = 1000;

impl From<skills::Error> for Error {
    fn from(e: skills::Error) -> Self {
        match e {
            skills::Error::TooLarge => Error::TooLarge,
            skills::Error::Io => Error::Io {
                action: "access skill filesystem",
                path: PathBuf::from("skills"),
                source: std::io::Error::other("skill filesystem I/O failed"),
            },
            skills::Error::FileLimit => Error::Io {
                action: "open skill metadata",
                path: PathBuf::from("skills"),
                source: std::io::Error::other("file descriptor limit exhausted"),
            },
            skills::Error::Drift | skills::Error::Collision => Error::Drift,
            _ => Error::Unsafe {
                path: PathBuf::from("skills"),
                reason: "skill package or owned artifact cannot be verified".into(),
            },
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Skill {
    name: ProfileName,
    sha256: String,
    disabled: BTreeSet<ProfileId>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u64,
    revision: u64,
    skills: Vec<Skill>,
}
impl Default for Index {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            skills: vec![],
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Artifact {
    Copy {
        receipt: OwnedCopy,
    },
    #[cfg(unix)]
    Link {
        receipt: OwnedLink,
    },
}
impl Artifact {
    fn verify(&self, path: &Path) -> Result<(), Error> {
        match self {
            Self::Copy { receipt } => receipt.verify(path)?,
            #[cfg(unix)]
            Self::Link { receipt } => receipt.verify(path)?,
        };
        Ok(())
    }
    fn remove(&self, path: &Path) -> Result<(), Error> {
        match self {
            Self::Copy { receipt } => match fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(Error::Drift),
                Ok(_) => receipt.resume_remove(path)?,
            },
            #[cfg(unix)]
            Self::Link { receipt } => match fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(Error::Drift),
                Ok(_) => receipt.remove(path)?,
            },
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Owned {
    name: ProfileName,
    profile: Option<ProfileId>,
    artifact: Artifact,
}
impl Owned {
    fn path(&self, root: &Path) -> PathBuf {
        artifact_path(root, &self.name, self.profile)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    Store { name: ProfileName, package: Package },
    Profile { name: ProfileName, id: ProfileId },
    Remove { owned: Owned },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u64,
    kind: String,
    before_registry: String,
    after_registry: String,
    before_index: Index,
    after_index: Index,
    before_manifest: Manifest,
    manifest: Manifest,
    steps: Vec<Step>,
    next: usize,
}

pub enum SkillsAction {
    Add {
        source: PathBuf,
        name: ProfileName,
    },
    Remove {
        name: ProfileName,
    },
    Switch {
        name: ProfileName,
        profile: ProfileName,
        enabled: bool,
    },
    List,
    Matrix,
    Sync,
}
impl SkillsAction {
    pub fn command(&self) -> &'static str {
        match self {
            Self::Add { .. } => "skills.add",
            Self::Remove { .. } => "skills.remove",
            Self::Switch { enabled: true, .. } => "skills.enable",
            Self::Switch { .. } => "skills.disable",
            Self::List => "skills.list",
            Self::Matrix => "skills.matrix",
            Self::Sync => "skills.sync",
        }
    }
}
// Compare excluded directory identities before opening any package file.
#[derive(Clone, Default)]
struct VendorHomes {
    configured: Vec<PathBuf>,
    default_home: Option<PathBuf>,
}
impl VendorHomes {
    #[cfg(not(test))]
    fn from_environment() -> Self {
        #[cfg(unix)]
        let default_home = std::env::var_os("HOME").map(PathBuf::from);
        #[cfg(windows)]
        let default_home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);
        Self {
            configured: ["CLAUDE_CONFIG_DIR", "CODEX_HOME"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from)
                .collect(),
            default_home,
        }
    }
    #[cfg(test)]
    fn from_environment() -> Self {
        panic!("skills unit tests must inject private vendor homes");
    }
}
fn home_io_error(source: std::io::Error) -> Error {
    if platform::fd_exhausted(&source) {
        Error::Io {
            action: "inspect skill exclusion",
            path: PathBuf::from("skills"),
            source,
        }
    } else {
        Error::ForbiddenSource
    }
}
fn home_error(error: Error) -> Error {
    match error {
        Error::Io { source, .. } => home_io_error(source),
        _ => Error::ForbiddenSource,
    }
}
fn source_package(root: &Path, source: &Path, vendor: &VendorHomes) -> Result<Package, Error> {
    for component in source.components() {
        if let std::path::Component::Normal(name) = component {
            let name = name.to_string_lossy().to_ascii_lowercase();
            if name.starts_with(".claude") || name.starts_with(".codex") || name == ".agents" {
                return Err(Error::ForbiddenSource);
            }
        }
    }
    if platform::contains_path(source, root)? {
        return Err(Error::ForbiddenSource);
    }
    let (object, held) = skills::source_directory(source)?;
    let mut homes = vec![root.to_path_buf()];
    for home in &vendor.configured {
        if home.as_os_str().is_empty() {
            return Err(Error::ForbiddenSource);
        }
        homes.push(home.clone());
    }
    if let Some(home) = &vendor.default_home {
        let entries = match fs::read_dir(home) {
            Ok(entries) => Some(entries),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(home_io_error(error)),
        };
        for entry in entries.into_iter().flatten() {
            let entry = entry.map_err(home_io_error)?;
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with(".claude") || name.starts_with(".codex") || name == ".agents" {
                // Default namespaces constrain directories only. Resolve aliases
                // using metadata; file/dangling aliases never open file contents.
                match fs::metadata(entry.path()) {
                    Ok(metadata) if metadata.is_dir() => homes.push(entry.path()),
                    Err(error) if platform::fd_exhausted(&error) => {
                        return Err(home_io_error(error));
                    }
                    _ => (),
                }
            }
        }
    }
    for home in homes {
        let Some((directory, ancestors)) = platform::skill_home(&home).map_err(home_error)? else {
            continue;
        };
        if object.identity == directory.identity
            || ancestors
                .ancestors
                .iter()
                .any(|p| p.identity == object.identity)
            || held
                .ancestors
                .iter()
                .any(|p| p.identity == directory.identity)
        {
            return Err(Error::ForbiddenSource);
        }
        // Each home is compared and closed here; only source handles survive.
        drop(directory);
        drop(ancestors);
    }
    Package::source(source, &object.identity).map_err(Into::into)
}

fn digest_valid(d: &str) -> bool {
    d.len() == 64
        && d.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn artifact_path(root: &Path, name: &ProfileName, id: Option<ProfileId>) -> PathBuf {
    match id {
        None => skills::store_path(root, name),
        Some(id) => root
            .join("profiles")
            .join(id.to_string())
            .join("cli/skills")
            .join(name.as_str()),
    }
}
fn active(p: &super::Profile) -> bool {
    p.state == Some(ProfileState::Active) && p.surfaces.is_some_and(|s| s.cli)
}
fn enabled(skill: &Skill, p: &super::Profile) -> bool {
    active(p) && !skill.disabled.contains(&p.id)
}
fn read_bytes(root: &Path, name: &str, limit: usize) -> Result<Option<Vec<u8>>, Error> {
    let path = root.join(name);
    if platform::inspect(&path, ObjectKind::File)?.is_none() {
        return Ok(None);
    }
    let file = platform::read_skill_object(&path, ObjectKind::File, false)?;
    platform::single_link(&file.file)?;
    let mut bytes = vec![];
    file.file
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| paths::io("read metadata", &path, e))?;
    if bytes.len() > limit {
        return Err(Error::TooLarge);
    }
    Ok(Some(bytes))
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
fn validate(
    _root: &Path,
    index: &Index,
    manifest: &Manifest,
    data: &store::Data,
) -> Result<(), Error> {
    if index.version != 1
        || manifest.version != 1
        || index.skills.len() > MAX_SKILLS
        || index.revision > data.revision
    {
        return Err(Error::RecoveryRequired);
    }
    let mut names = BTreeSet::new();
    for s in &index.skills {
        if ProfileName::parse_new(s.name.as_str()).is_err()
            || !names.insert(s.name.clone())
            || !digest_valid(&s.sha256)
            || s.disabled
                .iter()
                .any(|id| id.get() == 0 || !data.profiles.iter().any(|p| p.id == *id))
        {
            return Err(Error::RecoveryRequired);
        }
    }
    let mut owned = BTreeSet::new();
    for o in &manifest.entries {
        if !owned.insert((o.name.clone(), o.profile))
            || !names.contains(&o.name)
            || o.profile
                .is_some_and(|id| !data.profiles.iter().any(|p| p.id == id))
        {
            return Err(Error::RecoveryRequired);
        }
        match &o.artifact {
            Artifact::Copy { receipt } => {
                receipt.validate()?;
                if !index
                    .skills
                    .iter()
                    .any(|s| s.name == o.name && receipt.digest() == s.sha256)
                {
                    return Err(Error::RecoveryRequired);
                }
            }
            #[cfg(unix)]
            Artifact::Link { receipt } => {
                if o.profile.is_none() {
                    return Err(Error::RecoveryRequired);
                }
                receipt.validate_internal(_root, &o.name)?;
            }
        }
    }
    Ok(())
}
fn load(root: &Path, data: &store::Data) -> Result<(Index, Manifest), Error> {
    let index = read_bytes(root, INDEX, store::LIMIT)?
        .map(|b| decode(&b))
        .transpose()?
        .unwrap_or_default();
    let manifest = read_bytes(root, MANIFEST, store::LIMIT)?
        .map(|b| decode(&b))
        .transpose()?
        .unwrap_or_default();
    validate(root, &index, &manifest, data)?;
    Ok((index, manifest))
}
fn preflight_owned(root: &Path, owned: &Owned) -> Result<(), Error> {
    match fs::symlink_metadata(owned.path(root)) {
        Err(e) if owned.profile.is_some() && e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::Drift),
        Ok(_) => owned.artifact.verify(&owned.path(root)),
    }
}
#[derive(Clone, Copy)]
enum Repair {
    None,
    All,
    Profile(ProfileId),
}
impl Repair {
    fn includes(self, id: ProfileId) -> bool {
        matches!(self, Self::All) || matches!(self,Self::Profile(selected) if selected==id)
    }
}

fn preflight(root: &Path, index: &Index, manifest: &Manifest) -> Result<(), Error> {
    for o in &manifest.entries {
        preflight_owned(root, o)?;
    }
    for skill in &index.skills {
        if !manifest
            .entries
            .iter()
            .any(|o| o.name == skill.name && o.profile.is_none())
        {
            return Err(Error::Drift);
        }
        if Package::read(&skills::store_path(root, &skill.name))?.sha256 != skill.sha256 {
            return Err(Error::Drift);
        }
    }
    Ok(())
}
fn plan(
    root: &Path,
    data: &store::Data,
    index: &Index,
    manifest: &Manifest,
    package: Option<(ProfileName, Package)>,
    repair: Repair,
) -> Result<Vec<Step>, Error> {
    let mut steps = vec![];
    if let Some((name, package)) = package {
        steps.push(Step::Store { name, package });
    }
    for skill in &index.skills {
        for p in &data.profiles {
            if enabled(skill, p)
                && !manifest
                    .entries
                    .iter()
                    .any(|o| o.name == skill.name && o.profile == Some(p.id))
                || (enabled(skill, p)
                    && repair.includes(p.id)
                    && fs::symlink_metadata(artifact_path(root, &skill.name, Some(p.id)))
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound))
            {
                let path = artifact_path(root, &skill.name, Some(p.id));
                platform::anchor(&path)?;
                platform::inspect(path.parent().ok_or(Error::Drift)?, ObjectKind::Directory)?;
                if fs::symlink_metadata(&path).is_ok() {
                    return Err(Error::Drift);
                }
                steps.push(Step::Profile {
                    name: skill.name.clone(),
                    id: p.id,
                });
            }
        }
    }
    // Profile references go before their immutable store during removal.
    for o in manifest
        .entries
        .iter()
        .filter(|o| o.profile.is_some())
        .chain(manifest.entries.iter().filter(|o| o.profile.is_none()))
    {
        let wanted = index
            .skills
            .iter()
            .find(|s| s.name == o.name)
            .is_some_and(|s| {
                o.profile.is_none()
                    || data
                        .profiles
                        .iter()
                        .any(|p| Some(p.id) == o.profile && enabled(s, p))
            });
        if !wanted {
            steps.push(Step::Remove { owned: o.clone() });
        }
    }
    Ok(steps)
}
// Release registry lock before taking lifecycle guards; always reacquire and
// compare the revision before using the prepared ID-based plan.
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
    let ids: BTreeSet<_> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Profile { id, .. } => Some(*id),
            Step::Remove { owned } => owned.profile,
            Step::Store { .. } => None,
        })
        .collect();
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

fn sync_directory(path: &Path) -> Result<(), Error> {
    paths::sync(path)
}
fn mkdir(path: &Path) -> Result<(), Error> {
    if paths::mkdir(path)? {
        sync_directory(path.parent().ok_or(Error::RecoveryRequired)?)?;
    }
    Ok(())
}
fn apply(root: &Path, j: &mut Journal) -> Result<(), Error> {
    let after = store::parse(Path::new("registry.json"), j.after_registry.as_bytes())?;
    while j.next < j.steps.len() {
        let step = j.steps[j.next].clone();
        match step {
            Step::Store { name, package } => {
                mkdir(&root.join("shared"))?;
                mkdir(&root.join("shared/skills"))?;
                let path = skills::store_path(root, &name);
                package.resume_copy(&path)?;
                let receipt = OwnedCopy::capture(&path, &package.sha256)?;
                j.manifest
                    .entries
                    .retain(|o| !(o.name == name && o.profile.is_none()));
                j.manifest.entries.push(Owned {
                    name,
                    profile: None,
                    artifact: Artifact::Copy { receipt },
                });
                paths::sync(&root.join("shared/skills"))?;
            }
            Step::Profile { name, id } => {
                let skill = j
                    .after_index
                    .skills
                    .iter()
                    .find(|s| s.name == name)
                    .ok_or(Error::RecoveryRequired)?;
                let store_path = skills::store_path(root, &name);
                j.manifest
                    .entries
                    .iter()
                    .find(|o| o.name == name && o.profile.is_none())
                    .ok_or(Error::RecoveryRequired)?
                    .artifact
                    .verify(&store_path)?;
                if Package::read(&store_path)?.sha256 != skill.sha256 {
                    return Err(Error::Drift);
                }
                let path = artifact_path(root, &name, Some(id));
                mkdir(path.parent().ok_or(Error::RecoveryRequired)?)?;
                #[cfg(unix)]
                let artifact = {
                    let receipt = if fs::symlink_metadata(&path).is_ok() {
                        OwnedLink::capture_internal(root, &name, &path)?
                    } else {
                        OwnedLink::create(root, &name, &path, &skill.sha256)?
                    };
                    Artifact::Link { receipt }
                };
                // Confined junction creation is unavailable; explicit owned digest-copy fallback.
                #[cfg(windows)]
                let artifact = {
                    Package::read(&store_path)?.resume_copy(&path)?;
                    Artifact::Copy {
                        receipt: OwnedCopy::capture(&path, &skill.sha256)?,
                    }
                };
                j.manifest
                    .entries
                    .retain(|o| !(o.name == name && o.profile == Some(id)));
                j.manifest.entries.push(Owned {
                    name,
                    profile: Some(id),
                    artifact,
                });
                paths::sync(path.parent().ok_or(Error::RecoveryRequired)?)?;
            }
            Step::Remove { owned } => {
                owned.artifact.remove(&owned.path(root))?;
                j.manifest
                    .entries
                    .retain(|o| !(o.name == owned.name && o.profile == owned.profile));
                let parent = owned
                    .path(root)
                    .parent()
                    .ok_or(Error::RecoveryRequired)?
                    .to_path_buf();
                if platform::inspect(&parent, ObjectKind::Directory)?.is_some() {
                    paths::sync(&parent)?;
                }
            }
        }
        j.next += 1;
        write_atomic(root, JOURNAL, &bounded(j, JOURNAL_LIMIT)?)?;
        fault("step")?;
    }
    j.manifest
        .entries
        .sort_by(|a, b| (&a.name, a.profile).cmp(&(&b.name, b.profile)));
    validate(root, &j.after_index, &j.manifest, &after)?;
    for skill in &j.after_index.skills {
        if !j
            .manifest
            .entries
            .iter()
            .any(|o| o.name == skill.name && o.profile.is_none())
            || after.profiles.iter().any(|p| {
                j.manifest
                    .entries
                    .iter()
                    .any(|o| o.name == skill.name && o.profile == Some(p.id))
                    != enabled(skill, p)
            })
        {
            return Err(Error::RecoveryRequired);
        }
    }
    write_atomic(root, JOURNAL, &bounded(j, JOURNAL_LIMIT)?)?;
    write_atomic(root, INDEX, &bounded(&j.after_index, store::LIMIT)?)?;
    fault("index")?;
    write_atomic(root, MANIFEST, &bounded(&j.manifest, store::LIMIT)?)?;
    fault("manifest")?;
    let mut applied = false;
    let mut tmp = false;
    store::persist(root, j.after_registry.as_bytes(), &mut applied, &mut tmp)?;
    fault("registry")?;
    fs::remove_file(root.join(JOURNAL))
        .map_err(|e| paths::io("finish share transaction", root, e))?;
    paths::sync(root)
}
fn begin(
    root: &Path,
    before: &store::Data,
    after: &store::Data,
    before_index: Index,
    mut index: Index,
    manifest: Manifest,
    steps: Vec<Step>,
) -> Result<(), Error> {
    index.revision = after.revision;
    // Profile hooks and catalog edits must both fit every public view before intent.
    render(root, &SkillsAction::List, after, &index, &manifest)?;
    render(root, &SkillsAction::Matrix, after, &index, &manifest)?;
    // Reserve worst-case receipt space before any filesystem mutation.
    let mut manifest_size = bounded(&manifest, store::LIMIT)?.len();
    for step in &steps {
        let cost = match step {
            Step::Store { package, .. } => package.manifest_bound()?,
            Step::Profile { name, .. } => {
                #[cfg(unix)]
                {
                    skills::store_path(root, name)
                        .as_os_str()
                        .as_encoded_bytes()
                        .len()
                        * 4
                        + 512
                }
                #[cfg(windows)]
                {
                    if let Some(Step::Store { package, .. }) = steps
                        .iter()
                        .find(|s| matches!(s,Step::Store{name:n,..} if n==name))
                    {
                        package.manifest_bound()?
                    } else {
                        Package::read(&skills::store_path(root, name))?.manifest_bound()?
                    }
                }
            }
            Step::Remove { .. } => 0,
        };
        manifest_size = manifest_size.checked_add(cost).ok_or(Error::TooLarge)?;
    }
    if manifest_size > store::LIMIT - 1024 {
        return Err(Error::TooLarge);
    }
    // Bound every persisted snapshot and public output before publishing intent.
    bounded(&index, store::LIMIT)?;
    bounded(&manifest, store::LIMIT)?;
    let mut j = Journal {
        version: 1,
        kind: "skills".into(),
        before_registry: String::from_utf8(store::serialize(before)?)
            .map_err(|_| Error::RecoveryRequired)?,
        after_registry: String::from_utf8(store::serialize(after)?)
            .map_err(|_| Error::RecoveryRequired)?,
        before_index,
        after_index: index,
        before_manifest: manifest.clone(),
        manifest,
        steps,
        next: 0,
    };
    write_atomic(root, JOURNAL, &bounded(&j, JOURNAL_LIMIT)?)?;
    fault("journal")?;
    apply(root, &mut j)
}
fn recover(root: &Path, current: &store::Data) -> Result<(), Error> {
    let bytes = read_bytes(root, JOURNAL, JOURNAL_LIMIT)?.ok_or(Error::RecoveryRequired)?;
    let mut j: Journal = decode(&bytes)?;
    if j.version != 1
        || j.kind != "skills"
        || j.next > j.steps.len()
        || j.steps.len() > MAX_SKILLS * 1001
    {
        return Err(Error::RecoveryRequired);
    }
    let before = store::parse(Path::new("registry.json"), j.before_registry.as_bytes())?;
    let after = store::parse(Path::new("registry.json"), j.after_registry.as_bytes())?;
    let current = store::serialize(current)?;
    if current != j.before_registry.as_bytes() && current != j.after_registry.as_bytes()
        || after.revision != before.revision.checked_add(1).ok_or(Error::Exhausted)?
    {
        return Err(Error::Conflict);
    }
    validate(root, &j.before_index, &j.before_manifest, &before)?;
    let mut union_index = j.after_index.clone();
    for s in &j.before_index.skills {
        if !union_index.skills.iter().any(|v| v.name == s.name) {
            union_index.skills.push(s.clone());
        }
    }
    let mut union_data = store::parse(Path::new("registry.json"), j.after_registry.as_bytes())?;
    for p in &before.profiles {
        if !union_data.profiles.iter().any(|v| v.id == p.id) {
            union_data.profiles.push(p.clone());
        }
    }
    validate(root, &union_index, &j.manifest, &union_data)?;
    for step in &j.steps {
        match step {
            Step::Store { name, package } => {
                package.validate()?;
                if !j
                    .after_index
                    .skills
                    .iter()
                    .any(|s| s.name == *name && s.sha256 == package.sha256)
                {
                    return Err(Error::RecoveryRequired);
                }
            }
            Step::Profile { name, id } => {
                if !after.profiles.iter().any(|p| p.id == *id)
                    || !j.after_index.skills.iter().any(|s| s.name == *name)
                {
                    return Err(Error::RecoveryRequired);
                }
            }
            Step::Remove { owned } => {
                if !j.before_manifest.entries.contains(owned) {
                    return Err(Error::RecoveryRequired);
                }
            }
        }
    }
    for (name, old, new) in [
        (
            INDEX,
            bounded(&j.before_index, store::LIMIT)?,
            bounded(&j.after_index, store::LIMIT)?,
        ),
        (
            MANIFEST,
            bounded(&j.before_manifest, store::LIMIT)?,
            bounded(&j.manifest, store::LIMIT)?,
        ),
    ] {
        if let Some(bytes) = read_bytes(root, name, store::LIMIT)? {
            let value: Value = decode(&bytes)?;
            if value != decode::<Value>(&old)? && value != decode::<Value>(&new)? {
                return Err(Error::Drift);
            }
        }
    }
    apply(root, &mut j)
}

#[cfg(test)]
thread_local! { static FAULT: std::cell::RefCell<Option<&'static str>> = const {std::cell::RefCell::new(None)}; }
#[cfg(test)]
pub(super) fn set_fault(phase: Option<&'static str>) {
    FAULT.with(|f| *f.borrow_mut() = phase);
}
fn fault(_phase: &str) -> Result<(), Error> {
    #[cfg(test)]
    if FAULT.with(|f| f.borrow().is_some_and(|v| v == _phase)) {
        return Err(Error::RecoveryRequired);
    }
    Ok(())
}

impl Registry {
    pub fn skills(&self, action: SkillsAction) -> Result<Value, Error> {
        self.skills_with_homes(action, None)
    }
    fn skills_with_homes(
        &self,
        action: SkillsAction,
        homes: Option<&VendorHomes>,
    ) -> Result<Value, Error> {
        let _ancestors = platform::anchor(self.root())?;
        let read_only = matches!(action, SkillsAction::List | SkillsAction::Matrix);
        if read_only
            && platform::inspect(&self.root().join("registry.lock"), ObjectKind::File)?.is_none()
        {
            let data = store::load(self.root())?;
            store::precondition(&data, self.expected)?;
            let (index, manifest) = load(self.root(), &data)?;
            self.observed.set(Some(data.revision));
            return render(self.root(), &action, &data, &index, &manifest);
        }
        let lock = if read_only {
            store::admission_lock(self.root())?
        } else {
            store::lock(self.root(), Duration::from_secs(10))?
        };
        let mut data = store::load_unchecked(self.root())?;
        store::precondition(&data, self.expected)?;
        if crate::share::pending(self.root()) {
            if !matches!(action, SkillsAction::Sync) {
                return Err(Error::RecoveryRequired);
            }
            let journal: Journal = decode(
                &read_bytes(self.root(), JOURNAL, JOURNAL_LIMIT)?.ok_or(Error::RecoveryRequired)?,
            )?;
            if journal.kind != "skills" {
                return Err(Error::RecoveryRequired);
            }
            let (_lock, _guards) = guarded_relock(
                self.root(),
                data.revision,
                self.expected,
                &journal.steps,
                lock,
            )?;
            let current = store::load_unchecked(self.root())?;
            recover(self.root(), &current)?;
            data = store::load_unchecked(self.root())?;
            self.observed.set(Some(data.revision));
            return Ok(json!({"changed":true,"profile_id":null,"name":null,"warnings":[]}));
        }
        store::require_v3(&data)?;
        if data
            .profiles
            .iter()
            .any(|p| p.state != Some(ProfileState::Active))
        {
            return Err(Error::RecoveryRequired);
        }
        let (before_index, manifest) = load(self.root(), &data)?;
        if read_only {
            self.observed.set(Some(data.revision));
            return render(self.root(), &action, &data, &before_index, &manifest);
        }
        preflight(self.root(), &before_index, &manifest)?;
        let mut index = before_index.clone();
        let mut package = None;
        let mut target_id = None;
        let mut target_name = None;
        let mut repair = if matches!(action, SkillsAction::Sync) {
            Repair::All
        } else {
            Repair::None
        };
        match action {
            SkillsAction::Add { source, name } => {
                if index.skills.iter().any(|s| s.name == name) {
                    return Err(Error::Conflict);
                }
                if index.skills.len() >= MAX_SKILLS {
                    return Err(Error::TooLarge);
                }
                if fs::symlink_metadata(skills::store_path(self.root(), &name)).is_ok() {
                    return Err(Error::Drift);
                }
                let source = if source.is_absolute() {
                    source
                } else {
                    std::env::current_dir()
                        .map_err(|e| paths::io("resolve source", Path::new("skills"), e))?
                        .join(source)
                };
                let homes = homes.cloned().unwrap_or_else(VendorHomes::from_environment);
                let snapshot = source_package(self.root(), &source, &homes)?;
                target_name = Some(name.clone());
                index.skills.push(Skill {
                    name: name.clone(),
                    sha256: snapshot.sha256.clone(),
                    disabled: BTreeSet::new(),
                });
                package = Some((name, snapshot));
            }
            SkillsAction::Remove { name } => {
                let s = index
                    .skills
                    .iter()
                    .find(|s| s.name == name)
                    .ok_or(Error::SkillNotFound)?;
                if data.profiles.iter().any(|p| enabled(s, p)) {
                    return Err(Error::Conflict);
                }
                index.skills.retain(|s| s.name != name);
                target_name = Some(name);
            }
            SkillsAction::Switch {
                name,
                profile,
                enabled: on,
            } => {
                let p = data
                    .profiles
                    .iter()
                    .find(|p| p.name == profile)
                    .ok_or(Error::NotFound { name: profile })?;
                let s = index
                    .skills
                    .iter_mut()
                    .find(|s| s.name == name)
                    .ok_or(Error::SkillNotFound)?;
                if on {
                    s.disabled.remove(&p.id);
                } else {
                    s.disabled.insert(p.id);
                }
                target_id = Some(p.id);
                if on {
                    repair = Repair::Profile(p.id);
                }
                target_name = Some(name);
            }
            SkillsAction::Sync => (),
            _ => unreachable!(),
        }
        index.skills.sort_by(|a, b| a.name.cmp(&b.name));
        let steps = plan(self.root(), &data, &index, &manifest, package, repair)?;
        let changed = index != before_index || !steps.is_empty();
        let (_lock, _guards) =
            guarded_relock(self.root(), data.revision, self.expected, &steps, lock)?;
        store::load(self.root())?;
        preflight(self.root(), &before_index, &manifest)?;
        if changed {
            let mut after = store::parse(Path::new("registry.json"), &store::serialize(&data)?)?;
            after.revision = after.revision.checked_add(1).ok_or(Error::Exhausted)?;
            begin(
                self.root(),
                &data,
                &after,
                before_index,
                index,
                manifest,
                steps,
            )?;
            data = after;
        }
        self.observed.set(Some(data.revision));
        Ok(
            json!({"changed":changed,"profile_id":target_id.map(|id|id.to_string()),"name":target_name,"warnings":[]}),
        )
    }
}
fn render(
    root: &Path,
    action: &SkillsAction,
    data: &store::Data,
    index: &Index,
    manifest: &Manifest,
) -> Result<Value, Error> {
    let mut profiles: Vec<_> = data.profiles.iter().collect();
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    let value = if matches!(action, SkillsAction::Matrix) {
        json!({"profiles":profiles.iter().map(|p|json!({"id":p.id.to_string(),"name":p.name,"provider":p.provider})).collect::<Vec<_>>(),"rows":index.skills.iter().map(|s|json!({"name":s.name,"enabled":profiles.iter().map(|p|enabled(s,p)).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    } else {
        json!({"skills":index.skills.iter().map(|s| {
            let recorded=manifest.entries.iter().any(|o|o.name==s.name && o.profile.is_none()) && profiles.iter().all(|p| manifest.entries.iter().any(|o|o.name==s.name && o.profile==Some(p.id)) == enabled(s,p));
            let okay=recorded && manifest.entries.iter().filter(|o|o.name==s.name).all(|o|o.artifact.verify(&o.path(root)).is_ok()) && Package::read(&skills::store_path(root,&s.name)).is_ok_and(|p|p.sha256==s.sha256);
            let mut ids:Vec<_>=data.profiles.iter().collect();ids.sort_by_key(|p|p.id);
            json!({"name":s.name,"sha256":s.sha256,"switches":ids.iter().map(|p|json!({"profile_id":p.id.to_string(),"enabled":enabled(s,p)})).collect::<Vec<_>>(),"state":if okay{"ok"}else{"drift"}})
        }).collect::<Vec<_>>()})
    };
    // Leave framing space below the 1 MiB CLI envelope bound.
    bounded(&value, store::LIMIT - 1024)?;
    Ok(value)
}

pub(super) fn has_profile_references(
    root: &Path,
    data: &store::Data,
    id: ProfileId,
) -> Result<bool, Error> {
    let (index, manifest) = load(root, data)?;
    Ok(manifest.entries.iter().any(|o| o.profile == Some(id))
        || index.skills.iter().any(|s| {
            s.disabled.contains(&id) || data.profiles.iter().any(|p| p.id == id && enabled(s, p))
        }))
}

/// Existing profile mutations call here while retaining the registry lock.
/// The only side effects are the concrete skills plan and its registry commit.
pub(super) fn profile_commit(
    root: &Path,
    after: &store::Data,
    lock: &mut Option<store::RegistryLock>,
    expected: Option<u64>,
) -> Result<bool, Error> {
    let before = store::load_unchecked(root)?;
    let (old, manifest) = load(root, &before)?;
    if old.skills.is_empty() {
        return Ok(false);
    }
    let mut index = old.clone();
    for s in &mut index.skills {
        s.disabled
            .retain(|id| after.profiles.iter().any(|p| p.id == *id));
    }
    let steps = plan(root, after, &index, &manifest, None, Repair::None)?;
    if steps.is_empty() && index == old {
        return Ok(false);
    }
    for step in &steps {
        match step {
            Step::Profile { name, .. } => {
                let store = manifest
                    .entries
                    .iter()
                    .find(|o| o.name == *name && o.profile.is_none())
                    .ok_or(Error::Drift)?;
                preflight_owned(root, store)?;
            }
            Step::Remove { owned } => preflight_owned(root, owned)?,
            Step::Store { .. } => unreachable!(),
        }
    }
    let (registry_lock, _guards) = guarded_relock(
        root,
        before.revision,
        expected,
        &steps,
        lock.take().ok_or(Error::RecoveryRequired)?,
    )?;
    *lock = Some(registry_lock);
    store::load(root)?;
    // Same revision after reacquisition: no cooperative writer changed the plan.
    begin(root, &before, after, old, index, manifest, steps)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Provider, Surface};
    fn name(s: &str) -> ProfileName {
        ProfileName::parse_new(s).unwrap()
    }
    #[cfg(unix)]
    type Temp = tempfile::TempDir;
    #[cfg(windows)]
    type Temp = crate::platform::fixture_private::PrivateTempDir;
    fn fixture() -> (Temp, Registry, PathBuf) {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        #[cfg(unix)]
        let temp = {
            use std::os::unix::fs::PermissionsExt;
            tempfile::Builder::new()
                .prefix("stage06-")
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(&parent)
                .unwrap()
        };
        #[cfg(windows)]
        let temp = crate::platform::fixture_private::tempdir(&parent).unwrap();
        let source = temp.path().join("source");
        platform::create_directory(&source).unwrap();
        platform::create_file(&source.join("SKILL.md"))
            .unwrap()
            .file
            .write_all(b"example\n")
            .unwrap();
        let r = Registry::at(temp.path().join("root")).unwrap();
        (temp, r, source)
    }
    fn private_skills(r: &Registry, action: SkillsAction) -> Result<Value, Error> {
        let homes = VendorHomes {
            configured: vec![],
            default_home: Some(r.root().parent().unwrap().join("fake-home")),
        };
        r.skills_with_homes(action, Some(&homes))
    }
    #[test]
    #[should_panic(expected = "skills unit tests must inject private vendor homes")]
    fn stage06_unit_tests_forbid_ambient_vendor_home_access() {
        VendorHomes::from_environment();
    }
    fn add(r: &Registry, source: &Path) -> Value {
        private_skills(
            r,
            SkillsAction::Add {
                source: source.into(),
                name: name("example"),
            },
        )
        .unwrap()
    }
    fn switch(r: &Registry, enabled: bool) -> Result<Value, Error> {
        r.skills(SkillsAction::Switch {
            name: name("example"),
            profile: name("claude-work"),
            enabled,
        })
    }
    #[test]
    fn stage06_store_public_view_bound_refuses_before_journal() {
        let (_temp, r, _) = fixture();
        let mut data = store::Data::default();
        for i in 1..=100 {
            data.profiles.push(super::super::Profile {
                id: ProfileId::new(i),
                name: name(&format!("profile-{i}")),
                provider: Provider::Claude,
                surfaces: Some(super::super::model::Surfaces::default()),
                state: Some(ProfileState::Active),
                deletion_transaction: None,
            });
        }
        let index = Index {
            version: 1,
            revision: 0,
            skills: (0..1000)
                .map(|i| Skill {
                    name: name(&format!("skill-{i}")),
                    sha256: "0".repeat(64),
                    disabled: BTreeSet::new(),
                })
                .collect(),
        };
        assert!(matches!(
            begin(
                r.root(),
                &data,
                &data,
                Index::default(),
                index,
                Manifest::default(),
                vec![]
            ),
            Err(Error::TooLarge)
        ));
        assert!(!crate::share::pending(r.root()));
    }
    #[test]
    fn stage06_links_default_new_profiles_explicit_disable_stable_id() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        add(&r, &source);
        assert!(
            r.root()
                .join("profiles/1/cli/skills/example/SKILL.md")
                .is_file()
        );
        switch(&r, false).unwrap();
        assert!(!r.root().join("profiles/1/cli/skills/example").exists());
        r.rename(&name("claude-work"), name("codex-45")).unwrap();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        assert!(
            r.root()
                .join("profiles/2/cli/skills/example/SKILL.md")
                .is_file()
        );
        assert!(!r.root().join("profiles/1/cli/skills/example").exists());
        let matrix = r.skills(SkillsAction::Matrix).unwrap();
        assert_eq!(matrix["rows"][0]["enabled"], json!([true, false]));
        r.surface(name("claude-work"), Surface::Cli, false).unwrap();
        assert!(!r.root().join("profiles/2/cli/skills/example").exists());
        r.surface(name("claude-work"), Surface::Cli, true).unwrap();
        assert!(
            r.root()
                .join("profiles/2/cli/skills/example/SKILL.md")
                .is_file()
        );
    }
    #[test]
    fn stage06_store_remove_refuses_enabled_and_noop_keeps_revision() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        add(&r, &source);
        assert!(matches!(
            r.skills(SkillsAction::Remove {
                name: name("example")
            }),
            Err(Error::Conflict)
        ));
        let revision = r.revision();
        assert_eq!(switch(&r, true).unwrap()["changed"], false);
        assert_eq!(r.revision(), revision);
        switch(&r, false).unwrap();
        r.skills(SkillsAction::Remove {
            name: name("example"),
        })
        .unwrap();
        assert!(!skills::store_path(r.root(), &name("example")).exists());
        assert_eq!(r.skills(SkillsAction::List).unwrap(), json!({"skills":[]}));
    }
    #[test]
    fn stage06_crash_each_commit_boundary_and_recovery() {
        for phase in ["journal", "step", "index", "manifest", "registry"] {
            let (_temp, r, source) = fixture();
            r.add(name("claude-work"), Provider::Claude).unwrap();
            set_fault(Some(phase));
            assert!(
                private_skills(
                    &r,
                    SkillsAction::Add {
                        source,
                        name: name("example")
                    }
                )
                .is_err()
            );
            set_fault(None);
            assert!(matches!(r.list(), Err(Error::RecoveryRequired)));
            assert!(matches!(
                r.skills(SkillsAction::List),
                Err(Error::RecoveryRequired)
            ));
            r.skills(SkillsAction::Sync).unwrap();
            assert!(
                r.root()
                    .join("profiles/1/cli/skills/example/SKILL.md")
                    .is_file()
            );
            assert_eq!(
                r.skills(SkillsAction::List).unwrap()["skills"][0]["state"],
                "ok"
            );
            assert!(!crate::share::pending(r.root()));
        }
    }
    #[test]
    fn stage06_crash_partial_files_and_leaf_unlinks_resume() {
        for phase in ["directory", "file_created", "file_written"] {
            let (_temp, r, source) = fixture();
            skills::set_io_fault(Some(phase));
            assert!(
                private_skills(
                    &r,
                    SkillsAction::Add {
                        source,
                        name: name("example")
                    }
                )
                .is_err()
            );
            skills::set_io_fault(None);
            assert!(crate::share::pending(r.root()));
            r.skills(SkillsAction::Sync).unwrap();
            assert_eq!(
                r.skills(SkillsAction::List).unwrap()["skills"][0]["state"],
                "ok"
            );
        }
        let (_temp, r, source) = fixture();
        add(&r, &source);
        skills::set_io_fault(Some("unlink"));
        assert!(
            r.skills(SkillsAction::Remove {
                name: name("example")
            })
            .is_err()
        );
        skills::set_io_fault(None);
        r.skills(SkillsAction::Sync).unwrap();
        assert!(!skills::store_path(r.root(), &name("example")).exists());
    }
    #[test]
    fn stage06_crash_remove_resume_and_profile_create() {
        let (_temp, r, source) = fixture();
        add(&r, &source);
        set_fault(Some("journal"));
        assert!(r.add(name("claude-work"), Provider::Claude).is_err());
        set_fault(None);
        assert!(crate::share::pending(r.root()));
        r.skills(SkillsAction::Sync).unwrap();
        assert!(
            r.root()
                .join("profiles/1/cli/skills/example/SKILL.md")
                .is_file()
        );
        switch(&r, false).unwrap();
        set_fault(Some("step"));
        assert!(
            r.skills(SkillsAction::Remove {
                name: name("example")
            })
            .is_err()
        );
        set_fault(None);
        r.skills(SkillsAction::Sync).unwrap();
        assert!(!skills::store_path(r.root(), &name("example")).exists());
    }
    #[test]
    fn stage06_crash_concurrent_toggle_forget_and_stale_revision() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        add(&r, &source);
        let revision = r.revision().unwrap();
        let root = r.root().to_owned();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let a = barrier.clone();
            let root_a = root.clone();
            let one = scope.spawn(move || {
                a.wait();
                Registry::at(root_a)
                    .unwrap()
                    .expecting(Some(revision))
                    .skills(SkillsAction::Switch {
                        name: name("example"),
                        profile: name("claude-work"),
                        enabled: false,
                    })
                    .map(|_| ())
            });
            let b = barrier.clone();
            let root_b = root.clone();
            let two = scope.spawn(move || {
                b.wait();
                Registry::at(root_b)
                    .unwrap()
                    .expecting(Some(revision))
                    .forget(&name("claude-work"))
                    .map(|_| ())
            });
            [one.join().unwrap(), two.join().unwrap()]
        });
        assert!(results[0].is_ok());
        assert!(matches!(results[1], Err(Error::Conflict)));
        assert!(r.get(&name("claude-work")).is_ok());
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(Error::Conflict)))
                .count(),
            1
        );
        let stale = Registry::at(root).unwrap().expecting(Some(revision));
        assert!(matches!(
            stale.skills(SkillsAction::Switch {
                name: name("example"),
                profile: name("missing"),
                enabled: false
            }),
            Err(Error::Conflict)
        ));
    }
    #[test]
    fn stage06_drift_occupied_directory_and_corrupt_index_refuse_before_changes() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        let dir = r.root().join("profiles/1/cli/skills");
        platform::create_directory(&dir).unwrap();
        platform::create_directory(&dir.join("example")).unwrap();
        let before = fs::read(r.root().join("registry.json")).unwrap();
        assert!(matches!(
            private_skills(
                &r,
                SkillsAction::Add {
                    source: source.clone(),
                    name: name("example")
                }
            ),
            Err(Error::Drift)
        ));
        assert_eq!(fs::read(r.root().join("registry.json")).unwrap(), before);
        assert!(!crate::share::pending(r.root()));
        fs::remove_dir(dir.join("example")).unwrap();
        add(&r, &source);
        let bad = json!({"version":1,"revision":2,"skills":[{"name":"../escape","sha256":"0".repeat(64),"disabled":[]}]});
        write_atomic(r.root(), INDEX, &serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(r.skills(SkillsAction::Sync).is_err());
        assert!(
            r.root()
                .join("profiles/1/cli/skills/example/SKILL.md")
                .is_file()
        );
    }
    #[cfg(windows)]
    #[test]
    fn stage06_links_windows_owned_copy_drift_hardlink_and_remove() {
        let (_temp, r, source) = fixture();
        r.add(name("codex-45"), Provider::Codex).unwrap();
        add(&r, &source);
        let copy = r.root().join("profiles/1/cli/skills/example");
        use std::os::windows::fs::MetadataExt;
        assert_eq!(
            fs::symlink_metadata(&copy).unwrap().file_attributes() & 0x400,
            0
        );
        fs::write(copy.join("SKILL.md"), b"changed").unwrap();
        assert!(matches!(
            r.skills(SkillsAction::Switch {
                name: name("example"),
                profile: name("codex-45"),
                enabled: false
            }),
            Err(Error::Drift) | Err(Error::Unsafe { .. })
        ));
        fs::write(copy.join("SKILL.md"), b"example\n").unwrap();
        r.skills(SkillsAction::Switch {
            name: name("example"),
            profile: name("codex-45"),
            enabled: false,
        })
        .unwrap();
        assert!(!copy.exists());
        fs::hard_link(source.join("SKILL.md"), source.join("hardlink")).unwrap();
        assert!(
            Package::source(
                &source,
                &skills::source_directory(&source).unwrap().0.identity
            )
            .is_err()
        );
    }
    fn remove_profile_artifact(path: &Path) {
        #[cfg(unix)]
        fs::remove_file(path).unwrap();
        #[cfg(windows)]
        {
            let receipt = OwnedCopy::capture(path, &Package::read(path).unwrap().sha256).unwrap();
            receipt.resume_remove(path).unwrap();
        }
    }
    #[test]
    fn stage06_links_missing_owned_artifact_sync_disable_and_unrelated_profile_mutations() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        add(&r, &source);
        let path = r.root().join("profiles/1/cli/skills/example");
        remove_profile_artifact(&path);
        assert_eq!(
            r.skills(SkillsAction::List).unwrap()["skills"][0]["state"],
            "drift"
        );
        r.rename(&name("claude-work"), name("codex-45")).unwrap();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        assert!(!path.exists());
        r.skills(SkillsAction::Sync).unwrap();
        assert!(path.join("SKILL.md").is_file());
        remove_profile_artifact(&path);
        fs::remove_dir(path.parent().unwrap()).unwrap();
        r.skills(SkillsAction::Switch {
            name: name("example"),
            profile: name("codex-45"),
            enabled: false,
        })
        .unwrap();
        let (_, manifest) = load(r.root(), &store::load(r.root()).unwrap()).unwrap();
        assert!(
            !manifest
                .entries
                .iter()
                .any(|o| o.profile == Some(ProfileId::new(1)))
        );
        assert!(!crate::share::pending(r.root()));
    }
    #[test]
    fn stage06_links_profile_rename_ignores_unrelated_drift_but_sync_refuses() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        add(&r, &source);
        let path = r.root().join("profiles/1/cli/skills/example");
        remove_profile_artifact(&path);
        platform::create_directory(&path).unwrap();
        platform::create_file(&path.join("native"))
            .unwrap()
            .file
            .write_all(b"sentinel")
            .unwrap();
        r.rename(&name("claude-work"), name("codex-45")).unwrap();
        assert!(matches!(r.skills(SkillsAction::Sync), Err(Error::Drift)));
        assert_eq!(fs::read(path.join("native")).unwrap(), b"sentinel");
        assert!(!crate::share::pending(r.root()));
    }
    #[test]
    fn stage06_forget_sharing_references_conflict_without_effects() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        add(&r, &source);
        let registry = fs::read(r.root().join("registry.json")).unwrap();
        let manifest = fs::read(r.root().join(MANIFEST)).unwrap();
        assert!(matches!(
            r.forget(&name("claude-work")),
            Err(Error::Conflict)
        ));
        assert_eq!(registry, fs::read(r.root().join("registry.json")).unwrap());
        assert_eq!(manifest, fs::read(r.root().join(MANIFEST)).unwrap());
        assert!(
            r.root()
                .join("profiles/1/cli/skills/example/SKILL.md")
                .is_file()
        );
        switch(&r, false).unwrap();
        assert!(matches!(
            r.forget(&name("claude-work")),
            Err(Error::Conflict)
        ));
        r.skills(SkillsAction::Remove {
            name: name("example"),
        })
        .unwrap();
        r.forget(&name("claude-work")).unwrap();
        assert!(r.root().join("profiles/1/cli").is_dir());
    }
    #[test]
    fn stage06_lifecycle_guard_blocks_enable_sync_surface_and_recovery() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        add(&r, &source);
        switch(&r, false).unwrap();
        let guard =
            crate::launch::admission::acquire_auth_guard(r.root(), ProfileId::new(1)).unwrap();
        let before = fs::read(r.root().join("registry.json")).unwrap();
        assert!(matches!(switch(&r, true), Err(Error::AuthBusy)));
        assert_eq!(fs::read(r.root().join("registry.json")).unwrap(), before);
        assert!(!crate::share::pending(r.root()));
        drop(guard);
        switch(&r, true).unwrap();
        let path = r.root().join("profiles/1/cli/skills/example");
        remove_profile_artifact(&path);
        let guard =
            crate::launch::admission::acquire_auth_guard(r.root(), ProfileId::new(1)).unwrap();
        assert!(matches!(r.skills(SkillsAction::Sync), Err(Error::AuthBusy)));
        assert!(matches!(
            r.surface(name("claude-work"), Surface::Cli, false),
            Err(Error::AuthBusy)
        ));
        assert!(!crate::share::pending(r.root()));
        drop(guard);
        set_fault(Some("journal"));
        assert!(r.skills(SkillsAction::Sync).is_err());
        set_fault(None);
        let guard =
            crate::launch::admission::acquire_auth_guard(r.root(), ProfileId::new(1)).unwrap();
        assert!(matches!(r.skills(SkillsAction::Sync), Err(Error::AuthBusy)));
        assert!(crate::share::pending(r.root()));
        drop(guard);
        r.skills(SkillsAction::Sync).unwrap();
    }
    #[test]
    fn stage06_lifecycle_multiple_profiles_release_earlier_guards_on_contention() {
        let (_temp, r, source) = fixture();
        r.add(name("claude-work"), Provider::Claude).unwrap();
        r.add(name("codex-45"), Provider::Codex).unwrap();
        let guard =
            crate::launch::admission::acquire_auth_guard(r.root(), ProfileId::new(2)).unwrap();
        assert!(matches!(
            private_skills(
                &r,
                SkillsAction::Add {
                    source,
                    name: name("example")
                }
            ),
            Err(Error::AuthBusy)
        ));
        let first =
            crate::launch::admission::acquire_auth_guard(r.root(), ProfileId::new(1)).unwrap();
        assert!(!skills::store_path(r.root(), &name("example")).exists());
        assert!(!crate::share::pending(r.root()));
        drop(first);
        drop(guard);
    }
    #[test]
    fn stage06_links_missing_copy_and_parent_can_drop_owned_receipt() {
        let (_temp, r, source) = fixture();
        let parent = source.parent().unwrap().join("copies");
        platform::create_directory(&parent).unwrap();
        let copy = parent.join("example");
        let package = Package::source(
            &source,
            &skills::source_directory(&source).unwrap().0.identity,
        )
        .unwrap();
        package.resume_copy(&copy).unwrap();
        let receipt = OwnedCopy::capture(&copy, &package.sha256).unwrap();
        receipt.resume_remove(&copy).unwrap();
        fs::remove_dir(parent).unwrap();
        Artifact::Copy { receipt }.remove(&copy).unwrap();
        assert!(!copy.exists());
        assert!(!r.root().exists());
    }
    #[test]
    fn stage06_source_reopened_identity_must_match_authorized_directory() {
        let (temp, _r, source) = fixture();
        let (checked, ancestors) = skills::source_directory(&source).unwrap();
        let expected = checked.identity;
        drop(checked.file);
        drop(ancestors);
        fs::rename(&source, temp.path().join("original-source")).unwrap();
        platform::create_directory(&source).unwrap();
        platform::create_file(&source.join("SKILL.md"))
            .unwrap()
            .file
            .write_all(b"replacement")
            .unwrap();
        assert!(Package::source(&source, &expected).is_err());
        let current = skills::source_directory(&source).unwrap().0.identity;
        assert!(Package::source(&source, &current).is_ok());
    }
    #[test]
    fn stage06_exclusion_descriptor_exhaustion_is_io_not_forbidden_source() {
        #[cfg(unix)]
        let codes = [libc::EMFILE, libc::ENFILE];
        #[cfg(windows)]
        let codes = [4, 1450];
        for code in codes {
            assert!(matches!(
                home_error(Error::Io {
                    action: "test",
                    path: "private-home".into(),
                    source: std::io::Error::from_raw_os_error(code)
                }),
                Error::Io { .. }
            ));
        }
        assert!(matches!(
            home_io_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            Error::ForbiddenSource
        ));
    }
}
