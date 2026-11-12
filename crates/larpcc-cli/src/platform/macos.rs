use crate::registry::Error;
use libc::{c_int, c_void};
use std::{
    fs::{File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};
type Acl = *mut c_void;
type Entry = *mut c_void;
type FileSec = *mut c_void;
const FILESEC_ACL: c_int = 5;
const ACL_EXTENDED_ALLOW: c_int = 1;
const ACL_EXTENDED_DENY: c_int = 2;
const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = -1;
unsafe extern "C" {
    fn filesec_init() -> FileSec;
    fn filesec_free(security: FileSec);
    fn filesec_query_property(security: FileSec, property: c_int, present: *mut c_int) -> c_int;
    fn filesec_get_property(security: FileSec, property: c_int, value: *mut c_void) -> c_int;
    #[cfg_attr(target_arch = "x86_64", link_name = "fstatx_np$INODE64")]
    fn fstatx_np(fd: c_int, metadata: *mut libc::stat, security: FileSec) -> c_int;
    fn acl_valid(acl: Acl) -> c_int;
    fn acl_get_entry(acl: Acl, entry_id: c_int, entry: *mut Entry) -> c_int;
    fn acl_get_tag_type(entry: Entry, tag: *mut c_int) -> c_int;
    fn acl_free(acl: *mut c_void) -> c_int;
}
// Apple Libc filesec.c: filesec owns its security buffer; FILESEC_ACL getter
// returns an independent acl_copy_int_native allocation, not a borrowed pointer.
struct OwnedFileSec(FileSec);
impl Drop for OwnedFileSec {
    fn drop(&mut self) {
        // SAFETY: filesec_init transferred exactly one live allocation to this owner.
        unsafe {
            filesec_free(self.0);
        }
        #[cfg(test)]
        allocation_event(1);
    }
}
struct OwnedAcl(Acl);
impl Drop for OwnedAcl {
    fn drop(&mut self) {
        // SAFETY: filesec_get_property copied one independently owned ACL allocation.
        // Record successful native frees in isolated test-thread counters only.
        let _result = unsafe { acl_free(self.0) };
        #[cfg(test)]
        if _result == 0 {
            allocation_event(3);
        }
    }
}
fn refused(path: &Path) -> Error {
    Error::Unsafe {
        path: path.into(),
        reason: "access control cannot be verified".into(),
    }
}
pub(super) fn check(path: &Path) -> Result<(), Error> {
    let before = std::fs::symlink_metadata(path).map_err(|_| refused(path))?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| refused(path))?;
    let opened = file.metadata().map_err(|_| refused(path))?;
    if before.dev() != opened.dev()
        || before.ino() != opened.ino()
        || before.uid() != opened.uid()
        || before.gid() != opened.gid()
        || before.mode() != opened.mode()
    {
        return Err(refused(path));
    }
    check_file(&file, path)
}
pub(super) fn check_file(file: &File, path: &Path) -> Result<(), Error> {
    let fd = file.as_raw_fd();
    let ordinary = file.metadata().map_err(|_| refused(path))?;
    if !ordinary.is_file() && !ordinary.is_dir() {
        return Err(refused(path));
    }
    // SAFETY: File retains the descriptor; this query reports actual mount ACL support.
    if unsafe { libc::fpathconf(fd, libc::_PC_EXTENDED_SECURITY_NP) } != 1 {
        return Err(Error::Unsafe {
            path: path.into(),
            reason: "access control cannot be verified on this volume".into(),
        });
    }
    check_fd(fd, &ordinary, path)
}
fn check_fd(fd: c_int, ordinary: &std::fs::Metadata, path: &Path) -> Result<(), Error> {
    // SAFETY: filesec_init has no arguments and returns a new opaque allocation or null.
    let raw = unsafe { filesec_init() };
    if raw.is_null() {
        return Err(refused(path));
    }
    let security = OwnedFileSec(raw);
    #[cfg(test)]
    allocation_event(0);
    // SAFETY: zeroed libc::stat is valid writable output storage for the target Darwin ABI.
    let mut extended: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor, allocation and target-sized output remain live through the query.
    if unsafe { fstatx_np(fd, &mut extended, security.0) } != 0 {
        return Err(refused(path));
    }
    if extended.st_dev as u64 != ordinary.dev()
        || extended.st_ino != ordinary.ino()
        || extended.st_uid != ordinary.uid()
        || extended.st_gid != ordinary.gid()
        || u32::from(extended.st_mode) != ordinary.mode()
    {
        return Err(refused(path));
    }
    let mut present = 0;
    // SAFETY: security is initialized by successful fstatx_np; present is writable c_int storage.
    if unsafe { filesec_query_property(security.0, FILESEC_ACL, &mut present) } != 0 {
        return Err(refused(path));
    }
    // Absence is an explicit successful property query, never inferred from errno.
    if present == 0 {
        return Ok(());
    }
    let mut raw_acl: Acl = std::ptr::null_mut();
    // SAFETY: the getter writes one owned acl_t into correctly sized/aligned pointer storage.
    if unsafe { filesec_get_property(security.0, FILESEC_ACL, (&mut raw_acl as *mut Acl).cast()) }
        != 0
        || (raw_acl as usize) <= 1
    {
        return Err(refused(path));
    }
    let acl = OwnedAcl(raw_acl);
    #[cfg(test)]
    allocation_event(2);
    // SAFETY: the successful property copy supplied the ACL allocation retained above.
    if unsafe { acl_valid(acl.0) } != 0 {
        return Err(refused(path));
    }
    let mut which = ACL_FIRST_ENTRY;
    loop {
        let mut entry = std::ptr::null_mut();
        // SAFETY: the ACL is live and validated; output is initialized writable entry-pointer storage.
        let result = unsafe { acl_get_entry(acl.0, which, &mut entry) };
        let errno = io::Error::last_os_error();
        if result == -1 && errno.raw_os_error() == Some(libc::EINVAL) {
            return Ok(());
        }
        if result != 0 || entry.is_null() {
            return Err(refused(path));
        }
        let mut tag = 0;
        // SAFETY: successful iteration returned a borrowed live entry and tag is writable c_int storage.
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
            return Err(refused(path));
        }
        match tag {
            ACL_EXTENDED_ALLOW => {
                return Err(Error::Unsafe {
                    path: path.into(),
                    reason: "has an extended ACL that grants access".into(),
                });
            }
            ACL_EXTENDED_DENY => (),
            _ => return Err(refused(path)),
        }
        which = ACL_NEXT_ENTRY;
    }
}

