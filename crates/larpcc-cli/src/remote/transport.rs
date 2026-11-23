use super::{
    DispatchError,
    address::{Address, Host},
    protocol,
};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(super) const OUTPUT_LIMIT: usize = 1024 * 1024;
const VERSION_LIMIT: usize = 4096;
pub(super) const READ: Duration = Duration::from_secs(11);
pub(super) const MUTATION: Duration = Duration::from_secs(60);
const VERSION: Duration = Duration::from_secs(2);
const READINESS: Duration = Duration::from_secs(5);
pub(super) const READY: &[u8] = b"LARPCC-IMPORT/1 READY\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Unavailable,
    Start,
    Timeout,
    Cancelled,
    TooLarge,
    Io,
}

pub(super) struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: Option<i32>,
    pub sent: bool,
    pub ready: bool,
}

pub(super) struct Client {
    path: PathBuf,
    identity: Identity,
}

#[derive(PartialEq, Eq)]
struct Identity {
    length: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    object: (u64, u64, i64, i64),
    #[cfg(windows)]
    object: (u64, [u8; 16]),
}

impl Client {
    pub fn discover(
        spawn: impl FnOnce(&mut Command) -> Result<Child, DispatchError>,
    ) -> Result<Self, DispatchError> {
        let path = system_ssh().ok_or_else(unavailable)?;
        Self::verify(path, spawn)
    }

    fn verify(
        path: PathBuf,
        spawn: impl FnOnce(&mut Command) -> Result<Child, DispatchError>,
    ) -> Result<Self, DispatchError> {
        let identity = identity(&path).ok_or_else(unavailable)?;
        let mut command = direct_command(&path);
        command.arg("-V").stdin(Stdio::null());
        let child = spawn(pipes(&mut command))?;
        let output = collect(child, Vec::new(), None, VERSION_LIMIT, VERSION).map_err(|error| {
            if error == Failure::Cancelled {
                protocol::Error::transport(error, false).into()
            } else {
                unavailable()
            }
        })?;
        let line = match (output.stdout.is_empty(), output.stderr.is_empty()) {
            (true, false) => &output.stderr,
            (false, true) => &output.stdout,
            _ => return Err(unavailable()),
        };
        if output.code != Some(0) || !supported_version(line, cfg!(windows)) {
            return Err(unavailable());
        }
        let client = Self { path, identity };
        client.check_identity().map_err(|_| unavailable())?;
        Ok(client)
    }

    fn check_identity(&self) -> Result<(), Failure> {
        if identity(&self.path).as_ref() == Some(&self.identity) {
            Ok(())
        } else {
            Err(Failure::Unavailable)
        }
    }

    pub fn refresh(
        &mut self,
        spawn: impl FnOnce(&mut Command) -> Result<Child, DispatchError>,
    ) -> Result<(), DispatchError> {
        if self.check_identity().is_err() {
            *self = Self::verify(self.path.clone(), spawn)?;
        }
        Ok(())
    }

    pub fn spawn(&self, host: &Host, args: &[String]) -> Result<Child, Failure> {
        self.check_identity()?;
        let mut command = match host.ssh {
            Address::Direct { .. } => direct_command(&self.path),
            Address::Config { .. } => Command::new(&self.path),
        };
        pipes(command.args(args).stdin(Stdio::piped()))
            .spawn()
            .map_err(|_| Failure::Start)
    }
}

fn unavailable() -> DispatchError {
    protocol::Error::transport(Failure::Unavailable, false).into()
}

#[cfg(unix)]
fn system_ssh() -> Option<PathBuf> {
    [Path::new("/usr/bin/ssh"), Path::new("/bin/ssh")]
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .find(|path| identity(path).is_some())
}

#[cfg(windows)]
fn system_ssh() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut path = [0; 260];
    // SAFETY: the buffer is writable for the complete length passed to the API.
    let len = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
    if len == 0 || len >= path.len() {
        return None;
    }
    Some(
        PathBuf::from(std::ffi::OsString::from_wide(&path[..len]))
            .join("OpenSSH")
            .join("ssh.exe"),
    )
}

