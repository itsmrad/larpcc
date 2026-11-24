//! Stage 08 version-only probe: a separate reviewed descriptor table, never an
//! ArtifactRow, so it can never admit run/login/logout/status. Detected versions
//! are result-local; nothing is cached or persisted.
use crate::{
    launch::admission,
    registry::{Error, Provider},
};
use std::path::Path;
#[cfg(unix)]
use std::{
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant},
};

/// Combined stdout+stderr capture cap.
pub(crate) const MAX_OUTPUT: usize = 4096;
#[cfg(unix)]
const TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(unix)]
const REAP: Duration = Duration::from_secs(2);

// Every value is reviewed descriptor data, never inferred from an installed provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VersionDescriptor {
    pub provider: Provider,
    pub os: &'static str,
    pub arch: &'static str,
    pub sha256: [u8; 32],
    /// Exact argv after argv[0]; no other argument is ever added.
    pub argv: &'static [&'static str],
    /// The fresh isolated home variable; must be the provider's own.
    pub home_env: &'static str,
    /// Accepted output: exactly `prefix VERSION suffix LF` on stdout, empty stderr, exit 0.
    pub prefix: &'static str,
    pub suffix: &'static str,
    pub evidence: &'static str,
}

/// No source or digest is approved yet. Adding one is a separately reviewed change
/// citing the stage 14 Phase 2 step 1 evidence for that exact artifact.
pub(crate) const DESCRIPTORS: &[VersionDescriptor] = &[];

#[cfg(feature = "synthetic-launch")]
thread_local! {
    static INVENTED: std::cell::RefCell<Option<Vec<VersionDescriptor>>> =
        const { std::cell::RefCell::new(None) };
}
/// Nonpackaged driver only: invented descriptors for tests. The ordinary CLI,
/// even with all features, has no entrypoint that calls this.
#[cfg(feature = "synthetic-launch")]
#[allow(
    dead_code,
    reason = "Only the nonpackaged driver injects invented descriptors"
)]
pub(crate) fn synthetic_descriptors(table: Vec<VersionDescriptor>) {
    INVENTED.with(|slot| *slot.borrow_mut() = Some(table));
}
fn table() -> Vec<VersionDescriptor> {
    #[cfg(feature = "synthetic-launch")]
    if let Some(table) = INVENTED.with(|slot| slot.borrow().clone()) {
        return table;
    }
    DESCRIPTORS.to_vec()
}

