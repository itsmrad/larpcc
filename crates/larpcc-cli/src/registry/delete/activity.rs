//! Inspect only the argument vector of a verified desktop app.

#[cfg(any(test, target_os = "macos"))]
use crate::registry::Error;

#[cfg(any(test, target_os = "macos"))]
fn user_data_dir(bytes: &[u8]) -> Result<Option<&[u8]>, Error> {
    let (argc, bytes) = bytes.split_first_chunk::<4>().ok_or(Error::AuthBusy)?;
    let argc = i32::from_ne_bytes(*argc);
    if !(1..=4096).contains(&argc) {
        return Err(Error::AuthBusy);
    }
    let path_end = bytes.iter().position(|b| *b == 0).ok_or(Error::AuthBusy)?;
    if path_end == 0 {
        return Err(Error::AuthBusy);
    }
    let bytes = &bytes[path_end + 1..];
    let argv = bytes.iter().position(|b| *b != 0).ok_or(Error::AuthBusy)?;
    let mut bytes = &bytes[argv..];
    let mut data_dir = None;
    let mut value_next = false;
    for index in 0..argc {
        let end = bytes.iter().position(|b| *b == 0).ok_or(Error::AuthBusy)?;
        let arg = &bytes[..end];
        bytes = &bytes[end + 1..];
        if index == 0 {
            if arg.is_empty() {
                return Err(Error::AuthBusy);
            }
            continue;
        }
        let value = if value_next {
            value_next = false;
            Some(arg)
        } else if arg == b"--user-data-dir" {
            value_next = true;
            None
        } else {
            arg.strip_prefix(b"--user-data-dir=")
        };
        if let Some(value) = value
            && (!value.starts_with(b"/") || data_dir.replace(value).is_some())
        {
            return Err(Error::AuthBusy);
        }
    }
    if value_next {
        return Err(Error::AuthBusy);
    }
    Ok(data_dir)
}

#[cfg(test)]
#[test]
fn stage04_delete_recovery_exact_desktop_argv() {
    fn query(argc: i32, rest: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let mut packet = argc.to_ne_bytes().to_vec();
        packet.extend_from_slice(b"/Applications/ChatGPT.app/Contents/MacOS/ChatGPT\0\0ChatGPT\0");
        packet.extend_from_slice(rest);
        user_data_dir(&packet).map(|path| path.map(<[u8]>::to_vec))
    }
    for argv in [
        b"--user-data-dir=/fixture/desktop\0".as_slice(),
        b"--user-data-dir\0/fixture/desktop\0",
    ] {
        let argc = if argv.starts_with(b"--user-data-dir=") {
            2
        } else {
            3
        };
        assert_eq!(
            query(argc, argv).unwrap().as_deref(),
            Some(b"/fixture/desktop".as_slice())
        );
    }
    assert_eq!(
        query(1, b"--user-data-dir=/environment/ignored\0").unwrap(),
        None
    );
    assert_eq!(
        query(2, b"--another=--user-data-dir=/fixture/desktop\0").unwrap(),
        None
    );
    assert!(query(2, b"--user-data-dir=/truncated").is_err());
    assert!(query(2, b"--user-data-dir\0").is_err());
    assert!(query(3, b"--user-data-dir=/a\0--user-data-dir=/b\0").is_err());
}

