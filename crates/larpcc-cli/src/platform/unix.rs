use super::*;
use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
};
fn io_error(action: &'static str, path: &Path, source: io::Error) -> Error {
    Error::Io {
        action,
        path: path.into(),
        source,
    }
}
fn unsafe_path(path: &Path, reason: &str) -> Error {
    Error::Unsafe {
        path: path.into(),
        reason: reason.into(),
    }
}
fn uid() -> u32 {
    // SAFETY: geteuid has no arguments or memory preconditions.
    unsafe { libc::geteuid() }
}
fn identity(meta: &fs::Metadata) -> FileIdentity {
    FileIdentity::Unix {
        dev: meta.dev(),
        ino: meta.ino(),
    }
}
fn acl(path: &Path) -> Result<(), Error> {
    #[cfg(target_os = "macos")]
    super::macos::check(path)?;
    #[cfg(not(target_os = "macos"))]
    let _ = path;
    Ok(())
}
fn acl_file(file: &File, path: &Path) -> Result<(), Error> {
    #[cfg(target_os = "macos")]
    super::macos::check_file(file, path)?;
    #[cfg(not(target_os = "macos"))]
    let _ = (file, path);
    Ok(())
}
fn managed(path: &Path, meta: &fs::Metadata, kind: ObjectKind) -> Result<(), Error> {
    if meta.file_type().is_symlink() {
        return Err(unsafe_path(path, "is a symbolic link or reparse point"));
    }
    let directory = kind == ObjectKind::Directory;
    if directory && !meta.is_dir() {
        return Err(unsafe_path(path, "is not a directory"));
    }
    if !directory && !meta.is_file() {
        return Err(unsafe_path(path, "is not a regular file"));
    }
    if meta.uid() != uid() {
        return Err(unsafe_path(
            path,
            &format!("is owned by uid {}, expected {}", meta.uid(), uid()),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(Error::ManagedMode {
            path: path.into(),
            mode: meta.mode() & 0o7777,
            directory: meta.is_dir(),
        });
    }
    Ok(())
}
fn open(path: &Path, kind: ObjectKind, write: bool) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.read(true).write(write).custom_flags(
        libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if kind == ObjectKind::Directory {
                libc::O_DIRECTORY
            } else {
                libc::O_NONBLOCK
            },
    );
    options.open(path).map_err(|e| io_error("open", path, e))
}
pub fn inspect(path: &Path, kind: ObjectKind) -> Result<Option<CheckedObject>, Error> {
    inspect_with(path, kind, || open(path, kind, false)).map_err(|error| error.diagnostic(path))
}
pub(crate) fn inspect_replaceable(path: &Path) -> Result<Option<CheckedObject>, AcquisitionError> {
    inspect_with(path, ObjectKind::File, || {
        open(path, ObjectKind::File, false)
    })
}
fn inspect_with(
    path: &Path,
    kind: ObjectKind,
    open_object: impl FnOnce() -> Result<File, Error>,
) -> Result<Option<CheckedObject>, AcquisitionError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error("inspect", path, e).into()),
    };
    managed(path, &meta, kind)?;
    let file = match open_object() {
        Ok(file) => file,
        // A cooperating writer can rename an optional temp after safe metadata inspection.
        Err(Error::Io {
            action: "open",
            source,
            ..
        }) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let checked = verify_opened(file, path, kind)?;
    if identity(&meta) != checked.identity {
        return Err(AcquisitionError::Changed);
    }
    Ok(Some(checked))
}
pub fn verify_opened(file: File, path: &Path, kind: ObjectKind) -> Result<CheckedObject, Error> {
    let meta = file.metadata().map_err(|e| io_error("inspect", path, e))?;
    // The diagnostic path is not reopened; mode/type/owner and identity come from this handle.
    managed(path, &meta, kind)?;
    acl_file(&file, path)?;
    Ok(CheckedObject {
        file,
        identity: identity(&meta),
    })
}