#[cfg(unix)]
pub(crate) fn approved_candidate(provider: Provider, sha256: [u8; 32]) -> bool {
    table().iter().any(|descriptor| {
        descriptor.provider == provider
            && descriptor.os == std::env::consts::OS
            && descriptor.arch == std::env::consts::ARCH
            && descriptor.sha256 == sha256
            && well_formed(descriptor)
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Detection {
    // Only the Unix probe produces a version; Windows never spawns (known limitation).
    #[cfg_attr(not(unix), allow(dead_code))]
    Detected(String),
    /// No reviewed descriptor matches provider, OS, architecture and digest.
    Unapproved,
    /// Rejected output, exit, timeout or an artifact that changed. Never detailed.
    Failed,
}

pub(crate) fn home_env(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "CLAUDE_CONFIG_DIR",
        Provider::Codex => "CODEX_HOME",
    }
}
#[cfg(unix)]
fn fixture(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude-work",
        Provider::Codex => "codex-45",
    }
}

/// Probe the explicitly bound executable. `Err` only for operational failures
/// (workspace creation, unproven reap, cleanup); every probe refusal is `Failed`.
pub(crate) fn detect(
    provider: Provider,
    executable: &Path,
    root: &Path,
) -> Result<Detection, Error> {
    let candidates: Vec<_> = table()
        .into_iter()
        .filter(|d| {
            d.provider == provider
                && d.os == std::env::consts::OS
                && d.arch == std::env::consts::ARCH
        })
        .collect();
    // No candidate: refuse without reading the artifact bytes.
    if candidates.is_empty() {
        return Ok(Detection::Unapproved);
    }
    let Ok((resolved, identity)) = super::validate(provider, executable, root) else {
        return Ok(Detection::Failed);
    };
    let Ok(artifact) = admission::checked_artifact(executable, root) else {
        return Ok(Detection::Failed);
    };
    if artifact.identity != identity {
        return Ok(Detection::Failed);
    }
    let Some(descriptor) = candidates.into_iter().find(|d| d.sha256 == artifact.sha256) else {
        return Ok(Detection::Unapproved);
    };
    if !well_formed(&descriptor) {
        return Ok(Detection::Failed);
    }
    #[cfg(unix)]
    {
        probe(
            provider,
            &descriptor,
            executable,
            &resolved,
            root,
            &artifact,
        )
    }
    #[cfg(not(unix))]
    {
        // Windows has no checked tree removal for the throwaway home; never spawn.
        let _ = (resolved, artifact);
        Ok(Detection::Failed)
    }
}

fn well_formed(d: &VersionDescriptor) -> bool {
    let printable = |s: &str| s.bytes().all(|b| (0x20..0x7f).contains(&b));
    d.home_env == home_env(d.provider)
        && d.argv.len() <= 8
        && d.argv
            .iter()
            .all(|a| !a.is_empty() && a.len() <= 64 && printable(a))
        && d.prefix.len() <= 64
        && d.suffix.len() <= 64
        && printable(d.prefix)
        && printable(d.suffix)
}

#[cfg_attr(not(unix), allow(dead_code))]
/// Exactly one LF-terminated stdout line `prefix VERSION suffix`, empty stderr, exit 0.
pub(crate) fn parse(
    d: &VersionDescriptor,
    stdout: &[u8],
    stderr: &[u8],
    exit: Option<i32>,
) -> Option<String> {
    if exit != Some(0) || !stderr.is_empty() || stdout.len() + stderr.len() > MAX_OUTPUT {
        return None;
    }
    let line = std::str::from_utf8(stdout).ok()?.strip_suffix('\n')?;
    if line.bytes().any(|b| !(0x20..0x7f).contains(&b)) {
        return None;
    }
    let version = line.strip_prefix(d.prefix)?.strip_suffix(d.suffix)?;
    valid_version(version).then(|| version.to_owned())
}

#[cfg_attr(not(unix), allow(dead_code))]
/// `N.N.N` (1-9 digits, no leading zero) with an optional `-[0-9A-Za-z.]{1,32}`.
fn valid_version(version: &str) -> bool {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 9
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.len() <= 32
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
        })
}

