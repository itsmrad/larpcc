//! Windows-only synthetic fixtures. ACLs are supplied at creation, never repaired.
use std::{
    ffi::c_void,
    fs::File,
    io::{self, Write},
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
fn refused() -> io::Error {
    io::Error::other("synthetic fixture security cannot be verified")
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: only LocalAlloc-family API allocations enter this owner.
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut units: Vec<_> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(refused());
    }
    units.push(0);
    Ok(units)
}
fn sid(bytes: &[u8]) -> io::Result<&[u8]> {
    if bytes.len() < 8 || bytes[0] != 1 || bytes[1] > 15 {
        return Err(refused());
    }
    bytes
        .get(..8 + usize::from(bytes[1]) * 4)
        .ok_or_else(refused)
}
fn sid_text(bytes: &[u8]) -> String {
    let authority = bytes[2..8]
        .iter()
        .fold(0u64, |a, b| (a << 8) | u64::from(*b));
    let mut text = format!("S-1-{authority}");
    for n in bytes[8..].as_chunks::<4>().0 {
        text.push_str(&format!("-{}", u32::from_le_bytes(*n)));
    }
    text
}
fn user() -> io::Result<Vec<u8>> {
    let mut raw = null_mut();
    // SAFETY: current process pseudo-handle, valid query-only token output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: token ownership transferred by API.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut n = 0;
    // SAFETY: first call queries required buffer size only.
    unsafe {
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut n);
    }
    if n < size_of::<TOKEN_USER>() as u32 || n > 65536 {
        return Err(refused());
    }
    let mut buf = vec![0usize; (n as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned allocation covers requested output size.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buf.as_mut_ptr().cast(),
            n,
            &mut n,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: initialized aligned TOKEN_USER from successful query.
    let p = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let offset = (p as usize)
        .checked_sub(buf.as_ptr() as usize)
        .ok_or_else(refused)?;
    // SAFETY: initialized allocation viewed as bytes; SID range validated below.
    let bytes = unsafe {
        std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len() * size_of::<usize>())
    };
    Ok(sid(bytes.get(offset..).ok_or_else(refused)?)?.to_vec())
}
fn descriptor(directory: bool) -> io::Result<Local> {
    let user = sid_text(&user()?);
    let flags = if directory { "OICI" } else { "" };
    let text: Vec<u16> =
        format!("D:P(A;{flags};FA;;;{user})(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)")
            .encode_utf16()
            .chain(Some(0))
            .collect();
    let mut raw = null_mut();
    // SAFETY: terminated SDDL and valid output allocation pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), 1, &mut raw, null_mut())
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Local(raw))
}
pub fn create_directory(path: &Path) -> io::Result<()> {
    let sd = descriptor(true)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let name = wide(path)?;
    // SAFETY: descriptor lives through exclusive, nonrecursive creation.
    if unsafe { CreateDirectoryW(name.as_ptr(), &sa) } == 0 {
        return Err(io::Error::last_os_error());
    }
    verify_private(path, true)
}
pub fn create_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let sd = descriptor(false)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let name = wide(path)?;
    // SAFETY: CREATE_NEW refuses all collisions; descriptor lives through call.
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &sa,
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: exclusive ownership transferred by successful API.
    let mut file = unsafe { File::from_raw_handle(h) };
    verify_private(path, false)?;
    file.write_all(bytes)
}
pub fn verify_private(path: &Path, directory: bool) -> io::Result<()> {
    let name = wide(path)?;
    // SAFETY: no-follow metadata-only verification, noninheritable handle.
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful open transfers handle ownership.
    let file = unsafe { File::from_raw_handle(h) };
    // SAFETY: zero initialization valid for output structure.
    let mut tag: FILE_ATTRIBUTE_TAG_INFO = unsafe { zeroed() };
    // SAFETY: live handle, sized writable output.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || tag.ReparseTag != 0
        || (tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
    {
        return Err(refused());
    }
    let (mut owner, mut raw) = (null_mut(), null_mut());
    // SAFETY: query only; descriptor allocation returned with borrowed owner.
    if unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut raw,
        )
    } != 0
    {
        return Err(refused());
    }
    let sd = Local(raw);
    // SAFETY: successful API supplied descriptor allocation.
    if sd.0.is_null() || unsafe { IsValidSecurityDescriptor(sd.0) } == 0 {
        return Err(refused());
    }
    // SAFETY: validated descriptor size defines allocation byte range.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            sd.0.cast::<u8>(),
            GetSecurityDescriptorLength(sd.0) as usize,
        )
    };
    let at = |p: *mut c_void| bytes.get((p as usize).checked_sub(sd.0 as usize)?..);
    let owner = sid(at(owner).ok_or_else(refused)?)?;
    let user = user()?;
    if owner != user && sid_text(owner) != "S-1-5-32-544" {
        return Err(refused());
    }
    let (mut control, mut rev, mut present, mut defaulted) = (0, 0, 0, 0);
    let mut acl = null_mut();
    // SAFETY: descriptor and correctly typed output pointers remain live.
    if unsafe { GetSecurityDescriptorControl(sd.0, &mut control, &mut rev) } == 0
        || unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut acl, &mut defaulted) } == 0
        || present == 0
        || acl.is_null()
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(refused());
    }
    let acl = at(acl.cast()).ok_or_else(refused)?;
    if acl.len() < 8 || !matches!(acl[0], 2 | 4) {
        return Err(refused());
    }
    let n = u16::from_le_bytes([acl[2], acl[3]]) as usize;
    let count = u16::from_le_bytes([acl[4], acl[5]]);
    if n < 8 || n > acl.len() || count == 0 {
        return Err(refused());
    }
    let mut pos = 8;
    for _ in 0..count {
        let header = acl.get(pos..pos + 4).ok_or_else(refused)?;
        let size = u16::from_le_bytes([header[2], header[3]]) as usize;
        if header[0] > 1
            || header[1] & !0x1f != 0
            || size < 16
            || !size.is_multiple_of(4)
            || pos + size > n
        {
            return Err(refused());
        }
        let ace = acl.get(pos..pos + size).ok_or_else(refused)?;
        let mask = u32::from_le_bytes(ace[4..8].try_into().map_err(|_| refused())?);
        let sid = sid(&ace[8..])?;
        if sid.len() + 8 != size || mask & !0xf01f01ff != 0 {
            return Err(refused());
        }
        if header[0] == 0
            && mask != 0
            && sid != user
            && !matches!(sid_text(sid).as_str(), "S-1-5-18" | "S-1-5-32-544")
        {
            return Err(refused());
        }
        pos += size;
    }
    Ok(())
}
pub struct PrivateTempDir(PathBuf);
impl PrivateTempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn tempdir(parent: &Path) -> io::Result<PrivateTempDir> {
    verify_private(parent, true)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..1000 {
        let path = parent.join(format!(
            "larpcc-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match create_directory(&path) {
            Ok(()) => return Ok(PrivateTempDir(path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "synthetic fixture name collisions exhausted",
    ))
}
