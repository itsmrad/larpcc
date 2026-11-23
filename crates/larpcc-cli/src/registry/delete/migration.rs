use super::*;
use std::collections::HashSet;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Migration {
    revision: u64,
    before: [u8; 32],
    after: [u8; 32],
    profiles: FileIdentity,
    homes: Vec<Home>,
    next: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Home {
    id: ProfileId,
    legacy: FileIdentity,
    container: Option<Container>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Container {
    directory: FileIdentity,
    desktop: FileIdentity,
}

impl Home {
    fn roles(&self) -> (String, String, String) {
        (
            self.id.to_string(),
            format!(".larp-migrate-{}-legacy", self.id),
            format!(".larp-migrate-{}-container", self.id),
        )
    }

    fn verify_complete(&self, tree: &Profiles) -> Result<(), Error> {
        let (id, legacy, stage) = self.roles();
        let container = self.container.as_ref().ok_or(Error::RecoveryRequired)?;
        if tree.home(&id, None, false)?.as_ref() != Some(&container.directory)
            || !tree.has_only(&id, &[c"cli", c"desktop"])?
            || tree.home(&id, Some("cli"), true)?.as_ref() != Some(&self.legacy)
            || tree.home(&id, Some("desktop"), true)?.as_ref() != Some(&container.desktop)
            || tree.home(&legacy, None, false)?.is_some()
            || tree.home(&stage, None, false)?.is_some()
        {
            return Err(Error::Drift);
        }
        Ok(())
    }
}

fn upgrade(data: &mut store::Data) -> Result<Vec<u8>, Error> {
    data.version = 3;
    data.revision = data.revision.checked_add(1).ok_or(Error::Exhausted)?;
    for p in &mut data.profiles {
        p.surfaces = Some(Default::default());
        p.state = Some(ProfileState::Active);
    }
    store::serialize(data)
}

impl Registry {
    pub(crate) fn migrate_profiles(&self) -> Result<bool, Error> {
        let root = self.root();
        let _ancestors = platform::anchor(root)?.ancestors;
        let (ids, before) = {
            let (_lock, data) = locked(root, self.expected)?;
            let pending = read(root)?;
            if pending
                .as_ref()
                .is_some_and(|j| !matches!(j, Journal::Migrate { .. }))
            {
                return Err(Error::RecoveryRequired);
            }
            if data.version == 3 && pending.is_none() {
                self.observed.set(Some(data.revision));
                return Ok(false);
            }
            let ids = data.profiles.iter().map(|p| p.id).collect::<Vec<_>>();
            (ids, fingerprint(&store::serialize(&data)?))
        };
        // Registry order is ascending immutable ID. Never wait with the registry lock held.
        let _guards = ids
            .iter()
            .map(|id| acquire_auth_guard(root, *id))
            .collect::<Result<Vec<_>, _>>()?;
        let (_lock, mut data) = locked(root, self.expected)?;
        if fingerprint(&store::serialize(&data)?) != before {
            return Err(Error::Conflict);
        }
        let mut pending = read(root)?;
        if ids.is_empty() && pending.is_none() {
            persist(root, &upgrade(&mut data)?)?;
            self.observed.set(Some(data.revision));
            return Ok(true);
        }
        let tree = Profiles::open(root)?;
        if pending.is_none() {
            if data.version == 3 {
                return Err(Error::Conflict);
            }
            let mut homes = Vec::with_capacity(ids.len());
            let mut seen = HashSet::with_capacity(ids.len());
            for id in ids {
                let legacy = tree
                    .home(&id.to_string(), None, true)?
                    .ok_or(Error::Drift)?;
                if !seen.insert(legacy.clone()) {
                    return Err(Error::Drift);
                }
                let home = Home {
                    id,
                    legacy,
                    container: None,
                };
                let (_, legacy, stage) = home.roles();
                if tree.home(&legacy, None, false)?.is_some()
                    || tree.home(&stage, None, false)?.is_some()
                {
                    return Err(Error::Conflict);
                }
                homes.push(home);
            }
            let revision = data.revision;
            let after = fingerprint(&upgrade(&mut data)?);
            data = store::load_unchecked(root)?;
            if fingerprint(&store::serialize(&data)?) != before {
                return Err(Error::Conflict);
            }
            let journal = Journal::Migrate {
                version: 1,
                transaction: Migration {
                    revision,
                    before,
                    after,
                    profiles: tree.identity()?,
                    homes,
                    next: 0,
                },
            };
            write(root, &journal)?;
            fault("migrate journal")?;
            pending = Some(journal);
        }
        let mut journal = pending.ok_or(Error::RecoveryRequired)?;
        let revision = apply_migration(root, &tree, &mut journal, data, before)?;
        self.observed.set(Some(revision));
        Ok(true)
    }
}

fn migration(journal: &mut Journal) -> Result<&mut Migration, Error> {
    match journal {
        Journal::Migrate { transaction, .. } => Ok(transaction),
        _ => Err(Error::RecoveryRequired),
    }
}

fn apply_migration(
    root: &Path,
    tree: &Profiles,
    journal: &mut Journal,
    mut data: store::Data,
    current: [u8; 32],
) -> Result<u64, Error> {
    let tx = migration(journal)?;
    let revision = tx.revision.checked_add(1).ok_or(Error::Exhausted)?;
    if tx.next > tx.homes.len()
        || tx.homes.is_empty()
        || tx.profiles != tree.identity()?
        || !tx
            .homes
            .iter()
            .map(|h| h.id)
            .eq(data.profiles.iter().map(|p| p.id))
        || tx
            .homes
            .iter()
            .map(|h| &h.legacy)
            .collect::<HashSet<_>>()
            .len()
            != tx.homes.len()
        || tx
            .homes
            .iter()
            .skip(tx.next + 1)
            .any(|h| h.container.is_some())
    {
        return Err(Error::RecoveryRequired);
    }
    for home in &tx.homes[..tx.next] {
        home.verify_complete(tree)?;
    }
    if data.version == 3 {
        if data.revision != revision || tx.next != tx.homes.len() || current != tx.after {
            return Err(Error::Conflict);
        }
    } else {
        if data.revision != tx.revision || current != tx.before {
            return Err(Error::Conflict);
        }
        let bytes = upgrade(&mut data)?;
        if fingerprint(&bytes) != tx.after {
            return Err(Error::RecoveryRequired);
        }
        // Detect changes to not-yet-started homes before making more moves.
        for home in tx.homes.iter().skip(tx.next + 1) {
            let (id, legacy, stage) = home.roles();
            if tree.home(&id, None, true)?.as_ref() != Some(&home.legacy)
                || tree.home(&legacy, None, false)?.is_some()
                || tree.home(&stage, None, false)?.is_some()
            {
                return Err(Error::Drift);
            }
        }
        while migration(journal)?.next < migration(journal)?.homes.len() {
            move_home(root, tree, journal)?;
        }
        for home in &migration(journal)?.homes {
            home.verify_complete(tree)?;
        }
        persist(root, &bytes)?;
        fault("migrate registry")?;
    }
    paths::sync(root)?;
    finish(root)?;
    Ok(revision)
}

fn move_home(root: &Path, tree: &Profiles, journal: &mut Journal) -> Result<(), Error> {
    let tx = migration(journal)?;
    let home = &mut tx.homes[tx.next];
    let (id, legacy, stage) = home.roles();
    let mut current = tree.home(&id, None, false)?;
    let mut staged_legacy = tree.home(&legacy, None, true)?;
    let staged_container = tree.home(&stage, None, false)?;
    if home.container.is_none() {
        if staged_container.is_some() {
            // An interrupted create and an unrelated object are indistinguishable here.
            return Err(Error::Drift);
        }
        if current.as_ref() == Some(&home.legacy) && staged_legacy.is_none() {
            paths::sync(root)?;
            tree.rename_home(&id, &legacy, None, &home.legacy, true)?;
            fault("migrate legacy rename")?;
            current = None;
            staged_legacy = Some(home.legacy.clone());
        }
        if current.is_some() || staged_legacy.as_ref() != Some(&home.legacy) {
            return Err(Error::Drift);
        }
        tree.sync(None)?;
        let (directory, desktop) = tree.make_container(&stage)?;
        fault("migrate container create")?;
        home.container = Some(Container { directory, desktop });
        write(root, journal)?;
        fault("migrate container journal")?;
    }
    let tx = migration(journal)?;
    let home = &tx.homes[tx.next];
    let container = home.container.as_ref().ok_or(Error::RecoveryRequired)?;
    let staged_container = tree.home(&stage, None, false)?;
    if current.is_none()
        && staged_legacy.as_ref() == Some(&home.legacy)
        && staged_container.as_ref() == Some(&container.directory)
        && tree.has_only(&stage, &[c"desktop"])?
        && tree.home(&stage, Some("desktop"), true)?.as_ref() == Some(&container.desktop)
        && tree.home(&stage, Some("cli"), false)?.is_none()
    {
        paths::sync(root)?;
        tree.rename_home(&stage, &id, None, &container.directory, false)?;
        fault("migrate container rename")?;
        current = Some(container.directory.clone());
    } else if staged_container.is_some() {
        return Err(Error::Drift);
    }
    if current.as_ref() != Some(&container.directory)
        || tree.home(&id, Some("desktop"), true)?.as_ref() != Some(&container.desktop)
    {
        return Err(Error::Drift);
    }
    if let Some(staged_legacy) = staged_legacy {
        if staged_legacy != home.legacy || !tree.has_only(&id, &[c"desktop"])? {
            return Err(Error::Drift);
        }
        // Persist the observed container rename before moving the legacy home into it.
        tree.sync(None)?;
        write(root, journal)?;
        fault("migrate publish journal")?;
        let tx = migration(journal)?;
        let home = &tx.homes[tx.next];
        let container = home.container.as_ref().ok_or(Error::RecoveryRequired)?;
        tree.rename_home(
            &legacy,
            "cli",
            Some((&id, &container.directory)),
            &home.legacy,
            true,
        )?;
        fault("migrate cli rename")?;
    }
    let tx = migration(journal)?;
    tx.homes[tx.next].verify_complete(tree)?;
    tree.sync(Some(&id))?;
    tx.next += 1;
    write(root, journal)?;
    fault("migrate progress journal")
}