pub fn inspect_home(path: &Path) -> Result<DirStatus, Error> {
    Ok(match inspect(path, ObjectKind::Directory) {
        Ok(Some(_)) => DirStatus::Ok,
        Ok(None) => DirStatus::Missing,
        Err(Error::Unsafe { reason, .. }) => DirStatus::Unsafe(reason),
        Err(Error::ManagedMode { mode, .. }) => DirStatus::Unsafe(format!(
            "has mode {mode:04o}; group and other access must be removed"
        )),
        Err(_) => DirStatus::Unsafe("access control cannot be verified".into()),
    })
}
fn trusted(path: &Path, meta: &fs::Metadata) -> Result<(), Error> {
    if meta.uid() != 0 && meta.uid() != uid() {
        return Err(unsafe_path(path, "is owned by an untrusted principal"));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(unsafe_path(
            path,
            if meta.is_dir() {
                "has a foreign-writable ancestor"
            } else {
                "is writable by group or other users"
            },
        ));
    }
    acl(path)
}
// Validate each spelling component and each symlink target before following it.
fn resolve(
    path: &Path,
    hops: &mut usize,
    handles: &mut Vec<CheckedObject>,
) -> Result<PathBuf, Error> {
    let mut current = PathBuf::from("/");
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::RootDir | Component::CurDir => continue,
            Component::ParentDir => {
                current.pop();
                continue;
            }
            Component::Normal(n) => current.push(n),
            _ => {
                return Err(unsafe_path(
                    path,
                    "cannot resolve a trusted executable path",
                ));
            }
        }
        let meta = fs::symlink_metadata(&current).map_err(|e| io_error("inspect", &current, e))?;
        if meta.file_type().is_symlink() {
            *hops += 1;
            if *hops > 40 {
                return Err(unsafe_path(
                    path,
                    "cannot resolve a trusted executable path",
                ));
            }
            if meta.uid() != 0 && meta.uid() != uid() {
                return Err(unsafe_path(&current, "is owned by an untrusted principal"));
            }
            let target = fs::read_link(&current).map_err(|e| io_error("inspect", &current, e))?;
            let target = if target.is_absolute() {
                target
            } else {
                current.parent().unwrap_or(Path::new("/")).join(target)
            };
            current = resolve(&target, hops, handles)?;
        } else {
            trusted(&current, &meta)?;
            if meta.is_dir() {
                let file = open(&current, ObjectKind::Directory, false)?;
                let opened = file
                    .metadata()
                    .map_err(|e| io_error("inspect", &current, e))?;
                if identity(&meta) != identity(&opened) {
                    return Err(unsafe_path(
                        &current,
                        "filesystem identity cannot be verified",
                    ));
                }
                handles.push(CheckedObject {
                    file,
                    identity: identity(&opened),
                });
            }
        }
    }
    Ok(current)
}
pub fn anchor(path: &Path) -> Result<AnchoredPath, Error> {
    let mut ancestors = Vec::new();
    let mut parent = path.parent().unwrap_or(Path::new("/"));
    // Missing suffixes are permitted; creators still require the immediate parent.
    loop {
        match fs::symlink_metadata(parent) {
            Ok(_) => {
                resolve(parent, &mut 0, &mut ancestors)?;
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                parent = parent
                    .parent()
                    .ok_or_else(|| unsafe_path(path, "cannot resolve a trusted executable path"))?
            }
            Err(e) => return Err(io_error("inspect", parent, e)),
        }
    }
    let root_meta = fs::metadata("/").map_err(|e| io_error("inspect", Path::new("/"), e))?;
    trusted(Path::new("/"), &root_meta)?;
    Ok(AnchoredPath { ancestors })
}
pub fn create_file(path: &Path) -> Result<CheckedObject, Error> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| io_error("open", path, e))?;
    let result = (|| {
        let meta = file.metadata().map_err(|e| io_error("inspect", path, e))?;
        managed(path, &meta, ObjectKind::File)?;
        acl_file(&file, path)?;
        Ok(identity(&meta))
    })();
    match result {
        Ok(identity) => Ok(CheckedObject { file, identity }),
        Err(error) => {
            drop(file);
            let _ = fs::remove_file(path);
            Err(error)
        }
    }
}

