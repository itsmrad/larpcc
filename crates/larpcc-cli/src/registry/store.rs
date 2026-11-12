use super::{
    Error, Profile, ProfileEntry, ProfileId, ProfileName, Provider,
    paths::{self, io},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};
pub(super) const LIMIT: usize = 1_048_576;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Data {
    pub version: u64,
    pub revision: u64,
    pub next_id: u64,
    pub profiles: Vec<Profile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<super::Binding>,
}
impl Default for Data {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            next_id: 1,
            profiles: Vec::new(),
            bindings: Vec::new(),
        }
    }
}
pub(super) fn bounded(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}
pub(super) fn parse(path: &Path, bytes: &[u8]) -> Result<Data, Error> {
    let corrupt = |reason: String| Error::Corrupt {
        path: path.into(),
        reason,
    };
    if bytes.len() > LIMIT {
        return Err(corrupt("exceeds 1 MiB".into()));
    }
    // Probe only the version first, so future schemas can contain future fields.
    let probe: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| corrupt("invalid registry encoding".into()))?;
    let version = probe
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| corrupt("missing or invalid version".into()))?;
    if version > 2 {
        return Err(Error::NewerSchema {
            path: path.into(),
            found: version,
        });
    }
    let data: Data =
        serde_json::from_slice(bytes).map_err(|_| corrupt("invalid registry encoding".into()))?;
    if !matches!(data.version, 1 | 2) || data.next_id == 0 {
        return Err(corrupt("invalid version or next_id".into()));
    }
    if (version == 2 && probe.get("bindings").is_none())
        || (version == 1 && probe.get("bindings").is_some())
    {
        return Err(corrupt("invalid bindings field".into()));
    }
    let mut previous_provider = None;
    for binding in &data.bindings {
        let index = Provider::ALL
            .iter()
            .position(|p| *p == binding.provider)
            .unwrap_or(0);
        if previous_provider.is_some_and(|previous| previous >= index)
            || crate::provider::lexical(&binding.executable).is_err()
        {
            return Err(corrupt("invalid or unsorted bindings".into()));
        }
        previous_provider = Some(index);
    }
    let mut names = HashSet::new();
    let mut previous = 0;
    for profile in &data.profiles {
        if profile.id.get() <= previous
            || profile.id.get() >= data.next_id
            || !names.insert(&profile.name)
        {
            return Err(corrupt("invalid IDs or duplicate names".into()));
        }
        previous = profile.id.get();
    }
    Ok(data)
}
pub(super) fn load(root: &Path) -> Result<Data, Error> {
    load_with(root, |_| {})
}
pub(super) fn load_with(root: &Path, mut before_open: impl FnMut(&Path)) -> Result<Data, Error> {
    let _ancestors = crate::platform::anchor(root)?.ancestors;
    let _fixed = fixed(root)?;
    if paths::inspect(root, true)?.is_none() {
        return Ok(Data::default());
    }
    paths::inspect(&root.join("profiles"), true)?;
    let path = root.join("registry.json");
    let checked = crate::platform::acquire_replaceable(&path, || {
        let Some(inspected) = crate::platform::inspect_replaceable(&path)? else {
            return Ok(None);
        };
        let meta = inspected
            .file
            .metadata()
            .map_err(|e| io("inspect", &path, e))?;
        if meta.len() > LIMIT as u64 {
            return Err(Error::Corrupt {
                path: path.clone(),
                reason: "exceeds 1 MiB".into(),
            }
            .into());
        }
        before_open(&path);
        let opened = paths::read_options()
            .open(&path)
            .map_err(|e| io("open", &path, e))?;
        let checked =
            crate::platform::verify_opened(opened, &path, crate::platform::ObjectKind::File)?;
        if checked.identity != inspected.identity {
            return Err(crate::platform::AcquisitionError::Changed);
        }
        Ok(Some(checked))
    })?;
    let Some(checked) = checked else {
        return Ok(Data::default());
    };
    let file = checked.file;
    let bytes = bounded(file).map_err(|e| io("read", &path, e))?;
    parse(&path, &bytes)
}
pub(super) fn serialize(data: &Data) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let result = if data.version == 2 {
        #[derive(Serialize)]
        struct V2<'a> {
            version: u64,
            revision: u64,
            next_id: u64,
            profiles: &'a [Profile],
            bindings: &'a [super::Binding],
        }
        serde_json::to_writer_pretty(
            &mut bytes,
            &V2 {
                version: 2,
                revision: data.revision,
                next_id: data.next_id,
                profiles: &data.profiles,
                bindings: &data.bindings,
            },
        )
    } else {
        serde_json::to_writer_pretty(&mut bytes, data)
    };
    result.map_err(|e| {
        io(
            "write",
            Path::new("registry.json"),
            std::io::Error::other(e),
        )
    })?;
    bytes.push(b'\n');
    capacity(bytes)
}
pub(super) fn capacity(bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
    if bytes.len() > LIMIT {
        Err(Error::TooLarge)
    } else {
        Ok(bytes)
    }
}
fn lock(
    root: &Path,
    timeout: Duration,
) -> Result<(File, Vec<crate::platform::CheckedObject>), Error> {
    let _fixed = fixed(root)?;
    if paths::inspect(root, true)?.is_none()
        && paths::mkdir(root)?
        && let Some(parent) = root.parent()
    {
        paths::sync(parent)?;
    }
    let held = fixed(root)?;
    let mut known = false;
    let mut nonempty = false;
    for entry in fs::read_dir(root).map_err(|e| io("read", root, e))? {
        let entry = entry.map_err(|e| io("read", root, e))?;
        nonempty = true;
        known |= matches!(
            entry.file_name().to_str(),
            Some("registry.json" | "registry.lock" | "profiles")
        );
    }
    if nonempty && !known {
        return Err(Error::Unsafe {
            path: root.into(),
            reason: "is not empty and is not a Larp CC data directory".into(),
        });
    }
    let path = root.join("registry.lock");
    let file = match crate::platform::create_file(&path) {
        Ok(object) => object.file,
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            let inspected = crate::platform::inspect(&path, crate::platform::ObjectKind::File)?
                .ok_or_else(|| Error::Unsafe {
                    path: path.clone(),
                    reason: "filesystem identity cannot be verified".into(),
                })?;
            let opened = paths::options()
                .open(&path)
                .map_err(|e| io("open", &path, e))?;
            let checked =
                crate::platform::verify_opened(opened, &path, crate::platform::ObjectKind::File)?;
            if checked.identity != inspected.identity {
                return Err(Error::Unsafe {
                    path: path.clone(),
                    reason: "filesystem identity cannot be verified".into(),
                });
            }
            checked.file
        }
        Err(e) => return Err(e),
    };
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok((file, held)),
            Err(std::fs::TryLockError::WouldBlock) => {
                if start.elapsed() >= timeout {
                    return Err(Error::Busy { path });
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(io("lock", &path, e)),
        }
    }
}
pub(super) enum Mutation {
    Add(ProfileName, Provider),
    Rename(ProfileName, ProfileName),
    Forget(ProfileName),
}
pub(super) fn mutate(
    root: &Path,
    mutation: Mutation,
    timeout: Duration,
) -> Result<ProfileEntry, Error> {
    let _ancestors = crate::platform::anchor(root)?.ancestors;
    let _lock = lock(root, timeout)?;
    let _fixed = fixed(root)?;
    let mut data = load(root)?;
    let old_entries = paths::entries(root, &data.profiles)?;
    let (mut selected, add) = match mutation {
        Mutation::Add(name, provider) => {
            if data.profiles.iter().any(|p| p.name == name) {
                return Err(Error::NameTaken { name });
            }
            (
                Profile {
                    id: ProfileId::new(data.next_id),
                    name,
                    provider,
                },
                true,
            )
        }
        Mutation::Rename(from, to) => {
            let index = data
                .profiles
                .iter()
                .position(|p| p.name == from)
                .ok_or(Error::NotFound { name: from })?;
            if data.profiles.iter().any(|p| p.name == to) {
                return Err(Error::NameTaken { name: to });
            }
            data.profiles[index].name = to;
            (data.profiles[index].clone(), false)
        }
        Mutation::Forget(name) => {
            let index = data
                .profiles
                .iter()
                .position(|p| p.name == name)
                .ok_or(Error::NotFound { name })?;
            (data.profiles.remove(index), false)
        }
    };
    data.version = 2;
    data.revision = data.revision.checked_add(1).ok_or(Error::Exhausted)?;
    let parent = root.join("profiles");
    let mut created_home = None;
    let mut _profiles_pins = None;
    let mut created_temp = false;
    let temp = root.join("registry.json.tmp");
    let mut applied = false;
    let operation = (|| {
        let bytes = if add {
            let mut attempts = 0;
            loop {
                if attempts >= 1000 {
                    return Err(Error::Unsafe {
                        path: parent.clone(),
                        reason: "contains too many unregistered directories".into(),
                    });
                }
                attempts += 1;
                let successor = selected.id.get().checked_add(1).ok_or(Error::Exhausted)?;
                let home = parent.join(selected.id.to_string());
                match fs::symlink_metadata(&home) {
                    Ok(_) => {
                        selected.id = ProfileId::new(successor);
                        continue;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(io("inspect", &home, e)),
                }
                data.next_id = successor;
                data.profiles.push(selected.clone());
                let bytes = serialize(&data)?;
                data.profiles.pop();
                if paths::mkdir(&parent)? {
                    paths::sync(root)?;
                }
                _profiles_pins = Some(fixed(root)?);
                // Unlike parent creation, an existing candidate is always an orphan.
                match crate::platform::create_directory(&home) {
                    Ok(_) => {
                        created_home = Some(home);
                        paths::sync(&parent)?;
                        data.profiles.push(selected.clone());
                        break bytes;
                    }
                    Err(Error::Io { source: e, .. })
                        if e.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        selected.id = ProfileId::new(successor);
                    }
                    Err(e) => return Err(e),
                }
            }
        } else {
            serialize(&data)?
        };
        persist(root, &bytes, &mut applied, &mut created_temp)?;
        Ok(())
    })();
    if operation.is_err() && !applied {
        if let Some(home) = created_home {
            let _ = fs::remove_dir(home);
        }
        if created_temp {
            let _ = fs::remove_file(&temp);
        }
    }
    operation?;
    // Include the forgotten entry in the identity scan, preserving its informational status.
    if !data.profiles.iter().any(|p| p.id == selected.id) {
        return old_entries
            .into_iter()
            .find(|e| e.profile.id == selected.id)
            .ok_or(Error::NotFound {
                name: selected.name,
            });
    }
    paths::entries(root, &data.profiles)?
        .into_iter()
        .find(|e| e.profile.id == selected.id)
        .ok_or(Error::NotFound {
            name: selected.name,
        })
}
fn replace(from: &Path, to: &Path) -> Result<(), Error> {
    #[cfg(windows)]
    for _ in 0..10 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(io("rename", to, e)),
        }
    }
    fs::rename(from, to).map_err(|e| io("rename", to, e))
}
// Thread-local test failures never exist in the production binary.
#[cfg(test)]
pub(super) fn phase(action: &str, path: &Path) -> Result<(), Error> {
    super::tests::phase(action, path)
}

