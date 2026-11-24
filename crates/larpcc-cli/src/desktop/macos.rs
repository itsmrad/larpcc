use super::*;
use std::{
    ffi::c_void,
    fs::OpenOptions,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
};
type Ref = *const c_void;
// SecStaticCode.h: StrictValidate | CheckAllArchitectures | CheckNestedCode.
// Real-bundle timing proof is pending; keep the flags together for the budget decision.
const STATIC_CODE_FLAGS: u32 = (1 << 4) | (1 << 0) | (1 << 3);
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: Ref,
        bytes: *const u8,
        len: isize,
        directory: u8,
    ) -> Ref;
    fn CFStringCreateWithBytes(
        allocator: Ref,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> Ref;
    fn CFRelease(value: Ref);
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFDataGetTypeID() -> usize;
    fn CFDataGetLength(data: Ref) -> isize;
    fn CFDataGetBytePtr(data: Ref) -> *const u8;
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecStaticCodeCreateWithPath(url: Ref, flags: u32, code: *mut Ref) -> i32;
    fn SecRequirementCreateWithString(text: Ref, flags: u32, requirement: *mut Ref) -> i32;
    fn SecStaticCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
    fn SecCodeCopySigningInformation(code: Ref, flags: u32, information: *mut Ref) -> i32;
    static kSecCodeInfoUnique: Ref;
}
struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Result<Self, Error> {
        if value.is_null() {
            Err(Error::ArtifactInvalid)
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: each non-null Create result is released once by its sole owner.
        unsafe {
            CFRelease(self.0);
        }
    }
}
fn requirement() -> Result<Owned, Error> {
    // Reviewed current ChatGPT identity; no Claude identity or launch row is inferred.
    let requirement = br#"identifier "com.openai.codex" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = "2DC432GLL2""#;
    // SAFETY: fixed byte slice is live and its exact length is supplied.
    unsafe {
        let text = Owned::new(CFStringCreateWithBytes(
            std::ptr::null(),
            requirement.as_ptr(),
            requirement.len() as isize,
            0x08000100,
            0,
        ))?;
        let mut req = std::ptr::null();
        if SecRequirementCreateWithString(text.0, 0, &mut req) != 0 {
            return Err(Error::ArtifactInvalid);
        }
        Owned::new(req)
    }
}
pub(super) fn verify_signature(path: &Path) -> Result<(), Error> {
    let bytes = path.as_os_str().as_bytes();
    let req = requirement()?;
    // SAFETY: checked path bytes live across the call; initialized outputs follow Create ownership.
    unsafe {
        let url = Owned::new(CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        ))?;
        let mut code = std::ptr::null();
        if SecStaticCodeCreateWithPath(url.0, 0, &mut code) != 0 {
            return Err(Error::ArtifactInvalid);
        }
        let code = Owned::new(code)?;
        if SecStaticCodeCheckValidity(code.0, STATIC_CODE_FLAGS, req.0) != 0 {
            return Err(Error::ArtifactInvalid);
        }
    }
    spelling(path)
}
fn spelling(path: &Path) -> Result<(), Error> {
    for directory in [
        path.to_path_buf(),
        path.join("Contents"),
        path.join("Contents/MacOS"),
    ] {
        if !std::fs::symlink_metadata(directory)
            .map_err(|_| Error::ArtifactInvalid)?
            .file_type()
            .is_dir()
        {
            return Err(Error::ArtifactInvalid);
        }
    }
    let exe = path.join("Contents/MacOS/ChatGPT");
    if !std::fs::symlink_metadata(&exe)
        .map_err(|_| Error::ArtifactInvalid)?
        .file_type()
        .is_file()
    {
        return Err(Error::ArtifactInvalid);
    }
    Ok(())
}
fn code_directory_hash(path: &Path) -> Result<Vec<u8>, Error> {
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: Create/Copy results are owned; the data is borrowed only while its dictionary lives.
    unsafe {
        let url = Owned::new(CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        ))?;
        let mut code = std::ptr::null();
        if SecStaticCodeCreateWithPath(url.0, 0, &mut code) != 0 {
            return Err(Error::ArtifactInvalid);
        }
        let code = Owned::new(code)?;
        let mut info = std::ptr::null();
        if SecCodeCopySigningInformation(code.0, 1 << 1, &mut info) != 0 {
            return Err(Error::ArtifactInvalid);
        }
        let info = Owned::new(info)?;
        let hash = CFDictionaryGetValue(info.0, kSecCodeInfoUnique);
        if hash.is_null() || CFGetTypeID(hash) != CFDataGetTypeID() {
            return Err(Error::ArtifactInvalid);
        }
        let len = usize::try_from(CFDataGetLength(hash)).map_err(|_| Error::ArtifactInvalid)?;
        if !SIGNATURE_CDHASH_LENGTH.contains(&len) {
            return Err(Error::ArtifactInvalid);
        }
        let bytes = CFDataGetBytePtr(hash);
        if bytes.is_null() {
            return Err(Error::ArtifactInvalid);
        }
        Ok(std::slice::from_raw_parts(bytes, len).to_vec())
    }
}
fn file_key(object: &CheckedObject) -> Result<[i64; 7], Error> {
    let m = object
        .file
        .metadata()
        .map_err(|_| Error::AdmissionChanged)?;
    Ok([
        m.dev() as i64,
        m.ino() as i64,
        m.size() as i64,
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    ])
}
fn cache_key(
    path: &Path,
    executable: &CheckedObject,
    info: &CheckedObject,
) -> Result<SignatureCache, Error> {
    Ok(SignatureCache {
        version: SIGNATURE_CACHE_VERSION,
        executable: file_key(executable)?,
        info: file_key(info)?,
        cdhash: code_directory_hash(path)?,
    })
}
pub(super) fn validate(
    path: &Path,
    cached: Option<&SignatureCache>,
    bind: bool,
) -> Result<(CheckedApp, Option<SignatureCache>), Error> {
    spelling(path)?;
    let exe = path.join("Contents/MacOS/ChatGPT");
    let ancestry = app_anchor(&exe)?;
    let executable = app_file(&exe)?;
    let info = app_file(&path.join("Contents/Info.plist"))?;
    let executable_stamp = AppFileStamp::of(&executable)?;
    let info_stamp = AppFileStamp::of(&info)?;
    let key = cache_key(path, &executable, &info)?;
    if signature_check_needed(bind, cached, &key) {
        verify_signature(path)?;
    }
    if !bundle_value_matches(path, b"CFBundleIdentifier", b"com.openai.codex") {
        return Err(Error::ArtifactInvalid);
    }
    let app = CheckedApp {
        path: path.into(),
        ancestry,
        executable,
        info,
        executable_stamp,
        info_stamp,
    };
    app.recheck()?;
    if cache_key(path, &app.executable, &app.info)? != key {
        return Err(Error::AdmissionChanged);
    }
    Ok((app, Some(key)))
}

