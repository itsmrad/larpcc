use super::{Error, ProfileName, paths, store};
use crate::{platform, remote::Host};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path, time::Duration};

const FILE: &str = "hosts.json";
const TEMP: &str = "hosts.json.tmp";
const MAX_HOSTS: usize = 32;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hosts {
    version: u64,
    revision: String,
    pub hosts: Vec<Host>,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            version: 1,
            revision: "0".into(),
            hosts: Vec::new(),
        }
    }
}

fn corrupt(path: &Path) -> Error {
    Error::Corrupt {
        path: path.into(),
        reason: "invalid host store".into(),
    }
}

fn load(root: &Path) -> Result<Hosts, Error> {
    let path = root.join(FILE);
    let Some(object) =
        platform::acquire_replaceable(&path, || platform::inspect_replaceable(&path))?
    else {
        return Ok(Hosts::default());
    };
    platform::single_link(&object.file)?;
    let bytes = store::bounded(object.file).map_err(|e| paths::io("read", &path, e))?;
    if bytes.len() > store::LIMIT {
        return Err(corrupt(&path));
    }
    let data: Hosts = serde_json::from_slice(&bytes).map_err(|_| corrupt(&path))?;
    if data.version != 1
        || data.revision.parse::<u64>().map(|r| r.to_string()).as_ref() != Ok(&data.revision)
        || data.hosts.len() > MAX_HOSTS
        || data.hosts.iter().any(|host| !host.valid())
        || data
            .hosts
            .windows(2)
            .any(|p| p[0].name.as_str() >= p[1].name.as_str())
    {
        return Err(corrupt(&path));
    }
    Ok(data)
}

fn persist(root: &Path, data: &Hosts) -> Result<(), Error> {
    let tmp = root.join(TEMP);
    let target = root.join(FILE);
    let bytes = store::capacity(serde_json::to_vec(data).map_err(|_| corrupt(&target))?)?;
    if let Some(object) =
        platform::acquire_replaceable(&tmp, || platform::inspect_replaceable(&tmp))?
    {
        platform::single_link(&object.file)?;
        drop(object);
        fs::remove_file(&tmp).map_err(|e| paths::io("remove", &tmp, e))?;
    }
    let mut file = platform::create_file(&tmp)?.file;
    let written = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = written
        .map_err(|e| paths::io("write", &tmp, e))
        .and_then(|()| store::replace(&tmp, &target));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    paths::sync(root)
}

impl Hosts {
    pub fn revision(&self) -> u64 {
        self.revision.parse().expect("validated host revision")
    }

    fn require(&self, expected: Option<u64>) -> Result<(), Error> {
        if expected.is_some_and(|revision| revision != self.revision()) {
            return Err(Error::Conflict);
        }
        Ok(())
    }

    fn position(&self, name: &ProfileName) -> Result<usize, usize> {
        self.hosts
            .binary_search_by(|host| host.name.as_str().cmp(name.as_str()))
    }

    pub fn get(&self, name: &ProfileName) -> Result<&Host, Error> {
        self.position(name)
            .map(|index| &self.hosts[index])
            .map_err(|_| Error::NotFound { name: name.clone() })
    }
}

pub(crate) fn with_snapshot<T>(
    root: &Path,
    expected: Option<u64>,
    read: impl FnOnce(Hosts) -> Result<T, Error>,
) -> Result<T, Error> {
    let _fixed = store::fixed(root)?;
    let lock = root.join("registry.lock");
    let guard = if platform::inspect(&lock, platform::ObjectKind::File)?.is_none() {
        let path = root.join(FILE);
        if platform::acquire_replaceable(&path, || platform::inspect_replaceable(&path))?.is_none()
        {
            let data = Hosts::default();
            data.require(expected)?;
            return read(data);
        }
        store::admission_lock(root)?
    } else {
        store::admission_lock(root)?
    };
    let data = load(root)?;
    data.require(expected)?;
    let result = read(data);
    drop(guard);
    result
}

pub(crate) struct Route {
    pub host: Host,
    pub revision: u64,
}

impl Route {
    pub fn resolve(root: &Path, expected: Option<u64>, name: &ProfileName) -> Result<Self, Error> {
        with_snapshot(root, expected, |data| Self::from_snapshot(&data, name))
    }

    pub fn from_snapshot(data: &Hosts, name: &ProfileName) -> Result<Self, Error> {
        Ok(Self {
            host: data.get(name)?.clone(),
            revision: data.revision(),
        })
    }

    pub fn through_spawn<T>(&self, root: &Path, spawn: impl FnOnce() -> T) -> Result<T, Error> {
        let _guard = store::admission_lock(root).map_err(|error| match error {
            Error::AdmissionChanged => Error::Conflict,
            other => other,
        })?;
        let data = load(root)?;
        data.require(Some(self.revision))?;
        if data.get(&self.host.name).ok() != Some(&self.host) {
            return Err(Error::Conflict);
        }
        Ok(spawn())
    }
}

pub(crate) fn add(root: &Path, expected: Option<u64>, host: Host) -> Result<u64, Error> {
    change(root, expected, |data| {
        let index = data.position(&host.name).err().ok_or(Error::Conflict)?;
        if data.hosts.len() == MAX_HOSTS {
            return Err(Error::TooLarge);
        }
        if !host.valid() {
            return Err(corrupt(&root.join(FILE)));
        }
        data.hosts.insert(index, host);
        Ok(())
    })
}

pub(crate) fn remove(root: &Path, expected: Option<u64>, name: &ProfileName) -> Result<u64, Error> {
    change(root, expected, |data| {
        let index = data
            .position(name)
            .map_err(|_| Error::NotFound { name: name.clone() })?;
        data.hosts.remove(index);
        Ok(())
    })
}

fn change(
    root: &Path,
    expected: Option<u64>,
    update: impl FnOnce(&mut Hosts) -> Result<(), Error>,
) -> Result<u64, Error> {
    let _guard = store::lock(root, Duration::from_secs(10))?;
    let mut data = load(root)?;
    data.require(expected)?;
    update(&mut data)?;
    let revision = data.revision().checked_add(1).ok_or(Error::Exhausted)?;
    data.revision = revision.to_string();
    persist(root, &data)?;
    Ok(revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Address;

    #[test]
    fn stage13_revision_removed_and_readded_route_cannot_spawn() {
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
        let host = Host {
            name: "codex-45".parse().unwrap(),
            ssh: Address::config("route.test".into()).unwrap(),
            larp: None,
            larpcc_home: None,
            identity_file: None,
        };
        assert_eq!(add(&root, Some(0), host.clone()).unwrap(), 1);
        let route = Route::resolve(&root, Some(1), &host.name).unwrap();
        assert_eq!(route.through_spawn(&root, || 17).unwrap(), 17);
        assert_eq!(remove(&root, Some(1), &host.name).unwrap(), 2);
        assert_eq!(add(&root, Some(2), host.clone()).unwrap(), 3);
        assert!(matches!(
            route.through_spawn(&root, || panic!("stale pinned host spawned a process")),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            Route::resolve(&root, Some(1), &"claude-work".parse().unwrap()),
            Err(Error::Conflict)
        ));
    }
}