#[cfg(test)]
thread_local! { static ALLOCATIONS:std::cell::Cell<[usize;4]>=const {std::cell::Cell::new([0;4])}; }
#[cfg(test)]
fn allocation_event(index: usize) {
    ALLOCATIONS.with(|counts| {
        let mut value = counts.get();
        value[index] += 1;
        counts.set(value);
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn counts() -> [usize; 4] {
        ALLOCATIONS.with(std::cell::Cell::get)
    }
    fn reset() {
        ALLOCATIONS.with(|counts| counts.set([0; 4]));
    }
    #[test]
    fn native_rust_acquisition_frees_independent_owners_on_absence_refusal_and_query_failure() {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
        let temporary = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let path = temporary.path();
        let file = File::open(path).unwrap();
        reset();
        check_file(&file, path).unwrap();
        assert_eq!(counts(), [1, 1, 0, 0]);
        reset();
        assert!(check_fd(-1, &file.metadata().unwrap(), path).is_err());
        assert_eq!(counts(), [1, 1, 0, 0]);
        for (grant, reject) in [
            ("everyone allow list", true),
            ("everyone deny delete", false),
        ] {
            assert!(
                std::process::Command::new("/bin/chmod")
                    .env_clear()
                    .args(["+a", grant])
                    .arg(path)
                    .status()
                    .unwrap()
                    .success()
            );
            reset();
            let result = check_file(&file, path);
            let observed = counts();
            // Always remove our synthetic ACL before assertions or TempDir cleanup.
            assert!(
                std::process::Command::new("/bin/chmod")
                    .env_clear()
                    .arg("-N")
                    .arg(path)
                    .status()
                    .unwrap()
                    .success()
            );
            if reject {
                assert!(
                    matches!(result,Err(Error::Unsafe {reason,..}) if reason=="has an extended ACL that grants access")
                );
            } else {
                result.unwrap();
            }
            assert_eq!(observed, [1, 1, 1, 1]);
        }
        reset();
        check_file(&file, path).unwrap();
        assert_eq!(counts(), [1, 1, 0, 0]);
    }
}
