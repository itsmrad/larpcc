use super::{Error, ProfileName, paths, store};
use crate::platform;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io::Write, path::Path};

const FILE: &str = "catalog-sharing.json";
const JOURNAL: &str = "share-journal.json";
const MAX_NAMES: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Mcp,
    Skills,
}

impl Kind {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Skills => "skills",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct State {
    catalog_revision: String,
    names: Vec<ProfileName>,
}

impl State {
    fn empty(revision: u64) -> Self {
        Self {
            catalog_revision: revision.to_string(),
            names: Vec::new(),
        }
    }

    fn valid(&self) -> bool {
        self.catalog_revision
            .parse::<u64>()
            .is_ok_and(|n| n.to_string() == self.catalog_revision)
            && self.names.len() <= MAX_NAMES
            && self.names.windows(2).all(|p| p[0] < p[1])
            && self
                .names
                .iter()
                .all(|n| ProfileName::parse_new(n.as_str()).is_ok())
    }

    fn bound(&self, revision: u64) -> bool {
        self.catalog_revision == revision.to_string()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sharing {
    version: u64,
    mcp: State,
    skills: State,
}

impl Default for Sharing {
    fn default() -> Self {
        Self {
            version: 1,
            mcp: State::empty(0),
            skills: State::empty(0),
        }
    }
}

impl Sharing {
    fn state(&self, kind: Kind) -> &State {
        match kind {
            Kind::Mcp => &self.mcp,
            Kind::Skills => &self.skills,
        }
    }
    fn set(&mut self, kind: Kind, value: State) {
        match kind {
            Kind::Mcp => self.mcp = value,
            Kind::Skills => self.skills = value,
        }
    }

    pub(super) fn load(root: &Path) -> Result<Self, Error> {
        let path = root.join(FILE);
        let Some(bytes) = read(&path, store::LIMIT)? else {
            return Ok(Self::default());
        };
        let corrupt = || Error::Corrupt {
            path: path.clone(),
            reason: "invalid catalog sharing metadata".into(),
        };
        let flags: Self = serde_json::from_slice(&bytes).map_err(|_| corrupt())?;
        if flags.version != 1 || !flags.mcp.valid() || !flags.skills.valid() {
            return Err(corrupt());
        }
        Ok(flags)
    }

    pub(super) fn opted_in(&self, kind: Kind, revision: u64, name: &ProfileName) -> bool {
        let state = self.state(kind);
        state.bound(revision) && state.names.binary_search(name).is_ok()
    }
}

fn read(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, Error> {
    use std::io::Read;
    let Some(file) = platform::acquire_replaceable(path, || platform::inspect_replaceable(path))?
    else {
        return Ok(None);
    };
    platform::single_link(&file.file)?;
    let mut bytes = Vec::new();
    file.file
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| paths::io("read catalog metadata", path, e))?;
    if bytes.len() > limit {
        return Err(Error::TooLarge);
    }
    Ok(Some(bytes))
}

fn write(root: &Path, name: &str, value: &impl Serialize, limit: usize) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::RecoveryRequired)?;
    if bytes.len() > limit {
        return Err(Error::TooLarge);
    }
    let path = root.join(name);
    let temp = root.join(format!("{name}.tmp"));
    if let Some(existing) =
        platform::acquire_replaceable(&temp, || platform::inspect_replaceable(&temp))?
    {
        platform::single_link(&existing.file)?;
        drop(existing);
        fs::remove_file(&temp)
            .map_err(|e| paths::io("remove temporary catalog metadata", &temp, e))?;
    }
    let mut file = platform::create_file(&temp)?.file;
    let result = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file);
    result.map_err(|e| paths::io("write catalog metadata", &temp, e))?;
    store::replace(&temp, &path)?;
    paths::sync(root)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    kind: Kind,
    before: State,
    after: State,
}

impl Change {
    pub(super) fn for_catalog<'a>(
        root: &Path,
        kind: Kind,
        before_revision: u64,
        after_revision: u64,
        retained_names: impl Iterator<Item = &'a ProfileName>,
    ) -> Result<Option<Self>, Error> {
        let flags = Sharing::load(root)?;
        let before = flags.state(kind);
        if before.names.is_empty() || !before.bound(before_revision) {
            return Ok(None);
        }
        let names: BTreeSet<_> = retained_names.collect();
        let mut after = State::empty(after_revision);
        after.names.extend(
            before
                .names
                .iter()
                .filter(|name| names.contains(name))
                .cloned(),
        );
        Ok(Some(Self {
            kind,
            before: before.clone(),
            after,
        }))
    }

    pub(super) fn validate_catalog(
        &self,
        kind: Kind,
        before: u64,
        after: u64,
    ) -> Result<(), Error> {
        if self.kind != kind
            || !self.before.valid()
            || !self.after.valid()
            || !self.before.bound(before)
            || !self.after.bound(after)
            || self
                .after
                .names
                .iter()
                .any(|name| self.before.names.binary_search(name).is_err())
        {
            return Err(Error::RecoveryRequired);
        }
        Ok(())
    }

    pub(super) fn apply(&self, root: &Path) -> Result<(), Error> {
        if !self.before.valid() || !self.after.valid() {
            return Err(Error::RecoveryRequired);
        }
        let mut flags = Sharing::load(root)?;
        let current = flags.state(self.kind);
        if *current == self.after {
            return Ok(());
        }
        if *current != self.before {
            return Err(Error::Conflict);
        }
        flags.set(self.kind, self.after.clone());
        write(root, FILE, &flags, store::LIMIT)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u64,
    kind: String,
    before_registry: String,
    after_registry: String,
    change: Change,
}