#[cfg(windows)]
fn executable_pe(path: &Path) -> Option<(fs::Metadata, (u64, [u8; 16]))> {
    use std::{
        io::{Seek, SeekFrom},
        mem::size_of,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    if !path.is_absolute() || path.file_name() != Some(std::ffi::OsStr::new("ssh.exe")) {
        return None;
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.file_attributes() & 0x400 != 0 {
        return None;
    }
    let mut dos = [0; 64];
    file.read_exact(&mut dos).ok()?;
    if &dos[..2] != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().ok()?);
    if offset < 64 {
        return None;
    }
    file.seek(SeekFrom::Start(offset.into())).ok()?;
    let mut pe = [0; 26];
    file.read_exact(&mut pe).ok()?;
    let flags = u16::from_le_bytes([pe[22], pe[23]]);
    if &pe[..4] != b"PE\0\0"
        || flags & 0x2002 != 0x0002
        || !matches!(u16::from_le_bytes([pe[24], pe[25]]), 0x10b | 0x20b)
    {
        return None;
    }
    let mut id = FILE_ID_INFO::default();
    // SAFETY: the output buffer matches FileIdInfo and the file handle is live.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return None;
    }
    Some((meta, (id.VolumeSerialNumber, id.FileId.Identifier)))
}

fn identity(path: &Path) -> Option<Identity> {
    #[cfg(unix)]
    let meta = fs::symlink_metadata(path).ok()?;
    #[cfg(windows)]
    let (meta, object) = executable_pe(path)?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(unix)]
    if !path.is_absolute() || !meta.is_file() || meta.mode() & 0o111 == 0 {
        return None;
    }
    Some(Identity {
        length: meta.len(),
        modified: meta.modified().ok()?,
        #[cfg(unix)]
        object: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        #[cfg(windows)]
        object,
    })
}

fn direct_command(path: &Path) -> Command {
    let mut command = Command::new(path);
    command.env_clear();
    // OpenSSH resolves its own account files. Preserve OS settings and agent selection.
    for variable in [
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "SSH_AUTH_SOCK",
        "SystemRoot",
        "WINDIR",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "ProgramData",
        "TEMP",
        "TMP",
    ] {
        if let Some(value) = std::env::var_os(variable) {
            command.env(variable, value);
        }
    }
    command
}

fn pipes(command: &mut Command) -> &mut Command {
    command.stdout(Stdio::piped()).stderr(Stdio::piped())
}

fn supported_version(bytes: &[u8], windows: bool) -> bool {
    let Ok(line) = std::str::from_utf8(bytes) else {
        return false;
    };
    let line = line
        .strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(line);
    if line.is_empty() || line.chars().any(char::is_control) {
        return false;
    }
    let prefix = line.strip_prefix("OpenSSH_for_Windows_");
    let Some(version) =
        prefix.or_else(|| (!windows).then(|| line.strip_prefix("OpenSSH_")).flatten())
    else {
        return false;
    };
    let Some((major, remainder)) = version.split_once('.') else {
        return false;
    };
    let minor = remainder.trim_start_matches(|c: char| c.is_ascii_digit());
    let (minor, suffix) = remainder.split_at(remainder.len() - minor.len());
    if major.is_empty()
        || !major.bytes().all(|b| b.is_ascii_digit())
        || minor.is_empty()
        || (!suffix.is_empty() && !suffix.starts_with(['p', ' ', ',', '-', '_', '.']))
    {
        return false;
    }
    matches!((major.parse::<u32>(), minor.parse::<u32>()), (Ok(a), Ok(b)) if (a, b) >= (6, 8))
}

enum Event {
    Ready,
    Stdout(Result<Vec<u8>, Failure>),
    Stderr(Result<Vec<u8>, Failure>),
    Sent(bool),
}

fn read_tail(reader: &mut impl Read, mut bytes: Vec<u8>, limit: usize) -> Result<Vec<u8>, Failure> {
    let remaining = limit.checked_sub(bytes.len()).ok_or(Failure::TooLarge)?;
    reader
        .take(remaining as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Io)?;
    if bytes.len() > limit {
        return Err(Failure::TooLarge);
    }
    Ok(bytes)
}

