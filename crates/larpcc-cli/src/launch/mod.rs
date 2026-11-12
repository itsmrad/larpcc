#![allow(dead_code)] // Seed surface.
mod types;
use crate::registry::{Error, LaunchTarget};
use std::ffi::OsString;
pub use types::LaunchPlan;
pub fn plan(
    t: &LaunchTarget,
    _args: Vec<OsString>,
    _env: &[(OsString, OsString)],
) -> Result<LaunchPlan, Error> {
    Err(crate::platform::unavailable(&t.state_dir))
}
pub fn execute(plan: &LaunchPlan) -> Result<u32, Error> {
    Err(crate::platform::unavailable(&plan.executable))
}
