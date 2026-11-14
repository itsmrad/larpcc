#![allow(dead_code)]
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[cfg(windows)]
#[path = "../fixtures/windows/private.rs"]
mod windows_private;
#[cfg(windows)]
type FixtureTemp = windows_private::PrivateTempDir;
#[cfg(not(windows))]
type FixtureTemp = tempfile::TempDir;
pub struct Fixture {
    pub temp: FixtureTemp,
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        #[cfg(windows)]
        let temp = windows_private::tempdir(&parent).unwrap();
        #[cfg(not(windows))]
        let temp = {
            use std::os::unix::fs::PermissionsExt;
            tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(parent)
                .unwrap()
        };
        let root = temp.path().join("managed");
        for dir in ["home", "xdg", "local", "app", "claude", "codex"] {
            let path = temp.path().join(dir);
            create_directory(&path);
            for file in [
                ".credentials.json",
                "auth.json",
                ".claude.json",
                "config.toml",
            ] {
                create_file(
                    &path.join(file),
                    b"synthetic sentinel; never real account data\n",
                );
            }
        }
        Self { temp, root }
    }
    pub fn command(&self, args: &[&str]) -> Command {
        self.command_for(Path::new(env!("CARGO_BIN_EXE_larp")), args)
    }
    pub fn command_for(&self, binary: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(binary);
        cmd.env_clear().args(args).env("LARPCC_HOME", &self.root);
        #[cfg(windows)]
        cmd.env("SystemRoot", r"C:\Windows");
        for (var, dir) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_DATA_HOME", "xdg"),
            ("LOCALAPPDATA", "local"),
            ("APPDATA", "app"),
            ("CLAUDE_CONFIG_DIR", "claude"),
            ("CODEX_HOME", "codex"),
        ] {
            cmd.env(var, self.temp.path().join(dir));
        }
        cmd
    }
    pub fn run(&self, args: &[&str], code: i32) -> Output {
        let output = capture(self.command(args)).unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "args={args:?}\nstdout={}\nstderr={}",
            stdout(&output),
            stderr(&output)
        );
        if code == 0 {
            assert!(output.stderr.is_empty(), "{}", stderr(&output));
        } else {
            assert!(output.stdout.is_empty(), "{}", stdout(&output));
        }
        output
    }
    pub fn profile(&self, args: &[&str], code: i32) -> Output {
        self.run(&[&["cc", "profile"][..], args].concat(), code)
    }
    pub fn install_registry(&self, bytes: &[u8]) {
        create_directory(&self.root);
        create_file(&self.root.join("registry.json"), bytes);
    }
    // Exact bytes are stronger than a digest; record sizes and Unix modes as well.
    pub fn outside_snapshot(&self) -> BTreeMap<PathBuf, (u64, Vec<u8>, u32)> {
        fn walk(path: &Path, root: &Path, out: &mut BTreeMap<PathBuf, (u64, Vec<u8>, u32)>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path == root {
                    continue;
                }
                let meta = fs::symlink_metadata(&path).unwrap();
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    meta.permissions().mode()
                };
                #[cfg(not(unix))]
                let mode = u32::from(meta.permissions().readonly());
                let bytes = if meta.is_file() {
                    fs::read(&path).unwrap()
                } else {
                    vec![]
                };
                out.insert(path.clone(), (meta.len(), bytes, mode));
                if meta.is_dir() {
                    walk(&path, root, out);
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.temp.path(), &self.root, &mut out);
        out
    }
}
pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}
pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}
pub fn private(path: &Path, dir: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if dir { 0o700 } else { 0o600 }),
        )
        .unwrap();
    }
    #[cfg(windows)]
    windows_private::verify_private(path, dir).unwrap();
}

// Drain output concurrently so the timeout also bounds tests producing more than a pipe buffer.
pub fn capture(mut command: Command) -> std::io::Result<Output> {
    use std::process::Stdio;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    wait_output(command.spawn()?)
}
pub fn wait_output(mut child: std::process::Child) -> std::io::Result<Output> {
    use std::{
        io::{self, Read},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    fn reader(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<io::Result<Vec<u8>>> {
        let (send, recv) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = match pipe {
                Some(mut p) => p.read_to_end(&mut bytes).map(|_| bytes),
                None => Ok(bytes),
            };
            let _ = send.send(result);
        });
        recv
    }
    let stdout = reader(child.stdout.take());
    let stderr = reader(child.stderr.take());
    let end = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let cleanup = Instant::now() + Duration::from_secs(2);
            while Instant::now() < cleanup {
                if child.try_wait()?.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "synthetic child exceeded ten seconds",
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    let read = |r: mpsc::Receiver<io::Result<Vec<u8>>>| {
        r.recv_timeout(end.saturating_duration_since(Instant::now()))
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "synthetic pipe did not close"))?
    };
    Ok(Output {
        status,
        stdout: read(stdout)?,
        stderr: read(stderr)?,
    })
}

pub fn create_directory(path: &Path) {
    #[cfg(windows)]
    windows_private::create_directory(path).unwrap();
    #[cfg(unix)]
    {
        fs::create_dir(path).unwrap();
        private(path, true);
    }
}
pub fn create_file(path: &Path, bytes: &[u8]) {
    #[cfg(windows)]
    windows_private::create_file(path, bytes).unwrap();
    #[cfg(unix)]
    {
        fs::write(path, bytes).unwrap();
        private(path, false);
    }
}