#[cfg(target_os = "macos")]
pub(super) use macos::require_quiet;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        collections::HashSet,
        ffi::c_void,
        mem::MaybeUninit,
        os::unix::ffi::OsStrExt,
        path::{Path, PathBuf},
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };

    const PROCESS_LIMIT: usize = 4096;
    const ARG_LIMIT: usize = 64 * 1024;
    const PROC_UID_ONLY: u32 = 4; // Darwin sys/proc_info.h

    pub(in crate::registry::delete) fn require_quiet(paths: &[PathBuf]) -> Result<(), Error> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut expected = HashSet::new();
        for path in paths {
            within(deadline)?;
            expected.insert(path.as_os_str().as_bytes().to_vec());
            match path.canonicalize() {
                Ok(resolved) => {
                    expected.insert(resolved.as_os_str().as_bytes().to_vec());
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(Error::AuthBusy),
            }
        }
        let uid = unsafe { libc::geteuid() };
        let mut pids = [0i32; PROCESS_LIMIT];
        let capacity = std::mem::size_of_val(&pids);
        // SAFETY: the kernel writes at most capacity bytes into a live pid_t array.
        let length = unsafe {
            libc::proc_listpids(
                PROC_UID_ONLY,
                uid,
                pids.as_mut_ptr().cast(),
                capacity as i32,
            )
        };
        if length <= 0
            || length as usize >= capacity
            || !(length as usize).is_multiple_of(size_of::<i32>())
        {
            return Err(Error::AuthBusy);
        }
        for pid in pids[..length as usize / size_of::<i32>()].iter().copied() {
            within(deadline)?;
            if pid <= 0 {
                continue;
            }
            let Some(path) = executable(pid)? else {
                continue;
            };
            let path = Path::new(std::ffi::OsStr::from_bytes(&path));
            let requirement = if path.ends_with("ChatGPT.app/Contents/MacOS/ChatGPT") {
                b"identifier \"com.openai.codex\" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"2DC432GLL2\"".as_slice()
            } else if path.ends_with("Claude.app/Contents/MacOS/Claude") {
                // Anthropic's published macOS PPPC code requirement.
                b"identifier \"com.anthropic.claudefordesktop\" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"Q6L2SF6YDW\"".as_slice()
            } else {
                continue;
            };
            let before = stamp(pid, uid)?;
            validate(pid, requirement)?;
            within(deadline)?;
            let mut args = vec![0u8; ARG_LIMIT];
            let mut length = args.len();
            let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
            // SAFETY: output storage and length are bounded. Only argc argv entries are parsed.
            if unsafe {
                libc::sysctl(
                    mib.as_mut_ptr(),
                    3,
                    args.as_mut_ptr().cast(),
                    &mut length,
                    null_mut(),
                    0,
                )
            } != 0
                || length >= args.len()
            {
                return Err(Error::AuthBusy);
            }
            let active = user_data_dir(&args[..length])?.is_some_and(|dir| expected.contains(dir));
            drop(args);
            if stamp(pid, uid)? != before
                || executable(pid)?.as_deref() != Some(path.as_os_str().as_bytes())
            {
                return Err(Error::AuthBusy);
            }
            within(deadline)?;
            if active {
                return Err(Error::AuthBusy);
            }
        }
        within(deadline)
    }

    fn within(deadline: Instant) -> Result<(), Error> {
        if Instant::now() >= deadline {
            Err(Error::AuthBusy)
        } else {
            Ok(())
        }
    }

    fn executable(pid: i32) -> Result<Option<Vec<u8>>, Error> {
        let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: bounded writable path buffer. Signal 0 only probes liveness.
        let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
        if len <= 0 {
            if process_info(pid)
                .is_ok_and(|info| info.pbi_pid == pid as u32 && info.pbi_status == 5)
                || unsafe { libc::kill(pid, 0) } != 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                return Ok(None);
            }
            return Err(Error::AuthBusy);
        }
        if len as usize >= path.len() {
            return Err(Error::AuthBusy);
        }
        let end = path.iter().position(|b| *b == 0).ok_or(Error::AuthBusy)?;
        if end == 0 || path[0] != b'/' {
            return Err(Error::AuthBusy);
        }
        Ok(Some(path[..end].to_vec()))
    }

    fn process_info(pid: i32) -> Result<libc::proc_bsdinfo, Error> {
        let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
        let size = size_of::<libc::proc_bsdinfo>() as i32;
        // SAFETY: proc_pidinfo initializes the full output only on an exact-size response.
        if unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        } != size
        {
            return Err(Error::AuthBusy);
        }
        Ok(unsafe { info.assume_init() })
    }

    fn stamp(pid: i32, uid: u32) -> Result<(u64, u64), Error> {
        let info = process_info(pid)?;
        if info.pbi_pid != pid as u32
            || info.pbi_uid != uid
            || info.pbi_ruid != uid
            || info.pbi_svuid != uid
            || info.pbi_flags & 4 != 0
            || info.pbi_status == 5
            || info.pbi_start_tvsec == 0
            || info.pbi_start_tvusec >= 1_000_000
        {
            return Err(Error::AuthBusy);
        }
        Ok((info.pbi_start_tvsec, info.pbi_start_tvusec))
    }

    // TODO: share the process signature predicate with desktop dispatch after #24.
    type Ref = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
        fn CFDictionaryCreate(
            allocator: Ref,
            keys: *const Ref,
            values: *const Ref,
            count: isize,
            key_callbacks: Ref,
            value_callbacks: Ref,
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
        static kSecGuestAttributePid: Ref;
        fn SecCodeCopyGuestWithAttributes(
            host: Ref,
            attributes: Ref,
            flags: u32,
            code: *mut Ref,
        ) -> i32;
        fn SecRequirementCreateWithString(text: Ref, flags: u32, requirement: *mut Ref) -> i32;
        fn SecCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
    }
    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref) -> Result<Self, Error> {
            if value.is_null() {
                Err(Error::AuthBusy)
            } else {
                Ok(Self(value))
            }
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }

    fn validate(pid: i32, requirement: &[u8]) -> Result<(), Error> {
        // SAFETY: owned Create outputs are kept alive for borrowed dictionary values.
        unsafe {
            let number = Owned::new(CFNumberCreate(null(), 3, (&pid as *const i32).cast()))?;
            let key = kSecGuestAttributePid;
            let attrs = Owned::new(CFDictionaryCreate(
                null(),
                &key,
                &number.0,
                1,
                null(),
                null(),
            ))?;
            let mut code = null();
            if SecCodeCopyGuestWithAttributes(null(), attrs.0, 0, &mut code) != 0 {
                return Err(Error::AuthBusy);
            }
            let code = Owned::new(code)?;
            let text = Owned::new(CFStringCreateWithBytes(
                null(),
                requirement.as_ptr(),
                requirement.len() as isize,
                0x08000100,
                0,
            ))?;
            let mut requirement = null();
            if SecRequirementCreateWithString(text.0, 0, &mut requirement) != 0 {
                return Err(Error::AuthBusy);
            }
            let requirement = Owned::new(requirement)?;
            if SecCodeCheckValidity(code.0, 0, requirement.0) != 0 {
                return Err(Error::AuthBusy);
            }
        }
        Ok(())
    }
}
