//! Inspect only the argument vector of a verified desktop app.

#[cfg(any(test, target_os = "macos"))]
use crate::registry::Error;
#[cfg(any(test, target_os = "macos"))]
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

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

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum App {
    ChatGpt,
    Claude,
}

#[cfg(any(test, target_os = "macos"))]
type StartTime = (u64, u64);

#[cfg(any(test, target_os = "macos"))]
fn candidate(path: &Path) -> Option<(App, bool)> {
    for contents in path.ancestors().skip(1) {
        if contents.file_name().is_some_and(|name| name == "Contents") {
            let Some(bundle) = contents.parent().and_then(Path::file_name) else {
                continue;
            };
            let app = if bundle == "ChatGPT.app" {
                App::ChatGpt
            } else if bundle == "Claude.app" {
                App::Claude
            } else {
                continue;
            };
            let main = match app {
                App::ChatGpt => "MacOS/ChatGPT",
                App::Claude => "MacOS/Claude",
            };
            return Some((app, path.strip_prefix(contents).ok()? == Path::new(main)));
        }
    }
    None
}

#[cfg(any(test, target_os = "macos"))]
trait ProcessTable {
    fn pids(&self) -> Result<Vec<i32>, Error>;
    fn executable(&self, pid: i32) -> io::Result<Option<PathBuf>>;
    fn stamp(&self, pid: i32, previous: Option<StartTime>) -> Result<Option<StartTime>, Error>;
    fn validate(&self, pid: i32, app: App, main: bool) -> Result<(), Error>;
    fn args(&self, pid: i32) -> Result<Option<Vec<u8>>, Error>;
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
enum ScanError {
    Busy,
    Deadline,
}

#[cfg(any(test, target_os = "macos"))]
impl From<Error> for ScanError {
    fn from(_: Error) -> Self {
        Self::Busy
    }
}

#[cfg(any(test, target_os = "macos"))]
fn retry_scan(mut scan: impl FnMut() -> Result<(), ScanError>) -> Result<(), Error> {
    let result = match scan() {
        Err(ScanError::Deadline) => scan(),
        result => result,
    };
    result.map_err(|_| Error::AuthBusy)
}

#[cfg(any(test, target_os = "macos"))]
fn scan(
    table: &impl ProcessTable,
    expected: &HashSet<Vec<u8>>,
    check_deadline: impl Fn() -> Result<(), ScanError>,
) -> Result<(), ScanError> {
    for pid in table.pids()? {
        check_deadline()?;
        // A denied path probe has identified no candidate and must not trigger an argv read.
        let path = match table.executable(pid) {
            Ok(Some(path)) => path,
            Ok(None) => continue,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => continue,
            Err(_) => return Err(ScanError::Busy),
        };
        let Some((app, main)) = candidate(&path) else {
            continue;
        };
        let Some(before) = table.stamp(pid, None)? else {
            continue;
        };
        let validation = table.validate(pid, app, main);
        if !same_process(table, pid, before, &path)? {
            continue;
        }
        validation?;
        check_deadline()?;
        let args = table.args(pid);
        if !same_process(table, pid, before, &path)? {
            continue;
        }
        let Some(args) = args? else {
            continue;
        };
        if user_data_dir(&args)?.is_some_and(|dir| expected.contains(dir)) {
            return Err(ScanError::Busy);
        }
    }
    check_deadline()
}

#[cfg(any(test, target_os = "macos"))]
fn same_process(
    table: &impl ProcessTable,
    pid: i32,
    before: StartTime,
    path: &Path,
) -> Result<bool, ScanError> {
    let executable = table.executable(pid);
    if table.stamp(pid, Some(before))? != Some(before) {
        return Ok(false);
    }
    match executable {
        Ok(None) => Ok(false),
        Ok(Some(current)) if current == path => Ok(true),
        _ => Err(ScanError::Busy),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
    };

    const HELPER: &str =
        "/Applications/ChatGPT.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper";
    const TARGET: &str = "/fixture/desktop";

    struct Fixture {
        path: Option<&'static str>,
        stamps: RefCell<VecDeque<Option<StartTime>>>,
        args: Option<&'static [u8]>,
        reads: Cell<usize>,
        listings: Cell<usize>,
    }

    impl Fixture {
        fn candidate(stamps: &[Option<StartTime>], args: Option<&'static [u8]>) -> Self {
            Self {
                path: Some(HELPER),
                stamps: VecDeque::from(stamps.to_vec()).into(),
                args,
                reads: Cell::new(0),
                listings: Cell::new(0),
            }
        }

        fn scan(&self, deadline: impl Fn() -> Result<(), ScanError>) -> Result<(), ScanError> {
            scan(self, &HashSet::from([TARGET.as_bytes().to_vec()]), deadline)
        }
    }

    impl ProcessTable for Fixture {
        fn pids(&self) -> Result<Vec<i32>, Error> {
            self.listings.set(self.listings.get() + 1);
            Ok(vec![1])
        }

        fn executable(&self, _pid: i32) -> io::Result<Option<PathBuf>> {
            match self.path {
                Some(path) => Ok(Some(path.into())),
                // EPERM from proc_pidpath maps to this kind on macOS.
                None => Err(io::ErrorKind::PermissionDenied.into()),
            }
        }

        fn stamp(
            &self,
            _pid: i32,
            _previous: Option<StartTime>,
        ) -> Result<Option<StartTime>, Error> {
            Ok(self
                .stamps
                .borrow_mut()
                .pop_front()
                .expect("unexpected stamp"))
        }

        fn validate(&self, _pid: i32, app: App, main: bool) -> Result<(), Error> {
            assert_eq!((app, main), (App::ChatGpt, false));
            Ok(())
        }

        fn args(&self, _pid: i32) -> Result<Option<Vec<u8>>, Error> {
            self.reads.set(self.reads.get() + 1);
            let args = self.args.ok_or(Error::AuthBusy)?;
            let mut packet = 2i32.to_ne_bytes().to_vec();
            packet.extend_from_slice(b"/fixture/Helper\0\0Helper\0");
            packet.extend_from_slice(args);
            Ok(Some(packet))
        }
    }

    #[test]
    fn stage04_delete_skips_unrelated_and_denied_processes_before_argv() {
        for path in [Some("/Applications/Other.app/Contents/MacOS/Other"), None] {
            let table = Fixture {
                path,
                ..Fixture::candidate(&[], None)
            };
            assert_eq!(table.scan(|| Ok(())), Ok(()));
            assert_eq!(table.reads.get(), 0);
        }
    }

    #[test]
    fn stage04_delete_skips_vanished_or_reused_candidate() {
        let stamp = Some((10, 0));
        // Failed argv followed by either ESRCH (no stamp) or a new start time.
        for after in [None, Some((11, 0))] {
            let table = Fixture::candidate(&[stamp, stamp, after], None);
            assert_eq!(table.scan(|| Ok(())), Ok(()));
            assert_eq!(table.reads.get(), 1);
        }
    }

    #[test]
    fn stage04_delete_refuses_unreadable_truncated_or_active_candidate() {
        let stable = [Some((10, 0)); 3];
        for args in [
            None,
            Some(b"--user-data-dir=/truncated".as_slice()),
            Some(b"--user-data-dir=/fixture/desktop\0".as_slice()),
        ] {
            let table = Fixture::candidate(&stable, args);
            assert_eq!(table.scan(|| Ok(())), Err(ScanError::Busy));
            assert_eq!(table.reads.get(), 1);
        }
        let table = Fixture::candidate(&stable, Some(b"--user-data-dir=/fixture/desktop-other\0"));
        assert_eq!(table.scan(|| Ok(())), Ok(()));
    }

    #[test]
    fn stage04_delete_retries_only_a_deadline_and_at_most_once() {
        for retry_expired in [false, true] {
            let table = Fixture {
                path: None,
                ..Fixture::candidate(&[], None)
            };
            let result = retry_scan(|| {
                table.scan(|| {
                    if table.listings.get() == 1 || retry_expired {
                        Err(ScanError::Deadline)
                    } else {
                        Ok(())
                    }
                })
            });
            assert_eq!(result.is_err(), retry_expired);
            assert_eq!(table.listings.get(), 2);
        }
        let table = Fixture::candidate(&[Some((10, 0)); 3], None);
        assert!(matches!(
            retry_scan(|| table.scan(|| Ok(()))),
            Err(Error::AuthBusy)
        ));
        assert_eq!(table.listings.get(), 1);
    }
}

#[cfg(target_os = "macos")]
pub(super) use macos::require_quiet;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        ffi::{OsString, c_void},
        mem::MaybeUninit,
        os::unix::ffi::{OsStrExt, OsStringExt},
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };

    const PROCESS_LIMIT: usize = 4096;
    const ARG_LIMIT: usize = 64 * 1024;
    const PROC_UID_ONLY: u32 = 4; // Darwin sys/proc_info.h

    pub(in crate::registry::delete) fn require_quiet(paths: &[PathBuf]) -> Result<(), Error> {
        let table = NativeProcessTable {
            uid: unsafe { libc::geteuid() },
        };
        retry_scan(|| {
            let deadline = Instant::now() + Duration::from_secs(2);
            let check_deadline = || {
                if Instant::now() >= deadline {
                    Err(ScanError::Deadline)
                } else {
                    Ok(())
                }
            };
            let mut expected = HashSet::new();
            for path in paths {
                check_deadline()?;
                expected.insert(path.as_os_str().as_bytes().to_vec());
                match path.canonicalize() {
                    Ok(resolved) => {
                        expected.insert(resolved.as_os_str().as_bytes().to_vec());
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                    Err(_) => return Err(ScanError::Busy),
                }
            }
            check_deadline()?;
            scan(&table, &expected, check_deadline)
        })
    }

    struct NativeProcessTable {
        uid: u32,
    }

    impl ProcessTable for NativeProcessTable {
        fn pids(&self) -> Result<Vec<i32>, Error> {
            let mut pids = [0i32; PROCESS_LIMIT];
            let capacity = std::mem::size_of_val(&pids);
            // SAFETY: the kernel writes at most capacity bytes into a live pid_t array.
            let length = unsafe {
                libc::proc_listpids(
                    PROC_UID_ONLY,
                    self.uid,
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
            Ok(pids
                .into_iter()
                .take(length as usize / size_of::<i32>())
                .filter(|&pid| pid > 0)
                .collect())
        }

        fn executable(&self, pid: i32) -> io::Result<Option<PathBuf>> {
            let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
            // SAFETY: bounded writable path buffer.
            let len =
                unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
            if len <= 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH)
                    || process_info(pid).is_ok_and(|info| zombie(pid, &info))
                    || exited(pid)
                {
                    return Ok(None);
                }
                return Err(error);
            }
            if len as usize >= path.len() || path[0] != b'/' {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let end = path
                .iter()
                .position(|&byte| byte == 0)
                .ok_or(io::ErrorKind::InvalidData)?;
            Ok(Some(OsString::from_vec(path[..end].to_vec()).into()))
        }

        fn stamp(&self, pid: i32, previous: Option<StartTime>) -> Result<Option<StartTime>, Error> {
            let info = match process_info(pid) {
                Ok(info) => info,
                Err(e) if e.raw_os_error() == Some(libc::ESRCH) || exited(pid) => return Ok(None),
                Err(_) => return Err(Error::AuthBusy),
            };
            let start = (info.pbi_start_tvsec, info.pbi_start_tvusec);
            if info.pbi_pid == pid as u32 && previous.is_some_and(|previous| previous != start)
                || zombie(pid, &info)
            {
                return Ok(None);
            }
            if info.pbi_pid != pid as u32
                || info.pbi_uid != self.uid
                || info.pbi_ruid != self.uid
                || info.pbi_svuid != self.uid
                || info.pbi_flags & 4 != 0
                || info.pbi_start_tvsec == 0
                || info.pbi_start_tvusec >= 1_000_000
            {
                return Err(Error::AuthBusy);
            }
            Ok(Some(start))
        }

        fn validate(&self, pid: i32, app: App, main: bool) -> Result<(), Error> {
            let (identifier, team) = match app {
                App::ChatGpt => ("com.openai.codex", "2DC432GLL2"),
                App::Claude => ("com.anthropic.claudefordesktop", "Q6L2SF6YDW"),
            };
            let mut requirement = String::new();
            if main {
                requirement = format!("identifier \"{identifier}\" and ");
            }
            // Nested helpers have their own identifiers but must carry the bundle vendor's signature.
            requirement.push_str("anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = ");
            requirement.push_str(&format!("\"{team}\""));
            validate(pid, requirement.as_bytes())
        }

        fn args(&self, pid: i32) -> Result<Option<Vec<u8>>, Error> {
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
            {
                if io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                    return Ok(None);
                }
                return Err(Error::AuthBusy);
            }
            if length >= args.len() {
                return Err(Error::AuthBusy);
            }
            args.truncate(length);
            Ok(Some(args))
        }
    }

    fn exited(pid: i32) -> bool {
        // SAFETY: signal 0 only probes liveness.
        (unsafe { libc::kill(pid, 0) }) != 0
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn zombie(pid: i32, info: &libc::proc_bsdinfo) -> bool {
        info.pbi_pid == pid as u32 && info.pbi_status == 5
    }

    fn process_info(pid: i32) -> io::Result<libc::proc_bsdinfo> {
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
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { info.assume_init() })
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
