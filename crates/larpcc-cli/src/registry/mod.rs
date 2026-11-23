mod alias;
pub(crate) use alias::AliasPin;
mod desktop;
pub mod display;
mod error;
mod json;
mod model;
pub(crate) use model::parse_recorded_version;
mod paths;
mod skills;
mod store;
#[allow(unused_imports)]
pub use skills::SkillsAction;
#[cfg(test)]
mod tests;
pub use error::{Error, NameError};
pub use model::{
    BindOutcome, Binding, BindingState, DirStatus, Profile, ProfileEntry, ProfileId, ProfileName,
    Provider,
};
pub use model::{LaunchTarget, ProfileState, Surface};
use std::path::{Path, PathBuf};
pub struct Registry {
    root: PathBuf,
    expected: Option<u64>,
    pub(crate) alias_pin: Option<AliasPin>,
    observed: std::cell::Cell<Option<u64>>,
}
impl Registry {
    pub fn expecting(mut self, revision: Option<u64>) -> Self {
        self.expected = revision;
        self
    }
    pub(crate) fn pin_alias(mut self, pin: AliasPin) -> Self {
        self.expected = Some(pin.revision);
        self.alias_pin = Some(pin);
        self
    }
    pub fn surface(
        &self,
        name: ProfileName,
        surface: Surface,
        enabled: bool,
    ) -> Result<(ProfileEntry, bool), Error> {
        store::mutate_checked(
            self.root(),
            store::Mutation::Surface(name, surface, enabled),
            std::time::Duration::from_secs(10),
            self.expected,
        )
        .map(|(entry, revision, changed)| {
            self.observed.set(Some(revision));
            (entry, changed)
        })
    }

    pub fn bindings(&self) -> Result<Vec<(Provider, BindingState)>, Error> {
        let _ancestors = crate::platform::anchor(self.root())?.ancestors;
        let _fixed = store::fixed(self.root())?;
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        self.observed.set(Some(data.revision));
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
        store::bind(self.root(), p, exe, replace, self.expected).map(|(outcome, revision)| {
            self.observed.set(Some(revision));
            outcome
        })
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
        self.observed.set(Some(data.revision));
        let profile = data
            .profiles
            .iter()
            .find(|p| &p.name == name)
            .cloned()
            .ok_or_else(|| Error::NotFound { name: name.clone() })?;
        if let Some(pin) = &self.alias_pin {
            if profile.id != pin.profile_id() {
                return Err(Error::AdmissionChanged);
            }
            held.extend(
                pin.check(self.root(), &data)
                    .map_err(|_| Error::AdmissionChanged)?,
            );
        }
        if !candidate(profile.provider) {
            return Err(Error::QualificationPending);
        }
        store::require_v3(&data)?;
        if profile.state != Some(ProfileState::Active) || !profile.surfaces.is_some_and(|s| s.cli) {
            return Err(Error::RecoveryRequired);
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
        let state_dir = self
            .root()
            .join("profiles")
            .join(profile.id.to_string())
            .join("cli");
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
            held.extend(crate::platform::anchor(&entry.state_dir)?.ancestors);
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
            expected: None,
            alias_pin: None,
            observed: std::cell::Cell::new(None),
        })
    }
    pub fn revision(&self) -> Option<u64> {
        self.observed.get()
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn list(&self) -> Result<Vec<ProfileEntry>, Error> {
        let _ancestors = crate::platform::anchor(self.root())?.ancestors;
        let _fixed = store::fixed(self.root())?;
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        self.observed.set(Some(data.revision));
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
        store::mutate_checked(
            self.root(),
            store::Mutation::Add(name, provider),
            std::time::Duration::from_secs(10),
            self.expected,
        )
        .map(|(entry, revision, _)| {
            self.observed.set(Some(revision));
            entry
        })
    }
    pub fn rename(&self, from: &ProfileName, to: ProfileName) -> Result<ProfileEntry, Error> {
        store::mutate_checked(
            self.root(),
            store::Mutation::Rename(from.clone(), to),
            std::time::Duration::from_secs(10),
            self.expected,
        )
        .map(|(entry, revision, _)| {
            self.observed.set(Some(revision));
            entry
        })
    }
    pub fn forget(&self, name: &ProfileName) -> Result<ProfileEntry, Error> {
        store::mutate_checked(
            self.root(),
            store::Mutation::Forget(name.clone()),
            std::time::Duration::from_secs(10),
            self.expected,
        )
        .map(|(entry, revision, _)| {
            self.observed.set(Some(revision));
            entry
        })
    }
}