pub fn create_directory(path: &Path) -> Result<CheckedObject, Error> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|e| io_error("create directory", path, e))?;
    let result = inspect(path, ObjectKind::Directory).and_then(|object| {
        object.ok_or_else(|| unsafe_path(path, "filesystem identity cannot be verified"))
    });
    if result.is_err() {
        let _ = fs::remove_dir(path);
    }
    result
}
pub fn executable(path: &Path, root: &Path) -> Result<(PathBuf, FileIdentity), Error> {
    let invalid = |reason: &str| unsafe_path(path, reason);
    let mut handles = Vec::new();
    let resolved = resolve(path, &mut 0, &mut handles)?;
    let canonical_root = canonical_missing(root)?;
    if contains_path(&resolved, &canonical_root)? {
        return Err(invalid("is inside the Larp CC data directory"));
    }
    let meta = fs::symlink_metadata(&resolved).map_err(|e| io_error("inspect", path, e))?;
    if !meta.is_file() {
        return Err(invalid("is not a regular file"));
    }
    if meta.mode() & 0o111 == 0 {
        return Err(invalid("is not executable"));
    }
    let file = open(&resolved, ObjectKind::File, false)?;
    acl_file(&file, path)?;
    let opened = file.metadata().map_err(|e| io_error("inspect", path, e))?;
    if identity(&opened) != identity(&meta) {
        return Err(invalid("changed since validation"));
    }
    let mut magic = [0; 4];
    let mut reader = &file;
    let count = reader
        .read(&mut magic)
        .map_err(|e| io_error("read", path, e))?;
    #[cfg(target_os = "linux")]
    let native = magic == *b"\x7fELF";
    #[cfg(target_os = "macos")]
    let native = matches!(
        u32::from_be_bytes(magic),
        0xfeedface
            | 0xcefaedfe
            | 0xfeedfacf
            | 0xcffaedfe
            | 0xcafebabe
            | 0xbebafeca
            | 0xcafebabf
            | 0xbfbafeca
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let native = false;
    if count != 4 || !native {
        return Err(invalid("is not a native executable"));
    }
    let current =
        fs::File::open(std::env::current_exe().map_err(|e| io_error("inspect", path, e))?)
            .map_err(|e| io_error("open", path, e))?;
    if identity(
        &current
            .metadata()
            .map_err(|e| io_error("inspect", path, e))?,
    ) == identity(&opened)
    {
        return Err(invalid("is Larp CC itself"));
    }
    Ok((resolved, identity(&opened)))
}
fn canonical_missing(path: &Path) -> Result<PathBuf, Error> {
    let mut suffix = Vec::new();
    let mut prefix = path;
    loop {
        match fs::canonicalize(prefix) {
            Ok(mut p) => {
                for s in suffix.iter().rev() {
                    p.push(s);
                }
                return Ok(p);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                suffix.push(
                    prefix
                        .file_name()
                        .ok_or_else(|| {
                            unsafe_path(path, "cannot resolve a trusted executable path")
                        })?
                        .to_owned(),
                );
                prefix = prefix
                    .parent()
                    .ok_or_else(|| unsafe_path(path, "cannot resolve a trusted executable path"))?;
            }
            Err(e) => return Err(io_error("inspect", path, e)),
        }
    }
}

pub fn execve(
    path: &Path,
    argv0: &std::ffi::OsStr,
    args: &[std::ffi::OsString],
    delta: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Result<u32, Error> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let launch = |source| Error::Launch {
        path: path.into(),
        source,
    };
    let cstring = |bytes: &[u8]| {
        CString::new(bytes).map_err(|_| {
            launch(io::Error::new(
                io::ErrorKind::InvalidInput,
                "launch data contains a NUL character",
            ))
        })
    };
    let executable = cstring(path.as_os_str().as_bytes())?;
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(cstring(argv0.as_bytes())?);
    for arg in args {
        argv.push(cstring(arg.as_bytes())?);
    }
    let mut environment = std::env::vars_os().collect::<Vec<_>>();
    for (key, value) in delta {
        environment.retain(|(name, _)| name != key);
        environment.push((key.clone(), value.clone()));
    }
    let mut env = Vec::with_capacity(environment.len());
    for (key, value) in &environment {
        if key.as_bytes().contains(&b'=') {
            return Err(launch(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid environment variable",
            )));
        }
        let mut entry = key.as_bytes().to_vec();
        entry.push(b'=');
        entry.extend_from_slice(value.as_bytes());
        env.push(cstring(&entry)?);
    }
    let mut argv_pointers = argv.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
    argv_pointers.push(std::ptr::null());
    let mut env_pointers = env.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
    env_pointers.push(std::ptr::null());
    // All fallible buffer construction precedes signal changes.
    // SAFETY: sigaction storage is fully initialized by a successful query before use.
    let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: SIGPIPE is valid, new action is null (query only), and output is writable.
    if unsafe { libc::sigaction(libc::SIGPIPE, std::ptr::null(), &mut previous) } != 0 {
        return Err(launch(io::Error::last_os_error()));
    }
    let changed = previous.sa_sigaction != libc::SIG_DFL;
    if changed {
        // SAFETY: zeroed action is initialized with SIG_DFL and an empty signal set before registration.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = libc::SIG_DFL;
        // SAFETY: mask is writable initialized storage, and sigaction receives a valid action for SIGPIPE.
        if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
            || unsafe { libc::sigaction(libc::SIGPIPE, &action, std::ptr::null_mut()) } != 0
        {
            return Err(launch(io::Error::last_os_error()));
        }
    }
    // SAFETY: owned CStrings and null-terminated pointer arrays remain live for the call;
    // executable/argv/env contain no interior NUL. execve never searches PATH or invokes a shell.
    unsafe {
        libc::execve(
            executable.as_ptr(),
            argv_pointers.as_ptr(),
            env_pointers.as_ptr(),
        );
    }
    let source = io::Error::last_os_error();
    if changed {
        // SAFETY: previous is the complete action captured above; restoring it does not change the mask or other signals.
        if unsafe { libc::sigaction(libc::SIGPIPE, &previous, std::ptr::null_mut()) } != 0 {
            return Err(io_error(
                "restore SIGPIPE disposition after failed exec",
                path,
                io::Error::last_os_error(),
            ));
        }
    }
    Err(launch(source))
}

