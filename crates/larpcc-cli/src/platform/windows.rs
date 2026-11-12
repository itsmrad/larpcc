use super::*;
use std::{
    ffi::{OsString, c_void},
    io,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Prefix},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{
        SystemServices::FILE_PERSISTENT_ACLS,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

fn unsafe_path(path: &Path, reason: &str) -> Error {
    Error::Unsafe {
        path: path.into(),
        reason: reason.into(),
    }
}
fn io_path(path: &Path, action: &'static str) -> Error {
    Error::Io {
        path: path.into(),
        action,
        source: io::Error::last_os_error(),
    }
}
fn wide(path: &Path) -> Result<Vec<u16>, Error> {
    let mut text: Vec<_> = path.as_os_str().encode_wide().collect();
    if text.contains(&0) {
        return Err(unsafe_path(path, "contains a NUL character"));
    }
    text.push(0);
    Ok(text)
}

// Only local drive namespaces are admitted, including canonical verbatim disk paths.
fn drive_root(path: &Path) -> Result<PathBuf, Error> {
    match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::Disk(_) | Prefix::VerbatimDisk(_) if path.is_absolute() => {
                let mut root = PathBuf::from(p.as_os_str());
                root.push(r"\");
                Ok(root)
            }
            _ => Err(unsafe_path(
                path,
                "access control cannot be verified on this volume",
            )),
        },
        _ => Err(unsafe_path(
            path,
            "access control cannot be verified on this volume",
        )),
    }
}

struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: allocations are returned by the LocalAlloc-family security APIs.
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn descriptor(sddl: &str, path: &Path) -> Result<Local, Error> {
    let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut ptr = null_mut();
    // SAFETY: terminated input and valid output; allocation is owned by Local.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), 1, &mut ptr, null_mut())
    } == 0
    {
        return Err(io_path(path, "create security descriptor"));
    }
    Ok(Local(ptr))
}
fn sid_bytes(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 8 || bytes[0] != 1 || bytes[1] > 15 {
        return None;
    }
    bytes.get(..8 + usize::from(bytes[1]) * 4)
}
fn user_sid(path: &Path) -> Result<Vec<u8>, Error> {
    let mut token = null_mut();
    // SAFETY: current process pseudo-handle and valid token output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io_path(path, "query current user"));
    }
    // SAFETY: successful API transferred a real token handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut needed = 0;
    // SAFETY: first call only queries buffer length.
    unsafe {
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut needed);
    }
    if needed < size_of::<TOKEN_USER>() as u32 || needed > 65536 {
        return Err(unsafe_path(path, "access control cannot be verified"));
    }
    let mut buf = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned buffer of at least needed bytes, held until SID copied.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buf.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io_path(path, "query current user"));
    }
    // SAFETY: TOKEN_USER is initialized and correctly aligned by the successful API.
    let sid = unsafe { (*(buf.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let start = buf.as_ptr() as usize;
    let offset = (sid as usize)
        .checked_sub(start)
        .ok_or_else(|| unsafe_path(path, "access control cannot be verified"))?;
    // SAFETY: buffer allocation is initialized and its complete byte range is valid.
    let bytes = unsafe {
        std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len() * size_of::<usize>())
    };
    sid_bytes(bytes.get(offset..).unwrap_or_default())
        .map(<[u8]>::to_vec)
        .ok_or_else(|| unsafe_path(path, "access control cannot be verified"))
}
fn sid_string(sid: &[u8]) -> String {
    let authority = sid[2..8].iter().fold(0u64, |v, b| (v << 8) | u64::from(*b));
    let mut text = format!("S-1-{authority}");
    for part in sid[8..].as_chunks::<4>().0 {
        text.push_str(&format!("-{}", u32::from_le_bytes(*part)));
    }
    text
}
fn trusted(sid: &[u8], user: &[u8], installer: bool) -> bool {
    sid == user
        || matches!(sid_string(sid).as_str(), "S-1-5-18" | "S-1-5-32-544")
        || (installer
            && sid_string(sid) == "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464")
}
#[derive(Clone, Copy, PartialEq)]
enum Policy {
    Managed,
    Ancestor,
    Executable,
}

// Parse ordinary ACEs as bounded bytes; never cast unchecked ACE or SID layouts.
fn check_acl(acl: &[u8], user: &[u8], policy: Policy) -> Result<(), &'static str> {
    let bad = "contains an unsupported access control entry";
    if acl.len() < 8 || !matches!(acl[0], 2 | 4) {
        return Err(bad);
    }
    let length = u16::from_le_bytes([acl[2], acl[3]]) as usize;
    if length < 8 || length > acl.len() {
        return Err(bad);
    }
    let count = u16::from_le_bytes([acl[4], acl[5]]);
    if count == 0 {
        return Err("has an empty access control list");
    }
    let mut pos = 8;
    for _ in 0..count {
        let header = acl.get(pos..pos + 4).ok_or(bad)?;
        if header[0] > 1 || header[1] & !0x1f != 0 {
            return Err(bad);
        }
        let n = u16::from_le_bytes([header[2], header[3]]) as usize;
        if n < 16 || !n.is_multiple_of(4) {
            return Err(bad);
        }
        let ace = acl
            .get(pos..pos + n)
            .filter(|_| pos + n <= length)
            .ok_or(bad)?;
        let mask = u32::from_le_bytes(ace[4..8].try_into().map_err(|_| bad)?);
        // File-specific rights, standard rights and generic rights only.
        if mask & !0xf01f01ff != 0 {
            return Err(bad);
        }
        let sid = sid_bytes(&ace[8..])
            .filter(|s| s.len() + 8 == n)
            .ok_or(bad)?;
        if header[0] == 0 && mask != 0 {
            match policy {
                Policy::Managed if !trusted(sid, user, false) => {
                    return Err("grants access to other users");
                }
                Policy::Ancestor if header[1] & 8 != 0 => (),
                Policy::Ancestor if !trusted(sid, user, true) && mask & !0xa01200af != 0 => {
                    return Err("has a foreign-writable ancestor");
                }
                Policy::Executable if !trusted(sid, user, true) && mask & 0x500d0156 != 0 => {
                    return Err("grants write access to other users");
                }
                _ => (),
            }
        }
        pos += n;
    }
    Ok(())
}
fn security(file: &File, path: &Path, policy: Policy) -> Result<(), Error> {
    let user = user_sid(path)?;
    let mut owner = null_mut();
    let mut raw = null_mut();
    // SAFETY: handle is live; only owner/DACL requested, no writes or contents.
    let code = unsafe {
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
    };
    if code != 0 {
        return Err(unsafe_path(path, "access control cannot be verified"));
    }
    let sd = Local(raw);
    // SAFETY: GetSecurityInfo returned a self-relative security descriptor allocation.
    if sd.0.is_null() || unsafe { IsValidSecurityDescriptor(sd.0) } == 0 {
        return Err(unsafe_path(path, "access control cannot be verified"));
    }
    // SAFETY: descriptor has been validated; size describes this allocation.
    let len = unsafe { GetSecurityDescriptorLength(sd.0) } as usize;
    // SAFETY: descriptor is valid for its reported byte size until sd drops.
    let bytes = unsafe { std::slice::from_raw_parts(sd.0.cast::<u8>(), len) };
    let at = |ptr: *mut c_void| -> Option<&[u8]> {
        bytes.get((ptr as usize).checked_sub(sd.0 as usize)?..)
    };
    let owner = at(owner)
        .and_then(sid_bytes)
        .ok_or_else(|| unsafe_path(path, "access control cannot be verified"))?;
    let owner_ok = if policy == Policy::Managed {
        owner == user || sid_string(owner) == "S-1-5-32-544"
    } else {
        trusted(owner, &user, true)
    };
    if !owner_ok {
        return Err(unsafe_path(
            path,
            if policy == Policy::Managed {
                "owner is not a trusted managed-path owner"
            } else {
                "is owned by an untrusted principal"
            },
        ));
    }
    let (mut control, mut revision, mut present, mut defaulted) = (0, 0, 0, 0);
    let mut dacl = null_mut();
    // SAFETY: validated descriptor; all output pointers are live and correctly typed.
    if unsafe { GetSecurityDescriptorControl(sd.0, &mut control, &mut revision) } == 0
        || unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) } == 0
    {
        return Err(unsafe_path(path, "access control cannot be verified"));
    }
    if present == 0 || dacl.is_null() {
        return Err(unsafe_path(path, "has no access control list"));
    }
    if policy == Policy::Managed && control & SE_DACL_PROTECTED == 0 {
        return Err(unsafe_path(path, "has an unprotected access control list"));
    }
    let acl =
        at(dacl.cast()).ok_or_else(|| unsafe_path(path, "access control cannot be verified"))?;
    check_acl(acl, &user, policy).map_err(|r| unsafe_path(path, r))
}
fn local_acl_volume(drive_type: u32, flags: u32) -> bool {
    matches!(drive_type, 2 | 3 | 5 | 6) && flags & FILE_PERSISTENT_ACLS != 0
}
fn verify(
    file: File,
    path: &Path,
    kind: ObjectKind,
    policy: Policy,
) -> Result<CheckedObject, Error> {
    // SAFETY: zero is valid initialization for these plain output structures.
    let (mut tag, mut id): (FILE_ATTRIBUTE_TAG_INFO, FILE_ID_INFO) =
        unsafe { (zeroed(), zeroed()) };
    // SAFETY: live handle and correctly sized writable output structures.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(io_path(path, "inspect opened object"));
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.ReparseTag != 0 {
        return Err(unsafe_path(path, "is a symbolic link or reparse point"));
    }
    // SAFETY: live file handle; query has no borrowed outputs.
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(unsafe_path(path, "is not a regular file"));
    }
    if (tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != (kind == ObjectKind::Directory) {
        return Err(unsafe_path(
            path,
            if kind == ObjectKind::Directory {
                "is not a directory"
            } else {
                "is not a regular file"
            },
        ));
    }
    let root = wide(&drive_root(path)?)?;
    let mut flags = 0;
    // SAFETY: terminated local root; live handle and valid optional query outputs.
    let drive_type = unsafe { GetDriveTypeW(root.as_ptr()) };
    if unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut flags,
            null_mut(),
            0,
        )
    } == 0
        || !local_acl_volume(drive_type, flags)
    {
        return Err(unsafe_path(
            path,
            "access control cannot be verified on this volume",
        ));
    }
    security(&file, path, policy)?;
    // SAFETY: live handle and correctly sized output buffer, no borrowed pointers.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(unsafe_path(path, "filesystem identity cannot be verified"));
    }
    Ok(CheckedObject {
        file,
        identity: FileIdentity::Windows {
            volume: id.VolumeSerialNumber,
            id: id.FileId.Identifier,
        },
    })
}
fn open(path: &Path, access: u32, sharing: u32) -> Result<Option<File>, Error> {
    let name = wide(path)?;
    // SAFETY: terminated path; null security attributes ensure noninheritance.
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            sharing,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        let err = io::Error::last_os_error();
        if matches!(err.raw_os_error(), Some(2 | 3)) {
            return Ok(None);
        }
        return Err(Error::Io {
            action: "open for verification",
            path: path.into(),
            source: err,
        });
    }
    // SAFETY: successful CreateFileW transfers exclusive ownership of handle.
    Ok(Some(unsafe { File::from_raw_handle(h) }))
}
fn anchor_existing(path: &Path) -> Result<(AnchoredPath, bool), Error> {
    drive_root(path)?;
    let mut current = PathBuf::new();
    let mut ancestors = Vec::new();
    let parent = path
        .parent()
        .ok_or_else(|| unsafe_path(path, "cannot resolve a trusted executable path"))?;
    for component in parent.components() {
        match component {
            Component::Prefix(_) => {
                current.push(component);
                continue;
            }
            Component::RootDir | Component::Normal(_) => current.push(component),
            _ => return Err(unsafe_path(path, "must be normalized")),
        }
        // Directory read-category access enforces sharing; this handle is never enumerated.
        let file = match open(
            &current,
            READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )? {
            Some(file) => file,
            None => return Ok((AnchoredPath { ancestors }, false)),
        };
        ancestors.push(verify(
            file,
            &current,
            ObjectKind::Directory,
            Policy::Ancestor,
        )?);
    }
    Ok((AnchoredPath { ancestors }, true))
}
pub fn anchor(path: &Path) -> Result<AnchoredPath, Error> {
    anchor_existing(path).map(|(a, _)| a)
}
pub fn inspect(path: &Path, kind: ObjectKind) -> Result<Option<CheckedObject>, Error> {
    let (_parents, complete) = anchor_existing(path)?;
    if !complete {
        return Ok(None);
    }
    // Metadata-only, including directory security. Core reopens data with identity comparison.
    open(
        path,
        READ_CONTROL | FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ
            | FILE_SHARE_WRITE
            | if kind == ObjectKind::File {
                FILE_SHARE_DELETE
            } else {
                0
            },
    )?
    .map(|f| verify(f, path, kind, Policy::Managed))
    .transpose()
}
// Caller pins verified parents and compares the returned identity before data access.
pub fn verify_opened(file: File, path: &Path, kind: ObjectKind) -> Result<CheckedObject, Error> {
    verify(file, path, kind, Policy::Managed)
}
pub fn inspect_home(path: &Path) -> Result<DirStatus, Error> {
    Ok(match inspect(path, ObjectKind::Directory) {
        Ok(Some(_)) => DirStatus::Ok,
        Ok(None) => DirStatus::Missing,
        Err(Error::Unsafe { reason, .. }) => DirStatus::Unsafe(reason),
        Err(_) => DirStatus::Unsafe("access control cannot be verified".into()),
    })
}
fn create(path: &Path, kind: ObjectKind) -> Result<CheckedObject, Error> {
    let (_parents, complete) = anchor_existing(path)?;
    if !complete {
        return Err(Error::Io {
            action: "create directory",
            path: path.into(),
            source: io::Error::from(io::ErrorKind::NotFound),
        });
    }
    let user = sid_string(&user_sid(path)?);
    let flags = if kind == ObjectKind::Directory {
        "OICI"
    } else {
        ""
    };
    let sd = descriptor(
        &format!("D:P(A;{flags};FA;;;{user})(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)"),
        path,
    )?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let name = wide(path)?;
    if kind == ObjectKind::Directory {
        // SAFETY: descriptor stays alive through exclusive nonrecursive creation.
        if unsafe { CreateDirectoryW(name.as_ptr(), &sa) } == 0 {
            return Err(io_path(path, "create directory"));
        }
        return inspect(path, kind)?
            .ok_or_else(|| unsafe_path(path, "filesystem identity cannot be verified"));
    }
    // SAFETY: CREATE_NEW never adopts/truncates a collision; descriptor valid through call.
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &sa,
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(io_path(path, "create file"));
    }
    // SAFETY: successful creation transfers ownership of a noninheritable handle.
    verify(
        unsafe { File::from_raw_handle(h) },
        path,
        kind,
        Policy::Managed,
    )
}
pub fn create_file(path: &Path) -> Result<CheckedObject, Error> {
    create(path, ObjectKind::File)
}
pub fn create_directory(path: &Path) -> Result<CheckedObject, Error> {
    create(path, ObjectKind::Directory)
}

