//! Alias add/remove transactions. They share the single pending namespace
//! (`share-journal.json`) with skills but carry their own `alias` kind and steps, and only
//! touch the fixed `<root>/bin` launcher/receipt allowlist from `crate::aliases`.
use super::{
    Error, ProfileId, ProfileName, ProfileState, Registry, Surface, model::Alias, paths, store,
};
use crate::{
    aliases::{self, Artifact, Directories, DirectoryPin, Owned, Receipt, Source},
    platform::{self, CheckedObject, ObjectKind},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const JOURNAL: &str = "share-journal.json";
const KIND: &str = "alias";
// Two JSON-escaped registry snapshots plus small receipts.
const JOURNAL_LIMIT: usize = 6 * store::LIMIT;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Removed {
    alias: Alias,
    owned: Owned,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Staged {
    launcher: Artifact,
    receipt: Artifact,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    Add {
        name: ProfileName,
        staged: Option<Box<Staged>>,
    },
    Remove {
        aliases: Vec<Removed>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u64,
    kind: String,
    before_registry: String,
    after_registry: Option<String>,
    directories: DirectoryPin,
    step: Step,
}

fn staged_paths(root: &Path, name: &ProfileName) -> [PathBuf; 2] {
    [
        root.join("bin")
            .join(format!(".{}.launcher.pending", name.as_str())),
        root.join("bin")
            .join(format!(".{}.receipt.pending", name.as_str())),
    ]
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str) -> Result<[u8; 32], Error> {
    let mut out = [0; 32];
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::AliasInvalid);
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&value[2 * i..2 * i + 2], 16).map_err(|_| Error::AliasInvalid)?;
    }
    Ok(out)
}
fn utf8(bytes: Vec<u8>) -> Result<String, Error> {
    String::from_utf8(bytes).map_err(|_| Error::RecoveryRequired)
}
fn copy(data: &store::Data) -> Result<store::Data, Error> {
    store::parse(Path::new("registry.json"), &store::serialize(data)?)
}

fn write_journal(root: &Path, j: &Journal) -> Result<(), Error> {
    let bytes = serde_json::to_vec(j).map_err(|_| Error::RecoveryRequired)?;
    if bytes.len() > JOURNAL_LIMIT {
        return Err(Error::TooLarge);
    }
    let path = root.join(JOURNAL);
    let tmp = root.join(format!("{JOURNAL}.tmp"));
    if let Some(object) = platform::inspect(&tmp, ObjectKind::File)? {
        platform::single_link(&object.file)?;
        drop(object);
        fs::remove_file(&tmp).map_err(|e| paths::io("remove journal temporary", &tmp, e))?;
    }
    let mut file = platform::create_file(&tmp)?.file;
    std::io::Write::write_all(&mut file, &bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| paths::io("write journal", &tmp, e))?;
    drop(file);
    store::replace(&tmp, &path)?;
    paths::sync(root)
}
/// Only an alias-kind journal is read here; any other kind is left to its owner.
fn read_journal(root: &Path) -> Result<Option<Journal>, Error> {
    let path = root.join(JOURNAL);
    if platform::inspect(&path, ObjectKind::File)?.is_none() {
        return Ok(None);
    }
    let object = platform::read_skill_object(&path, ObjectKind::File, false)?;
    platform::single_link(&object.file)?;
    let mut bytes = vec![];
    object
        .file
        .take(JOURNAL_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| paths::io("read journal", &path, e))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::RecoveryRequired)?;
    if bytes.len() > JOURNAL_LIMIT || value.get("kind") != Some(&json!(KIND)) {
        return Err(Error::RecoveryRequired);
    }
    let j: Journal = serde_json::from_value(value).map_err(|_| Error::RecoveryRequired)?;
    if j.version != 2
        || match &j.step {
            Step::Add { name, .. } => ProfileName::parse_new(name.as_str()).is_err(),
            Step::Remove { aliases } => aliases
                .iter()
                .any(|r| ProfileName::parse_new(r.alias.name.as_str()).is_err()),
        }
    {
        return Err(Error::RecoveryRequired);
    }
    Ok(Some(j))
}
fn finish(root: &Path) -> Result<(), Error> {
    fs::remove_file(root.join(JOURNAL))
        .map_err(|e| paths::io("finish alias transaction", root, e))?;
    paths::sync(root)
}
fn verify_directories(root: &Path, pin: &DirectoryPin) -> Result<Directories, Error> {
    let dirs = Directories::open(root)?;
    if &dirs.pin() != pin {
        return Err(Error::AliasInvalid);
    }
    Ok(dirs)
}

