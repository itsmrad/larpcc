//! Compiled test-only processes. No production dispatch or environment unlock.
use super::*;
use std::os::windows::process::CommandExt;
use std::{
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::{
    Console::*,
    Threading::{CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW},
};
const ENTRY: &str = "platform::windows::lifecycle::entry";
static EVENTS: AtomicUsize = AtomicUsize::new(0);
unsafe extern "system" fn count(event: u32) -> i32 {
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        EVENTS.fetch_add(1, Ordering::SeqCst);
        1
    } else {
        0
    }
}
fn command(mode: &str, root: &Path) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", ENTRY, "--nocapture"])
        .env_clear()
        .env("SystemRoot", r"C:\Windows")
        .env("LARPCC_WINDOWS_CASE", mode)
        .env("LARPCC_WINDOWS_ROOT", root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}
fn bounded(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("native test child exceeded 10s; bounded cleanup attempted");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn ready(root: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("ready").exists() {
        assert!(Instant::now() < deadline, "fake readiness deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn plan(mode: &str, root: &Path) -> crate::launch::LaunchPlan {
    crate::launch::LaunchPlan {
        executable: std::env::current_exe().unwrap(),
        argv0: OsString::new(),
        argv: ["--exact", ENTRY, "--nocapture"].map(OsString::from).into(),
        cwd: None,
        env_delta: vec![
            ("LARPCC_WINDOWS_CASE".into(), mode.into()),
            ("LARPCC_WINDOWS_ROOT".into(), root.as_os_str().into()),
        ],
        identity: FileIdentity::Windows {
            volume: 0,
            id: [0; 16],
        },
    }
}
#[test]
fn entry() {
    let Some(mode) = std::env::var_os("LARPCC_WINDOWS_CASE") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("LARPCC_WINDOWS_ROOT").unwrap());
    match mode.to_str().unwrap() {
        "fake" => {
            // SAFETY: static atomic-only callback remains live through process exit.
            assert_ne!(unsafe { SetConsoleCtrlHandler(Some(count), 1) }, 0);
            std::fs::write(root.join("ready"), b"ready").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while EVENTS.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            // Deliberately keep child alive after event: wrapper must continue waiting.
            std::thread::sleep(Duration::from_millis(300));
            std::fs::write(
                root.join("events"),
                EVENTS.load(Ordering::SeqCst).to_string(),
            )
            .unwrap();
        }
        "driver" => {
            std::process::exit(spawn_wait(&plan("fake", &root)).unwrap() as i32);
        }
        "ctrl" | "break" => {
            let is_break = mode == "break";
            // SAFETY: harness absorbs broadcast while driver/fake retain independent handlers.
            assert_ne!(
                unsafe { SetConsoleCtrlHandler(Some(console_handler), 1) },
                0
            );
            let mut cmd = command("driver", &root);
            if is_break {
                cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
            }
            let mut child = cmd.spawn().unwrap();
            ready(&root);
            // SAFETY: own isolated console; group ID is our known direct child, never an ambient process.
            assert_ne!(
                unsafe {
                    GenerateConsoleCtrlEvent(
                        if is_break {
                            CTRL_BREAK_EVENT
                        } else {
                            CTRL_C_EVENT
                        },
                        if is_break { child.id() } else { 0 },
                    )
                },
                0
            );
            std::thread::sleep(Duration::from_millis(100));
            assert!(
                child.try_wait().unwrap().is_none(),
                "wrapper exited before fake"
            );
            let out = bounded(child);
            assert!(out.status.success(), "{out:?}");
            assert_eq!(std::fs::read_to_string(root.join("events")).unwrap(), "1");
        }
        "exit" => {
            std::process::exit(
                std::fs::read_to_string(root.join("code"))
                    .unwrap()
                    .parse::<u32>()
                    .unwrap() as i32,
            );
        }
        "sleep" => {
            std::fs::write(root.join("pid"), std::process::id().to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(5));
        }
        "faults" => {
            FAULT.with(|f| f.set(1));
            assert!(matches!(
                spawn_wait(&plan("exit", &root)),
                Err(Error::Console {
                    action: "register console handler",
                    ..
                })
            ));
            FAULT.with(|f| f.set(0));
            let mut missing = plan("exit", &root);
            missing.executable = root.join("missing.exe");
            assert!(matches!(spawn_wait(&missing), Err(Error::Launch { .. })));
            FAULT.with(|f| f.set(2));
            assert!(matches!(
                spawn_wait(&plan("exit", &root)),
                Err(Error::Console {
                    action: "unregister console handler",
                    ..
                })
            ));
            FAULT.with(|f| f.set(3));
            assert!(matches!(
                spawn_wait(&plan("sleep", &root)),
                Err(Error::Wait { .. })
            ));
            let pid = FAILED_CHILD.with(|p| p.get());
            assert_ne!(pid, 0);
            // SAFETY: query only the direct child ID recorded by the failed wait seam; never terminate by PID.
            let h = unsafe {
                windows_sys::Win32::System::Threading::OpenProcess(
                    windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            if !h.is_null() {
                // SAFETY: successful OpenProcess transfers ownership.
                let h = unsafe { OwnedHandle::from_raw_handle(h) };
                let mut code = 0;
                // SAFETY: live process handle and valid status output.
                assert_ne!(
                    unsafe {
                        windows_sys::Win32::System::Threading::GetExitCodeProcess(
                            h.as_raw_handle(),
                            &mut code,
                        )
                    },
                    0
                );
                assert_ne!(code, 259, "failed-wait cleanup left direct child running");
            }
            FAULT.with(|f| f.set(0));
            // Remove handler left deliberately registered by failed unregistration seam.
            assert!(set_handler(false));
        }
        "status" => {
            for code in [0, 1, 2, 42, 255, 256, 0xc000013a, u32::MAX] {
                std::fs::write(root.join("code"), code.to_string()).unwrap();
                assert_eq!(spawn_wait(&plan("exit", &root)).unwrap(), code);
            }
        }
        other => panic!("unknown synthetic mode {other}"),
    }
}
fn fixture() -> tempfile::TempDir {
    let parent = std::env::var_os("LARPCC_TEST_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    tempfile::tempdir_in(parent).unwrap()
}
#[test]
fn w04_ctrl_c_and_break_deliver_exactly_once_without_forwarding() {
    for mode in ["ctrl", "break"] {
        let root = fixture();
        let output = bounded(
            command(mode, root.path())
                .creation_flags(CREATE_NEW_CONSOLE)
                .spawn()
                .unwrap(),
        );
        assert!(output.status.success(), "{mode}: {output:?}");
    }
}
#[test]
fn w04_failure_paths_never_report_success() {
    let root = fixture();
    std::fs::write(root.path().join("code"), "0").unwrap();
    let output = bounded(
        command("faults", root.path())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    );
    assert!(output.status.success(), "{output:?}");
}
#[test]
fn w05_no_window_preserves_full_u32_exit_status() {
    let root = fixture();
    let output = bounded(
        command("status", root.path())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    );
    assert!(output.status.success(), "{output:?}");
}