pub(super) fn collect(
    mut child: Child,
    script: Vec<u8>,
    payload: Option<Vec<u8>>,
    stdout_limit: usize,
    timeout: Duration,
) -> Result<Output, Failure> {
    let started = Instant::now();
    let importing = payload.is_some();
    let (events, receive) = mpsc::channel();
    let (send_payload, ready) = mpsc::channel();
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let stdin = child.stdin.take();
    let stdout_events = events.clone();
    thread::spawn(move || {
        let mut prefix = Vec::new();
        let result = (|| {
            if importing {
                (&mut stdout)
                    .take(READY.len() as u64)
                    .read_to_end(&mut prefix)
                    .map_err(|_| Failure::Io)?;
                if prefix == READY {
                    prefix.clear();
                    let _ = stdout_events.send(Event::Ready);
                }
            }
            read_tail(&mut stdout, prefix, stdout_limit)
        })();
        let _ = stdout_events.send(Event::Stdout(result));
    });
    let stderr_events = events.clone();
    thread::spawn(move || {
        let limit = if stdout_limit == VERSION_LIMIT {
            VERSION_LIMIT
        } else {
            OUTPUT_LIMIT
        };
        let _ = stderr_events.send(Event::Stderr(read_tail(&mut stderr, Vec::new(), limit)));
    });
    thread::spawn(move || {
        let sent = if let Some(mut stdin) = stdin {
            stdin.write_all(&script).is_ok()
                && payload
                    .is_none_or(|bytes| ready.recv().is_ok() && stdin.write_all(&bytes).is_ok())
        } else {
            script.is_empty() && payload.is_none()
        };
        let _ = events.send(Event::Sent(sent));
    });
    let result = (|| {
        let mut stdout = None;
        let mut stderr = None;
        let mut sent = None;
        let mut ready = false;
        let mut payload_signal = Some(send_payload);
        let mut status = None;
        loop {
            if super::cancel::requested() {
                return Err(Failure::Cancelled);
            }
            if status.is_none() {
                status = child.try_wait().map_err(|_| Failure::Io)?;
            }
            if status.is_some() && stdout.is_some() && stderr.is_some() && sent.is_some() {
                break;
            }
            let budget = if importing && !ready {
                timeout.min(READINESS)
            } else {
                timeout
            };
            let remaining = budget
                .checked_sub(started.elapsed())
                .ok_or(Failure::Timeout)?;
            if status.is_some() && stdout.is_some() && !ready {
                payload_signal.take();
            }
            match receive.recv_timeout(remaining.min(Duration::from_millis(10))) {
                Ok(Event::Ready) => {
                    ready = true;
                    if let Some(signal) = payload_signal.take() {
                        let _ = signal.send(());
                    }
                }
                Ok(Event::Stdout(output)) => stdout = Some(output?),
                Ok(Event::Stderr(output)) => stderr = Some(output?),
                Ok(Event::Sent(complete)) => sent = Some(complete),
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) if status.is_some() => {
                    return Err(Failure::Io);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    thread::sleep(remaining.min(Duration::from_millis(10)));
                }
            }
        }
        Ok(Output {
            stdout: stdout.expect("stdout drained"),
            stderr: stderr.expect("stderr drained"),
            code: status.and_then(|status| status.code()),
            sent: sent.expect("writer finished"),
            ready,
        })
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "synthetic-launch")]
    use crate::remote::protocol;

    #[test]
    fn stage13_transport_version_floor() {
        assert!(supported_version(b"OpenSSH_6.8p1 Debian, OpenSSL\n", false));
        assert!(supported_version(b"OpenSSH_6.10\n", false));
        assert!(supported_version(
            b"OpenSSH_for_Windows_9.5p1, LibreSSL\r\n",
            true
        ));
        assert!(!supported_version(b"OpenSSH_9.8p1\n", true));
        for input in [
            "OpenSSH_6.7p9",
            "OpenSSH_6..8",
            "OpenSSH_6.8xyz",
            "OpenSSH_6.8\nwarning",
            "OpenSSH_6.8\u{1b}",
            "other OpenSSH_6.8",
            "",
        ] {
            assert!(!supported_version(input.as_bytes(), false), "{input:?}");
        }
    }

    #[cfg(feature = "synthetic-launch")]
    fn fake_fixture() -> (tempfile::TempDir, PathBuf) {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let temp = tempfile::tempdir_in(parent).unwrap();
        let own_binary = std::env::current_exe().unwrap();
        let build = own_binary.parent().unwrap().parent().unwrap();
        let suffix = std::env::consts::EXE_SUFFIX;
        let source = build.join(format!("larpcc-fake-ssh{suffix}"));
        let path = temp.path().join(format!("ssh{suffix}"));
        fs::copy(source, &path).unwrap();
        (temp, path.canonicalize().unwrap())
    }

    #[cfg(feature = "synthetic-launch")]
    fn run_fake(path: &Path) -> Child {
        pipes(
            Command::new(path)
                .env_clear()
                .args(["--", "fixture", "sh -s"])
                .stdin(Stdio::piped()),
        )
        .spawn()
        .unwrap()
    }

    #[test]
    #[cfg(feature = "synthetic-launch")]
    fn stage13_transport_fake_stream_limits_timeout_and_stderr() {
        let (temp, executable) = fake_fixture();
        let path = temp.path();
        let script = b"exec 'larp' </dev/null\n";
        let reply = b"{\"schema\":1,\"command\":\"profile.list\",\"ok\":true,\"revision\":\"0\",\"data\":{\"profiles\":[]}}\n";
        fs::write(path.join("reply"), reply).unwrap();
        let run = |scenario, timeout| {
            fs::write(path.join("scenario"), scenario).unwrap();
            collect(
                run_fake(&executable),
                script.to_vec(),
                None,
                OUTPUT_LIMIT,
                timeout,
            )
        };
        for scenario in ["large-out", "large-err"] {
            assert!(matches!(
                run(scenario, Duration::from_secs(5)),
                Err(Failure::TooLarge)
            ));
        }
        assert!(matches!(
            run("timeout", Duration::from_millis(250)),
            Err(Failure::Timeout)
        ));
        for scenario in ["banner", "warning"] {
            let output = run(scenario, Duration::from_secs(5)).unwrap();
            assert!(!output.stderr.is_empty());
            assert_eq!(
                protocol::reply(output, "profile.list", false),
                Err(protocol::OUTPUT)
            );
        }
        let output = run("refusal", Duration::from_secs(5)).unwrap();
        let read_error = protocol::reply(output, "profile.list", false).unwrap_err();
        assert_eq!(read_error.code, "ssh_failed");
        assert!(!read_error.message.contains("sentinel"));
        let output = run("refusal", Duration::from_secs(5)).unwrap();
        assert_eq!(
            protocol::reply(output, "profile.add", true),
            Err(protocol::UNKNOWN)
        );
    }

    #[test]
    #[cfg(feature = "synthetic-launch")]
    fn stage13_skill_transfer_ready_handshake_and_early_refusal() {
        let (temp, executable) = fake_fixture();
        let path = temp.path();
        let payload = b"synthetic framed bytes";
        let script = b"exec 'larp'\n";
        let run = |scenario, receipt: &[u8], code: &[u8]| {
            fs::write(path.join("scenario"), scenario).unwrap();
            fs::write(path.join("reply"), receipt).unwrap();
            fs::write(path.join("exit"), code).unwrap();
            collect(
                run_fake(&executable),
                script.to_vec(),
                Some(payload.to_vec()),
                OUTPUT_LIMIT,
                Duration::from_secs(5),
            )
            .unwrap()
        };
        let success = b"{\"schema\":1,\"command\":\"host.catalog.import\",\"ok\":true,\"revision\":\"1\",\"data\":{\"changed\":true,\"profile_id\":null,\"name\":\"codex-45\",\"warnings\":[]}}\n";
        let output = run("import", success, b"0");
        assert!(output.ready);
        assert_eq!(output.stdout, success);
        assert_eq!(fs::read(path.join("payload")).unwrap(), payload);
        assert!(protocol::reply(output, "host.catalog.import", true).is_ok());
        fs::remove_file(path.join("payload")).unwrap();
        let refusal = b"{\"schema\":1,\"command\":\"host.catalog.import\",\"ok\":false,\"error\":{\"code\":\"conflict\",\"message\":\"untrusted\"}}\n";
        let output = run("early", refusal, b"1");
        assert!(!output.ready);
        assert!(!path.join("payload").exists());
        assert_eq!(
            protocol::reply(output, "host.catalog.import", true)
                .unwrap_err()
                .code,
            "conflict"
        );
    }

    #[test]
    #[cfg(feature = "synthetic-launch")]
    fn stage13_transport_verified_fake_and_no_executable_fallback() {
        let (temp, executable) = fake_fixture();
        let spawn = |command: &mut Command| command.spawn().map_err(|_| unavailable());
        assert!(Client::verify(executable.clone(), spawn).is_ok());
        assert!(matches!(
            Client::verify(temp.path().join("missing"), |_| panic!("missing executable spawned")),
            Err(DispatchError::Remote(error)) if error.code == "ssh_unavailable"
        ));
        #[cfg(windows)]
        {
            fs::write(&executable, b"@echo OpenSSH_for_Windows_9.5").unwrap();
            assert!(matches!(
                Client::verify(executable, |_| panic!("script masquerading as executable spawned")),
                Err(DispatchError::Remote(error)) if error.code == "ssh_unavailable"
            ));
        }
    }
}