/// Call under the registry write lock with the unchecked current registry.
fn recover(root: &Path, current: &store::Data) -> Result<(), Error> {
    let Some(j) = read_journal(root)? else {
        return Ok(());
    };
    let dirs = verify_directories(root, &j.directories)?;
    let current_bytes = store::serialize(current)?;
    let committed = j
        .after_registry
        .as_deref()
        .is_some_and(|after| after.as_bytes() == current_bytes);
    if !committed && current_bytes != j.before_registry.as_bytes() {
        return Err(Error::Conflict);
    }
    match &j.step {
        Step::Add { name, staged } => {
            let registered = current.aliases.iter().find(|a| &a.name == name);
            if committed != registered.is_some() {
                return Err(Error::RecoveryRequired);
            }
            let [stage_launcher, stage_receipt] = staged_paths(root, name);
            let launcher = aliases::launcher_path(root, name);
            let receipt = aliases::receipt_path(root, name);
            let files = [
                (&launcher, staged.as_ref().map(|s| &s.launcher)),
                (&stage_launcher, staged.as_ref().map(|s| &s.launcher)),
                (&receipt, staged.as_ref().map(|s| &s.receipt)),
                (&stage_receipt, staged.as_ref().map(|s| &s.receipt)),
            ];
            let mut present = [false; 4];
            for ((path, expected), present) in files.iter().zip(&mut present) {
                *present = match expected {
                    Some(expected) => match expected.present(&dirs, path)? {
                        Some(true) => true,
                        Some(false) => return Err(Error::AliasInvalid),
                        None => false,
                    },
                    None if dirs.read(path)?.is_some() => return Err(Error::AliasInvalid),
                    None => false,
                };
            }
            let [
                launcher_present,
                stage_launcher_present,
                receipt_present,
                stage_receipt_present,
            ] = present;
            if launcher_present && stage_launcher_present
                || receipt_present && stage_receipt_present
            {
                return Err(Error::AliasInvalid);
            }
            if let Some(alias) = registered {
                if present != [true, false, true, false] {
                    return Err(Error::AliasInvalid);
                }
                ownership(root, &dirs, alias)?;
                return finish(root);
            }
            if receipt_present || stage_receipt_present {
                let path = if receipt_present {
                    &receipt
                } else {
                    &stage_receipt
                };
                let staged = staged.as_ref().ok_or(Error::RecoveryRequired)?;
                let receipt =
                    Receipt::staged(root, &dirs, path, name, &staged.launcher, &staged.receipt)?;
                let mut expected = copy(current)?;
                expected.aliases.push(Alias {
                    name: name.clone(),
                    profile_id: receipt.profile_id(),
                    surface: receipt.surface(),
                    receipt: hex(&staged.receipt.digest),
                });
                expected.aliases.sort_by(|a, b| a.name.cmp(&b.name));
                expected.revision = expected.revision.checked_add(1).ok_or(Error::Exhausted)?;
                let after = j.after_registry.as_deref().ok_or(Error::RecoveryRequired)?;
                if store::serialize(&expected)? != after.as_bytes() {
                    return Err(Error::RecoveryRequired);
                }
            } else if launcher_present || stage_launcher_present {
                return Err(Error::AliasInvalid);
            }
            for ((path, artifact), present) in files.into_iter().zip(present) {
                if present {
                    verify_directories(root, &j.directories)?;
                    let artifact = artifact.ok_or(Error::RecoveryRequired)?;
                    if artifact.present(&dirs, path)? != Some(true) {
                        return Err(Error::AliasInvalid);
                    }
                    dirs.remove(path, &artifact.identity)?;
                }
            }
            dirs.sync()?;
        }
        Step::Remove { aliases: removed } => {
            let after = j.after_registry.as_deref().ok_or(Error::RecoveryRequired)?;
            if committed {
                if removed.is_empty() {
                    return Err(Error::RecoveryRequired);
                }
                for r in removed {
                    let name = &r.alias.name;
                    if current.aliases.iter().any(|a| &a.name == name)
                        || dirs.read(&aliases::launcher_path(root, name))?.is_some()
                        || dirs.read(&aliases::receipt_path(root, name))?.is_some()
                    {
                        return Err(Error::AliasInvalid);
                    }
                }
                return finish(root);
            }
            let mut expected = copy(current)?;
            expected
                .aliases
                .retain(|a| !removed.iter().any(|r| &r.alias == a));
            expected.revision = expected.revision.checked_add(1).ok_or(Error::Exhausted)?;
            if removed.is_empty()
                || expected.aliases.len() + removed.len() != current.aliases.len()
                || removed.iter().any(|r| !current.aliases.contains(&r.alias))
                || store::serialize(&expected)? != after.as_bytes()
            {
                return Err(Error::RecoveryRequired);
            }
            remove_artifacts(root, &j.directories, removed)?;
            store::persist(root, after.as_bytes(), &mut false, &mut false)?;
        }
    }
    finish(root)
}

fn ownership(root: &Path, dirs: &Directories, alias: &Alias) -> Result<Owned, Error> {
    aliases::owned(
        root,
        dirs,
        &alias.name,
        alias.profile_id,
        alias.surface,
        &unhex(&alias.receipt)?,
    )
}

fn remove_artifacts(root: &Path, pin: &DirectoryPin, removed: &[Removed]) -> Result<(), Error> {
    let dirs = verify_directories(root, pin)?;
    let proven = prepare_removal(root, &dirs, removed, true)?;
    apply_removal(root, pin, &dirs, proven)
}

fn prepare_removal<'a>(
    root: &Path,
    dirs: &Directories,
    removed: &'a [Removed],
    resuming: bool,
) -> Result<Vec<(&'a ProfileName, Owned)>, Error> {
    let mut proven = Vec::with_capacity(removed.len());
    // Check the whole set before touching any entry. A missing artifact may have
    // been removed before interruption, but any present artifact must agree.
    for r in removed {
        let owned = ownership(root, dirs, &r.alias)?;
        if (!resuming && owned != r.owned)
            || owned
                .launcher
                .as_ref()
                .is_some_and(|a| Some(a) != r.owned.launcher.as_ref())
            || owned
                .receipt
                .as_ref()
                .is_some_and(|a| Some(a) != r.owned.receipt.as_ref())
        {
            return Err(Error::AliasInvalid);
        }
        proven.push((&r.alias.name, owned));
    }
    Ok(proven)
}

