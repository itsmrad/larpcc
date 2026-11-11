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
}
impl Default for Data {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            next_id: 1,
            profiles: Vec::new(),
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
        serde_json::from_slice(bytes).map_err(|e| corrupt(e.to_string()))?;
    let version = probe
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| corrupt("missing or invalid version".into()))?;
    if version > 1 {
        return Err(Error::NewerSchema {
            path: path.into(),
            found: version,
        });
    }
    let data: Data = serde_json::from_slice(bytes).map_err(|e| corrupt(e.to_string()))?;
    if data.version != 1 || data.next_id == 0 {
        return Err(corrupt("invalid version or next_id".into()));
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
    if paths::inspect(root, true)?.is_none() {
        return Ok(Data::default());
    }
    paths::inspect(&root.join("profiles"), true)?;
    let path = root.join("registry.json");
    let Some(meta) = paths::inspect(&path, false)? else {
        return Ok(Data::default());
    };
    if meta.len() > LIMIT as u64 {
        return Err(Error::Corrupt {
            path,
            reason: "exceeds 1 MiB".into(),
        });
    }
    let file = File::open(&path).map_err(|e| io("open", &path, e))?;
    let bytes = bounded(file).map_err(|e| io("read", &path, e))?;
    parse(&path, &bytes)
}
pub(super) fn serialize(data: &Data) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    serde_json::to_writer_pretty(&mut bytes, data).map_err(|e| {
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
fn lock(root: &Path, timeout: Duration) -> Result<File, Error> {
    if paths::inspect(root, true)?.is_none()
        && paths::mkdir(root)?
        && let Some(parent) = root.parent()
    {
        paths::sync(parent)?;
    }
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
    match paths::options().create_new(true).open(&path) {
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(io("open", &path, e)),
    }
    paths::inspect(&path, false)?;
    let file = paths::options()
        .open(&path)
        .map_err(|e| io("open", &path, e))?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
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
    let _lock = lock(root, timeout)?;
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
    data.revision = data.revision.checked_add(1).ok_or(Error::Exhausted)?;
    let parent = root.join("profiles");
    let mut created_home = None;
    let mut created_temp = false;
    let temp = root.join("registry.json.tmp");
    let target = root.join("registry.json");
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
                paths::inspect(&parent, true)?;
                // Unlike parent creation, an existing candidate is always an orphan.
                #[cfg(unix)]
                let mut builder = fs::DirBuilder::new();
                #[cfg(not(unix))]
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                match builder.create(&home) {
                    Ok(()) => {
                        created_home = Some(home);
                        paths::sync(&parent)?;
                        data.profiles.push(selected.clone());
                        break bytes;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        selected.id = ProfileId::new(successor);
                    }
                    Err(e) => return Err(io("create directory", &home, e)),
                }
            }
        } else {
            serialize(&data)?
        };
        match fs::remove_file(&temp) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(io("remove", &temp, e)),
        }
        let mut file = paths::options()
            .create_new(true)
            .open(&temp)
            .map_err(|e| io("open", &temp, e))?;
        created_temp = true;
        #[cfg(test)]
        phase("write", &temp)?;
        file.write_all(&bytes).map_err(|e| io("write", &temp, e))?;
        #[cfg(test)]
        phase("file sync", &temp)?;
        file.sync_all().map_err(|e| io("sync", &temp, e))?;
        drop(file);
        #[cfg(test)]
        phase("replace", &target)?;
        replace(&temp, &target)?;
        applied = true;
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