pub fn contains_path(path: &Path, root: &Path) -> Result<bool, Error> {
    Ok(path.starts_with(root))
}

pub(crate) fn open_artifact(path: &Path, root: &Path) -> Result<(File, FileIdentity), Error> {
    let (resolved, expected) = executable(path, root)?;
    let file = open(&resolved, ObjectKind::File, false)?;
    let meta = file.metadata().map_err(|e| io_error("inspect", path, e))?;
    trusted(&resolved, &meta)?;
    acl_file(&file, path)?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 || identity(&meta) != expected {
        return Err(Error::ArtifactInvalid);
    }
    Ok((file, expected))
}

pub(crate) fn open_auth_lock(path: &Path) -> Result<CheckedObject, Error> {
    let checked = inspect(path, ObjectKind::File)?.ok_or(Error::AdmissionChanged)?;
    single_link(&checked.file)?;
    Ok(checked)
}

pub(crate) fn single_link(file: &File) -> Result<(), Error> {
    if file
        .metadata()
        .map_err(|_| Error::NamespaceUnverified)?
        .nlink()
        != 1
    {
        return Err(Error::NamespaceUnverified);
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum AuthExit {
    Code(u32),
    Signal(i32),
}

#[cfg(feature = "synthetic-launch")]
thread_local! { static AUTH_FAULT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }
#[cfg(feature = "synthetic-launch")]
#[allow(
    dead_code,
    reason = "Only the nonpackaged driver sets failure injection; ordinary all-features CLI has no entrypoint"
)]
pub(crate) fn synthetic_auth_fault(value: u8) {
    AUTH_FAULT.with(|f| f.set(value));
}
fn auth_fault(value: u8) -> bool {
    #[cfg(feature = "synthetic-launch")]
    {
        AUTH_FAULT.with(|f| f.get() == value)
    }
    #[cfg(not(feature = "synthetic-launch"))]
    {
        let _ = value;
        false
    }
}

