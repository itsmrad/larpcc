use crate::{platform::FileIdentity, registry::Error};
use std::{ffi::CStr, path::Path};

// The available Windows primitives do not establish durable, confined tree mutation.
pub(crate) enum Profiles {}

impl Profiles {
    pub(crate) fn open(_root: &Path) -> Result<Self, Error> {
        Err(Error::RecoveryRequired)
    }

    pub(crate) fn identity(&self) -> Result<FileIdentity, Error> {
        match *self {}
    }
    pub(crate) fn home(
        &self,
        _role: &str,
        _child: Option<&str>,
        _empty: bool,
    ) -> Result<Option<FileIdentity>, Error> {
        match *self {}
    }
    pub(crate) fn remove(&self, _role: &str, _expected: &FileIdentity) -> Result<(), Error> {
        match *self {}
    }
    pub(crate) fn has_only(&self, _role: &str, _allowed: &[&CStr]) -> Result<bool, Error> {
        match *self {}
    }
    pub(crate) fn make_container(
        &self,
        _role: &str,
    ) -> Result<(FileIdentity, FileIdentity), Error> {
        match *self {}
    }
    pub(crate) fn rename_home(
        &self,
        _from: &str,
        _to: &str,
        _container: Option<(&str, &FileIdentity)>,
        _expected: &FileIdentity,
        _empty: bool,
    ) -> Result<(), Error> {
        match *self {}
    }
    pub(crate) fn sync(&self, _role: Option<&str>) -> Result<(), Error> {
        match *self {}
    }
}
