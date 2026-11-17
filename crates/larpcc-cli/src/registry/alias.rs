//! Alias add/remove transactions. They share the single pending namespace
//! (`share-journal.json`) with skills but carry their own `alias` kind and steps, and only
//! touch the fixed `<root>/bin` launcher/receipt allowlist from `crate::aliases`.
use super::{
    Error, ProfileId, ProfileName, ProfileState, Registry, Surface, model::Alias, paths, store,
};
use crate::{
    aliases::{self, Receipt, Removal, Source},
    platform::{self, CheckedObject, FileIdentity, ObjectKind},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{ffi::OsStr, fs, io::Read, path::Path, time::Duration};

const JOURNAL: &str = "share-journal.json";
const KIND: &str = "alias";
// Two JSON-escaped registry snapshots plus small receipts.
const JOURNAL_LIMIT: usize = 6 * store::LIMIT;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Removed {
    alias: Alias,
    launcher: Option<(FileIdentity, [u8; 32])>,
    receipt: Option<[u8; 32]>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    /// Rolled back unless the registry already committed `after_registry`.
    Add {
        name: ProfileName,
        profile_id: ProfileId,
        surface: Surface,
        launcher: Option<FileIdentity>,
        receipt: Option<String>,
    },
    /// Rolled forward: every artifact was proven ours before the journal was written.
    Remove { aliases: Vec<Removed> },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u64,
    kind: String,
    before_registry: String,
    after_registry: Option<String>,
    step: Step,
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str) -> Result<[u8; 32], Error> {
    let mut out = [0; 32];
    if value.len() != 64 {
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
    if j.version != 1 {
        return Err(Error::RecoveryRequired);
    }
    Ok(Some(j))
}
fn finish(root: &Path) -> Result<(), Error> {
    fs::remove_file(root.join(JOURNAL))
        .map_err(|e| paths::io("finish alias transaction", root, e))?;
    paths::sync(root)
}
fn sync_bin(root: &Path) -> Result<(), Error> {
    if platform::inspect(&root.join("bin"), ObjectKind::Directory)?.is_some() {
        paths::sync(&root.join("bin"))?;
    }
    Ok(())
}

/// Call under the registry write lock with the unchecked current registry.
/// Refusal leaves the journal pending, so every registry consumer keeps refusing.
fn recover(root: &Path, current: &store::Data) -> Result<(), Error> {
    let Some(j) = read_journal(root)? else {
        return Ok(());
    };
    let current_bytes = store::serialize(current)?;
    if j.after_registry
        .as_deref()
        .is_some_and(|after| after.as_bytes() == current_bytes)
    {
        return finish(root);
    }
    if current_bytes != j.before_registry.as_bytes() {
        return Err(Error::Conflict);
    }
    match &j.step {
        Step::Add {
            name,
            launcher,
            receipt,
            ..
        } => {
            // A registered name was never this transaction's to create.
            if current.aliases.iter().any(|a| &a.name == name) {
                return Err(Error::RecoveryRequired);
            }
            if let Some(bytes) = receipt {
                aliases::remove_owned(
                    &aliases::receipt_path(root, name),
                    aliases::receipt_owner(bytes.as_bytes(), true),
                )?;
            }
            if let Some(identity) = launcher {
                aliases::remove_owned(
                    &aliases::launcher_path(root, name),
                    aliases::launcher_owner(identity, None),
                )?;
            }
            sync_bin(root)?;
        }
        Step::Remove { aliases: removed } => {
            let after = j.after_registry.as_deref().ok_or(Error::RecoveryRequired)?;
            let mut expected = copy(current)?;
            expected
                .aliases
                .retain(|a| !removed.iter().any(|r| &r.alias == a));
            expected.revision = expected.revision.checked_add(1).ok_or(Error::Exhausted)?;
            if removed.is_empty()
                || removed.iter().any(|r| !current.aliases.contains(&r.alias))
                || store::serialize(&expected)? != after.as_bytes()
            {
                return Err(Error::RecoveryRequired);
            }
            remove_artifacts(root, removed, &mut false)?;
            store::persist(root, after.as_bytes(), &mut false, &mut false)?;
        }
    }
    finish(root)
}
fn remove_artifacts(root: &Path, removed: &[Removed], touched: &mut bool) -> Result<(), Error> {
    for r in removed {
        let name = &r.alias.name;
        let launcher = match &r.launcher {
            Some((identity, sha)) => aliases::remove_owned(
                &aliases::launcher_path(root, name),
                aliases::launcher_owner(identity, Some(sha)),
            )?,
            None => aliases::remove_owned(&aliases::launcher_path(root, name), |_| Ok(false))?,
        };
        *touched |= matches!(launcher, Removal::Removed);
        if matches!(launcher, Removal::Foreign) {
            return Err(Error::AliasInvalid);
        }
        let receipt = match &r.receipt {
            Some(sha) => aliases::remove_owned(
                &aliases::receipt_path(root, name),
                aliases::receipt_digest_owner(sha),
            )?,
            None => aliases::remove_owned(&aliases::receipt_path(root, name), |_| Ok(false))?,
        };
        *touched |= matches!(receipt, Removal::Removed);
        if matches!(receipt, Removal::Foreign) {
            return Err(Error::AliasInvalid);
        }
        fault("artifact")?;
    }
    sync_bin(root)
}

/// Journal-remove the selected aliases' launchers, receipts and mappings in one registry
/// commit. Call under the registry write lock after the revision precondition.
fn remove_aliases(
    root: &Path,
    data: store::Data,
    select: impl Fn(&Alias) -> bool,
) -> Result<store::Data, Error> {
    let mut removed = vec![];
    for a in data.aliases.iter().filter(|a| select(a)) {
        let digest = unhex(&a.receipt)?;
        let owned = aliases::owned(root, &a.name, a.profile_id, a.surface, &digest)?;
        removed.push(Removed {
            alias: a.clone(),
            launcher: owned.launcher,
            receipt: owned.receipt,
        });
    }
    if removed.is_empty() {
        return Ok(data);
    }
    let mut after = copy(&data)?;
    after.aliases.retain(|a| !select(a));
    after.revision = after.revision.checked_add(1).ok_or(Error::Exhausted)?;
    let after_bytes = store::serialize(&after)?;
    write_journal(
        root,
        &Journal {
            version: 1,
            kind: KIND.into(),
            before_registry: utf8(store::serialize(&data)?)?,
            after_registry: Some(utf8(after_bytes.clone())?),
            step: Step::Remove {
                aliases: removed.clone(),
            },
        },
    )?;
    let mut touched = false;
    let result = (|| {
        fault("journal")?;
        remove_artifacts(root, &removed, &mut touched)?;
        store::persist(root, &after_bytes, &mut false, &mut false)?;
        fault("registry")
    })();
    if let Err(error) = result {
        if !crashed() {
            // Nothing removed yet: abandon the intent. Otherwise finish what began.
            let _ = if touched {
                store::load_unchecked(root).and_then(|current| recover(root, &current))
            } else {
                finish(root)
            };
        }
        return Err(error);
    }
    finish(root)?;
    Ok(after)
}

/// Profile-delete entry: under the caller's registry write lock (after its revision
/// precondition, before its tombstone commit), journal-remove every alias of `id` through
/// this module's own allowlist and commit. Returns the committed registry (or `data`
/// unchanged when `id` has no aliases); the caller builds its tombstone from the result.
#[allow(
    dead_code,
    reason = "profile delete (a later stage) calls this under the same lock"
)]
pub(super) fn remove_profile_aliases(
    root: &Path,
    data: store::Data,
    id: ProfileId,
) -> Result<store::Data, Error> {
    remove_aliases(root, data, |a| a.profile_id == id)
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
    /// alias transaction (and re-check, since recovery may commit).
    fn alias_resumed(&self) -> Result<store::Data, Error> {
        let data = store::load_unchecked(self.root())?;
        store::precondition(&data, self.expected)?;
        if !crate::share::pending(self.root()) {
            return Ok(data);
        }
        recover(self.root(), &data)?;
        let data = store::load(self.root())?;
        store::precondition(&data, self.expected)?;
        Ok(data)
    }

    pub fn alias_list(&self) -> Result<Value, Error> {
        let root = self.root();
        let _ancestors = platform::anchor(root)?.ancestors;
        let _fixed = store::fixed(root)?;
        if crate::share::pending(root) {
            let _lock = store::lock(root, Duration::from_secs(10))?;
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
        let _ancestors = platform::anchor(root)?.ancestors;
        let _lock = store::lock(root, Duration::from_secs(10))?;
        let _fixed = store::fixed(root)?;
        let data = self.alias_resumed()?;
        active(&data)?;
        let profile_id = data
            .profiles
            .iter()
            .find(|p| &p.name == profile)
            .ok_or_else(|| Error::NotFound {
                name: profile.clone(),
            })?
            .id;
        if name.as_str() == "larp" || data.aliases.iter().any(|a| a.name == name) {
            return Err(Error::Conflict);
        }
        aliases::check_collision(root, &name, search_path)?;
        let source = source(root)?;
        let bin = root.join("bin");
        if paths::mkdir(&bin)? {
            paths::sync(root)?;
        }
        let mut j = Journal {
            version: 1,
            kind: KIND.into(),
            before_registry: utf8(store::serialize(&data)?)?,
            after_registry: None,
            step: Step::Add {
                name: name.clone(),
                profile_id,
                surface,
                launcher: None,
                receipt: None,
            },
        };
        write_journal(root, &j)?;
        let result = (|| {
            fault("journal")?;
            let launcher = aliases::create_launcher(root, &name)?;
            if let Step::Add { launcher: slot, .. } = &mut j.step {
                *slot = Some(launcher.identity.clone());
            }
            write_journal(root, &j)?;
            fault("launcher")?;
            aliases::fill_launcher(launcher, &source)?;
            let receipt =
                Receipt::prepare(root, name.clone(), profile_id, surface, &source)?.encode()?;
            let mut after = copy(&data)?;
            after.aliases.push(Alias {
                name: name.clone(),
                profile_id,
                surface,
                receipt: hex(&aliases::digest(&receipt)),
            });
            after.aliases.sort_by(|a, b| a.name.cmp(&b.name));
            after.revision = after.revision.checked_add(1).ok_or(Error::Exhausted)?;
            let after_bytes = store::serialize(&after)?;
            j.after_registry = Some(utf8(after_bytes.clone())?);
            if let Step::Add { receipt: slot, .. } = &mut j.step {
                *slot = Some(utf8(receipt.clone())?);
            }
            write_journal(root, &j)?;
            fault("prepared")?;
            aliases::write_receipt(root, &name, &receipt)?;
            paths::sync(&bin)?;
            fault("receipt")?;
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
        let _lock = store::lock(root, Duration::from_secs(10))?;
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

    /// Alias dispatch: the pinned root's registered mapping must match the adjacent
    /// receipt and its independently owned digest, with no pending journal. Returns the
    /// current profile name, the snapshot revision to pin admission to, and held handles.
    pub(crate) fn alias_target(
        &self,
        receipt: &Receipt,
        digest: &[u8; 32],
        executable: &Path,
        argv0: &OsStr,
    ) -> Result<(ProfileName, u64, Vec<CheckedObject>), Error> {
        let root = self.root();
        let _ancestors = platform::anchor(root)?.ancestors;
        let _fixed = store::fixed(root)?;
        let _lock = store::admission_lock(root)?;
        let data = store::load(root)?;
        store::require_v3(&data)?;
        let alias = data
            .aliases
            .iter()
            .find(|a| &a.name == receipt.name())
            .ok_or(Error::AliasInvalid)?;
        if alias.profile_id != receipt.profile_id()
            || alias.surface != receipt.surface()
            || unhex(&alias.receipt)? != *digest
        {
            return Err(Error::AliasInvalid);
        }
        let held = receipt.validate(executable, argv0)?;
        let profile = data
            .profiles
            .iter()
            .find(|p| p.id == alias.profile_id)
            .ok_or(Error::AliasInvalid)?;
        Ok((profile.name.clone(), data.revision, held))
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
                .alias_target(&receipt, &digest, &launcher, OsStr::new(alias))
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
        for phase in ["journal", "launcher", "prepared", "receipt", "registry"] {
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
            if phase == "receipt" {
                let bytes = fs::read(f.receipt("codex-45")).unwrap();
                rewrite(&f.receipt("codex-45"), &bytes[..bytes.len() / 2]);
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
        let identity = platform::inspect(&launcher, ObjectKind::File)
            .unwrap()
            .unwrap()
            .identity;
        let current = store::load(f.r.root()).unwrap();
        write_journal(
            f.r.root(),
            &Journal {
                version: 1,
                kind: KIND.into(),
                before_registry: utf8(store::serialize(&current).unwrap()).unwrap(),
                after_registry: None,
                step: Step::Add {
                    name: name("codex-45"),
                    profile_id: ProfileId::new(1),
                    surface: Surface::Cli,
                    launcher: Some(identity),
                    receipt: None,
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
    fn stage05_transaction_profile_cleanup_entry() {
        let f = fixture();
        f.add("codex-45", "claude-work").unwrap();
        f.add("claude-alt", "claude-work").unwrap();
        f.add("other", "codex-45").unwrap();
        let root = f.r.root();
        let lock = store::lock(root, Duration::from_secs(10)).unwrap();
        let data = store::load(root).unwrap();
        let before = data.revision;
        let after = remove_profile_aliases(root, data, ProfileId::new(1)).unwrap();
        assert_eq!(after.revision, before + 1);
        assert_eq!(after.aliases.len(), 1);
        let unchanged = remove_profile_aliases(root, after, ProfileId::new(1)).unwrap();
        assert_eq!(unchanged.revision, before + 1);
        drop(lock);
        assert!(!f.launcher("codex-45").exists() && !f.launcher("claude-alt").exists());
        assert_eq!(f.states(), ["other:ok"]);
        assert_eq!(store::load(root).unwrap().revision, before + 1);
    }
}