// Handler state is process-local. The shipped CLI is single-threaded during dispatch.
static AUTH_SIGNALS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
extern "C" fn auth_signal(signal: i32) {
    let bit = match signal {
        libc::SIGTERM => 1,
        libc::SIGHUP => 2,
        _ => 0,
    };
    AUTH_SIGNALS.fetch_or(bit, std::sync::atomic::Ordering::SeqCst);
}
struct AuthSignals(Vec<(i32, libc::sigaction)>);
impl AuthSignals {
    fn install() -> Result<Self, Error> {
        AUTH_SIGNALS.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut saved = Self(Vec::new());
        for signal in [
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGCHLD,
        ] {
            // SAFETY: action storage is initialized before registration; handler only updates
            // a lock-free atomic, and uses no allocation, locks or process APIs.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = auth_signal as *const () as usize;
            // No SA_RESTART: waitpid must return EINTR for parent-directed cancellation.
            let mut previous = action;
            // SAFETY: valid signal numbers and writable initialized mask/action storage.
            if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
                || unsafe { libc::sigaction(signal, &action, &mut previous) } != 0
            {
                return Err(Error::Console {
                    action: "install authentication signal handling",
                    source: io::Error::last_os_error(),
                });
            }
            saved.0.push((signal, previous));
            if auth_fault(1) {
                return Err(Error::Console {
                    action: "install authentication signal handling",
                    source: io::Error::other("synthetic setup failure"),
                });
            }
        }
        Ok(saved)
    }
}
impl Drop for AuthSignals {
    fn drop(&mut self) {
        for (signal, previous) in self.0.iter().rev() {
            // SAFETY: these are the exact complete dispositions captured by install.
            unsafe {
                libc::sigaction(*signal, previous, std::ptr::null_mut());
            }
        }
    }
}
struct AuthMask(libc::sigset_t);
impl AuthMask {
    fn block() -> io::Result<Self> {
        // SAFETY: fully initialized writable mask storage; this single-threaded CLI
        // changes only its own mask, after spawn so the child inherits the original.
        let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut previous = blocked;
        // SAFETY: valid signal numbers and writable sets.
        let ok = unsafe {
            libc::sigemptyset(&mut blocked) == 0
                && libc::sigaddset(&mut blocked, libc::SIGCHLD) == 0
                && libc::sigaddset(&mut blocked, libc::SIGTERM) == 0
                && libc::sigaddset(&mut blocked, libc::SIGHUP) == 0
                && libc::sigprocmask(libc::SIG_BLOCK, &blocked, &mut previous) == 0
        };
        if ok {
            Ok(Self(previous))
        } else {
            Err(io::Error::last_os_error())
        }
    }
    fn suspend(&self) {
        let mut waiting = self.0;
        // SAFETY: initialized captured mask; unblock SIGCHLD even if inherited blocked
        // so child exit always wakes this supervisor. Restore the original on drop.
        unsafe {
            libc::sigdelset(&mut waiting, libc::SIGCHLD);
            libc::sigsuspend(&waiting);
        }
    }
}
impl Drop for AuthMask {
    fn drop(&mut self) {
        // SAFETY: restore exactly the mask captured above, after direct-child reap.
        unsafe {
            libc::sigprocmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut());
        }
    }
}
fn cancel_reap(pid: libc::pid_t) {
    // SAFETY: pid is the known unreaped direct child. Never kill an unknown tree.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
    let mut status = 0;
    loop {
        // SAFETY: same known child, writable status; no other reaper.
        if unsafe { libc::waitpid(pid, &mut status, 0) } == pid {
            return;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            return;
        }
        // Retain the cooperative guard if the OS cannot establish reap yet.
        if error.kind() != io::ErrorKind::Interrupted {
            std::thread::yield_now();
        }
    }
}
fn pending_auth_signal() -> Option<i32> {
    let pending = AUTH_SIGNALS.load(std::sync::atomic::Ordering::SeqCst);
    if pending & 1 != 0 {
        Some(libc::SIGTERM)
    } else if pending & 2 != 0 {
        Some(libc::SIGHUP)
    } else {
        None
    }
}
struct AuthExec {
    path: std::ffi::CString,
    _argv: Vec<std::ffi::CString>,
    _env: Vec<std::ffi::CString>,
    argv: Vec<*const libc::c_char>,
    env: Vec<*const libc::c_char>,
}
// SAFETY: every pointer refers to an immutable CString buffer owned by this struct;
// moving it never moves those allocations. No mutation or pointer escape is exposed.
unsafe impl Send for AuthExec {}
// SAFETY: all storage is immutable after construction; exec only reads the buffers.
unsafe impl Sync for AuthExec {}
impl AuthExec {
    fn prepare(plan: &crate::launch::LaunchPlan) -> Result<Self, Error> {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let invalid = || Error::Launch {
            path: plan.executable.clone(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "launch data contains a NUL character",
            ),
        };
        let string = |value: &[u8]| CString::new(value).map_err(|_| invalid());
        let path = string(plan.executable.as_os_str().as_bytes())?;
        let mut args = vec![string(plan.argv0.as_bytes())?];
        for arg in &plan.argv {
            args.push(string(arg.as_bytes())?);
        }
        let mut inherited = std::env::vars_os().collect::<Vec<_>>();
        for (key, value) in &plan.env_delta {
            inherited.retain(|(k, _)| k != key);
            inherited.push((key.clone(), value.clone()));
        }
        let mut environment = Vec::new();
        for (key, value) in inherited {
            if key.as_bytes().contains(&b'=') {
                return Err(Error::Launch {
                    path: plan.executable.clone(),
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid environment variable",
                    ),
                });
            }
            let mut bytes = key.as_bytes().to_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(value.as_bytes());
            environment.push(string(&bytes)?);
        }
        let mut argv = args.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
        argv.push(std::ptr::null());
        let mut env = environment.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
        env.push(std::ptr::null());
        Ok(Self {
            path,
            _argv: args,
            _env: environment,
            argv,
            env,
        })
    }
    fn execute(&self) -> io::Error {
        // SAFETY: owned CStrings and null-terminated pointer arrays stay live for
        // this call. Exact-path execve never searches PATH or falls back to a shell.
        unsafe {
            libc::execve(self.path.as_ptr(), self.argv.as_ptr(), self.env.as_ptr());
        }
        io::Error::last_os_error()
    }
}

