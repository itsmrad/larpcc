use super::*;
pub fn anchor(path: &Path) -> Result<AnchoredPath, Error> {
    Err(unavailable(path))
}
pub fn inspect(path: &Path, _kind: ObjectKind) -> Result<Option<CheckedObject>, Error> {
    Err(unavailable(path))
}
pub fn inspect_home(path: &Path) -> Result<DirStatus, Error> {
    Err(unavailable(path))
}
pub fn create_file(path: &Path) -> Result<CheckedObject, Error> {
    Err(unavailable(path))
}
pub fn create_directory(path: &Path) -> Result<CheckedObject, Error> {
    Err(unavailable(path))
}
pub fn executable(path: &Path, _root: &Path) -> Result<(PathBuf, FileIdentity), Error> {
    Err(unavailable(path))
}
#[cfg(feature = "synthetic-launch")]
pub fn spawn_wait(plan: &crate::launch::LaunchPlan) -> Result<u32, Error> {
    Err(unavailable(&plan.executable))
}
