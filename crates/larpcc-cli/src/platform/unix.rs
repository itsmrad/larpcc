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
    inspect_with(path, kind, || open(path, kind, false))
}
fn inspect_with(
    path: &Path,
    kind: ObjectKind,
    open_object: impl FnOnce() -> Result<File, Error>,
) -> Result<Option<CheckedObject>, Error> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error("inspect", path, e)),
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
        Err(error) => return Err(error),
    };
    let checked = verify_opened(file, path, kind)?;
    if identity(&meta) != checked.identity {
        return Err(unsafe_path(path, "filesystem identity cannot be verified"));
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

#[cfg(feature = "synthetic-launch")]
#[allow(
    dead_code,
    reason = "Execution is used only by the separate synthetic driver."
)]
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
            matches!(result,Err(Error::Io {source,..}) if source.kind()==io::ErrorKind::PermissionDenied)
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let opened = std::cell::Cell::new(false);
        let result = inspect_with(&path, ObjectKind::File, || {
            opened.set(true);
            fs::remove_file(&path).unwrap();
            open(&path, ObjectKind::File, false)
        });
        assert!(matches!(result, Err(Error::ManagedMode { .. })));
        assert!(!opened.get());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(temporary.path().join("registry.json"), &path).unwrap();
        assert!(matches!(
            inspect_with(&path, ObjectKind::File, || panic!(
                "unsafe symlink must refuse before open"
            )),
            Err(Error::Unsafe { .. })
        ));
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

pub fn contains_path(path: &Path, root: &Path) -> Result<bool, Error> {
    Ok(path.starts_with(root))
}