/// Temporary LARPCC_HOME `<root>/probes/<nonce>` with one throwaway fixture profile.
#[cfg(unix)]
struct Workspace {
    probes: PathBuf,
    temp: PathBuf,
    fixture: &'static str,
}
#[cfg(unix)]
impl Workspace {
    fn create(root: &Path, provider: Provider) -> Result<Self, Error> {
        use crate::platform::{self, ObjectKind};
        let probes = root.join("probes");
        match platform::create_directory(&probes) {
            Ok(_) => (),
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                platform::inspect(&probes, ObjectKind::Directory)?.ok_or(Error::Drift)?;
            }
            Err(error) => return Err(error),
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        for attempt in 0..16u32 {
            let temp = probes.join(format!("{}-{nanos}-{attempt}", std::process::id()));
            match platform::create_directory(&temp) {
                Ok(_) => {
                    let workspace = Self {
                        probes,
                        temp,
                        fixture: fixture(provider),
                    };
                    if let Err(error) = workspace.populate() {
                        let _ = workspace.remove();
                        return Err(error);
                    }
                    return Ok(workspace);
                }
                Err(Error::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(Error::Drift)
    }
    fn profile(&self) -> PathBuf {
        self.temp.join("profiles").join(self.fixture)
    }
    fn home(&self) -> PathBuf {
        self.profile().join("home")
    }
    fn cli(&self) -> PathBuf {
        self.profile().join("cli")
    }
    fn tmp(&self) -> PathBuf {
        self.profile().join("tmp")
    }
    fn populate(&self) -> Result<(), Error> {
        let profiles = self.temp.join("profiles");
        for path in [
            profiles,
            self.profile(),
            self.home(),
            self.cli(),
            self.tmp(),
        ] {
            crate::platform::create_directory(&path)?;
        }
        Ok(())
    }
    /// Checked handle-relative tree removal, then the now-empty directories.
    fn remove(&self) -> Result<(), Error> {
        use crate::platform::{self, ObjectKind};
        let profiles = self.temp.join("profiles");
        if platform::inspect(&profiles, ObjectKind::Directory)?.is_some() {
            let tree = platform::removal::Profiles::open(&self.temp)?;
            if let Some(object) = platform::inspect(&self.profile(), ObjectKind::Directory)? {
                tree.remove(self.fixture, &object.identity)?;
            }
            drop(tree);
            std::fs::remove_dir(&profiles).map_err(|source| Error::Io {
                action: "remove probe directory",
                path: profiles.clone(),
                source,
            })?;
        }
        std::fs::remove_dir(&self.temp).map_err(|source| Error::Io {
            action: "remove probe directory",
            path: self.temp.clone(),
            source,
        })?;
        // Another concurrent probe may still use the shared parent.
        let _ = std::fs::remove_dir(&self.probes);
        Ok(())
    }
    fn environment(&self, provider: Provider) -> Vec<(OsString, OsString)> {
        let home = self.home();
        let mut env: Vec<(OsString, OsString)> = [
            ("HOME", home.clone()),
            ("USERPROFILE", home.clone()),
            ("XDG_CONFIG_HOME", home.join(".config")),
            ("XDG_DATA_HOME", home.join(".local/share")),
            ("XDG_STATE_HOME", home.join(".local/state")),
            ("XDG_CACHE_HOME", home.join(".cache")),
            ("APPDATA", home.join("AppData/Roaming")),
            ("LOCALAPPDATA", home.join("AppData/Local")),
            ("TMPDIR", self.tmp()),
            ("PATH", PathBuf::from("/usr/bin:/bin")),
            (home_env(provider), self.cli()),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into_os_string()))
        .collect();
        env.sort();
        env
    }
}

#[cfg(unix)]
fn probe(
    provider: Provider,
    d: &VersionDescriptor,
    executable: &Path,
    resolved: &Path,
    root: &Path,
    artifact: &admission::CheckedArtifact,
) -> Result<Detection, Error> {
    let workspace = Workspace::create(root, provider)?;
    // Same contract as stage 3 dispatch: the last step before process creation re-resolves
    // the stored spelling and re-hashes the current bytes against the retained verified
    // handle; any change refuses. Dispatch is then by exact pathname, so a same-user
    // path-reopen race after these checks remains an explicit, documented residual.
    let revalidate = || {
        let Ok((now, identity)) = super::validate(provider, executable, root) else {
            return false;
        };
        let Ok(fresh) = admission::checked_artifact(&now, root) else {
            return false;
        };
        now == resolved
            && identity == artifact.identity
            && fresh.identity == artifact.identity
            && fresh.size == artifact.size
            && fresh.sha256 == artifact.sha256
            && artifact
                .file
                .metadata()
                .is_ok_and(|m| m.len() == artifact.size)
    };
    let env = workspace.environment(provider);
    let result = spawn(resolved, d.argv, &env, &workspace.home(), revalidate)
        .map(|captured| captured.and_then(|c| parse(d, &c.stdout, &c.stderr, c.exit)));
    // Cleanup always runs; its failure is an operational failure, never a pass.
    let cleanup = workspace.remove();
    let version = result?;
    cleanup?;
    Ok(match version {
        Some(version) => Detection::Detected(version),
        None => Detection::Failed,
    })
}

#[cfg(unix)]
struct Captured {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit: Option<i32>,
    #[cfg(test)]
    status: std::process::ExitStatus,
}

// Exact-path execve storage; pointers refer to the owned CString heap buffers.
#[cfg(unix)]
struct Exec {
    path: std::ffi::CString,
    _args: Vec<std::ffi::CString>,
    _env: Vec<std::ffi::CString>,
    argv: Vec<*const libc::c_char>,
    envp: Vec<*const libc::c_char>,
}
// SAFETY: every pointer refers to an immutable CString buffer owned by this struct;
// moving it never moves those allocations. No mutation or pointer escape is exposed.
#[cfg(unix)]
unsafe impl Send for Exec {}
// SAFETY: all storage is immutable after construction; exec only reads the buffers.
#[cfg(unix)]
unsafe impl Sync for Exec {}
#[cfg(unix)]
impl Exec {
    fn prepare(path: &Path, argv: &[&str], env: &[(OsString, OsString)]) -> Option<Self> {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut args = vec![path.clone()];
        for arg in argv {
            args.push(CString::new(*arg).ok()?);
        }
        let mut environment = Vec::new();
        for (key, value) in env {
            let mut bytes = key.as_bytes().to_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(value.as_bytes());
            environment.push(CString::new(bytes).ok()?);
        }
        let mut argv: Vec<_> = args.iter().map(|s| s.as_ptr()).collect();
        argv.push(std::ptr::null());
        let mut envp: Vec<_> = environment.iter().map(|s| s.as_ptr()).collect();
        envp.push(std::ptr::null());
        Some(Self {
            path,
            _args: args,
            _env: environment,
            argv,
            envp,
        })
    }
}

/// Direct child only: stdin closed, explicit env, bounded capture, kill and reap.
/// `Ok(None)` is a rejected probe; `Err` means the child's reap could not be proved.
#[cfg(unix)]
fn spawn(
    path: &Path,
    argv: &[&str],
    env: &[(OsString, OsString)],
    cwd: &Path,
    revalidate: impl FnOnce() -> bool,
) -> Result<Option<Captured>, Error> {
    use std::{
        os::unix::{io::AsRawFd, process::CommandExt},
        process::{Command, Stdio},
    };
    let Some(exec) = Exec::prepare(path, argv, env) else {
        return Ok(None);
    };
    // Every descriptor the child needs exists before the final revalidation: the
    // checked home directory (entered with fchdir, never by pathname), /dev/null and
    // both pipes. Spawn then does no path lookup besides the documented exec itself.
    let Ok(Some(home)) = crate::platform::inspect(cwd, crate::platform::ObjectKind::Directory)
    else {
        return Ok(None);
    };
    let Ok(null) = std::fs::File::open("/dev/null") else {
        return Ok(None);
    };
    let (Ok((out_read, out_write)), Ok((err_read, err_write))) = (std::io::pipe(), std::io::pipe())
    else {
        return Ok(None);
    };
    let home_fd = home.file.as_raw_fd();
    // No env builder call: std would allocate an env array at spawn, after the final
    // check. The child's environment is only the prepared list execve receives below.
    let mut command = Command::new(path);
    command
        .stdin(Stdio::from(null))
        .stdout(Stdio::from(out_write))
        .stderr(Stdio::from(err_write));
    // SAFETY: the hook only calls async-signal-safe fchdir/execve on a descriptor and
    // storage prepared before fork. Exec failure returns Err, so std never falls back
    // to a shell or PATH search.
    unsafe {
        command.pre_exec(move || {
            // Bind the whole Send/Sync struct; disjoint field capture would move raw pointers.
            let exec = &exec;
            if libc::fchdir(home_fd) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::execve(exec.path.as_ptr(), exec.argv.as_ptr(), exec.envp.as_ptr());
            Err(std::io::Error::last_os_error())
        });
    }
    // The revalidation is the last step before fork/exec.
    if !revalidate() {
        return Ok(None);
    }
    let spawned = command.spawn();
    // Close the parent's copies of the child's write ends, or the pipes never reach EOF.
    drop(command);
    drop(home);
    let Ok(mut child) = spawned else {
        return Ok(None);
    };
    let deadline = Instant::now() + TIMEOUT;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut out_pipe = Some(out_read);
    let mut err_pipe = Some(err_read);
    let mut rejected = false;
    loop {
        let mut fds = Vec::new();
        if let Some(pipe) = &out_pipe {
            fds.push(libc::pollfd {
                fd: pipe.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        if let Some(pipe) = &err_pipe {
            fds.push(libc::pollfd {
                fd: pipe.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        if fds.is_empty() {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            rejected = true;
            break;
        }
        let timeout = remaining.as_millis().clamp(1, 1000) as libc::c_int;
        // SAFETY: fds is a live, correctly sized pollfd array for this call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            rejected = true;
            break;
        }
        for fd in fds.iter().filter(|fd| fd.revents != 0) {
            let out = out_pipe.as_ref().is_some_and(|p| p.as_raw_fd() == fd.fd);
            let used = stdout.len() + stderr.len();
            let step = if out {
                out_pipe
                    .as_mut()
                    .map(|p| read_bounded(p, &mut stdout, used))
            } else {
                err_pipe
                    .as_mut()
                    .map(|p| read_bounded(p, &mut stderr, used))
            };
            match step {
                Some(Step::Eof) | None => {
                    if out {
                        out_pipe = None;
                    } else {
                        err_pipe = None;
                    }
                }
                Some(Step::Data) => (),
                Some(Step::Reject) => {
                    rejected = true;
                    break;
                }
            }
        }
        if rejected {
            break;
        }
    }
    drop(out_pipe);
    drop(err_pipe);
    let mut status = None;
    if !rejected {
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
    }
    let Some(status) = status else {
        // Known unreaped direct child: SIGKILL, then prove the reap within the bound.
        let _ = child.kill();
        let end = Instant::now() + REAP;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return Ok(None),
                Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(10)),
                Ok(None) | Err(_) => {
                    return Err(Error::Wait {
                        source: std::io::Error::other("version probe child was not reaped"),
                        cleanup_failed: true,
                    });
                }
            }
        }
    };
    Ok(Some(Captured {
        stdout,
        stderr,
        exit: status.code(),
        #[cfg(test)]
        status,
    }))
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Data,
    Eof,
    Reject,
}
/// One read into `sink` without passing the combined cap: it requests at most the
/// remaining budget plus one overflow byte, and an overflowing read appends nothing.
#[cfg_attr(not(unix), allow(dead_code))]
fn read_bounded(reader: &mut impl std::io::Read, sink: &mut Vec<u8>, used: usize) -> Step {
    let mut buffer = [0u8; MAX_OUTPUT + 1];
    let budget = (MAX_OUTPUT + 1).saturating_sub(used).max(1);
    match reader.read(&mut buffer[..budget]) {
        Ok(0) => Step::Eof,
        Ok(n) if used + n > MAX_OUTPUT => Step::Reject,
        Ok(n) => {
            sink.extend_from_slice(&buffer[..n]);
            Step::Data
        }
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Step::Data,
        Err(_) => Step::Reject,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Always offers more bytes than asked and records the largest request.
    struct Flood(usize);
    impl std::io::Read for Flood {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.0 = self.0.max(buffer.len());
            buffer.fill(b'x');
            Ok(buffer.len())
        }
    }
    #[test]
    fn stage08_capture_never_exceeds_combined_cap() {
        // Both streams near the cap at once: each read is limited to the shared budget.
        let (mut out, mut err) = (vec![b'o'; 2048], vec![b'e'; 2047]);
        let (mut a, mut b) = (Flood(0), Flood(0));
        let used = out.len() + err.len();
        assert_eq!(read_bounded(&mut a, &mut out, used), Step::Reject);
        assert_eq!(a.0, MAX_OUTPUT + 1 - used);
        assert_eq!(read_bounded(&mut b, &mut err, used), Step::Reject);
        assert_eq!(out.len() + err.len(), used, "overflowing reads appended");
        // Exactly the cap is accepted; one byte more is refused before appending.
        let mut sink = vec![b'o'; MAX_OUTPUT - 1];
        let mut one = &b"z"[..];
        assert_eq!(
            read_bounded(&mut one, &mut sink, MAX_OUTPUT - 1),
            Step::Data
        );
        assert_eq!(sink.len(), MAX_OUTPUT);
        let mut flood = Flood(0);
        assert_eq!(
            read_bounded(&mut flood, &mut sink, MAX_OUTPUT),
            Step::Reject
        );
        assert_eq!((flood.0, sink.len()), (1, MAX_OUTPUT));
        let mut empty = &b""[..];
        assert_eq!(read_bounded(&mut empty, &mut sink, MAX_OUTPUT), Step::Eof);
    }
    #[cfg(unix)]
    #[test]
    fn stage08_spawn_enters_held_home_without_pathname_chdir() {
        use std::os::unix::{fs::PermissionsExt, process::ExitStatusExt};
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
        let temp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let (home, moved) = (temp.path().join("home"), temp.path().join("moved"));
        std::fs::create_dir(&home).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        // The final revalidation renames the home away: a pathname chdir would now fail.
        let mut checked = false;
        let captured = spawn(Path::new("/bin/pwd"), &[], &[], &home, || {
            std::fs::rename(&home, &moved).unwrap();
            checked = true;
            true
        })
        .unwrap()
        .expect("child started in the held directory");
        assert!(checked);
        assert_eq!(
            captured.exit,
            Some(0),
            "direct child status: {} (raw wait status: {})",
            captured.status,
            captured.status.into_raw()
        );
        assert!(captured.stderr.is_empty());
        let mut expected = std::fs::canonicalize(&moved)
            .unwrap()
            .into_os_string()
            .into_encoded_bytes();
        expected.push(b'\n');
        assert_eq!(captured.stdout, expected);
        // A refused revalidation starts nothing.
        assert!(
            spawn(Path::new("/bin/pwd"), &[], &[], &moved, || false)
                .unwrap()
                .is_none()
        );
    }
    fn descriptor() -> VersionDescriptor {
        VersionDescriptor {
            provider: Provider::Codex,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            sha256: [7; 32],
            argv: &["invented-version"],
            home_env: "CODEX_HOME",
            prefix: "invented-cli ",
            suffix: " (fixture)",
            evidence: "synthetic fixture only",
        }
    }
    #[test]
    fn stage08_shipped_descriptor_table_is_empty() {
        assert!(DESCRIPTORS.is_empty());
    }
    #[test]
    fn stage08_version_grammar_is_exact() {
        let d = descriptor();
        let ok = |out: &[u8]| parse(&d, out, b"", Some(0));
        assert_eq!(
            ok(b"invented-cli 1.2.3 (fixture)\n").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            ok(b"invented-cli 0.98.0-alpha.2 (fixture)\n").as_deref(),
            Some("0.98.0-alpha.2")
        );
        for bad in [
            &b"invented-cli 1.2.3 (fixture)"[..],
            b"invented-cli 1.2.3 (fixture)\r\n",
            b"invented-cli 1.2.3 (fixture)\n\n",
            b"invented-cli 1.2 (fixture)\n",
            b"invented-cli 01.2.3 (fixture)\n",
            b"invented-cli 1.2.3.4 (fixture)\n",
            b"invented-cli 1.2.3- (fixture)\n",
            b"invented-cli 1.2.3-a_b (fixture)\n",
            b"Invented-cli 1.2.3 (fixture)\n",
            b"invented-cli 1.2.3 (fixture) \n",
            b"invented-cli 1.2.3\x1b (fixture)\n",
            b"\xffinvented-cli 1.2.3 (fixture)\n",
            b"",
        ] {
            assert_eq!(ok(bad), None, "{bad:?}");
        }
        let line = b"invented-cli 1.2.3 (fixture)\n";
        assert_eq!(parse(&d, line, b"x", Some(0)), None);
        assert_eq!(parse(&d, line, b"", Some(1)), None);
        assert_eq!(parse(&d, line, b"", None), None);
        let mut long = b"invented-cli 1.2.3-".to_vec();
        long.extend(std::iter::repeat_n(b'a', 33));
        long.extend_from_slice(b" (fixture)\n");
        assert_eq!(ok(&long), None);
    }
    #[test]
    fn stage08_malformed_descriptors_are_refused() {
        let mut d = descriptor();
        assert!(well_formed(&d));
        d.home_env = "CLAUDE_CONFIG_DIR";
        assert!(!well_formed(&d));
        let mut d = descriptor();
        d.argv = &[""];
        assert!(!well_formed(&d));
        let mut d = descriptor();
        d.prefix = "tab\t";
        assert!(!well_formed(&d));
    }
}