fn resolved(file: &File, path: &Path) -> Result<PathBuf, Error> {
    let mut buf = vec![0u16; 32768];
    // SAFETY: live handle, bounded writable UTF-16 buffer.
    let n = unsafe {
        GetFinalPathNameByHandleW(file.as_raw_handle(), buf.as_mut_ptr(), buf.len() as u32, 0)
    } as usize;
    if n == 0 || n >= buf.len() {
        return Err(unsafe_path(
            path,
            "cannot resolve a trusted executable path",
        ));
    }
    let result = PathBuf::from(OsString::from_wide(&buf[..n]));
    drive_root(&result)?;
    Ok(result)
}
pub fn contains_path(path: &Path, root: &Path) -> Result<bool, Error> {
    drive_root(path)?;
    drive_root(root)?;
    let units = |component: Component<'_>| -> Vec<u16> {
        match component {
            Component::Prefix(p) => match p.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => vec![u16::from(d), u16::from(b':')],
                _ => p.as_os_str().encode_wide().collect(),
            },
            c => c.as_os_str().encode_wide().collect(),
        }
    };
    let mut parts = path.components();
    for r in root.components() {
        let Some(p) = parts.next() else {
            return Ok(false);
        };
        if !ordinal_units(&units(p), &units(r)).map_err(|source| Error::Io {
            action: "compare native path",
            path: path.into(),
            source,
        })? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn ordinal_units(a: &[u16], b: &[u16]) -> io::Result<bool> {
    use windows_sys::Win32::Globalization::CompareStringOrdinal;
    let an = i32::try_from(a.len()).map_err(|_| io::Error::other("native path is too long"))?;
    let bn = i32::try_from(b.len()).map_err(|_| io::Error::other("native path is too long"))?;
    // SAFETY: exact counted native UTF-16 buffers, checked i32 lengths, no normalization/loss.
    match unsafe { CompareStringOrdinal(a.as_ptr(), an, b.as_ptr(), bn, 1) } {
        1 | 3 => Ok(false),
        2 => Ok(true),
        _ => Err(io::Error::last_os_error()),
    }
}
// Windows extension matching is ordinal case-insensitive; exact UTF-16 avoids lossy conversion.
fn ordinal_equal(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> io::Result<bool> {
    ordinal_units(
        &a.encode_wide().collect::<Vec<_>>(),
        &b.encode_wide().collect::<Vec<_>>(),
    )
}

pub fn executable(path: &Path, root: &Path) -> Result<(PathBuf, FileIdentity), Error> {
    use std::io::Read;
    let _parents = anchor(path)?;
    let file = open(
        path,
        READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_READ_DATA,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )?
    .ok_or_else(|| unsafe_path(path, "does not exist"))?;
    let mut checked = verify(file, path, ObjectKind::File, Policy::Executable)?;
    let target = resolved(&checked.file, path)?;
    let _resolved_parents = anchor(&target)?;
    let canonical_root =
        if let Some(f) = open(
            root,
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )? {
            resolved(&f, root)?
        } else {
            let mut existing = root;
            let mut suffix = Vec::new();
            loop {
                suffix.push(existing.file_name().ok_or_else(|| {
                    unsafe_path(root, "cannot resolve a trusted executable path")
                })?);
                existing = existing
                    .parent()
                    .ok_or_else(|| unsafe_path(root, "cannot resolve a trusted executable path"))?;
                if let Some(f) = open(
                    existing,
                    READ_CONTROL | FILE_READ_ATTRIBUTES,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                )? {
                    let mut p = resolved(&f, existing)?;
                    for part in suffix.iter().rev() {
                        p.push(part);
                    }
                    break p;
                }
            }
        };
    if contains_path(&target, &canonical_root)? {
        return Err(unsafe_path(path, "is inside the Larp CC data directory"));
    }
    for p in [path, target.as_path()] {
        if !p
            .extension()
            .is_some_and(|x| ordinal_equal(x, std::ffi::OsStr::new("exe")).unwrap_or(false))
        {
            return Err(unsafe_path(path, "must end in .exe"));
        }
    }
    let mut magic = [0; 4];
    let n = checked.file.read(&mut magic).map_err(|source| Error::Io {
        action: "read executable header",
        path: path.into(),
        source,
    })?;
    if n < 2 || magic[..2] != *b"MZ" {
        return Err(unsafe_path(path, "is not a supported native executable"));
    }
    let own = std::env::current_exe().map_err(|source| Error::Io {
        action: "locate current executable",
        path: path.into(),
        source,
    })?;
    let own = open(
        &own,
        READ_CONTROL | FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )?
    .ok_or_else(|| unsafe_path(path, "filesystem identity cannot be verified"))?;
    // SAFETY: plain output structure and live handle, full 128-bit file identity query.
    let mut id: FILE_ID_INFO = unsafe { zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            own.as_raw_handle(),
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(unsafe_path(path, "filesystem identity cannot be verified"));
    }
    if checked.identity
        == (FileIdentity::Windows {
            volume: id.VolumeSerialNumber,
            id: id.FileId.Identifier,
        })
    {
        return Err(unsafe_path(path, "is Larp CC itself"));
    }
    Ok((target, checked.identity))
}

#[cfg(feature = "synthetic-launch")]
unsafe extern "system" fn console_handler(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    i32::from(event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT)
}
#[cfg(all(test, feature = "synthetic-launch"))]
thread_local! {
    static FAULT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static REMOVALS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static HANDLER_INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAILED_CHILD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
#[cfg(feature = "synthetic-launch")]
fn set_handler(add: bool) -> bool {
    #[cfg(test)]
    {
        if !add {
            REMOVALS.with(|n| n.set(n.get() + 1));
        }
        if add && FAULT.with(|f| f.get() == 1) {
            return false;
        }
    }
    // SAFETY: static handler remains valid throughout the process lifetime.
    let success = unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(
            Some(console_handler),
            i32::from(add),
        ) != 0
    };
    #[cfg(test)]
    {
        if success {
            HANDLER_INSTALLED.with(|h| h.set(add));
        }
        // Inject failure after real removal so test processes leave no real handler installed.
        if !add && FAULT.with(|f| matches!(f.get(), 2 | 5 | 6)) {
            // SAFETY: thread-local deterministic error for the injected failed API result.
            unsafe {
                SetLastError(6);
            }
            return false;
        }
    }
    success
}

#[cfg(feature = "synthetic-launch")]
fn wait_child(child: &mut std::process::Child) -> io::Result<std::process::ExitStatus> {
    #[cfg(test)]
    if FAULT.with(|f| matches!(f.get(), 3 | 4 | 5)) {
        FAILED_CHILD.with(|p| p.set(child.id()));
        return Err(io::Error::from_raw_os_error(6));
    }
    child.wait()
}
#[cfg(feature = "synthetic-launch")]
fn poll_cleanup(child: &mut std::process::Child) -> io::Result<Option<std::process::ExitStatus>> {
    #[cfg(test)]
    if FAULT.with(|f| f.get() == 4) {
        return Err(io::Error::other("injected cleanup query failure"));
    }
    child.try_wait()
}
#[cfg(feature = "synthetic-launch")]
pub fn spawn_wait(plan: &crate::launch::LaunchPlan) -> Result<u32, Error> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // SAFETY: static handler performs no allocation/IO and is valid until removal.
    if !set_handler(true) {
        return Err(Error::Console {
            action: "register console handler",
            source: io::Error::last_os_error(),
        });
    }
    let mut command = Command::new(&plan.executable);
    command
        .args(&plan.argv)
        .envs(plan.env_delta.iter().cloned())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(cwd) = &plan.cwd {
        command.current_dir(cwd);
    }
    let result = match command.spawn() {
        Err(source) => Err(Error::Launch {
            path: plan.executable.clone(),
            source,
        }),
        Ok(mut child) => match wait_child(&mut child) {
            Ok(status) => status
                .code()
                .map(|code| code as u32)
                .ok_or_else(|| Error::Wait {
                    source: io::Error::other("child status is unavailable"),
                    cleanup_failed: false,
                }),
            Err(source) => {
                let _ = child.kill();
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut reaped = false;
                while Instant::now() < deadline {
                    if matches!(poll_cleanup(&mut child), Ok(Some(_))) {
                        reaped = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(Error::Wait {
                    source,
                    cleanup_failed: !reaped,
                })
            }
        },
    };
    // SAFETY: removes exactly our registered static handler, after known child completion/cleanup.
    // Preserve a primary spawn/wait error; standalone removal failure is reported below.
    let removed = set_handler(false);
    if !removed && result.is_ok() {
        return Err(Error::Console {
            action: "unregister console handler",
            source: io::Error::last_os_error(),
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn acl(aces: &[(u8, u8, u32, &str)]) -> Vec<u8> {
        let path = Path::new(r"C:\synthetic");
        let entries: String = aces
            .iter()
            .map(|(kind, flags, mask, sid)| {
                let kind = if *kind == 0 { "A" } else { "D" };
                let flags = if *flags == 8 { "IO" } else { "" };
                format!("({kind};{flags};{mask:#x};;;{sid})")
            })
            .collect();
        let sd = descriptor(&format!("D:P{entries}"), path).unwrap();
        let (mut present, mut defaulted) = (0, 0);
        let mut ptr = null_mut();
        // SAFETY: descriptor from SDDL API is live, output pointers valid.
        assert_ne!(
            unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut ptr, &mut defaulted) },
            0
        );
        // SAFETY: valid ACL returned by API; copy before descriptor is freed.
        unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), (*ptr).AclSize as usize).to_vec() }
    }
    #[test]
    fn w01_w08_acl_policy_distinguishes_managed_files_and_existing_ancestors() {
        let user = user_sid(Path::new(r"C:\synthetic")).unwrap();
        for mask in [
            FILE_ADD_FILE,
            FILE_ADD_SUBDIRECTORY,
            FILE_LIST_DIRECTORY,
            READ_CONTROL,
            SYNCHRONIZE,
            0x80000000,
            0x20000000,
        ] {
            let a = acl(&[(0, 0, mask, "WD")]);
            assert_eq!(check_acl(&a, &user, Policy::Ancestor), Ok(()));
            assert!(check_acl(&a, &user, Policy::Managed).is_err());
        }
        for mask in [
            FILE_DELETE_CHILD,
            FILE_WRITE_EA,
            FILE_WRITE_ATTRIBUTES,
            DELETE,
            WRITE_DAC,
            WRITE_OWNER,
            0x40000000,
            0x10000000,
        ] {
            let effective = acl(&[(0, 0, mask, "WD")]);
            assert_eq!(
                check_acl(&effective, &user, Policy::Ancestor),
                Err("has a foreign-writable ancestor")
            );
            let inherit_only = acl(&[(0, 8, mask, "WD")]);
            assert_eq!(check_acl(&inherit_only, &user, Policy::Ancestor), Ok(()));
            assert!(check_acl(&inherit_only, &user, Policy::Managed).is_err());
        }
        for sid in ["WD", "BU", "AU", "CO", "S-1-5-21-1-2-3-1001"] {
            assert!(check_acl(&acl(&[(0, 0, READ_CONTROL, sid)]), &user, Policy::Managed).is_err());
        }
        assert!(
            check_acl(
                &acl(&[(1, 0, FILE_ALL_ACCESS, "WD"), (0, 0, WRITE_DAC, "WD")]),
                &user,
                Policy::Ancestor
            )
            .is_err()
        );
        for mask in [
            FILE_WRITE_DATA,
            FILE_APPEND_DATA,
            FILE_WRITE_EA,
            FILE_WRITE_ATTRIBUTES,
            DELETE,
            WRITE_DAC,
            WRITE_OWNER,
            0x40000000,
            0x10000000,
        ] {
            assert!(check_acl(&acl(&[(0, 0, mask, "WD")]), &user, Policy::Executable).is_err());
        }
    }
    #[test]
    fn w01_local_volume_acl_capability_refuses_remote_unknown_and_unsupported() {
        for drive_type in 0..=6 {
            assert!(!local_acl_volume(drive_type, 0));
            assert_eq!(
                local_acl_volume(drive_type, FILE_PERSISTENT_ACLS),
                matches!(drive_type, 2 | 3 | 5 | 6)
            );
        }
    }
    #[test]
    fn w01_unknown_truncated_and_empty_acls_fail_closed() {
        let user = user_sid(Path::new(r"C:\synthetic")).unwrap();
        let good = acl(&[(0, 0, FILE_ALL_ACCESS, "SY")]);
        for end in 0..good.len() {
            assert!(check_acl(&good[..end], &user, Policy::Managed).is_err());
        }
        for kind in 2..=u8::MAX {
            let mut a = good.clone();
            a[8] = kind;
            assert!(check_acl(&a, &user, Policy::Managed).is_err());
        }
        for flags in [0x20, 0x40, 0x80] {
            let mut a = good.clone();
            a[9] = flags;
            assert!(check_acl(&a, &user, Policy::Managed).is_err());
        }
        assert!(check_acl(&acl(&[(0, 0, 0x02000000, "SY")]), &user, Policy::Managed).is_err());
        assert_eq!(
            check_acl(&[2, 0, 8, 0, 0, 0, 0, 0], &user, Policy::Managed),
            Err("has an empty access control list")
        );
    }
    fn fixture() -> fixture_private::PrivateTempDir {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        fixture_private::tempdir(&parent).unwrap()
    }
    // Fixtures receive descriptors only at creation; no ACL writes, even in tests.
    fn directory_with(path: &Path, sddl: &str) {
        let sd = descriptor(sddl, path).unwrap();
        if sddl == "D:NO_ACCESS_CONTROL" {
            // SAFETY: modify only the new in-memory descriptor before object creation.
            assert_ne!(
                unsafe { SetSecurityDescriptorControl(sd.0, SE_DACL_PROTECTED, SE_DACL_PROTECTED) },
                0
            );
        }
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let name = wide(path).unwrap();
        // SAFETY: live descriptor, terminated path, nonrecursive new directory.
        assert_ne!(
            unsafe { CreateDirectoryW(name.as_ptr(), &sa) },
            0,
            "{}",
            io::Error::last_os_error()
        );
    }
    #[test]
    fn w01_actual_token_elevation_is_recorded() {
        let mut raw = null_mut();
        // SAFETY: current process query and valid output pointer.
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) },
            0
        );
        // SAFETY: API transferred token ownership.
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0;
        // SAFETY: live token and sized output buffer.
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    TokenElevation,
                    (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                    size_of::<TOKEN_ELEVATION>() as u32,
                    &mut len,
                )
            },
            0
        );
        eprintln!(
            "W01 actual TokenIsElevated={}; non-elevated acceptance requires false",
            elevation.TokenIsElevated != 0
        );
    }
    #[test]
    fn w02_new_objects_private_collision_safe_and_handles_noninheritable() {
        let temp = fixture();
        let root = temp.path().join("private");
        let object = create_directory(&root).unwrap();
        let mut flags = 0;
        // SAFETY: live handle and valid output.
        assert_ne!(
            unsafe { GetHandleInformation(object.file.as_raw_handle(), &mut flags) },
            0
        );
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        drop(object);
        fixture_private::create_file(&root.join("helper-sentinel"), b"synthetic private").unwrap();
        let path = root.join("metadata");
        let mut file = create_file(&path).unwrap();
        file.file.write_all(b"synthetic unchanged").unwrap();
        let identity = file.identity.clone();
        drop(file);
        assert!(create_file(&path).is_err());
        assert!(create_directory(&root).is_err());
        let inspected = inspect(&path, ObjectKind::File).unwrap().unwrap();
        assert_eq!(identity, inspected.identity);
        assert!(
            inspected
                .file
                .try_clone()
                .unwrap()
                .read(&mut [0; 1])
                .is_err(),
            "inspection must not require data-read access"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic unchanged");
    }
    #[test]
    fn w01_w03_existing_security_refuses_without_changing_contents_or_acl() {
        let temp = fixture();
        let user = sid_string(&user_sid(temp.path()).unwrap());
        let mut specimens = Vec::new();
        for (i, sddl) in [
            format!("D:AI(A;;FA;;;{user})"),
            format!("D:P(A;;FA;;;{user})(A;;FR;;;WD)"),
            "D:P".into(),
            "D:NO_ACCESS_CONTROL".into(),
        ]
        .iter()
        .enumerate()
        {
            let path = temp.path().join(format!("unsafe-{i}"));
            if i == 0 {
                // No explicit descriptor: obtain the native unprotected inherited ACL fixture.
                std::fs::create_dir(&path).unwrap();
            } else {
                directory_with(&path, sddl);
            }
            let before = snapshot_named_security(&path);
            assert!(
                inspect(&path, ObjectKind::Directory).is_err(),
                "fixture class {i}"
            );
            assert_eq!(snapshot_named_security(&path), before);
            if i >= 2 {
                let (mut present, mut defaulted) = (0, 0);
                let mut dacl = null_mut();
                // SAFETY: complete copied native descriptor and valid output pointers.
                assert_ne!(
                    unsafe {
                        GetSecurityDescriptorDacl(
                            before.as_ptr().cast_mut().cast(),
                            &mut present,
                            &mut dacl,
                            &mut defaulted,
                        )
                    },
                    0
                );
                assert_ne!(present, 0);
                if i == 2 {
                    assert!(!dacl.is_null());
                    // SAFETY: nonnull ACL within the live complete descriptor.
                    assert_eq!(unsafe { (*dacl).AceCount }, 0);
                } else {
                    assert!(dacl.is_null(), "NULL-DACL specimen must have a NULL DACL");
                }
            }
            specimens.push((i, path));
        }
        // Finish every refusal and descriptor assertion, including NULL DACL, before cleanup.
        for (i, path) in specimens {
            let name = wide(&path).unwrap();
            // SAFETY: remove this known empty synthetic directory through its trusted parent.
            if unsafe { RemoveDirectoryW(name.as_ptr()) } == 0 {
                let error = io::Error::last_os_error();
                // shortcut: retain only this empty-DACL specimen on ephemeral hosted CI;
                // cleanup remains unqualified until native deletion is independently supported.
                if i == 2
                    && error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
                    && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
                    && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
                    && std::env::var("RUNNER_OS").as_deref() == Ok("Windows")
                {
                    eprintln!("empty-DACL specimen cleanup unsupported");
                } else {
                    panic!("fixture cleanup class {i}: {error}");
                }
            }
        }
    }
    fn snapshot_named_security(path: &Path) -> Vec<u8> {
        let name = wide(path).unwrap();
        let mut raw = null_mut();
        // SAFETY: known newly-created synthetic path; security query only, no content or ACL writes.
        assert_eq!(
            unsafe {
                GetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    &mut raw,
                )
            },
            0
        );
        let sd = Local(raw);
        // SAFETY: API returned a complete descriptor allocation; copy before owner drops.
        unsafe {
            std::slice::from_raw_parts(
                sd.0.cast::<u8>(),
                GetSecurityDescriptorLength(sd.0) as usize,
            )
            .to_vec()
        }
    }
    #[cfg(feature = "synthetic-launch")]
    fn snapshot_security(file: &File) -> Vec<u8> {
        let mut raw = null_mut();
        // SAFETY: live handle, valid outputs, copy before Local owner drops.
        assert_eq!(
            unsafe {
                GetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    &mut raw,
                )
            },
            0
        );
        let sd = Local(raw);
        // SAFETY: successful API returns complete descriptor allocation.
        unsafe {
            std::slice::from_raw_parts(
                sd.0.cast::<u8>(),
                GetSecurityDescriptorLength(sd.0) as usize,
            )
            .to_vec()
        }
    }
    #[test]
    fn w07_parent_pins_block_rename_but_metadata_inspection_allows_replace() {
        let temp = fixture();
        let root = temp.path().join("private");
        drop(create_directory(&root).unwrap());
        let path = root.join("registry.json");
        drop(create_file(&path).unwrap());
        let pins = anchor(&path).unwrap();
        assert!(std::fs::rename(&root, temp.path().join("moved")).is_err());
        let old = inspect(&path, ObjectKind::File).unwrap().unwrap();
        let next = root.join("next");
        drop(create_file(&next).unwrap());
        std::fs::rename(next, &path).unwrap();
        assert_ne!(
            old.identity,
            inspect(&path, ObjectKind::File).unwrap().unwrap().identity
        );
        drop((pins, old));
        std::fs::rename(&root, temp.path().join("moved")).unwrap();
    }
    #[test]
    fn w08_native_sibling_only_ancestor_and_effective_next_component() {
        let temp = fixture();
        let user = sid_string(&user_sid(temp.path()).unwrap());
        let parent = temp.path().join("siblings");
        directory_with(
            &parent,
            &format!("D:P(A;;FA;;;{user})(A;;0x6;;;WD)(A;OICIIO;0x40000;;;WD)"),
        );
        let safe = parent.join("selected");
        directory_with(&safe, &format!("D:P(A;;FA;;;{user})"));
        assert!(anchor(&safe.join("leaf")).is_ok());
        drop(create_file(&safe.join("new")).unwrap());
        let executable_path = safe.join("native.exe");
        let mut native = create_file(&executable_path).unwrap();
        native.file.write_all(b"MZxx").unwrap();
        drop(native);
        assert!(executable(&executable_path, &safe.join("data")).is_ok());
        let hostile = parent.join("hostile");
        directory_with(&hostile, &format!("D:P(A;;FA;;;{user})(A;;WD;;;WD)"));
        assert!(anchor(&hostile.join("leaf")).is_err());
        assert!(create_directory(&hostile).is_err());
        assert!(inspect(&hostile, ObjectKind::Directory).is_err());
    }
    #[cfg(feature = "synthetic-launch")]
    #[test]
    fn w01_w03_managed_set_refuses_before_launch_and_preserves_synthetic_contents() {
        use crate::registry::{ProfileName, Provider, Registry};
        for class in [
            "root",
            "profiles",
            "registry.json",
            "registry.lock",
            "registry.json.tmp",
            "selected",
            "other",
        ] {
            let temp = fixture();
            let root = temp.path().join("managed");
            let registry = Registry::at(root.clone()).unwrap();
            let selected = ProfileName::parse("selected").unwrap();
            let other = ProfileName::parse("other").unwrap();
            let a = registry.add(selected.clone(), Provider::Claude).unwrap();
            let b = registry.add(other, Provider::Codex).unwrap();
            let sentinel = b.state_dir.join("synthetic-sentinel");
            std::fs::write(&sentinel, b"invented provider contents; never consumed").unwrap();
            let (path, directory) = match class {
                "root" => (root.clone(), true),
                "profiles" => (root.join("profiles"), true),
                "selected" => (a.state_dir, true),
                "other" => (b.state_dir, true),
                name => (root.join(name), false),
            };
            let saved = temp.path().join("saved");
            let existed = path.exists();
            if existed {
                std::fs::rename(&path, &saved).unwrap();
            }
            let user = sid_string(&user_sid(&path).unwrap());
            let sddl = format!("D:P(A;;FA;;;{user})(A;;FR;;;WD)");
            if directory {
                directory_with(&path, &sddl);
            } else {
                let sd = descriptor(&sddl, &path).unwrap();
                let sa = SECURITY_ATTRIBUTES {
                    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: sd.0,
                    bInheritHandle: 0,
                };
                let name = wide(&path).unwrap();
                // SAFETY: exclusive creation of synthetic unsafe metadata fixture only.
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
                assert_ne!(h, INVALID_HANDLE_VALUE);
                // SAFETY: successful creation transfers ownership, no existing contents changed.
                drop(unsafe { File::from_raw_handle(h) });
            }
            let bad = open(
                &path,
                READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )
            .unwrap()
            .unwrap();
            let before_acl = snapshot_security(&bad);
            assert!(
                matches!(registry.launch_target(&selected), Err(Error::Unsafe { .. })),
                "class={class}"
            );
            assert_eq!(snapshot_security(&bad), before_acl, "class={class}");
            let actual_sentinel = if existed && sentinel.starts_with(&path) {
                saved.join(sentinel.strip_prefix(&path).unwrap())
            } else {
                sentinel
            };
            assert_eq!(
                std::fs::read(actual_sentinel).unwrap(),
                b"invented provider contents; never consumed"
            );
        }
    }
    #[test]
    fn w01_read_control_verification_needs_no_dacl_write_or_directory_enumeration() {
        let temp = fixture();
        let user = sid_string(&user_sid(temp.path()).unwrap());
        let read_only = temp.path().join("read-control");
        directory_with(&read_only, &format!("D:P(A;;0x120080;;;{user})"));
        assert!(
            inspect(&read_only, ObjectKind::Directory)
                .unwrap()
                .is_some()
        );
        assert!(create_file(&read_only.join("denied")).is_err());
        std::fs::remove_dir(&read_only).unwrap();
        let home = temp.path().join("home");
        drop(create_directory(&home).unwrap());
        let sentinel = home.join("synthetic-content");
        drop(create_file(&sentinel).unwrap());
        let name = wide(&sentinel).unwrap();
        // SAFETY: exclusive synthetic-content handle makes any content open incompatible.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        assert_ne!(h, INVALID_HANDLE_VALUE);
        // SAFETY: successful open transfers ownership.
        let held = unsafe { File::from_raw_handle(h) };
        assert_eq!(inspect_home(&home).unwrap(), DirStatus::Ok);
        drop(held);
    }
    #[test]
    fn w01_opened_verifier_checks_actual_handle_without_reopening_name() {
        let temp = fixture();
        let root = temp.path().join("private");
        drop(create_directory(&root).unwrap());
        let path = root.join("data");
        let old = create_file(&path).unwrap();
        let expected = old.identity.clone();
        drop(old);
        let old = open(
            &path,
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap()
        .unwrap();
        let _parents = anchor(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let newer = create_file(&path).unwrap();
        assert_ne!(newer.identity, expected);
        assert_eq!(
            verify_opened(old, &path, ObjectKind::File)
                .unwrap()
                .identity,
            expected
        );
        drop(newer);
        let name = wide(&root).unwrap();
        drop(_parents);
        // SAFETY: own synthetic directory, data-category directory open with exclusive sharing.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        assert_ne!(h, INVALID_HANDLE_VALUE);
        // SAFETY: successful exclusive open transfers handle ownership.
        let exclusive = unsafe { File::from_raw_handle(h) };
        assert!(anchor(&path).is_err());
        drop(exclusive);
        assert!(inspect(&path, ObjectKind::File).unwrap().is_some());
    }
    #[test]
    fn w06_executable_magic_identity_hardlink_self_and_ancestry() {
        let temp = fixture();
        let selected = temp.path().join("native");
        drop(create_directory(&selected).unwrap());
        let data = selected.join("data");
        drop(create_directory(&data).unwrap());
        let path = selected.join("native.exe");
        let mut created = create_file(&path).unwrap();
        created
            .file
            .write_all(b"MZxxsynthetic header only; never executed")
            .unwrap();
        let expected = created.identity.clone();
        drop(created);
        let (resolved, id) = executable(&path, &data).unwrap();
        assert_eq!(expected, id);
        assert!(resolved.is_absolute());
        assert!(executable(&path, &selected).is_err());
        let alias = selected.join("self.exe");
        std::fs::hard_link(std::env::current_exe().unwrap(), &alias).unwrap();
        assert!(
            matches!(executable(&alias, &data), Err(Error::Unsafe { reason, .. }) if reason == "is Larp CC itself")
        );
        let before = std::fs::read(&path).unwrap();
        let old = inspect(&path, ObjectKind::File).unwrap().unwrap();
        let newer = selected.join("new.exe");
        drop(create_file(&newer).unwrap());
        std::fs::rename(&newer, &path).unwrap();
        assert_ne!(
            old.identity,
            inspect(&path, ObjectKind::File).unwrap().unwrap().identity
        );
        assert!(executable(&path, &data).is_err());
        assert!(before.starts_with(b"MZ"));
    }
    #[test]
    fn w06_native_file_and_directory_reparse_paths_refuse() {
        let temp = fixture();
        let root = temp.path().join("private");
        drop(create_directory(&root).unwrap());
        let target = root.join("target.exe");
        drop(create_file(&target).unwrap());
        let link = root.join("link.exe");
        std::os::windows::fs::symlink_file(&target, &link)
            .expect("native file-reparse fixture unavailable; W06 cannot qualify on this runner");
        assert!(
            matches!(inspect(&link, ObjectKind::File), Err(Error::Unsafe { reason, .. }) if reason == "is a symbolic link or reparse point")
        );
        assert!(
            matches!(executable(&link, &root.join("data")), Err(Error::Unsafe { reason, .. }) if reason == "is a symbolic link or reparse point")
        );
        let selected = root.join("selected");
        drop(create_directory(&selected).unwrap());
        let alias = root.join("directory-link");
        std::os::windows::fs::symlink_dir(&selected, &alias).expect(
            "native directory-reparse fixture unavailable; W06 cannot qualify on this runner",
        );
        assert!(
            matches!(anchor(&alias.join("leaf")), Err(Error::Unsafe { reason, .. }) if reason == "is a symbolic link or reparse point")
        );
        assert!(create_file(&alias.join("collision")).is_err());
        assert!(!selected.join("collision").exists());
    }
    #[test]
    fn w06_ordinal_component_boundaries_and_native_surrogates() {
        assert!(contains_path(Path::new(r"C:\DATA\child"), Path::new(r"c:\data")).unwrap());
        assert!(!contains_path(Path::new(r"C:\database"), Path::new(r"C:\data")).unwrap());
        assert!(contains_path(Path::new(r"\\?\C:\DATA\child"), Path::new(r"c:\data")).unwrap());
        let a = OsString::from_wide(&[0xd800]);
        let b = OsString::from_wide(&[0xd801]);
        assert!(!ordinal_equal(&a, &b).unwrap());
        assert!(wide(Path::new(&OsString::from_wide(&[0x43, 0, 0x44]))).is_err());
    }
}

#[cfg(all(test, feature = "synthetic-launch"))]
#[path = "../../tests/fixtures/windows/lifecycle.rs"]
mod lifecycle;

#[cfg(test)]
use super::fixture_private;
