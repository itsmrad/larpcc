pub mod display;
mod error;
mod model;
mod paths;
mod store;
#[cfg(test)]
mod tests;
pub use error::{Error, NameError};
pub use model::LaunchTarget;
pub use model::{
    BindOutcome, Binding, BindingState, DirStatus, Profile, ProfileEntry, ProfileId, ProfileName,
    Provider,
};
use std::path::{Path, PathBuf};
pub struct Registry {
    root: PathBuf,
}
impl Registry {
    pub fn bindings(&self) -> Result<Vec<(Provider, BindingState)>, Error> {
        let _ancestors = crate::platform::anchor(self.root())?.ancestors;
        let _fixed = store::fixed(self.root())?;
        let data = store::load(self.root())?;
        Ok(Provider::ALL
            .into_iter()
            .map(|provider| {
                let state = match data.bindings.iter().find(|b| b.provider == provider) {
                    None => BindingState::Unbound,
                    Some(binding) => {
                        match crate::provider::validate(provider, &binding.executable, self.root())
                        {
                            Ok(_) => BindingState::Ok(binding.clone()),
                            Err(
                                Error::InvalidExecutable { reason, .. }
                                | Error::Unsafe { reason, .. },
                            ) => BindingState::Invalid(binding.clone(), reason),
                            Err(_) => BindingState::Invalid(
                                binding.clone(),
                                "cannot resolve a trusted executable path".into(),
                            ),
                        }
                    }
                };
                (provider, state)
            })
            .collect())
    }
    pub fn bind(&self, p: Provider, exe: PathBuf, replace: bool) -> Result<BindOutcome, Error> {
        store::bind(self.root(), p, exe, replace)
    }
    pub(crate) fn launch_target(
        &self,
        name: &ProfileName,
    ) -> Result<crate::launch::admission::AdmissionSnapshot, Error> {
        self.admission_snapshot(name, |_| true)
    }
    pub(crate) fn admission_snapshot(
        &self,
        name: &ProfileName,
        candidate: impl Fn(Provider) -> bool,
    ) -> Result<crate::launch::admission::AdmissionSnapshot, Error> {
        let _ancestors = crate::platform::anchor(self.root())?.ancestors;
        let mut held = store::fixed(self.root())?;
        held.extend(_ancestors);
        let data = store::load(self.root())?;
        let profile = data
            .profiles
            .iter()
            .find(|p| &p.name == name)
            .cloned()
            .ok_or_else(|| Error::NotFound { name: name.clone() })?;
        if !candidate(profile.provider) {
            return Err(Error::QualificationPending);
        }
        let entries = paths::entries(self.root(), &data.profiles)?;
        for entry in &entries {
            if let DirStatus::Unsafe(reason) = &entry.status {
                return Err(Error::Unsafe {
                    path: entry.state_dir.clone(),
                    reason: reason.clone(),
                });
            }
        }
        let state_dir = self.root().join("profiles").join(profile.id.to_string());
        if !entries
            .iter()
            .any(|e| e.profile.id == profile.id && e.status == DirStatus::Ok)
        {
            return Err(Error::HomeUnavailable {
                name: name.clone(),
                status: DirStatus::Missing,
            });
        }
        let mut homes = Vec::new();
        for entry in &entries {
            if let Some(object) =
                crate::platform::inspect(&entry.state_dir, crate::platform::ObjectKind::Directory)?
            {
                homes.push((entry.profile.id, object.identity.clone()));
                held.push(object);
            }
        }
        let binding = data
            .bindings
            .iter()
            .find(|b| b.provider == profile.provider)
            .ok_or(Error::Unbound {
                provider: profile.provider,
            })?;
        let (resolved_executable, identity) =
            crate::provider::validate(profile.provider, &binding.executable, self.root())?;
        Ok(crate::launch::admission::AdmissionSnapshot {
            revision: data.revision,
            homes,
            held,
            target: LaunchTarget {
                profile,
                state_dir,
                stored_executable: binding.executable.clone(),
                resolved_executable,
                identity,
            },
        })
    }

    pub(crate) fn admission_lock(
        &self,
    ) -> Result<(std::fs::File, Vec<crate::platform::CheckedObject>), Error> {
        store::admission_lock(self.root())
    }
    pub fn locate() -> Result<Registry, Error> {
        Self::at(paths::resolve(|key| std::env::var_os(key))?)
    }
    pub fn at(root: PathBuf) -> Result<Registry, Error> {
        paths::validate(&root)?;
        // Lexical components remove trailing separators/dots that hide a root symlink.
        Ok(Self {
            root: root.components().collect(),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn list(&self) -> Result<Vec<ProfileEntry>, Error> {
        let _ancestors = crate::platform::anchor(self.root())?.ancestors;
        let _fixed = store::fixed(self.root())?;
        let data = store::load(self.root())?;
        let mut entries = paths::entries(&self.root, &data.profiles)?;
        entries.sort_by(|a, b| a.profile.name.cmp(&b.profile.name));
        Ok(entries)
    }
    pub fn get(&self, name: &ProfileName) -> Result<ProfileEntry, Error> {
        self.list()?
            .into_iter()
            .find(|e| &e.profile.name == name)
            .ok_or_else(|| Error::NotFound { name: name.clone() })
    }
    pub fn add(&self, name: ProfileName, provider: Provider) -> Result<ProfileEntry, Error> {
        store::mutate(
            self.root(),
            store::Mutation::Add(name, provider),
            std::time::Duration::from_secs(10),
        )
    }
    pub fn rename(&self, from: &ProfileName, to: ProfileName) -> Result<ProfileEntry, Error> {
        store::mutate(
            self.root(),
            store::Mutation::Rename(from.clone(), to),
            std::time::Duration::from_secs(10),
        )
    }
    pub fn forget(&self, name: &ProfileName) -> Result<ProfileEntry, Error> {
        store::mutate(
            self.root(),
            store::Mutation::Forget(name.clone()),
            std::time::Duration::from_secs(10),
        )
    }
}
