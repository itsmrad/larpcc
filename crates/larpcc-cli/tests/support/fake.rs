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
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
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
fn main() -> ExitCode {
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
