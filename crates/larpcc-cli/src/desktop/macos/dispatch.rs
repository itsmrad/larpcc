use super::*;
use crate::desktop::{Activity, DesktopReceipt, InspectionCode, ValidatedDesktopTarget, singleton};
use std::{
    mem::{MaybeUninit, size_of},
    time::{Duration, Instant},
};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Serial {
    high: u32,
    low: u32,
}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn GetProcessForPID(pid: i32, serial: *mut Serial) -> i32;
    fn GetProcessPID(serial: *const Serial, pid: *mut i32) -> i32;
    fn SetFrontProcess(serial: *const Serial) -> i32;
}
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
    fn SecCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
}
#[derive(PartialEq, Eq)]
struct Stamp {
    pid: i32,
    birth: (u64, u64),
    serial: Serial,
}
struct Process {
    stamp: Stamp,
    code: Owned,
    lock: singleton::Lock,
}
fn stamp(pid: i32, expected: &Path) -> Result<Stamp, InspectionCode> {
    // SAFETY: signal zero performs a permission/liveness query, never sends a signal.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return Err(match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => InspectionCode::Stale,
            Some(libc::EPERM) => InspectionCode::Denied,
            _ => InspectionCode::Unavailable,
        });
    }
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
    // SAFETY: output has exactly PROC_PIDTBSDINFO size; no environment or argv is requested.
    let size = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if size != size_of::<libc::proc_bsdinfo>() as i32 {
        return Err(InspectionCode::Unavailable);
    }
    // SAFETY: successful full-size query initialized info.
    let info = unsafe { info.assume_init() };
    // SAFETY: geteuid has no pointer preconditions.
    let uid = unsafe { libc::geteuid() };
    if info.pbi_pid != pid as u32
        || info.pbi_uid != uid
        || info.pbi_ruid != uid
        || info.pbi_svuid != uid
        || info.pbi_start_tvsec == 0
        || info.pbi_start_tvusec >= 1_000_000
        || info.pbi_flags & 4 != 0
        || info.pbi_status == 5
    {
        return Err(InspectionCode::IdentityChanged);
    }
    let mut path = [0u8; 4096];
    // SAFETY: bounded proc_pidpath copies executable path only, not argv/env.
    let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    if len <= 0 {
        return Err(InspectionCode::Unavailable);
    }
    if len as usize >= path.len() {
        return Err(InspectionCode::Truncated);
    }
    let end = path
        .iter()
        .position(|b| *b == 0)
        .ok_or(InspectionCode::Truncated)?;
    if &path[..end] != expected.as_os_str().as_bytes() {
        return Err(InspectionCode::IdentityChanged);
    }
    let mut serial = Serial { high: 0, low: 0 };
    // SAFETY: fixed-size output pointer, positive verified PID.
    if unsafe { GetProcessForPID(pid, &mut serial) } != 0 {
        return Err(InspectionCode::Unavailable);
    }
    let mut actual = 0;
    // SAFETY: live initialized PSN maps an individual process instance, not a name.
    if unsafe { GetProcessPID(&serial, &mut actual) } != 0 || actual != pid {
        return Err(InspectionCode::IdentityChanged);
    }
    Ok(Stamp {
        pid,
        birth: (info.pbi_start_tvsec, info.pbi_start_tvusec),
        serial,
    })
}
fn dynamic_code(pid: i32) -> Result<Owned, InspectionCode> {
    // SAFETY: CFNumber copies the i32; dictionary borrows objects kept alive for the call.
    unsafe {
        let number = Owned::new(CFNumberCreate(
            std::ptr::null(),
            3,
            (&pid as *const i32).cast(),
        ))
        .map_err(|_| InspectionCode::Unavailable)?;
        let key = kSecGuestAttributePid;
        let dictionary = Owned::new(CFDictionaryCreate(
            std::ptr::null(),
            &key,
            &number.0,
            1,
            std::ptr::null(),
            std::ptr::null(),
        ))
        .map_err(|_| InspectionCode::Unavailable)?;
        let mut code = std::ptr::null();
        if SecCodeCopyGuestWithAttributes(std::ptr::null(), dictionary.0, 0, &mut code) != 0 {
            return Err(InspectionCode::Denied);
        }
        let code = Owned::new(code).map_err(|_| InspectionCode::Unavailable)?;
        check_code(&code)?;
        Ok(code)
    }
}
fn check_code(code: &Owned) -> Result<(), InspectionCode> {
    let req = requirement().map_err(|_| InspectionCode::Unavailable)?;
    // SAFETY: both references are live owned Security objects of the required types.
    if unsafe { SecCodeCheckValidity(code.0, 0, req.0) } != 0 {
        return Err(InspectionCode::IdentityChanged);
    }
    Ok(())
}
fn before_lock(stamp: &Stamp, lock: &singleton::Lock) -> bool {
    let birth = (stamp.birth.0 as i64, (stamp.birth.1 * 1000) as i64);
    birth <= lock.created && birth <= lock.modified
}
fn inspect(target: &ValidatedDesktopTarget) -> Result<Option<Process>, InspectionCode> {
    if target.app.path != Path::new("/Applications/ChatGPT.app") {
        return Err(InspectionCode::IdentityChanged);
    }
    target
        .recheck()
        .map_err(|_| InspectionCode::IdentityChanged)?;
    let hostname = singleton::hostname()?;
    let lock = match singleton::read(&target.snapshot.desktop, &hostname) {
        Ok(lock) => lock,
        Err(InspectionCode::Missing) => {
            target
                .recheck()
                .map_err(|_| InspectionCode::IdentityChanged)?;
            return match singleton::read(&target.snapshot.desktop, &hostname) {
                Err(InspectionCode::Missing) => Ok(None),
                _ => Err(InspectionCode::IdentityChanged),
            };
        }
        Err(code) => return Err(code),
    };
    let exe = target.app.path.join("Contents/MacOS/ChatGPT");
    let initial = match stamp(lock.pid, &exe) {
        Ok(stamp) => stamp,
        Err(InspectionCode::Stale) => {
            if singleton::read(&target.snapshot.desktop, &hostname)? != lock {
                return Err(InspectionCode::IdentityChanged);
            }
            // A positive PID can be reused between queries. Dead is accepted only
            // if a second targeted liveness query still reports ESRCH.
            if unsafe { libc::kill(lock.pid, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
            {
                return Err(InspectionCode::IdentityChanged);
            }
            return Ok(None);
        }
        Err(code) => return Err(code),
    };
    if !before_lock(&initial, &lock) {
        return Err(InspectionCode::IdentityChanged);
    }
    let code = dynamic_code(lock.pid)?;
    let process = Process {
        stamp: initial,
        code,
        lock,
    };
    recheck(target, &process)?;
    Ok(Some(process))
}
fn recheck(target: &ValidatedDesktopTarget, process: &Process) -> Result<(), InspectionCode> {
    target
        .recheck()
        .map_err(|_| InspectionCode::IdentityChanged)?;
    check_code(&process.code)?;
    let now = stamp(
        process.stamp.pid,
        &target.app.path.join("Contents/MacOS/ChatGPT"),
    )?;
    if now != process.stamp
        || !before_lock(&now, &process.lock)
        || singleton::read(&target.snapshot.desktop, &singleton::hostname()?)? != process.lock
    {
        return Err(InspectionCode::IdentityChanged);
    }
    Ok(())
}
pub(in crate::desktop) fn activity(target: &ValidatedDesktopTarget) -> Activity {
    if target.snapshot.provider != Provider::Codex {
        return Activity::Unknown(InspectionCode::Unavailable);
    }
    let start = Instant::now();
    let result = inspect(target);
    if start.elapsed() > Duration::from_secs(2) {
        return Activity::Unknown(InspectionCode::Timeout);
    }
    match result {
        Ok(Some(_)) => Activity::KnownActive,
        Ok(None) => Activity::Inactive,
        Err(code) => Activity::Unknown(code),
    }
}
pub(in crate::desktop) fn open(
    target: &ValidatedDesktopTarget,
    mut revalidate: impl FnMut() -> Result<(), Error>,
    mut notice: impl FnMut(&'static str),
) -> Result<DesktopReceipt, Error> {
    if target.snapshot.provider != Provider::Codex {
        return Err(Error::QualificationPending);
    }
    if target.app.path != Path::new("/Applications/ChatGPT.app") {
        return Err(Error::ArtifactInvalid);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let process = inspect(target).map_err(|_| Error::AdmissionChanged)?;
    revalidate()?;
    target.recheck()?;
    if Instant::now() >= deadline {
        return Err(Error::AdmissionChanged);
    }
    if !super::tested_version(&target.app.path) {
        notice(
            "desktop version has not been tested; native authentication and isolation are unverified",
        );
    }
    let outcome = if let Some(process) = process {
        recheck(target, &process).map_err(|_| Error::AdmissionChanged)?;
        // PSN is retained process-instance identity: PID reuse cannot redirect this activation.
        // SAFETY: live checked PSN, native API performs activation only, never termination.
        if unsafe { SetFrontProcess(&process.stamp.serial) } != 0 {
            return Err(Error::AdmissionChanged);
        }
        recheck(target, &process).map_err(|_| Error::AdmissionChanged)?;
        "focused"
    } else {
        launch(target, deadline, &mut revalidate)?;
        "launched"
    };
    Ok(DesktopReceipt {
        profile_id: target.snapshot.profile_id,
        provider: target.snapshot.provider,
        surface: "desktop",
        outcome,
        native_auth: "not_observed",
    })
}

unsafe extern "C" {
    fn _NSGetEnviron() -> *mut *mut *mut libc::c_char;
}
fn environment_names() -> Result<Vec<std::ffi::OsString>, Error> {
    use std::os::unix::ffi::OsStringExt;
    let mut names = Vec::new();
    // SAFETY: this CLI performs dispatch on its main thread with no concurrent
    // environment mutation. Inspect only name bytes before '='; never read values.
    unsafe {
        let root = _NSGetEnviron();
        if root.is_null() || (*root).is_null() {
            return Err(Error::AdmissionChanged);
        }
        let entries = *root;
        for index in 0..4096 {
            let entry = *entries.add(index);
            if entry.is_null() {
                return Ok(names);
            }
            let mut name = Vec::new();
            let mut complete = false;
            for offset in 0..4096 {
                let byte = *entry.add(offset) as u8;
                if byte == b'=' {
                    complete = true;
                    break;
                }
                if byte == 0 {
                    return Err(Error::AdmissionChanged);
                }
                name.push(byte);
            }
            if !complete || name.is_empty() {
                return Err(Error::AdmissionChanged);
            }
            if name.starts_with(b"CLAUDE")
                || name.starts_with(b"ANTHROPIC_")
                || name == b"CODEX_HOME"
            {
                names.push(std::ffi::OsString::from_vec(name));
            }
        }
    }
    Err(Error::AdmissionChanged)
}
fn launch(
    target: &ValidatedDesktopTarget,
    deadline: Instant,
    revalidate: &mut impl FnMut() -> Result<(), Error>,
) -> Result<(), Error> {
    use std::{
        ffi::OsString,
        process::{Command, Stdio},
    };
    let names = environment_names()?;
    let mut data = OsString::from("--user-data-dir=");
    data.push(&target.snapshot.desktop.path);
    let mut command = Command::new("/usr/bin/open");
    command.args([
        OsString::from("-n"),
        OsString::from("-a"),
        target.app.path.clone().into_os_string(),
        OsString::from("--args"),
        data,
    ]);
    for name in names {
        command.env_remove(name);
    }
    command
        .env("CODEX_HOME", &target.snapshot.cli.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // System executable validation is separate from the retained vendor target.
    let (_, open_identity) =
        platform::executable(Path::new("/usr/bin/open"), &target.snapshot.root.path)?;
    let (resolved, current_identity) =
        platform::executable(Path::new("/usr/bin/open"), &target.snapshot.root.path)?;
    if open_identity != current_identity {
        return Err(Error::AdmissionChanged);
    }
    let open_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&resolved)
        .map_err(|_| Error::AdmissionChanged)?;
    let meta = open_file.metadata().map_err(|_| Error::AdmissionChanged)?;
    if open_identity
        != (platform::FileIdentity::Unix {
            dev: meta.dev(),
            ino: meta.ino(),
        })
    {
        return Err(Error::AdmissionChanged);
    }
    target.recheck()?;
    if inspect(target)
        .map_err(|_| Error::AdmissionChanged)?
        .is_some()
        || Instant::now() >= deadline
    {
        return Err(Error::AdmissionChanged);
    }
    revalidate()?;
    if Instant::now() >= deadline {
        return Err(Error::AdmissionChanged);
    }
    let mut child = command.spawn().map_err(|_| Error::AdmissionChanged)?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(Error::AdmissionChanged)
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                // Only our unreaped /usr/bin/open child can be terminated; never the vendor app.
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::AdmissionChanged);
            }
        }
    }
}
