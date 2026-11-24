#[cfg(unix)]
#[path = "unix_resize.rs"]
mod resize;
#[cfg(windows)]
#[path = "windows_parent.rs"]
mod windows_parent;
use std::{
    io::{self, IsTerminal, Write},
    process::ExitCode,
};
#[cfg(unix)]
#[path = "unix_observe.rs"]
mod unix;
fn encoded(value: &std::ffi::OsStr) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().map(|u| format!("{u:04x}")).collect()
    }
}
fn run() -> io::Result<u32> {
    let mode = std::env::var("LARPCC_FAKE_MODE").unwrap_or_else(|_| "exit".into());
    #[cfg(unix)]
    if mode == "wait-ctrl" {
        resize::install_resize()?;
    }
    let mut record = serde_json::json!({
     "args":std::env::args_os().map(|a|encoded(&a)).collect::<Vec<_>>(),
     "pid":std::process::id(),
     "cwd":encoded(std::env::current_dir()?.as_os_str()),
     "stdin_terminal":io::stdin().is_terminal(),
     "stdout_terminal":io::stdout().is_terminal(),
     "stderr_terminal":io::stderr().is_terminal(),
    });
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::FILETIME,
            System::Threading::{GetCurrentProcess, GetProcessTimes},
        };
        let mut created = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut exited, mut kernel, mut user) = (created, created, created);
        // SAFETY: current-process pseudo-handle and valid FILETIME output buffers.
        if unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        record["windows_creation_time"] = serde_json::json!(
            (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
        );
    }
    for key in [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "HOME",
        "USERPROFILE",
        "XDG_DATA_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "PWD",
        "LARPCC_TEST_VALUE",
    ] {
        record[key] = std::env::var_os(key)
            .map(|v| serde_json::Value::String(encoded(&v)))
            .unwrap_or(serde_json::Value::Null);
    }
    #[cfg(unix)]
    {
        record["unix"] = unix::observe()?;
    }
    if let Some(path) = std::env::var_os("LARPCC_FAKE_RECORD") {
        let mut pending = path.clone();
        pending.push(".pending");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        if mode == "pause-record" {
            file.write_all(b"{")?;
            std::thread::sleep(std::time::Duration::from_secs(5));
            return Err(io::Error::other("synthetic record publication interrupted"));
        }
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
        drop(file);
        // Publish complete JSON atomically without replacing another child's marker.
        std::fs::hard_link(&pending, &path)?;
        std::fs::remove_file(pending)?;
    }
    match mode.as_str() {
        "exit" => (),
        "echo-io" => {
            io::copy(&mut io::stdin().lock(), &mut io::stdout().lock())?;
        }
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(5)),
        "bounded-grandchild" => std::thread::sleep(std::time::Duration::from_secs(3)),
        "grandchild" => {
            #[cfg(windows)]
            if let Some(release) = std::env::var_os("LARPCC_FAKE_EXIT_RELEASE") {
                // Let the test retain our identity-checked handle before we can exit.
                let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !std::path::Path::new(&release).exists() {
                    if std::time::Instant::now() >= end {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "synthetic exit observation handshake",
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            let mut command = std::process::Command::new(std::env::current_exe()?);
            command
                .env_clear()
                .env("LARPCC_FAKE_MODE", "bounded-grandchild");
            #[cfg(windows)]
            command.env("SystemRoot", r"C:\Windows");
            for key in [
                "CLAUDE_CONFIG_DIR",
                "CODEX_HOME",
                "HOME",
                "USERPROFILE",
                "XDG_DATA_HOME",
                "APPDATA",
                "LOCALAPPDATA",
                "PWD",
                "LARPCC_TEST_VALUE",
            ] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            if let Some(path) = std::env::var_os("LARPCC_FAKE_RECORD") {
                command.env(
                    "LARPCC_FAKE_RECORD",
                    std::path::PathBuf::from(path).with_extension("grandchild"),
                );
            }
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
        }
        "wait-ctrl" => {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < end {
                #[cfg(unix)]
                if let Some((rows, columns)) = resize::resized_size()?
                    && let Some(path) = std::env::var_os("LARPCC_FAKE_RECORD")
                {
                    let path = std::path::PathBuf::from(path).with_extension("resize");
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)?;
                    serde_json::to_writer(
                        file,
                        &serde_json::json!({"rows":rows,"columns":columns}),
                    )?;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        "binary-output" => {
            let buffer = [0, 255, 128, 27].repeat(32768);
            io::stdout().write_all(&buffer)?;
            io::stderr().write_all(&buffer)?;
        }
        "output" => {
            let buffer = vec![b'x'; 8192];
            let mut out = io::stdout().lock();
            for _ in 0..1024 {
                out.write_all(&buffer)?;
            }
        }
        _ => return Err(io::Error::other("unknown synthetic mode")),
    }
    if let Some(path) = std::env::var_os("LARPCC_FAKE_RECORD") {
        let mut done = path;
        done.push(".done");
        let done = std::path::PathBuf::from(done);
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(done)?;
        file.write_all(b"synthetic helper completed\n")?;
    }
    Ok(std::env::var("LARPCC_FAKE_EXIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0))
}
// Stage 08 version modes. The probe env is clean, so the mode comes from the invented
// descriptor's exact argv. The record is written next to this executable, outside the
// probe home, so tests can assert the child env and that cleanup removed the home.
fn version_mode(mode: &str) -> io::Result<u32> {
    use std::io::Read;
    let mut stdin = [0u8; 1];
    let stdin_eof = matches!(io::stdin().read(&mut stdin), Ok(0));
    let env: serde_json::Map<_, _> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                serde_json::Value::String(v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let record = serde_json::json!({
        "args": std::env::args().skip(1).collect::<Vec<_>>(),
        "pid": std::process::id(),
        "cwd": std::env::current_dir()?.to_string_lossy(),
        "stdin_eof": stdin_eof,
        "env": env,
    });
    let exe = std::env::current_exe()?;
    std::fs::write(
        exe.with_extension(format!("probe-{mode}")),
        serde_json::to_vec(&record)?,
    )?;
    // Leave state in both throwaway homes so cleanup has to remove real trees.
    for key in ["HOME", "CODEX_HOME", "CLAUDE_CONFIG_DIR"] {
        if let Some(home) = std::env::var_os(key) {
            let dir = std::path::Path::new(&home).join("written").join("nested");
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("state"), b"synthetic probe state\n")?;
        }
    }
    let line = b"invented-cli 1.2.3 (fixture)\n";
    let mut out = io::stdout().lock();
    match mode {
        "ok" => out.write_all(line)?,
        "stderr" => {
            out.write_all(line)?;
            io::stderr().write_all(b"noise\n")?;
        }
        "nonzero" => {
            out.write_all(line)?;
            return Ok(3);
        }
        "oversize" => out.write_all(&[b'x'; 8192])?,
        // Both streams near the combined cap at once (2 x 2100 > 4096).
        "split" => {
            out.write_all(&[b'x'; 2100])?;
            io::stderr().write_all(&[b'x'; 2100])?;
        }
        "grammar" => out.write_all(b"invented-cli 1.2 (fixture)\n")?,
        "lines" => {
            out.write_all(line)?;
            out.write_all(b"second\n")?;
        }
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(30)),
        #[cfg(unix)]
        "stuck" => {
            use std::os::unix::fs::PermissionsExt;
            let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
            let stuck = home.join("stuck");
            std::fs::create_dir(&stuck)?;
            std::fs::write(stuck.join("file"), b"x")?;
            std::fs::set_permissions(&stuck, std::fs::Permissions::from_mode(0o500))?;
            out.write_all(line)?;
        }
        _ => return Err(io::Error::other("unknown synthetic version mode")),
    }
    out.flush()?;
    Ok(0)
}
fn main() -> ExitCode {
    if let Some(mode) = std::env::args_os().nth(1).and_then(|a| {
        a.to_str()?
            .strip_prefix("invented-version-")
            .map(str::to_owned)
    }) {
        return match version_mode(&mode) {
            Ok(code) => std::process::exit(code as i32),
            Err(_) => ExitCode::FAILURE,
        };
    }
    #[cfg(windows)]
    if std::env::var_os("LARPCC_FAKE_MODE").as_deref()
        == Some(std::ffi::OsStr::new("prepare-parent"))
    {
        return match windows_parent::prepare() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("synthetic parent: {error}");
                ExitCode::FAILURE
            }
        };
    }
    match run() {
        Ok(code) => std::process::exit(code as i32),
        Err(error) => {
            eprintln!("synthetic helper: {error}");
            ExitCode::FAILURE
        }
    }
}
