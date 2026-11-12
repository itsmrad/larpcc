// Darwin C-enum ABI; native SDK and iterator validation remain acceptance gates.
#![allow(dead_code)]
use libc::{c_char, c_int, c_void};
type Acl = *mut c_void;
type Entry = *mut c_void;
const ACL_TYPE_EXTENDED: c_int = 0x100;
const ACL_EXTENDED_ALLOW: c_int = 1;
const ACL_EXTENDED_DENY: c_int = 2;
const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = -1;
unsafe extern "C" {
    fn acl_get_link_np(path: *const c_char, kind: c_int) -> Acl;
    fn acl_get_entry(acl: Acl, entry_id: c_int, entry: *mut Entry) -> c_int;
    fn acl_get_tag_type(entry: Entry, tag: *mut c_int) -> c_int;
    fn acl_free(acl: *mut c_void) -> c_int;
}