const JOURNAL_KIND: &str = "catalog-sharing";
const JOURNAL_LIMIT: usize = 4 * store::LIMIT;

pub(super) fn share(
    root: &Path,
    registry: &store::Data,
    kind: Kind,
    catalog_revision: u64,
    name: &ProfileName,
    enabled: bool,
) -> Result<(bool, u64), Error> {
    let flags = Sharing::load(root)?;
    let before = flags.state(kind);
    let mut after = if before.bound(catalog_revision) {
        before.clone()
    } else {
        State::empty(catalog_revision)
    };
    match (after.names.binary_search(name), enabled) {
        (Ok(index), false) => {
            after.names.remove(index);
        }
        (Err(index), true) => {
            if after.names.len() == MAX_NAMES {
                return Err(Error::TooLarge);
            }
            after.names.insert(index, name.clone());
        }
        _ => return Ok((false, registry.revision)),
    }
    let mut next = store::parse(Path::new("registry.json"), &store::serialize(registry)?)?;
    next.revision = registry.revision.checked_add(1).ok_or(Error::Exhausted)?;
    let journal = Journal {
        version: 1,
        kind: JOURNAL_KIND.into(),
        before_registry: serialized(registry)?,
        after_registry: serialized(&next)?,
        change: Change {
            kind,
            before: before.clone(),
            after,
        },
    };
    write(root, JOURNAL, &journal, JOURNAL_LIMIT)?;
    apply_journal(root, registry, journal)?;
    Ok((true, next.revision))
}

fn serialized(registry: &store::Data) -> Result<String, Error> {
    String::from_utf8(store::serialize(registry)?).map_err(|_| Error::RecoveryRequired)
}

pub(super) fn recover(root: &Path, current: &store::Data) -> Result<bool, Error> {
    #[derive(Deserialize)]
    struct Header {
        kind: String,
    }
    let bytes = match read(&root.join(JOURNAL), JOURNAL_LIMIT) {
        Ok(Some(bytes)) => bytes,
        Ok(None) | Err(Error::TooLarge) => return Ok(false),
        Err(error) => return Err(error),
    };
    let header: Header = serde_json::from_slice(&bytes).map_err(|_| Error::RecoveryRequired)?;
    if header.kind != JOURNAL_KIND {
        return Ok(false);
    }
    let journal = serde_json::from_slice(&bytes).map_err(|_| Error::RecoveryRequired)?;
    apply_journal(root, current, journal)?;
    Ok(true)
}

fn apply_journal(root: &Path, current: &store::Data, journal: Journal) -> Result<(), Error> {
    let path = root.join("registry.json");
    let mut before = store::parse(&path, journal.before_registry.as_bytes())?;
    let current_bytes = serialized(current)?;
    if journal.version != 1
        || journal.kind != JOURNAL_KIND
        || journal.before_registry != serialized(&before)?
        || (current_bytes != journal.before_registry && current_bytes != journal.after_registry)
    {
        return Err(Error::RecoveryRequired);
    }
    before.revision = before
        .revision
        .checked_add(1)
        .ok_or(Error::RecoveryRequired)?;
    if serialized(&before)? != journal.after_registry {
        return Err(Error::RecoveryRequired);
    }
    journal.change.apply(root)?;
    if current_bytes != journal.after_registry {
        store::persist(
            root,
            journal.after_registry.as_bytes(),
            &mut false,
            &mut false,
        )?;
    }
    fs::remove_file(root.join(JOURNAL))
        .map_err(|e| paths::io("finish catalog sharing transaction", root, e))?;
    paths::sync(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage13_catalog_sharing_revision_binding_and_recovery() {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        #[cfg(unix)]
        let temp = {
            use std::os::unix::fs::PermissionsExt;
            tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(parent)
                .unwrap()
        };
        #[cfg(windows)]
        let temp = platform::fixture_private::tempdir(&parent).unwrap();
        let root = temp.path().join("managed");
        let _guard = store::lock(&root, std::time::Duration::from_secs(1)).unwrap();
        let name = "codex-45".parse().unwrap();
        let registry = store::load(&root).unwrap();
        assert_eq!(
            share(&root, &registry, Kind::Mcp, 0, &name, true).unwrap(),
            (true, 1)
        );
        let registry = store::load(&root).unwrap();
        assert_eq!(
            share(&root, &registry, Kind::Mcp, 0, &name, true).unwrap(),
            (false, 1)
        );
        let flags = Sharing::load(&root).unwrap();
        assert!(flags.opted_in(Kind::Mcp, 0, &name));
        assert!(
            !flags.opted_in(Kind::Mcp, 1, &name),
            "a legacy writer advanced its catalog"
        );
        assert!(!flags.opted_in(Kind::Skills, 0, &name));
        let removed = Change::for_catalog(&root, Kind::Mcp, 0, 2, std::iter::empty())
            .unwrap()
            .unwrap();

        // An interrupted commit may already have written sharing state. Reads still
        // require explicit recovery while the journal owns the registry revision.
        let before = serialized(&registry).unwrap();
        let mut after = store::load(&root).unwrap();
        after.revision = 2;
        let journal = Journal {
            version: 1,
            kind: JOURNAL_KIND.into(),
            before_registry: before,
            after_registry: serialized(&after).unwrap(),
            change: removed,
        };
        write(&root, JOURNAL, &journal, JOURNAL_LIMIT).unwrap();
        journal.change.apply(&root).unwrap();
        assert!(matches!(store::load(&root), Err(Error::RecoveryRequired)));
        assert!(recover(&root, &registry).unwrap());
        assert_eq!(store::load(&root).unwrap().revision, 2);
        assert!(!Sharing::load(&root).unwrap().opted_in(Kind::Mcp, 2, &name));
    }
}