// Retain parent handles; data handles are released before atomic replacement.
pub(super) fn fixed(root: &Path) -> Result<Vec<crate::platform::CheckedObject>, Error> {
    use crate::platform::{ObjectKind, inspect};
    let mut held = Vec::new();
    for path in [root.to_path_buf(), root.join("profiles")] {
        if let Some(object) = inspect(&path, ObjectKind::Directory)? {
            held.push(object);
        }
    }
    // The leaf is a syntactic sentinel only; anchor inspects its existing parents, never profile 1.
    held.extend(crate::platform::anchor(&root.join("profiles").join("1"))?.ancestors);
    for name in ["registry.json", "registry.lock", "registry.json.tmp"] {
        let path = root.join(name);
        if name == "registry.lock" {
            inspect(&path, ObjectKind::File)?;
        } else {
            crate::platform::acquire_replaceable(&path, || {
                crate::platform::inspect_replaceable(&path)
            })?;
        }
    }
    Ok(held)
}
fn persist(
    root: &Path,
    bytes: &[u8],
    applied: &mut bool,
    created_temp: &mut bool,
) -> Result<(), Error> {
    let temp = root.join("registry.json.tmp");
    let target = root.join("registry.json");
    match fs::remove_file(&temp) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(io("remove", &temp, e)),
    }
    let mut file = crate::platform::create_file(&temp)?.file;
    *created_temp = true;
    #[cfg(test)]
    phase("write", &temp)?;
    file.write_all(bytes).map_err(|e| io("write", &temp, e))?;
    #[cfg(test)]
    phase("file sync", &temp)?;
    file.sync_all().map_err(|e| io("sync", &temp, e))?;
    drop(file);
    #[cfg(test)]
    phase("replace", &target)?;
    replace(&temp, &target)?;
    *applied = true;
    paths::sync(root).map_err(|e| {
        io(
            "sync",
            root,
            std::io::Error::other(format!(
                "the change was applied but may not be durable: {e}"
            )),
        )
    })?;
    Ok(())
}
pub(super) fn bind(
    root: &Path,
    provider: Provider,
    executable: std::path::PathBuf,
    replace_existing: bool,
) -> Result<super::BindOutcome, Error> {
    crate::provider::input(provider, &executable, root)?;
    let _ancestors = crate::platform::anchor(root)?.ancestors;
    let _lock = lock(root, Duration::from_secs(10))?;
    let _fixed = fixed(root)?;
    let mut data = load(root)?;
    let old = data
        .bindings
        .iter()
        .find(|b| b.provider == provider)
        .map(|b| b.executable.clone());
    if let Some(old) = &old {
        if old == &executable {
            return Ok(super::BindOutcome::Unchanged);
        }
        if !replace_existing {
            return Err(Error::AlreadyBound {
                provider,
                old: old.clone(),
            });
        }
    }
    crate::provider::validate(provider, &executable, root)?;
    data.version = 2;
    data.revision = data.revision.checked_add(1).ok_or(Error::Exhausted)?;
    data.bindings.retain(|b| b.provider != provider);
    data.bindings.push(super::Binding {
        provider,
        executable,
    });
    data.bindings
        .sort_by_key(|b| Provider::ALL.iter().position(|p| *p == b.provider));
    let bytes = serialize(&data)?;
    let mut applied = false;
    let mut created = false;
    let result = persist(root, &bytes, &mut applied, &mut created);
    if result.is_err() && !applied && created {
        let _ = fs::remove_file(root.join("registry.json.tmp"));
    }
    result?;
    Ok(match old {
        Some(old) => super::BindOutcome::Replaced { old },
        None => super::BindOutcome::Bound,
    })
}
