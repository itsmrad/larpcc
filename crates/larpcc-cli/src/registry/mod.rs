mod error;
mod model;
pub use error::{Error, NameError};
pub use model::{DirStatus, Profile, ProfileEntry, ProfileId, ProfileName, Provider};
use std::path::{Path, PathBuf};
pub struct Registry {
    root: PathBuf,
}
impl Registry {
    pub fn locate() -> Result<Registry, Error> {
        let _ = ();
        unimplemented!("stage 1 registry")
    }
    pub fn at(root: PathBuf) -> Result<Registry, Error> {
        let _ = (root,);
        unimplemented!("stage 1 registry")
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn list(&self) -> Result<Vec<ProfileEntry>, Error> {
        let _ = (self,);
        unimplemented!("stage 1 registry")
    }
    pub fn get(&self, name: &ProfileName) -> Result<ProfileEntry, Error> {
        let _ = (self, name);
        unimplemented!("stage 1 registry")
    }
    pub fn add(&self, name: ProfileName, provider: Provider) -> Result<ProfileEntry, Error> {
        let _ = (self, name, provider);
        unimplemented!("stage 1 registry")
    }
    pub fn rename(&self, from: &ProfileName, to: ProfileName) -> Result<ProfileEntry, Error> {
        let _ = (self, from, to);
        unimplemented!("stage 1 registry")
    }
    pub fn forget(&self, name: &ProfileName) -> Result<ProfileEntry, Error> {
        let _ = (self, name);
        unimplemented!("stage 1 registry")
    }
}
