use super::{
    Error, Profile, ProfileId, ProfileName, ProfileState, Registry, alias, mcp, paths, skills,
    store,
};
use crate::{
    launch::admission::acquire_auth_guard,
    platform::{self, FileIdentity, ObjectKind, removal::Profiles},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

mod activity;
mod migration;
#[cfg(all(test, unix))]
mod tests;

const JOURNAL: &str = "share-journal.json";
const STAGING: &str = "share-journal.json.lifecycle.tmp";
const JOURNAL_LIMIT: usize = 6 * store::LIMIT;
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Journal {
    Delete {
        version: u8,
        transaction: Box<Deletion>,
    },
    Migrate {
        version: u8,
        transaction: migration::Migration,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deletion {
    id: ProfileId,
    name: ProfileName,
    revision: u64,
    before: [u8; 32],
    tombstone: [u8; 32],
    after: [u8; 32],
    profiles: FileIdentity,
    home: FileIdentity,
    references: ReferenceCleanup,
    step: DeleteStep,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceCleanup {
    aliases: Option<alias::ProfileCleanup>,
    skills: Option<skills::ProfileCleanup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mcp: Option<mcp::ProfileCleanup>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeleteStep {
    Prepared,
    ReferencesRemoved,
    FolderRemoved,
}

fn read(root: &Path) -> Result<Option<Journal>, Error> {
    let Some((bytes, _)) = journal_bytes(root, JOURNAL)? else {
        return Ok(None);
    };
    match serde_json::from_slice(&bytes) {
        Ok(j @ (Journal::Delete { version: 1, .. } | Journal::Migrate { version: 1, .. })) => {
            Ok(Some(j))
        }
        _ => Err(Error::RecoveryRequired),
    }
}

fn journal_bytes(
    root: &Path,
    name: &str,
) -> Result<Option<(Vec<u8>, platform::CheckedObject)>, Error> {
    let path = root.join(name);
    let Some(file) = platform::inspect(&path, ObjectKind::File)? else {
        return Ok(None);
    };
    let opened = paths::read_options()
        .open(&path)
        .map_err(|e| paths::io("open", &path, e))?;
    let opened = platform::verify_opened(opened, &path, ObjectKind::File)?;
    if opened.identity != file.identity {
        return Err(Error::Drift);
    }
    platform::single_link(&opened.file)?;
    let mut bytes = Vec::new();
    (&opened.file)
        .take(JOURNAL_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| paths::io("read", &path, e))?;
    if bytes.len() > JOURNAL_LIMIT {
        return Err(Error::RecoveryRequired);
    }
    Ok(Some((bytes, opened)))
}

fn write(root: &Path, journal: &Journal) -> Result<(), Error> {
    let bytes = serde_json::to_vec(journal).map_err(|_| Error::RecoveryRequired)?;
    if bytes.len() > JOURNAL_LIMIT {
        return Err(Error::TooLarge);
    }
    let temp = root.join(STAGING);
    let identity = match platform::create_file(&temp) {
        Ok(mut object) => {
            object
                .file
                .write_all(&bytes)
                .and_then(|_| object.file.sync_all())
                .map_err(|e| paths::io("write", &temp, e))?;
            fault("lifecycle staging")?;
            object.identity
        }
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            let (staged, object) = journal_bytes(root, STAGING)?.ok_or(Error::Drift)?;
            if staged != bytes || !same_transaction(read(root)?, journal) {
                return Err(Error::Drift);
            }
            object
                .file
                .sync_all()
                .map_err(|e| paths::io("sync", &temp, e))?;
            object.identity
        }
        Err(e) => return Err(e),
    };
    if platform::inspect(&temp, ObjectKind::File)?.map(|o| o.identity) != Some(identity) {
        return Err(Error::Drift);
    }
    store::replace(&temp, &root.join(JOURNAL))?;
    paths::sync(root)
}

fn same_transaction(current: Option<Journal>, next: &Journal) -> bool {
    match (current, next) {
        (
            Some(Journal::Delete {
                transaction: active,
                ..
            }),
            Journal::Delete {
                transaction: next, ..
            },
        ) => {
            active.id == next.id
                && active.before == next.before
                && active.after == next.after
                && active.home == next.home
        }
        (
            Some(Journal::Migrate {
                transaction: active,
                ..
            }),
            Journal::Migrate {
                transaction: next, ..
            },
        ) => active.same_transaction(next),
        _ => false,
    }
}

fn finish(root: &Path) -> Result<(), Error> {
    let path = root.join(JOURNAL);
    fs::remove_file(&path).map_err(|e| paths::io("remove", &path, e))?;
    paths::sync(root)
}

fn fingerprint(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn locked(root: &Path, expected: Option<u64>) -> Result<(store::RegistryLock, store::Data), Error> {
    let lock = store::lock(root, LOCK_TIMEOUT)?;
    let data = store::load_unchecked(root)?;
    store::precondition(&data, expected)?;
    Ok((lock, data))
}

fn initial(
    root: &Path,
    expected: Option<u64>,
) -> Result<(Option<store::RegistryLock>, store::Data), Error> {
    if platform::inspect(&root.join("registry.lock"), ObjectKind::File)?.is_none() {
        let data = store::load_unchecked(root)?;
        store::precondition(&data, expected)?;
        if data.version == 3 && data.revision == 0 && !crate::share::pending(root) {
            return Ok((None, data));
        }
    }
    locked(root, expected).map(|(lock, data)| (Some(lock), data))
}

fn selected<'a>(data: &'a store::Data, name: &ProfileName) -> Result<&'a Profile, Error> {
    data.profiles
        .iter()
        .find(|p| &p.name == name)
        .ok_or_else(|| Error::NotFound { name: name.clone() })
}

fn deletion_id(
    data: &store::Data,
    journal: Option<Journal>,
    name: &ProfileName,
    recover: bool,
) -> Result<ProfileId, Error> {
    match journal {
        Some(Journal::Delete {
            transaction: tx, ..
        }) if tx.name == *name => {
            store::require_v3(data)?;
            if tx.id.get() == 0 || tx.id.get() >= data.next_id {
                return Err(Error::RecoveryRequired);
            }
            if let Ok(p) = selected(data, name) {
                if p.id != tx.id {
                    return Err(Error::Conflict);
                }
            } else if data.revision != tx.revision.checked_add(2).ok_or(Error::Exhausted)?
                || tx.step != DeleteStep::FolderRemoved
                || fingerprint(&store::serialize(data)?) != tx.after
            {
                return Err(Error::RecoveryRequired);
            }
            Ok(tx.id)
        }
        Some(_) => Err(Error::RecoveryRequired),
        None if recover => Err(Error::RecoveryRequired),
        None => {
            store::require_v3(data)?;
            let profile = selected(data, name)?;
            if profile.state != Some(ProfileState::Active)
                || data
                    .profiles
                    .iter()
                    .any(|p| p.state == Some(ProfileState::Deleting))
            {
                return Err(Error::RecoveryRequired);
            }
            Ok(profile.id)
        }
    }
}

fn mark(data: &mut store::Data, id: ProfileId) -> Result<(), Error> {
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or(Error::RecoveryRequired)?;
    profile.state = Some(ProfileState::Deleting);
    profile.deletion_transaction = Some(format!("delete-{id}"));
    data.revision = data.revision.checked_add(1).ok_or(Error::Exhausted)?;
    Ok(())
}

fn retire(data: &mut store::Data, id: ProfileId) -> Result<(), Error> {
    data.aliases.retain(|a| a.profile_id != id);
    data.profiles.retain(|p| p.id != id);
    data.revision = data.revision.checked_add(1).ok_or(Error::Exhausted)?;
    Ok(())
}

fn persist(root: &Path, bytes: &[u8]) -> Result<(), Error> {
    store::persist(root, bytes, &mut false, &mut false)
}

impl Registry {
    pub(crate) fn delete_profile(
        &self,
        name: ProfileName,
        recover: bool,
    ) -> Result<ProfileId, Error> {
        let root = self.root();
        let _ancestors = platform::anchor(root)?.ancestors;
        let (id, snapshot) = {
            let (_lock, data) = initial(root, self.expected)?;
            let id = deletion_id(&data, read(root)?, &name, recover)?;
            (id, fingerprint(&store::serialize(&data)?))
        };
        let _guard = acquire_auth_guard(root, id)?;
        let role = id.to_string();
        #[cfg(target_os = "macos")]
        activity::require_quiet(&[root.join("profiles").join(&role).join("desktop")])?;
        let (_lock, mut data) = locked(root, self.expected)?;
        let pending = read(root)?;
        if fingerprint(&store::serialize(&data)?) != snapshot
            || pending.as_ref().is_some_and(|j| !matches!(j, Journal::Delete { transaction, .. } if transaction.id == id && transaction.name == name))
        {
            return Err(Error::Conflict);
        }
        let profiles = Profiles::open(root)?;
        let mut journal = match pending {
            Some(j) => j,
            None if !recover => {
                if deletion_id(&data, None, &name, false)? != id {
                    return Err(Error::Conflict);
                }
                let home = profiles.home(&role, None, false)?.ok_or(Error::Drift)?;
                quiet(root, &data, selected(&data, &name)?, false)?;
                let aliases = alias::prepare_profile_cleanup(root, &data, id)?;
                let skills = skills::prepare_profile_cleanup(root, &data, id)?;
                skills::validate_profile_cleanup(root, &data, id, skills.as_ref(), false)?;
                let mcp = mcp::prepare_profile_cleanup(root, &data, id)?;
                let revision = data.revision;
                mark(&mut data, id)?;
                let tombstone = fingerprint(&store::serialize(&data)?);
                retire(&mut data, id)?;
                let after = fingerprint(&store::serialize(&data)?);
                data = store::load_unchecked(root)?;
                if fingerprint(&store::serialize(&data)?) != snapshot {
                    return Err(Error::Conflict);
                }
                let j = Journal::Delete {
                    version: 1,
                    transaction: Box::new(Deletion {
                        id,
                        name: name.clone(),
                        revision,
                        before: snapshot,
                        tombstone,
                        after,
                        profiles: profiles.identity()?,
                        home,
                        references: ReferenceCleanup {
                            aliases,
                            skills,
                            mcp,
                        },
                        step: DeleteStep::Prepared,
                    }),
                };
                write(root, &j)?;
                fault("delete journal")?;
                j
            }
            None => return Err(Error::RecoveryRequired),
        };
        let revision = apply(root, &profiles, &role, data, &mut journal)?;
        self.observed.set(Some(revision));
        Ok(id)
    }
}

fn apply(
    root: &Path,
    profiles: &Profiles,
    role: &str,
    mut data: store::Data,
    journal: &mut Journal,
) -> Result<u64, Error> {
    store::require_v3(&data)?;
    let Journal::Delete {
        transaction: tx, ..
    } = journal
    else {
        return Err(Error::RecoveryRequired);
    };
    let revision = tx.revision.checked_add(2).ok_or(Error::Exhausted)?;
    let id = tx.id;
    let after = tx.after;
    if profiles.identity()? != tx.profiles || id.get() >= data.next_id {
        return Err(Error::Drift);
    }
    let current = fingerprint(&store::serialize(&data)?);
    let home = profiles.home(role, None, false)?;
    if data.revision == revision && current == after && tx.step == DeleteStep::FolderRemoved {
        if home.is_some()
            || data
                .profiles
                .iter()
                .any(|p| p.id == id || p.name == tx.name)
        {
            return Err(Error::Drift);
        }
        if let Some(cleanup) = &tx.references.mcp {
            cleanup.verify_removed(root, id)?;
        }
        if mcp::has_profile_references(root, &data, id)? {
            return Err(Error::Drift);
        }
        paths::sync(root)?;
        finish(root)?;
        return Ok(revision);
    }
    if home.as_ref().is_some_and(|found| found != &tx.home)
        || (tx.step == DeleteStep::FolderRemoved && home.is_some())
        || (tx.step == DeleteStep::Prepared && home.is_none())
    {
        return Err(Error::Drift);
    }
    let p = selected(&data, &tx.name)?;
    if p.id != id {
        return Err(Error::Conflict);
    }
    let publish = if current == tx.before
        && data.revision == tx.revision
        && tx.step == DeleteStep::Prepared
        && p.state == Some(ProfileState::Active)
    {
        quiet(root, &data, p, false)?;
        mark(&mut data, id)?;
        let bytes = store::serialize(&data)?;
        if fingerprint(&bytes) != tx.tombstone {
            return Err(Error::RecoveryRequired);
        }
        Some(bytes)
    } else if current == tx.tombstone
        && data.revision == revision - 1
        && p.state == Some(ProfileState::Deleting)
    {
        if home.is_some() {
            quiet(root, &data, p, true)?;
        }
        None
    } else {
        return Err(Error::Conflict);
    };
    let resuming = publish.is_none();
    let aliases =
        alias::validate_profile_cleanup(root, &data, id, tx.references.aliases.as_ref(), resuming)?;
    let skills =
        skills::validate_profile_cleanup(root, &data, id, tx.references.skills.as_ref(), resuming)?;
    mcp::validate_profile_cleanup(root, &data, id, tx.references.mcp.as_ref(), resuming)?;
    retire(&mut data, id)?;
    let bytes = store::serialize(&data)?;
    if fingerprint(&bytes) != after {
        return Err(Error::RecoveryRequired);
    }
    if let Some(tombstone) = publish {
        persist(root, &tombstone)?;
        fault("delete tombstone")?;
    } else {
        // A prior commit may have replaced registry.json without syncing its parent.
        paths::sync(root)?;
    }
    if tx.step == DeleteStep::Prepared {
        if let Some(aliases) = aliases {
            aliases.apply(root)?;
        }
        fault("delete aliases")?;
        skills.apply(root)?;
        fault("delete skills")?;
        if let Some(cleanup) = &tx.references.mcp {
            mcp::apply_profile_cleanup(root, cleanup)?;
            fault("delete mcp")?;
            cleanup.verify_removed(root, id)?;
        }
        tx.step = DeleteStep::ReferencesRemoved;
        write(root, journal)?;
        fault("delete references")?;
    } else {
        if let Some(aliases) = aliases {
            aliases.verify_removed()?;
        }
        skills.verify_removed()?;
        if let Some(cleanup) = &tx.references.mcp {
            cleanup.verify_removed(root, id)?;
        }
    }
    let Journal::Delete {
        transaction: tx, ..
    } = journal
    else {
        return Err(Error::RecoveryRequired);
    };
    if tx.step == DeleteStep::ReferencesRemoved {
        profiles.remove(role, &tx.home)?;
        tx.step = DeleteStep::FolderRemoved;
        write(root, journal)?;
        fault("delete folder")?;
    }
    let Journal::Delete {
        transaction: tx, ..
    } = journal
    else {
        return Err(Error::RecoveryRequired);
    };
    if let Some(cleanup) = &tx.references.mcp {
        cleanup.verify_removed(root, id)?;
    }
    persist(root, &bytes)?;
    fault("delete registry")?;
    finish(root)?;
    Ok(revision)
}

fn quiet(root: &Path, data: &store::Data, profile: &Profile, partial: bool) -> Result<(), Error> {
    #[cfg(target_os = "macos")]
    {
        use crate::desktop::{
            self, Activity, CheckedDirectory, CheckedSnapshot, ValidatedDesktopTarget,
        };
        let directory = |path: std::path::PathBuf| {
            let ancestry = platform::anchor(&path)?;
            let object = platform::inspect(&path, ObjectKind::Directory)?.ok_or(Error::AuthBusy)?;
            Ok::<_, Error>(CheckedDirectory {
                path,
                object,
                ancestry,
            })
        };
        let container = root.join("profiles").join(profile.id.to_string());
        let path = container.join("desktop");
        let ancestry = platform::anchor(&path)?;
        let Some(object) = platform::inspect(&path, ObjectKind::Directory)? else {
            return if partial {
                Ok(())
            } else {
                Err(Error::AuthBusy)
            };
        };
        let desktop = CheckedDirectory {
            path,
            object,
            ancestry,
        };
        if !desktop::deletion_lock_present(&desktop)? {
            return Ok(());
        }
        let application = data
            .desktop_bindings
            .iter()
            .find(|b| b.provider == profile.provider)
            .ok_or(Error::AuthBusy)?;
        let app = desktop::validate_binding(profile.provider, &application.path)
            .map_err(|_| Error::AuthBusy)?;
        let snapshot = CheckedSnapshot {
            profile_id: profile.id,
            provider: profile.provider,
            revision: data.revision,
            recorded_version: application.recorded_version.clone(),
            root: directory(root.to_owned())?,
            cli: directory(container.join("cli"))?,
            desktop,
            container: directory(container)?,
        };
        let target =
            ValidatedDesktopTarget::from_checked(snapshot, app).map_err(|_| Error::AuthBusy)?;
        if desktop::activity_for(&target) != Activity::Inactive {
            return Err(Error::AuthBusy);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (root, data, profile, partial);
    Ok(())
}

#[cfg(test)]
thread_local! { static FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }

pub(super) fn fault(_step: &str) -> Result<(), Error> {
    #[cfg(test)]
    if FAULT.get() == Some(_step) {
        return Err(Error::RecoveryRequired);
    }
    Ok(())
}