pub(super) mod dispatch;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFBundleCreate(allocator: Ref, url: Ref) -> Ref;
    fn CFBundleGetValueForInfoDictionaryKey(bundle: Ref, key: Ref) -> Ref;
    fn CFStringGetTypeID() -> usize;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetCString(value: Ref, buffer: *mut i8, size: isize, encoding: u32) -> u8;
}
fn tested_version(path: &Path) -> bool {
    bundle_value_matches(path, b"CFBundleShortVersionString", b"26.1111.21308")
}
fn bundle_value_matches(path: &Path, key: &[u8], expected: &[u8]) -> bool {
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: exact valid slice lengths, Create outputs uniquely owned; borrowed
    // Info.plist value stays within bundle lifetime. No native user state is read.
    unsafe {
        let Ok(url) = Owned::new(CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        )) else {
            return false;
        };
        let Ok(bundle) = Owned::new(CFBundleCreate(std::ptr::null(), url.0)) else {
            return false;
        };
        let Ok(key) = Owned::new(CFStringCreateWithBytes(
            std::ptr::null(),
            key.as_ptr(),
            key.len() as isize,
            0x08000100,
            0,
        )) else {
            return false;
        };
        let value = CFBundleGetValueForInfoDictionaryKey(bundle.0, key.0);
        if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
            return false;
        }
        let mut buffer = [0i8; 128];
        if CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            0x08000100,
        ) == 0
        {
            return false;
        }
        std::ffi::CStr::from_ptr(buffer.as_ptr()).to_bytes() == expected
    }
}
