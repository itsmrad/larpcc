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
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecStaticCodeCreateWithPath(url: Ref, flags: u32, code: *mut Ref) -> i32;
    fn SecRequirementCreateWithString(text: Ref, flags: u32, requirement: *mut Ref) -> i32;
    fn SecStaticCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
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
pub(super) fn verify_signature(path: &Path) -> Result<(), Error> {
    // Reviewed current ChatGPT identity; no Claude identity or launch row is inferred.
    let requirement = br#"identifier "com.openai.codex" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = "2DC432GLL2""#;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: input slices live across calls; lengths are exact, outputs initialized;
    // CoreFoundation/Security ownership follows their Create rule.
    unsafe {
        let url = Owned::new(CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        ))?;
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
        let req = Owned::new(req)?;
        let mut code = std::ptr::null();
        if SecStaticCodeCreateWithPath(url.0, 0, &mut code) != 0 {
            return Err(Error::ArtifactInvalid);
        }
        let code = Owned::new(code)?;
        // kSecCSStrictValidate | kSecCSCheckAllArchitectures.
        if SecStaticCodeCheckValidity(code.0, (1 << 4) | 1, req.0) != 0 {
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
pub(super) fn validate(path: &Path) -> Result<CheckedApp, Error> {
    spelling(path)?;
    let exe = path.join("Contents/MacOS/ChatGPT");
    let ancestry = platform::anchor(&exe)?;
    let (_, identity) =
        platform::executable(&exe, Path::new("/Applications/ChatGPT.app/never-managed"))?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&exe)
        .map_err(|_| Error::ArtifactInvalid)?;
    let metadata = file.metadata().map_err(|_| Error::ArtifactInvalid)?;
    if identity
        != (platform::FileIdentity::Unix {
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    {
        return Err(Error::AdmissionChanged);
    }
    let app = CheckedApp {
        path: path.into(),
        ancestry,
        executable: CheckedObject { file, identity },
    };
    app.recheck()?;
    Ok(app)
}
