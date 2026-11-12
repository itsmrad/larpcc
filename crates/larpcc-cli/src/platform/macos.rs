use crate::registry::Error;
use libc::{c_char, c_int, c_void};
use std::{ffi::CString, io, os::unix::ffi::OsStrExt, path::Path};
type Acl = *mut c_void;
type Entry = *mut c_void;
const ACL_TYPE_EXTENDED: c_int = 0x100;
const ACL_EXTENDED_ALLOW: c_int = 1;
const ACL_EXTENDED_DENY: c_int = 2;
const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = -1;
unsafe extern "C" {
    fn acl_get_fd_np(fd: c_int, kind: c_int) -> Acl;
    fn acl_get_link_np(path: *const c_char, kind: c_int) -> Acl;
    fn acl_valid(acl: Acl) -> c_int;
    fn acl_get_entry(acl: Acl, entry_id: c_int, entry: *mut Entry) -> c_int;
    fn acl_get_tag_type(entry: Entry, tag: *mut c_int) -> c_int;
    fn acl_free(acl: *mut c_void) -> c_int;
}
struct OwnedAcl(Acl);
impl Drop for OwnedAcl {
    fn drop(&mut self) {
        // SAFETY: acquisition returned one owned ACL allocation; entries are borrowed.
        unsafe {
            acl_free(self.0);
        }
    }
}
pub(super) fn check(path: &Path) -> Result<(), Error> {
    let fail = |reason: &str| Error::Unsafe {
        path: path.into(),
        reason: reason.into(),
    };
    let name = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| fail("access control cannot be verified"))?;
    // SAFETY: path is NUL-terminated and lives through the call; result is owned.
    let raw = unsafe { acl_get_link_np(name.as_ptr(), ACL_TYPE_EXTENDED) };
    validate(raw, path)
}
pub(super) fn check_file(file: &std::fs::File, path: &Path) -> Result<(), Error> {
    use std::os::fd::AsRawFd;
    // SAFETY: File owns a live descriptor throughout acquisition; returned ACL is owned.
    let raw = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    validate(raw, path)
}
fn validate(raw: Acl, path: &Path) -> Result<(), Error> {
    let fail = |reason: &str| Error::Unsafe {
        path: path.into(),
        reason: reason.into(),
    };
    if raw.is_null() {
        return Err(fail("access control cannot be verified"));
    }
    let acl = OwnedAcl(raw);
    // SAFETY: acl owns a live ACL allocation.
    if unsafe { acl_valid(acl.0) } != 0 {
        return Err(fail("access control cannot be verified"));
    }
    let mut which = ACL_FIRST_ENTRY;
    loop {
        let mut entry = std::ptr::null_mut();
        // SAFETY: ACL is validated and output pointer is initialized writable storage.
        let result = unsafe { acl_get_entry(acl.0, which, &mut entry) };
        if result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
            return Ok(());
        }
        if result != 0 || entry.is_null() {
            return Err(fail("access control cannot be verified"));
        }
        let mut tag = 0;
        // SAFETY: successful iteration returned a borrowed live entry; tag is writable.
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
            return Err(fail("access control cannot be verified"));
        }
        match tag {
            ACL_EXTENDED_ALLOW => return Err(fail("has an extended ACL that grants access")),
            ACL_EXTENDED_DENY => (),
            _ => return Err(fail("access control cannot be verified")),
        }
        which = ACL_NEXT_ENTRY;
    }
}
