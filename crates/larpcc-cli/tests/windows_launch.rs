#![cfg(all(windows, feature = "synthetic-launch"))]
mod common;
use common::*;
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::FILETIME,
    System::Threading::{
        CREATE_NO_WINDOW, GetExitCodeProcess, GetProcessTimes, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
    },
};
fn hex(value: &OsStr) -> String {
    value.encode_wide().map(|u| format!("{u:04x}")).collect()
}
fn setup() -> Fixture {
    let f = Fixture::new();
    let fake = f.temp.path().join("synthetic-native.exe");
    fs::copy(env!("CARGO_BIN_EXE_larpcc-fake-provider"), &fake).unwrap();
    for provider in ["claude", "codex"] {
        f.run(
            &["cc", "provider", "bind", provider, fake.to_str().unwrap()],
            0,
        );
        for suffix in ["a", "b"] {
            f.profile(
                &[
                    "add",
                    &format!("{provider}{suffix}"),
                    "--provider",
                    provider,
                ],
                0,
            );
        }
    }
    f
}
fn driver(f: &Fixture, name: &str) -> Command {
    let mut command = f.command_for(
        Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
        &["cc", "run", name, "--"],
    );
    command
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .current_dir(f.temp.path())
        .env("PWD", f.temp.path())
        .creation_flags(CREATE_NO_WINDOW);
    command
}
#[test]
fn p01_l01_four_homes_exact_wtf16_argv_cwd_and_unchanged_registry() {
    let f = setup();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    let args: Vec<OsString> = [
        "--label",
        "login",
        "prompt",
        "",
        "a b",
        "a\"b",
        r"trailing\",
        ";$(x)&|<>^%",
        "é",
        "--",
        "--config",
    ]
    .map(OsString::from)
    .into_iter()
    .chain([OsString::from_wide(&[0xd800, 0x61, 0xdcff])])
    .collect();
    let mut homes = std::collections::HashSet::new();
    for (index, name) in ["claudea", "claudeb", "codexa", "codexb"]
        .iter()
        .enumerate()
    {
        let record = f.temp.path().join(format!("record-{name}.json"));
        let mut command = driver(&f, name);
        command
            .args(&args)
            .env("LARPCC_FAKE_RECORD", &record)
            .env("LARPCC_TEST_VALUE", "invented-sentinel");
        let output = capture(command).unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
        let actual: Vec<_> = value["args"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(actual, args.iter().map(|a| hex(a)).collect::<Vec<_>>());
        assert_eq!(value["cwd"], hex(f.temp.path().as_os_str()));
        assert_eq!(value["PWD"], hex(f.temp.path().as_os_str()));
        let selected = if name.starts_with("claude") {
            "CLAUDE_CONFIG_DIR"
        } else {
            "CODEX_HOME"
        };
        assert_eq!(
            value[selected],
            hex(f
                .root
                .join("profiles")
                .join((index + 1).to_string())
                .as_os_str())
        );
        homes.insert(value[selected].as_str().unwrap().to_owned());
        assert_eq!(value["stdin_terminal"], false);
        assert_eq!(value["stdout_terminal"], false);
    }
    assert_eq!(homes.len(), 4);
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}
#[test]
fn l02_w05_direct_child_full_u32_status_and_no_launcher_output() {
    let f = setup();
    for code in [0u32, 1, 2, 42, 255, 256, 0xc000013a, u32::MAX] {
        let mut command = driver(&f, "claudea");
        command
            .arg("prompt")
            .env("LARPCC_FAKE_EXIT", code.to_string());
        let out = capture(command).unwrap();
        assert_eq!(out.status.code().map(|n| n as u32), Some(code));
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
    }
}
#[test]
fn l04_w05_piped_input_and_eight_megabyte_output_are_exact() {
    let f = setup();
    let mut command = driver(&f, "claudea");
    command
        .arg("prompt")
        .env("LARPCC_FAKE_MODE", "echo-io")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let input = b"synthetic\0binary\xff\r\n";
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = wait_output(child).unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(out.stdout, input);
    assert!(out.stderr.is_empty());
    let mut command = driver(&f, "claudea");
    command.arg("prompt").env("LARPCC_FAKE_MODE", "output");
    let out = capture(command).unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(out.stdout, vec![b'x'; 8 * 1024 * 1024]);
    assert!(out.stderr.is_empty());
}
#[test]
fn p02_p04_provisional_conflicts_refuse_without_child_or_registry_changes() {
    let f = setup();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    for (index, args) in [
        vec!["login"],
        vec!["--config=synthetic", "prompt"],
        vec!["--label", "x", "--label", "y", "prompt"],
    ]
    .into_iter()
    .enumerate()
    {
        let record = f.temp.path().join(format!("refused-{index}"));
        let mut command = driver(&f, "claudea");
        command.args(args).env("LARPCC_FAKE_RECORD", &record);
        let out = capture(command).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(!record.exists());
    }
    for key in ["claude_config_dir", "Larpcc_Synthetic_Deny_"] {
        let record = f.temp.path().join(format!("refused-{key}"));
        let mut command = driver(&f, "claudea");
        command
            .arg("prompt")
            .env(key, "")
            .env("LARPCC_FAKE_RECORD", &record);
        let out = capture(command).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(!record.exists());
    }
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}

// Retain one identity-checked handle: never terminate a possibly reused recorded PID.
struct Grandchild(OwnedHandle);
impl Grandchild {
    fn open(record: &serde_json::Value) -> Self {
        let pid = u32::try_from(record["pid"].as_u64().unwrap()).unwrap();
        assert_ne!(pid, 0);
        // SAFETY: open only the synthetic record's PID, no inheritance or privilege changes.
        let raw = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        assert!(
            !raw.is_null(),
            "grandchild cleanup handle unavailable: {}",
            io::Error::last_os_error()
        );
        // SAFETY: successful open transfers ownership; all later operations reuse this handle.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let (created, _) =
            Self::times(&handle).expect("child identity query failed; refusing termination");
        assert_eq!(
            record["windows_creation_time"].as_u64(),
            Some(created),
            "child creation identity mismatch; refusing termination"
        );
        Self(handle)
    }
    fn times(handle: &OwnedHandle) -> io::Result<(u64, u64)> {
        let mut created = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut exited, mut kernel, mut user) = (created, created, created);
        // SAFETY: retained process handle and valid FILETIME output buffers.
        if unsafe {
            GetProcessTimes(
                handle.as_raw_handle(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ticks =
            |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        Ok((ticks(created), ticks(exited)))
    }
    fn status(&self) -> io::Result<u32> {
        let mut code = 0;
        // SAFETY: retained identity-checked process handle and valid output pointer.
        if unsafe { GetExitCodeProcess(self.0.as_raw_handle(), &mut code) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(code)
        }
    }
}
impl Drop for Grandchild {
    fn drop(&mut self) {
        let ok = match self.status() {
            Ok(259) => {
                // SAFETY: terminate only our retained, creation-time-verified synthetic process.
                let killed = unsafe { TerminateProcess(self.0.as_raw_handle(), 1) } != 0;
                let end = Instant::now() + Duration::from_secs(2);
                while matches!(self.status(), Ok(259)) && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(10));
                }
                killed && matches!(self.status(), Ok(code) if code != 259)
            }
            Ok(_) => true,
            Err(_) => false, // Query failure never authorizes termination.
        };
        if !ok {
            eprintln!("grandchild cleanup gate failed; no unverified termination");
            // Preserve the original failed assertion while reporting cleanup failure.
            assert!(std::thread::panicking(), "grandchild cleanup gate failed");
        }
    }
}

fn process_record(path: &Path, end: Instant) -> serde_json::Value {
    loop {
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(record) => return record,
                Err(error) if error.is_eof() => (),
                Err(error) => panic!("invalid synthetic process record: {error}"),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => panic!("cannot read synthetic process record: {error}"),
        }
        assert!(
            Instant::now() < end,
            "process record deadline; cleanup identity unavailable"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn l07_bounded_grandchild_inherits_exact_selected_home() {
    let f = setup();
    let before = fs::read(f.root.join("registry.json")).unwrap();
    for (name, selected, id) in [
        ("claudea", "CLAUDE_CONFIG_DIR", "1"),
        ("codexb", "CODEX_HOME", "4"),
    ] {
        let record = f.temp.path().join(format!("descendant-{name}"));
        let child_record = record.with_extension("grandchild");
        let done = child_record.with_extension("grandchild.done");
        let release = record.with_extension("release");
        let mut command = driver(&f, name);
        command
            .arg("prompt")
            .env("LARPCC_FAKE_MODE", "grandchild")
            .env("LARPCC_FAKE_RECORD", &record)
            .env("LARPCC_FAKE_EXIT_RELEASE", &release)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let end = started + Duration::from_secs(10);
        let launcher = command.spawn().unwrap();
        // A clone of our direct Child handle already has a known identity; no PID lookup.
        let launcher_handle = Grandchild(launcher.as_handle().try_clone_to_owned().unwrap());
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = wait_output(launcher);
            let _ = send.send((Instant::now(), output));
        });
        let parent = process_record(&record, end);
        let direct = Grandchild::open(&parent);
        create_file(&release, b"observe direct exit");
        let grandchild = process_record(&child_record, end);
        let child = Grandchild::open(&grandchild);
        while Instant::now() < end {
            if direct.status().unwrap() != 259
                && launcher_handle.status().unwrap() != 259
                && child.status().unwrap() != 259
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let observed = started.elapsed();
        let (direct_created, direct_exited) = Grandchild::times(&direct.0).unwrap();
        let (launcher_created, launcher_exited) = Grandchild::times(&launcher_handle.0).unwrap();
        let (child_created, child_exited) = Grandchild::times(&child.0).unwrap();
        eprintln!(
            "L07 {name} process timing: direct_status={} launcher_status={} grandchild_status={} startup_to_direct_ms={} direct_lifetime_ms={} launcher_after_direct_ms={:?} grandchild_lifetime_ms={} all_exits_observed_ms={}",
            direct.status().unwrap(),
            launcher_handle.status().unwrap(),
            child.status().unwrap(),
            direct_created.saturating_sub(launcher_created) / 10_000,
            direct_exited.saturating_sub(direct_created) / 10_000,
            launcher_exited
                .checked_sub(direct_exited)
                .map(|ticks| ticks / 10_000),
            child_exited.saturating_sub(child_created) / 10_000,
            observed.as_millis()
        );
        assert_eq!(
            direct.status().unwrap(),
            0,
            "direct fake must exit successfully"
        );
        assert_eq!(
            launcher_handle.status().unwrap(),
            0,
            "launcher must exit successfully"
        );
        assert_eq!(
            child.status().unwrap(),
            0,
            "grandchild must exit itself successfully"
        );
        let (collected, out) = receive
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .expect("output collection exceeded ten seconds after independent process observation");
        let out = out.unwrap();
        eprintln!(
            "L07 {name} output collected_ms={} collection_after_all_exits_observed_ms={}",
            collected.duration_since(started).as_millis(),
            collected
                .saturating_duration_since(started + observed)
                .as_millis()
        );
        // FILETIME is in 100ns units; compare actual exits, excluding startup and pipe draining.
        assert!(
            launcher_exited
                .checked_sub(direct_exited)
                .is_some_and(|ticks| ticks < 20_000_000),
            "launcher lingered at least two seconds after actual direct child exit"
        );
        assert!(
            child_exited
                .checked_sub(child_created)
                .is_some_and(|ticks| ticks < 50_000_000),
            "bounded grandchild did not exit itself within five seconds"
        );
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
        assert_eq!(fs::read(&done).unwrap(), b"synthetic helper completed\n");
        assert_ne!(parent["pid"], grandchild["pid"]);
        assert_eq!(
            parent[selected],
            hex(f.root.join("profiles").join(id).as_os_str())
        );
        assert_eq!(grandchild[selected], parent[selected]);
        for key in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "HOME",
            "USERPROFILE",
            "XDG_DATA_HOME",
            "APPDATA",
            "LOCALAPPDATA",
            "PWD",
        ] {
            assert_eq!(
                grandchild[key], parent[key],
                "grandchild environment key {key}"
            );
        }
    }
    assert_eq!(fs::read(f.root.join("registry.json")).unwrap(), before);
}
