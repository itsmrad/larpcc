pub mod display;
mod error;
mod model;
mod paths;
mod store;
#[cfg(all(test, not(windows)))]
mod tests;
pub use error::{Error, NameError};
#[allow(unused_imports)] // Seed surface.
pub use model::{
    BindOutcome, Binding, BindingState, DirStatus, LaunchTarget, Profile, ProfileEntry, ProfileId,
    ProfileName, Provider,
};
use std::path::{Path, PathBuf};
pub struct Registry {
    root: PathBuf,
}
#[allow(dead_code)] // Seed surface.
impl Registry {
    pub fn bindings(&self) -> Result<Vec<(Provider, BindingState)>, Error> {
        Err(crate::platform::unavailable(self.root()))
    }
    pub fn bind(&self, _p: Provider, _exe: PathBuf, _replace: bool) -> Result<BindOutcome, Error> {
        Err(crate::platform::unavailable(self.root()))
    }
    #[cfg(feature = "synthetic-launch")]
    pub fn launch_target(&self, _name: &ProfileName) -> Result<LaunchTarget, Error> {
        Err(crate::platform::unavailable(self.root()))
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
