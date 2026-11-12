#![allow(dead_code)] // Seed surface.
use crate::platform::FileIdentity;
use std::{ffi::OsString, path::PathBuf};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    pub executable: PathBuf,
    pub argv0: OsString,
    pub argv: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env_delta: Vec<(OsString, OsString)>,
    pub identity: FileIdentity,
}