fn apply_removal(
    root: &Path,
    pin: &DirectoryPin,
    dirs: &Directories,
    proven: Vec<(&ProfileName, Owned)>,
) -> Result<(), Error> {
    for (name, owned) in proven {
        for (path, artifact) in [
            (aliases::launcher_path(root, name), owned.launcher),
            (aliases::receipt_path(root, name), owned.receipt),
        ] {
            if let Some(artifact) = artifact {
                verify_directories(root, pin)?;
                if artifact.present(dirs, &path)? != Some(true) {
                    return Err(Error::AliasInvalid);
                }
                dirs.remove(&path, &artifact.identity)?;
            }
        }
        fault("artifact")?;
    }
    dirs.sync()
}

/// Journal-remove the selected aliases' launchers, receipts and mappings in one registry
/// commit. Call under the registry write lock after the revision precondition.
fn remove_aliases(
    root: &Path,
    data: store::Data,
    select: impl Fn(&Alias) -> bool,
) -> Result<store::Data, Error> {
    let selected: Vec<_> = data.aliases.iter().filter(|a| select(a)).collect();
    if selected.is_empty() {
        return Ok(data);
    }
    let dirs = Directories::open(root)?;
    let mut removed = Vec::with_capacity(selected.len());
    for a in selected {
        removed.push(Removed {
            alias: a.clone(),
            owned: ownership(root, &dirs, a)?,
        });
    }
    let pin = dirs.pin();
    let mut after = copy(&data)?;
    after.aliases.retain(|a| !select(a));
    after.revision = after.revision.checked_add(1).ok_or(Error::Exhausted)?;
    let after_bytes = store::serialize(&after)?;
    let j = Journal {
        version: 2,
        kind: KIND.into(),
        before_registry: utf8(store::serialize(&data)?)?,
        after_registry: Some(utf8(after_bytes.clone())?),
        directories: pin,
        step: Step::Remove { aliases: removed },
    };
    write_journal(root, &j)?;
    fault("journal")?;
    let Step::Remove { aliases } = &j.step else {
        unreachable!()
    };
    remove_artifacts(root, &j.directories, aliases)?;
    store::persist(root, &after_bytes, &mut false, &mut false)?;
    fault("registry")?;
    finish(root)?;
    Ok(after)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProfileCleanup {
    directories: DirectoryPin,
    aliases: Vec<Removed>,
}

pub(super) fn prepare_profile_cleanup(
    root: &Path,
    data: &store::Data,
    id: ProfileId,
) -> Result<Option<ProfileCleanup>, Error> {
    let aliases = data.aliases.iter().filter(|a| a.profile_id == id);
    if aliases.clone().next().is_none() {
        return Ok(None);
    }
    let dirs = Directories::open(root)?;
    let aliases = aliases
        .map(|alias| {
            Ok(Removed {
                alias: alias.clone(),
                owned: ownership(root, &dirs, alias)?,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok(Some(ProfileCleanup {
        directories: dirs.pin(),
        aliases,
    }))
}

pub(super) struct PreparedProfileCleanup<'a> {
    dirs: Directories,
    pin: &'a DirectoryPin,
    artifacts: Vec<(&'a ProfileName, Owned)>,
}
impl PreparedProfileCleanup<'_> {
    pub(super) fn apply(self, root: &Path) -> Result<(), Error> {
        apply_removal(root, self.pin, &self.dirs, self.artifacts)
    }

    pub(super) fn verify_removed(&self) -> Result<(), Error> {
        if self
            .artifacts
            .iter()
            .any(|(_, a)| a.launcher.is_some() || a.receipt.is_some())
        {
            return Err(Error::AliasInvalid);
        }
        Ok(())
    }
}

pub(super) fn validate_profile_cleanup<'a>(
    root: &Path,
    data: &store::Data,
    id: ProfileId,
    cleanup: Option<&'a ProfileCleanup>,
    resuming: bool,
) -> Result<Option<PreparedProfileCleanup<'a>>, Error> {
    let mut aliases = data.aliases.iter().filter(|a| a.profile_id == id);
    let Some(cleanup) = cleanup else {
        return if aliases.next().is_none() {
            Ok(None)
        } else {
            Err(Error::RecoveryRequired)
        };
    };
    if cleanup.aliases.is_empty() || !aliases.eq(cleanup.aliases.iter().map(|r| &r.alias)) {
        return Err(Error::RecoveryRequired);
    }
    let dirs = verify_directories(root, &cleanup.directories)?;
    let artifacts = prepare_removal(root, &dirs, &cleanup.aliases, resuming)?;
    Ok(Some(PreparedProfileCleanup {
        dirs,
        pin: &cleanup.directories,
        artifacts,
    }))
}

pub(super) fn alias_json(root: &Path, a: &Alias) -> Value {
    let state = match unhex(&a.receipt) {
        Ok(digest) => aliases::state(root, &a.name, a.profile_id, a.surface, &digest),
        Err(_) => "drift",
    };
    json!({"name":a.name,"profile_id":a.profile_id.to_string(),"surface":a.surface,
        "path":super::display::path(&aliases::launcher_path(root,&a.name)),"state":state})
}
fn active(data: &store::Data) -> Result<(), Error> {
    store::require_v3(data)?;
    if data
        .profiles
        .iter()
        .any(|p| p.state != Some(ProfileState::Active))
    {
        return Err(Error::RecoveryRequired);
    }
    Ok(())
}

#[cfg(test)]
thread_local! { static FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
#[allow(
    dead_code,
    reason = "fault injection is driven by this module's unit tests"
)]
pub(super) fn set_fault(phase: Option<&'static str>) {
    FAULT.with(|f| f.set(phase));
}
fn fault(_phase: &str) -> Result<(), Error> {
    #[cfg(test)]
    if FAULT.with(|f| f.get() == Some(_phase)) {
        return Err(Error::RecoveryRequired);
    }
    Ok(())
}
// A simulated crash skips in-process cleanup, exactly like an abrupt exit.
fn crashed() -> bool {
    #[cfg(test)]
    return FAULT.with(|f| f.get().is_some());
    #[cfg(not(test))]
    false
}

impl Registry {
    /// Under the write lock: check the precondition first, then resume an interrupted
    /// alias transaction only for callers without a pinned revision.
    fn alias_mutation_lock(&self) -> Result<store::RegistryLock, Error> {
        let root = self.root();
        if self.expected.is_some() {
            if platform::inspect(&root.join("registry.lock"), ObjectKind::File)?.is_none() {
                let data = store::load_unchecked(root)?;
                store::precondition(&data, self.expected)?;
                if data.revision != 0 || crate::share::pending(root) {
                    return Err(Error::RecoveryRequired);
                }
                // No valid target or lock exists. A pinned alias operation creates neither.
                return Err(Error::AliasNotFound);
            }
            return store::admission_lock(root);
        }
        store::lock(root, Duration::from_secs(10))
    }

    fn alias_resumed(&self) -> Result<store::Data, Error> {
        let data = store::load_unchecked(self.root())?;
        store::precondition(&data, self.expected)?;
        if !crate::share::pending(self.root()) {
            return Ok(data);
        }
        if self.expected.is_some() {
            return Err(Error::RecoveryRequired);
        }
        recover(self.root(), &data)?;
        store::load(self.root())
    }

    pub fn alias_list(&self) -> Result<Value, Error> {
        let root = self.root();
        let _ancestors = platform::anchor(root)?.ancestors;
        let _fixed = store::fixed(root)?;
        if crate::share::pending(root) {
            let _lock = self.alias_mutation_lock()?;
            self.alias_resumed()?;
        }
        let (data, _lock) = store::snapshot(root, self.expected)?;
        self.observed.set(Some(data.revision));
        let mut list: Vec<_> = data.aliases.iter().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(json!({"aliases":list.into_iter().map(|a| alias_json(root, a)).collect::<Vec<_>>()}))
    }

    pub fn alias_add(
        &self,
        name: ProfileName,
        profile: &ProfileName,
        surface: Surface,
        search_path: Option<&OsStr>,
    ) -> Result<Value, Error> {
        self.alias_add_with(name, profile, surface, search_path, Source::current)
    }

    /// Lock, journal, exclusive artifacts, identity/hash validation, registry commit.
    pub(crate) fn alias_add_with(
        &self,
        name: ProfileName,
        profile: &ProfileName,
        surface: Surface,
        search_path: Option<&OsStr>,
        source: impl FnOnce(&Path) -> Result<Source, Error>,
    ) -> Result<Value, Error> {
        let root = self.root();
        let encoding = || {
            root.to_str().ok_or_else(|| Error::InvalidRoot {
                path: root.into(),
                reason: "alias receipts require a Unicode root path".into(),
            })
        };
        if self.expected.is_none() {
            encoding()?;
        }
        let _ancestors = platform::anchor(root)?.ancestors;
        let _lock = self.alias_mutation_lock()?;
        let _fixed = store::fixed(root)?;
        let data = self.alias_resumed()?;
        active(&data)?;
        encoding()?;
        let profile_id = data
            .profiles
            .iter()
            .find(|p| &p.name == profile)
            .ok_or_else(|| Error::NotFound {
                name: profile.clone(),
            })?
            .id;
        if ProfileName::parse_new(name.as_str()).is_err()
            || data.aliases.iter().any(|a| a.name == name)
        {
            return Err(Error::Conflict);
        }
        aliases::check_collision(root, &name, search_path)?;
        let source = source(root)?;
        if source.path.to_str().is_none() {
            return Err(Error::NamespaceUnverified);
        }
        let bin = root.join("bin");
        if paths::mkdir(&bin)? {
            paths::sync(root)?;
        }
        let dirs = Directories::open(root)?;
        let mut j = Journal {
            version: 2,
            kind: KIND.into(),
            before_registry: utf8(store::serialize(&data)?)?,
            after_registry: None,
            directories: dirs.pin(),
            step: Step::Add {
                name: name.clone(),
                staged: None,
            },
        };
        write_journal(root, &j)?;
        let result = (|| {
            fault("journal")?;
            let [stage_launcher, stage_receipt] = staged_paths(root, &name);
            let launcher = dirs.create(&stage_launcher)?;
            let launcher_identity = launcher.identity.clone();
            fault("launcher")?;
            aliases::fill_launcher(launcher, &source)?;
            let receipt = Receipt::for_launcher(
                root,
                &dirs,
                name.clone(),
                profile_id,
                surface,
                &source,
                &launcher_identity,
            )?
            .encode()?;
            let mut receipt_file = dirs.create(&stage_receipt)?;
            receipt_file
                .file
                .write_all(&receipt)
                .and_then(|()| receipt_file.file.sync_all())
                .map_err(|_| Error::AliasInvalid)?;
            let receipt_identity = receipt_file.identity;
            drop(receipt_file.file);
            dirs.sync()?;
            let mut after = copy(&data)?;
            let digest = aliases::digest(&receipt);
            after.aliases.push(Alias {
                name: name.clone(),
                profile_id,
                surface,
                receipt: hex(&digest),
            });
            after.aliases.sort_by(|a, b| a.name.cmp(&b.name));
            after.revision = after.revision.checked_add(1).ok_or(Error::Exhausted)?;
            let after_bytes = store::serialize(&after)?;
            let staged = Box::new(Staged {
                launcher: Artifact {
                    identity: launcher_identity,
                    digest: source.sha256,
                },
                receipt: Artifact {
                    identity: receipt_identity,
                    digest,
                },
            });
            j.after_registry = Some(utf8(after_bytes.clone())?);
            j.step = Step::Add {
                name: name.clone(),
                staged: Some(staged),
            };
            verify_directories(root, &j.directories)?;
            write_journal(root, &j)?;
            fault("prepared")?;
            let Step::Add {
                staged: Some(staged),
                ..
            } = &j.step
            else {
                unreachable!()
            };
            dirs.publish(
                &stage_launcher,
                &aliases::launcher_path(root, &name),
                &staged.launcher,
            )?;
            fault("published_launcher")?;
            verify_directories(root, &j.directories)?;
            dirs.publish(
                &stage_receipt,
                &aliases::receipt_path(root, &name),
                &staged.receipt,
            )?;
            dirs.sync()?;
            fault("receipt")?;
            verify_directories(root, &j.directories)?;
            if ownership(
                root,
                &dirs,
                after
                    .aliases
                    .iter()
                    .find(|a| a.name == name)
                    .ok_or(Error::AliasInvalid)?,
            )? != (Owned {
                launcher: Some(staged.launcher.clone()),
                receipt: Some(staged.receipt.clone()),
            }) {
                return Err(Error::AliasInvalid);
            }
            store::persist(root, &after_bytes, &mut false, &mut false)?;
            fault("registry")?;
            Ok(after.revision)
        })();
        let revision = match result {
            Ok(revision) => revision,
            Err(error) => {
                if !crashed() {
                    let _ = store::load_unchecked(root).and_then(|current| recover(root, &current));
                }
                return Err(error);
            }
        };
        finish(root)?;
        self.observed.set(Some(revision));
        Ok(json!({"changed":true,"profile_id":profile_id.to_string(),"name":name,"warnings":[]}))
    }

    pub fn alias_remove(&self, name: &ProfileName) -> Result<Value, Error> {
        let root = self.root();
        let _ancestors = platform::anchor(root)?.ancestors;
        let _lock = self.alias_mutation_lock()?;
        let _fixed = store::fixed(root)?;
        let data = self.alias_resumed()?;
        active(&data)?;
        let alias = data
            .aliases
            .iter()
            .find(|a| &a.name == name)
            .cloned()
            .ok_or(Error::AliasNotFound)?;
        let after = remove_aliases(root, data, |a| a.name == alias.name)?;
        self.observed.set(Some(after.revision));
        Ok(
            json!({"changed":true,"profile_id":alias.profile_id.to_string(),"name":alias.name,"warnings":[]}),
        )
    }

    /// Start with the adjacent receipt, then bind it to the committed mapping.
    pub(crate) fn alias_target(
        &self,
        receipt: Receipt,
        digest: [u8; 32],
        executable: &Path,
        argv0: &OsStr,
    ) -> Result<(ProfileName, AliasPin, Vec<CheckedObject>), Error> {
        let _lock = store::admission_lock(self.root())?;
        let data = store::load(self.root())?;
        let pin = AliasPin {
            receipt,
            digest,
            revision: data.revision,
        };
        let held = pin.check(self.root(), &data)?;
        if aliases::invocation(argv0)?.as_ref() != Some(pin.receipt.name()) {
            return Err(Error::AliasInvalid);
        }
        let profile = data
            .profiles
            .iter()
            .find(|p| p.id == pin.profile_id())
            .ok_or(Error::AliasInvalid)?;
        if !aliases::same_launcher(executable, self.root(), pin.receipt.name())? {
            return Err(Error::AliasInvalid);
        }
        Ok((profile.name.clone(), pin, held))
    }
}

#[derive(Clone)]
pub(crate) struct AliasPin {
    receipt: Receipt,
    digest: [u8; 32],
    pub(crate) revision: u64,
}
impl AliasPin {
    pub(crate) fn profile_id(&self) -> ProfileId {
        self.receipt.profile_id()
    }
    pub(super) fn check(
        &self,
        root: &Path,
        data: &store::Data,
    ) -> Result<Vec<CheckedObject>, Error> {
        store::require_v3(data)?;
        let alias = data
            .aliases
            .iter()
            .find(|a| &a.name == self.receipt.name())
            .ok_or(Error::AliasInvalid)?;
        if self.revision != data.revision
            || alias.profile_id != self.profile_id()
            || alias.surface != self.receipt.surface()
            || unhex(&alias.receipt)? != self.digest
        {
            return Err(Error::AliasInvalid);
        }
        self.receipt.check_mapping(root, &self.digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Provider;
    use std::{io::Write, path::PathBuf};
    #[cfg(unix)]
    type Temp = tempfile::TempDir;
    #[cfg(windows)]
    type Temp = crate::platform::fixture_private::PrivateTempDir;
    fn name(s: &str) -> ProfileName {
        ProfileName::parse_new(s).unwrap()
    }
    struct F {
        _temp: Temp,
        r: Registry,
        source: PathBuf,
    }
    // A synthetic "Larp image": native magic only, never executed by these tests.
    fn fixture() -> F {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        #[cfg(unix)]
        let temp = {
            use std::os::unix::fs::PermissionsExt;
            tempfile::Builder::new()
                .prefix("stage05-")
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(&parent)
                .unwrap()
        };
        #[cfg(windows)]
        let temp = crate::platform::fixture_private::tempdir(&parent).unwrap();
        let install = temp.path().join("install");
        platform::create_directory(&install).unwrap();
        let source = install.join(if cfg!(windows) { "larp.exe" } else { "larp" });
        let mut file = platform::create_file(&source).unwrap().file;
        let magic: &[u8] = if cfg!(target_os = "macos") {
            &[0xcf, 0xfa, 0xed, 0xfe]
        } else if cfg!(windows) {
            b"MZ\0\0"
        } else {
            b"\x7fELF"
        };
        file.write_all(magic).unwrap();
        file.write_all(b" synthetic larp image\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        drop(file);
        let r = Registry::at(temp.path().join("root")).unwrap();
        r.add(name("claude-work"), Provider::Codex).unwrap();
        r.add(name("codex-45"), Provider::Claude).unwrap();
        F {
            _temp: temp,
            r,
            source,
        }
    }
    impl F {
        fn add(&self, alias: &str, profile: &str) -> Result<Value, Error> {
            self.add_on(&self.r, alias, profile)
        }
        fn add_on(&self, r: &Registry, alias: &str, profile: &str) -> Result<Value, Error> {
            r.alias_add_with(name(alias), &name(profile), Surface::Cli, None, |root| {
                Ok(aliases::test_source(&self.source, root))
            })
        }
        fn launcher(&self, alias: &str) -> PathBuf {
            aliases::launcher_path(self.r.root(), &name(alias))
        }
        fn receipt(&self, alias: &str) -> PathBuf {
            aliases::receipt_path(self.r.root(), &name(alias))
        }
        fn pending(&self) -> bool {
            crate::share::pending(self.r.root())
        }
        fn states(&self) -> Vec<String> {
            self.r.alias_list().unwrap()["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    format!(
                        "{}:{}",
                        a["name"].as_str().unwrap(),
                        a["state"].as_str().unwrap()
                    )
                })
                .collect()
        }
        fn target(&self, alias: &str) -> Result<ProfileName, Error> {
            let launcher = self.launcher(alias);
            let (receipt, digest) = Receipt::adjacent(&launcher, &name(alias))?;
            self.r
                .alias_target(receipt, digest, &launcher, OsStr::new(alias))
                .map(|(profile, _, _)| profile)
        }
    }
    fn rewrite(path: &Path, bytes: &[u8]) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap();
        file.write_all(bytes).unwrap();
    }

    #[test]
    fn stage05_transaction_add_list_remove_rename_forget() {
        let f = fixture();
        let stale = Registry::at(f.r.root().into()).unwrap().expecting(Some(1));
        assert!(matches!(
            f.add_on(&stale, "codex-45", "claude-work"),
            Err(Error::Conflict)
        ));
        assert!(!f.launcher("codex-45").exists());
        assert!(matches!(f.add("x", "missing"), Err(Error::NotFound { .. })));
        assert_eq!(
            f.add("codex-45", "claude-work").unwrap(),
            json!({"changed":true,"profile_id":"1","name":"codex-45","warnings":[]})
        );
        assert_eq!(f.r.revision(), Some(3));
        assert!(!f.pending() && f.launcher("codex-45").exists() && f.receipt("codex-45").exists());
        assert!(matches!(
            f.add("codex-45", "codex-45"),
            Err(Error::Conflict)
        ));
        assert_eq!(f.states(), ["codex-45:ok"]);
        assert_eq!(f.target("codex-45").unwrap(), name("claude-work"));
        f.r.rename(&name("claude-work"), name("claude-x")).unwrap();
        assert_eq!(f.target("codex-45").unwrap(), name("claude-x"));
        assert_eq!(f.r.alias_list().unwrap()["aliases"][0]["profile_id"], "1");
        assert!(matches!(
            f.r.forget(&name("claude-x")),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            f.r.alias_remove(&name("other")),
            Err(Error::AliasNotFound)
        ));
        assert_eq!(
            f.r.alias_remove(&name("codex-45")).unwrap(),
            json!({"changed":true,"profile_id":"1","name":"codex-45","warnings":[]})
        );
        assert!(!f.launcher("codex-45").exists() && !f.receipt("codex-45").exists());
        assert!(f.states().is_empty());
        f.r.forget(&name("claude-x")).unwrap();
    }

    #[test]
    fn stage05_transaction_interrupted_add_rolls_back_or_finishes() {
        for phase in [
            "journal",
            "prepared",
            "published_launcher",
            "receipt",
            "registry",
        ] {
            let f = fixture();
            set_fault(Some(phase));
            assert!(f.add("codex-45", "claude-work").is_err(), "{phase}");
            set_fault(None);
            assert!(f.pending(), "{phase}");
            // A pending receipt never dispatches and blocks every registry consumer.
            assert!(matches!(f.r.list(), Err(Error::RecoveryRequired)));
            if f.receipt("codex-45").exists() {
                assert!(f.target("codex-45").is_err(), "{phase}");
            }
            let states = f.states();
            assert!(!f.pending(), "{phase}");
            if phase == "registry" {
                assert_eq!(states, ["codex-45:ok"]);
                assert_eq!(f.target("codex-45").unwrap(), name("claude-work"));
            } else {
                assert!(states.is_empty(), "{phase}");
                assert!(!f.launcher("codex-45").exists(), "{phase}");
                assert!(!f.receipt("codex-45").exists(), "{phase}");
                f.add("codex-45", "claude-work").unwrap();
            }
        }
    }

    #[test]
    fn stage05_transaction_interrupted_remove_rolls_forward() {
        for phase in ["journal", "artifact", "registry"] {
            let f = fixture();
            f.add("codex-45", "claude-work").unwrap();
            set_fault(Some(phase));
            assert!(f.r.alias_remove(&name("codex-45")).is_err(), "{phase}");
            set_fault(None);
            assert!(f.pending(), "{phase}");
            assert!(f.target("codex-45").is_err(), "{phase}");
            assert!(f.states().is_empty(), "{phase}");
            assert!(!f.pending());
            assert!(!f.launcher("codex-45").exists() && !f.receipt("codex-45").exists());
        }
    }

    #[test]
    fn stage05_transaction_drift_refuses_missing_retires() {
        let f = fixture();
        f.add("codex-45", "claude-work").unwrap();
        let launcher = f.launcher("codex-45");
        let receipt = f.receipt("codex-45");
        let original = fs::read(&launcher).unwrap();
        let mut changed = original.clone();
        changed.push(b'!');
        rewrite(&launcher, &changed);
        assert_eq!(f.states(), ["codex-45:drift"]);
        assert!(f.target("codex-45").is_err());
        assert!(matches!(
            f.r.alias_remove(&name("codex-45")),
            Err(Error::AliasInvalid)
        ));
        assert!(launcher.exists() && !f.pending());
        rewrite(&launcher, &original);
        assert_eq!(f.states(), ["codex-45:ok"]);
        // Forged receipt bytes never match the registry-owned digest.
        let bytes = fs::read(&receipt).unwrap();
        let mut forged: Value = serde_json::from_slice(&bytes).unwrap();
        forged["profile_id"] = 2.into();
        rewrite(&receipt, &serde_json::to_vec(&forged).unwrap());
        assert_eq!(f.states(), ["codex-45:drift"]);
        assert!(f.target("codex-45").is_err());
        assert!(f.r.alias_remove(&name("codex-45")).is_err());
        // A launcher without its receipt is never deleted.
        fs::remove_file(&receipt).unwrap();
        assert_eq!(f.states(), ["codex-45:drift"]);
        assert!(f.r.alias_remove(&name("codex-45")).is_err());
        assert!(launcher.exists());
        rewrite_new(&receipt, &bytes);
        assert_eq!(f.states(), ["codex-45:ok"]);
        // Missing launcher: the owned receipt and mapping retire together.
        fs::remove_file(&launcher).unwrap();
        assert_eq!(f.states(), ["codex-45:missing"]);
        f.r.alias_remove(&name("codex-45")).unwrap();
        assert!(!receipt.exists());
        // Both missing: absence is established, the mapping retires.
        f.add("codex-45", "claude-work").unwrap();
        fs::remove_file(&launcher).unwrap();
        fs::remove_file(&receipt).unwrap();
        f.r.alias_remove(&name("codex-45")).unwrap();
        assert!(f.states().is_empty());
    }
    fn rewrite_new(path: &Path, bytes: &[u8]) {
        platform::create_file(path)
            .unwrap()
            .file
            .write_all(bytes)
            .unwrap();
    }

    #[test]
    fn stage05_transaction_recovery_refuses_drift_and_foreign_journals() {
        let f = fixture();
        f.add("codex-45", "claude-work").unwrap();
        let launcher = f.launcher("codex-45");
        set_fault(Some("journal"));
        assert!(f.r.alias_remove(&name("codex-45")).is_err());
        set_fault(None);
        // Identical bytes at a new identity are drift: recovery refuses and stays pending.
        // (allocate the replacement first so the filesystem cannot reuse the identity)
        let replacement = f.r.root().join("bin").join("replacement");
        rewrite_new(&replacement, &fs::read(&launcher).unwrap());
        fs::remove_file(&launcher).unwrap();
        fs::rename(&replacement, &launcher).unwrap();
        assert!(matches!(f.r.alias_list(), Err(Error::AliasInvalid)));
        assert!(f.pending() && launcher.exists());
        fs::remove_file(&launcher).unwrap();
        assert!(f.states().is_empty());
        assert!(!f.pending() && !f.receipt("codex-45").exists());
        // Another owner's journal is never adopted or removed by aliases.
        let journal = f.r.root().join(JOURNAL);
        rewrite_new(&journal, br#"{"version":1,"kind":"skills"}"#);
        assert!(matches!(
            f.add("codex-45", "claude-work"),
            Err(Error::RecoveryRequired)
        ));
        assert!(matches!(f.r.alias_list(), Err(Error::RecoveryRequired)));
        assert_eq!(
            fs::read(&journal).unwrap(),
            br#"{"version":1,"kind":"skills"}"#
        );
        fs::remove_file(&journal).unwrap();
        // A forged add journal naming a registered alias never deletes its launcher.
        f.add("codex-45", "claude-work").unwrap();
        let current = store::load(f.r.root()).unwrap();
        write_journal(
            f.r.root(),
            &Journal {
                version: 2,
                kind: KIND.into(),
                before_registry: utf8(store::serialize(&current).unwrap()).unwrap(),
                after_registry: None,
                directories: Directories::open(f.r.root()).unwrap().pin(),
                step: Step::Add {
                    name: name("codex-45"),
                    staged: None,
                },
            },
        )
        .unwrap();
        assert!(matches!(f.r.alias_list(), Err(Error::RecoveryRequired)));
        assert!(launcher.exists() && f.pending());
        finish(f.r.root()).unwrap();
        assert_eq!(f.states(), ["codex-45:ok"]);
    }

    #[test]
    fn stage05_transaction_unjournaled_stage_and_changed_receipt_stay_pending() {
        let f = fixture();
        set_fault(Some("launcher"));
        assert!(f.add("codex-45", "claude-work").is_err());
        set_fault(None);
        let [stage, _] = staged_paths(f.r.root(), &name("codex-45"));
        assert!(matches!(f.r.alias_list(), Err(Error::AliasInvalid)));
        assert!(stage.exists() && f.pending());
        assert!(!f.launcher("codex-45").exists());
        fs::remove_file(stage).unwrap();
        assert!(f.states().is_empty());

        set_fault(Some("receipt"));
        assert!(f.add("codex-45", "claude-work").is_err());
        set_fault(None);
        let receipt = f.receipt("codex-45");
        rewrite(&receipt, b"");
        assert!(matches!(f.r.alias_list(), Err(Error::AliasInvalid)));
        assert!(receipt.exists() && f.launcher("codex-45").exists() && f.pending());
    }

    #[test]
    fn stage05_transaction_pinned_revision_does_not_replay_remove() {
        let f = fixture();
        f.add("codex-45", "claude-work").unwrap();
        set_fault(Some("journal"));
        assert!(f.r.alias_remove(&name("codex-45")).is_err());
        set_fault(None);
        for (revision, stale) in [(2, true), (3, false)] {
            let pinned = Registry::at(f.r.root().into())
                .unwrap()
                .expecting(Some(revision));
            let error = pinned.alias_remove(&name("codex-45")).unwrap_err();
            assert!(if stale {
                matches!(error, Error::Conflict)
            } else {
                matches!(error, Error::RecoveryRequired)
            });
            assert!(
                f.pending() && f.launcher("codex-45").exists() && f.receipt("codex-45").exists()
            );
        }
        assert!(f.states().is_empty());
    }

    #[test]
    fn stage05_transaction_replaced_bin_refuses_even_with_original_artifacts() {
        let f = fixture();
        f.add("codex-45", "claude-work").unwrap();
        let bin = f.r.root().join("bin");
        let original = f.r.root().join("previous-bin");
        fs::rename(&bin, &original).unwrap();
        platform::create_directory(&bin).unwrap();
        for path in [f.launcher("codex-45"), f.receipt("codex-45")] {
            fs::rename(original.join(path.file_name().unwrap()), &path).unwrap();
        }
        assert!(matches!(
            f.r.alias_remove(&name("codex-45")),
            Err(Error::AliasInvalid)
        ));
        assert!(f.launcher("codex-45").exists() && f.receipt("codex-45").exists());
    }

    #[cfg(unix)]
    #[test]
    fn stage05_transaction_managed_bin_requires_a_real_directory() {
        use std::os::unix::fs::symlink;
        let f = fixture();
        let separate = f._temp.path().join("separate");
        platform::create_directory(&separate).unwrap();
        symlink(&separate, f.r.root().join("bin")).unwrap();
        assert!(Directories::open(f.r.root()).is_err());
        assert!(fs::read_dir(&separate).unwrap().next().is_none());
    }

    #[test]
    fn stage05_dispatch_rechecks_pin_even_at_same_revision() {
        let f = fixture();
        f.add("codex-45", "claude-work").unwrap();
        let launcher = f.launcher("codex-45");
        let (receipt, digest) = Receipt::adjacent(&launcher, &name("codex-45")).unwrap();
        let (_, pin, held) =
            f.r.alias_target(receipt, digest, &launcher, OsStr::new("codex-45"))
                .unwrap();
        drop(held);
        let data = store::load(f.r.root()).unwrap();
        pin.check(f.r.root(), &data).unwrap();
        let mut changed = copy(&data).unwrap();
        changed.aliases[0].profile_id = ProfileId::new(2);
        assert!(pin.check(f.r.root(), &changed).is_err());
        changed.aliases[0].profile_id = ProfileId::new(1);
        changed.aliases[0].receipt = "0".repeat(64);
        assert!(pin.check(f.r.root(), &changed).is_err());
        rewrite(&f.receipt("codex-45"), b"{}");
        assert!(pin.check(f.r.root(), &data).is_err());
    }

    #[test]
    fn stage05_transaction_profile_cleanup_entry() {
        let f = fixture();
        f.add("codex-45", "claude-work").unwrap();
        f.add("claude-alt", "claude-work").unwrap();
        f.add("other", "codex-45").unwrap();
        let root = f.r.root();
        let lock = store::lock(root, Duration::from_secs(10)).unwrap();
        let data = store::load(root).unwrap();
        let id = ProfileId::new(1);
        let plan = prepare_profile_cleanup(root, &data, id).unwrap().unwrap();
        assert_eq!(plan.aliases.len(), 2);
        let prepared = validate_profile_cleanup(root, &data, id, Some(&plan), false)
            .unwrap()
            .unwrap();
        assert_eq!(prepared.artifacts.len(), 2);
        drop(prepared);
        drop(lock);
        assert!(f.launcher("codex-45").exists() && f.launcher("claude-alt").exists());
        assert_eq!(store::load(root).unwrap().revision, data.revision);
    }
}