pub(crate) fn auth_spawn_wait(plan: &crate::launch::LaunchPlan) -> Result<AuthExit, Error> {
    use std::os::unix::process::CommandExt;
    let prepared = AuthExec::prepare(plan)?;
    let signals = AuthSignals::install()?;
    if auth_fault(3) {
        return Ok(AuthExit::Signal(libc::SIGTERM));
    }
    if let Some(signal) = pending_auth_signal() {
        return Ok(AuthExit::Signal(signal));
    }
    let mut command = std::process::Command::new(&plan.executable);
    if let Some(cwd) = &plan.cwd {
        command.current_dir(cwd);
    }
    // Caught dispositions reset at exec; preserve dispositions that were originally
    // ignored before the wrapper installed its handlers (including SIGCHLD).
    let ignored = signals
        .0
        .iter()
        .filter(|(_, action)| action.sa_sigaction == libc::SIG_IGN)
        .map(|(signal, action)| (*signal, *action))
        .collect::<Vec<_>>();
    // SAFETY: the child hook only iterates storage prepared before fork and calls
    // async-signal-safe sigaction/execve. Exec failure always returns Err, so std
    // cannot use execvp's shell fallback. No allocation, locks or environment access.
    unsafe {
        command.pre_exec(move || {
            for (signal, action) in &ignored {
                if libc::sigaction(*signal, action, std::ptr::null_mut()) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Err(prepared.execute())
        });
    }
    let child = command.spawn().map_err(|source| Error::Launch {
        path: plan.executable.clone(),
        source,
    })?;
    let pid = child.id() as libc::pid_t;
    if auth_fault(2) {
        cancel_reap(pid);
        return Err(Error::Wait {
            source: io::Error::other("synthetic wait failure"),
            cleanup_failed: false,
        });
    }
    let mask = match AuthMask::block() {
        Ok(mask) => mask,
        Err(source) => {
            cancel_reap(pid);
            return Err(Error::Wait {
                source,
                cleanup_failed: false,
            });
        }
    };
    let mut sent = 0;
    let mut status = 0;
    loop {
        let pending = AUTH_SIGNALS.load(std::sync::atomic::Ordering::SeqCst);
        for (bit, signal) in [(1, libc::SIGTERM), (2, libc::SIGHUP)] {
            if pending & bit != 0 && sent & bit == 0 {
                // SAFETY: pid is our known unreaped direct child. Never a group/tree kill.
                unsafe {
                    libc::kill(pid, signal);
                }
                sent |= bit;
            }
        }
        // SAFETY: known direct child PID and writable status; no other thread reaps it.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == pid {
            break;
        }
        if result == 0 {
            mask.suspend();
            continue;
        }
        let source = io::Error::last_os_error();
        if source.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        // A wait failure cannot release the auth guard while the child is alive.
        cancel_reap(pid);
        return Err(Error::Wait {
            source,
            cleanup_failed: false,
        });
    }
    if libc::WIFEXITED(status) {
        Ok(AuthExit::Code(libc::WEXITSTATUS(status) as u32))
    } else {
        Ok(AuthExit::Signal(libc::WTERMSIG(status)))
    }
}
pub(crate) fn terminate_by_signal(signal: i32) -> Result<u32, Error> {
    // Called only after the direct child is reaped and every auth guard is dropped.
    // SAFETY: initialize default action and a single-signal mask before OS calls.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = libc::SIG_DFL;
    // SAFETY: valid initialized storage and a signal obtained from waitpid/our handlers.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: valid masks/actions; unblocking permits the same native signal death.
    let success = unsafe {
        libc::sigemptyset(&mut action.sa_mask) == 0
            && libc::sigaction(signal, &action, std::ptr::null_mut()) == 0
            && libc::sigemptyset(&mut set) == 0
            && libc::sigaddset(&mut set, signal) == 0
            && libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut()) == 0
            && libc::raise(signal) == 0
    };
    Err(Error::Console {
        action: "propagate provider signal",
        source: if success {
            io::Error::other("signal did not terminate process")
        } else {
            io::Error::last_os_error()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disappearing_safe_temp_is_missing_but_unsafe_observation_and_other_errors_refuse() {
        use std::os::unix::fs::PermissionsExt;
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
        let temporary = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let path = temporary.path().join("registry.json.tmp");
        drop(create_file(&path).unwrap());
        let result = inspect_with(&path, ObjectKind::File, || {
            fs::rename(&path, temporary.path().join("registry.json")).unwrap();
            open(&path, ObjectKind::File, false)
        })
        .unwrap();
        assert!(result.is_none());
        drop(create_file(&path).unwrap());
        let result = inspect_with(&path, ObjectKind::File, || {
            Err(io_error(
                "open",
                &path,
                io::ErrorKind::PermissionDenied.into(),
            ))
        });
        assert!(
            matches!(result,Err(AcquisitionError::Failed(Error::Io {source,..})) if source.kind()==io::ErrorKind::PermissionDenied)
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let opened = std::cell::Cell::new(false);
        let result = inspect_with(&path, ObjectKind::File, || {
            opened.set(true);
            fs::remove_file(&path).unwrap();
            open(&path, ObjectKind::File, false)
        });
        assert!(matches!(
            result,
            Err(AcquisitionError::Failed(Error::ManagedMode { .. }))
        ));
        assert!(!opened.get());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(temporary.path().join("registry.json"), &path).unwrap();
        assert!(matches!(
            inspect_with(&path, ObjectKind::File, || panic!(
                "unsafe symlink must refuse before open"
            )),
            Err(AcquisitionError::Failed(Error::Unsafe { .. }))
        ));
    }
    #[test]
    fn replaceable_inspection_retries_only_validated_identity_changes() {
        use std::os::unix::fs::PermissionsExt;
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
        let temporary = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        for name in ["registry.json", "registry.json.tmp"] {
            let path = temporary.path().join(name);
            drop(create_file(&path).unwrap());
            let replacement = temporary.path().join("replacement");
            let mut calls = 0;
            let object = acquire_replaceable(&path, || {
                inspect_with(&path, ObjectKind::File, || {
                    calls += 1;
                    if calls == 1 {
                        drop(create_file(&replacement).unwrap());
                        fs::rename(&replacement, &path).unwrap();
                    }
                    open(&path, ObjectKind::File, false)
                })
            })
            .unwrap()
            .unwrap();
            assert_eq!(calls, 2);
            assert_eq!(
                object.identity,
                identity(&fs::symlink_metadata(&path).unwrap())
            );
            // The same transition is still an immediate refusal for ordinary inspection.
            let error = inspect_with(&path, ObjectKind::File, || {
                drop(create_file(&replacement).unwrap());
                fs::rename(&replacement, &path).unwrap();
                open(&path, ObjectKind::File, false)
            })
            .err()
            .unwrap()
            .diagnostic(&path);
            assert!(matches!(error, Error::Unsafe { .. }));
            let mut calls = 0;
            let error = acquire_replaceable(&path, || {
                inspect_with(&path, ObjectKind::File, || {
                    calls += 1;
                    drop(create_file(&replacement).unwrap());
                    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o644)).unwrap();
                    fs::rename(&replacement, &path).unwrap();
                    open(&path, ObjectKind::File, false)
                })
            })
            .err()
            .unwrap();
            assert_eq!(calls, 1);
            assert!(matches!(error, Error::ManagedMode { .. }));
        }
    }
    #[cfg(feature = "synthetic-launch")]
    #[test]
    fn nul_is_rejected_before_signal_changes_or_execution() {
        let error = execve(
            Path::new("/nonexistent-synthetic-native"),
            std::ffi::OsStr::new("native"),
            &[std::ffi::OsString::from("bad\0arg")],
            &[],
        )
        .unwrap_err();
        assert!(
            matches!(error,Error::Launch {source,..} if source.kind()==io::ErrorKind::InvalidInput)
        );
    }
}
