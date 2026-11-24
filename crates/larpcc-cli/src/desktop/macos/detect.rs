//! Signed-bundle discovery for the CLI. The caller confines bundle paths before
//! any plist or signed-resource reads.
use super::{
    CFDictionaryGetValue, CFGetTypeID, CFStringCreateWithBytes, CFStringGetCString,
    CFStringGetTypeID, Owned, Ref, app_anchor, app_file, validate,
};
use crate::registry::{Provider, display};
use serde_json::{Value, json};
use std::{ffi::CStr, io::Read, path::Path, ptr};

const MAX_PLIST_BYTES: u64 = 256 * 1024;
const UTF8: u32 = 0x08000100;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    fn CFPropertyListCreateWithData(
        allocator: Ref,
        data: Ref,
        options: usize,
        format: *mut isize,
        error: *mut Ref,
    ) -> Ref;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFStringGetLength(value: Ref) -> isize;
}

pub(in crate::desktop) fn collect(
    provider: Provider,
    path: &Path,
    resolved: &Path,
    confined: bool,
) -> Value {
    let plist = confined.then(|| read_plist(resolved)).flatten();
    let (bundle_id, version) = match plist {
        Some(plist) => (
            plist_string(&plist, b"CFBundleIdentifier"),
            plist_string(&plist, b"CFBundleShortVersionString"),
        ),
        None => (None, None),
    };
    let requirement = match (provider, confined) {
        (Provider::Claude, _) | (_, false) => "unavailable",
        (Provider::Codex, true) if validate(resolved, None, true).is_ok() => "passed",
        (Provider::Codex, true) => "failed",
    };
    json!({
        "provider": provider,
        "path": display::path(path),
        "resolved_path": display::path(resolved),
        "bundle_id": bundle_id,
        "version": version,
        "code_requirement": requirement,
    })
}

fn read_plist(app: &Path) -> Option<Owned> {
    let path = app.join("Contents/Info.plist");
    let _ancestry = app_anchor(&path).ok()?;
    let info = app_file(&path).ok()?;
    let mut bytes = Vec::new();
    (&info.file)
        .take(MAX_PLIST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).ok()? > MAX_PLIST_BYTES {
        return None;
    }
    // SAFETY: CFDataCreate copies the bounded slice. The plist is immutable and
    // null output pointers request neither a format nor an error object.
    unsafe {
        let data = Owned::new(CFDataCreate(
            ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
        ))
        .ok()?;
        let plist = Owned::new(CFPropertyListCreateWithData(
            ptr::null(),
            data.0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        ))
        .ok()?;
        (CFGetTypeID(plist.0) == CFDictionaryGetTypeID()).then_some(plist)
    }
}

fn plist_string(plist: &Owned, key: &[u8]) -> Option<String> {
    // SAFETY: the dictionary is held by the caller, and string conversions use
    // a typed borrowed value and the actual fixed buffer length.
    unsafe {
        let key = Owned::new(CFStringCreateWithBytes(
            ptr::null(),
            key.as_ptr(),
            key.len() as isize,
            UTF8,
            0,
        ))
        .ok()?;
        let value = CFDictionaryGetValue(plist.0, key.0);
        if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
            return None;
        }
        let mut buffer = [0i8; 129];
        if CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) == 0 {
            return None;
        }
        let bytes = CStr::from_ptr(buffer.as_ptr()).to_bytes();
        let length = usize::try_from(CFStringGetLength(value)).ok()?;
        if bytes.len() != length
            || bytes.is_empty()
            || !bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(b))
        {
            return None;
        }
        String::from_utf8(bytes.to_vec()).ok()
    }
}
