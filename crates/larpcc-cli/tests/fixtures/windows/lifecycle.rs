//! Compiled test-only processes. No production dispatch or environment unlock.
use super::*;
use crate::platform::{FileIdentity, fixture_private};
use std::os::windows::process::CommandExt;
use std::{
    ffi::OsString,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
};
use std::{
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::{
    Console::*,
    Threading::{CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW},
};
const ENTRY: &str = "launch::windows::lifecycle::entry";
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
            let output = child.wait_with_output().unwrap();
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(
                    stdout.contains(ENTRY) && stdout.contains("test result: ok. 1 passed"),
                    "synthetic entry must actually execute: {output:?}"
                );
            }
            return output;
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
        "auth-live" | "auth-unknown" => wait_fault(&root, mode == "auth-unknown", true),
        "run-live" | "run-unknown" => wait_fault(&root, mode == "run-unknown", false),
        "auth-busy" | "auth-free" => {
            use crate::launch::admission::acquire_auth_guard;
            let registry = crate::registry::Registry::at(root.join("managed")).unwrap();
            let id = registry
                .admission_snapshot(&"work".parse().unwrap(), |_| true)
                .unwrap()
                .target
                .profile
                .id;
            let result = acquire_auth_guard(registry.root(), id);
            if mode == "auth-busy" {
                assert!(matches!(result, Err(Error::AuthBusy)));
            } else {
                assert!(result.is_ok());
            }
        }
        "auth-fake" => {
            std::fs::write(root.join("ready"), b"ready").unwrap();
            // Self-bounded even if the wrapper fails or is killed by the test harness.
            std::thread::sleep(Duration::from_secs(5));
        }
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
            let removals = REMOVALS.with(|n| n.get());
            assert!(matches!(
                spawn_wait(&plan("sleep", &root)),
                Err(Error::Console {
                    action: "register console handler",
                    ..
                })
            ));
            assert_eq!(REMOVALS.with(|n| n.get()), removals);
            assert!(!HANDLER_INSTALLED.with(|h| h.get()));
            assert!(!root.join("pid").exists());
            for fault in [0, 6] {
                FAULT.with(|f| f.set(fault));
                let before = REMOVALS.with(|n| n.get());
                let mut missing = plan("exit", &root);
                missing.executable = root.join("missing.exe");
                match spawn_wait(&missing) {
                    Err(Error::Launch { source, .. }) => assert_eq!(source.raw_os_error(), Some(2)),
                    other => panic!("spawn error must remain primary: {other:?}"),
                }
                assert_eq!(REMOVALS.with(|n| n.get()), before + 1);
                assert!(!HANDLER_INSTALLED.with(|h| h.get()));
            }
            FAULT.with(|f| f.set(0));
            let mut nul = plan("sleep", &root);
            nul.argv.push(OsString::from_wide(&[0x61, 0, 0x62]));
            let before = REMOVALS.with(|n| n.get());
            match spawn_wait(&nul) {
                Err(Error::Launch { source, .. }) => {
                    assert_eq!(source.kind(), io::ErrorKind::InvalidInput)
                }
                other => panic!("NUL must fail before child creation: {other:?}"),
            }
            assert_eq!(REMOVALS.with(|n| n.get()), before + 1);
            assert!(!HANDLER_INSTALLED.with(|h| h.get()));
            assert!(!root.join("pid").exists());
            FAULT.with(|f| f.set(2));
            let before = REMOVALS.with(|n| n.get());
            assert!(matches!(
                spawn_wait(&plan("exit", &root)),
                Err(Error::Console {
                    action: "unregister console handler",
                    ..
                })
            ));
            assert_eq!(REMOVALS.with(|n| n.get()), before + 1);
            assert!(!HANDLER_INSTALLED.with(|h| h.get()));
            for fault in [3, 4, 5] {
                FAULT.with(|f| f.set(fault));
                let before = REMOVALS.with(|n| n.get());
                match spawn_wait(&plan("sleep", &root)) {
                    Err(Error::Wait {
                        source,
                        cleanup_failed,
                    }) => {
                        assert_eq!(
                            source.raw_os_error(),
                            Some(6),
                            "original native wait error must be retained"
                        );
                        assert_eq!(cleanup_failed, fault == 4);
                    }
                    other => panic!("wait error must remain primary: {other:?}"),
                }
                assert_eq!(REMOVALS.with(|n| n.get()), before + 1);
                assert!(!HANDLER_INSTALLED.with(|h| h.get()));
                let pid = FAILED_CHILD.with(|p| p.get());
                assert_ne!(pid, 0);
                // SAFETY: query only the known failed-wait child, never terminate by PID.
                let raw = unsafe {
                    windows_sys::Win32::System::Threading::OpenProcess(
                        windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        pid,
                    )
                };
                if !raw.is_null() {
                    // SAFETY: successful API transfers a query handle.
                    let h = unsafe { OwnedHandle::from_raw_handle(raw) };
                    let mut code = 0;
                    // SAFETY: live query handle and valid output buffer.
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
            }
            FAULT.with(|f| f.set(0));
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
fn fixture() -> fixture_private::PrivateTempDir {
    let parent = std::env::var_os("LARPCC_TEST_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    fixture_private::tempdir(&parent).unwrap()
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

// Only this unit-test binary can inject lifecycle faults; no CLI/env fault switch ships.
struct AuthFault {
    release: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reaped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    child: std::sync::mpsc::SyncSender<OwnedHandle>,
    unknown: bool,
    root: PathBuf,
}
thread_local! {
    static AUTH_FAULT: std::cell::RefCell<Option<AuthFault>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn begin_auth_fault(child: &Child) -> bool {
    use std::os::windows::io::AsHandle;
    AUTH_FAULT.with(|f| {
        let f = f.borrow();
        let Some(f) = f.as_ref() else {
            return false;
        };
        f.child
            .send(child.as_handle().try_clone_to_owned().unwrap())
            .unwrap();
        true
    })
}
pub(super) fn hold_cleanup() -> bool {
    AUTH_FAULT.with(|f| {
        f.borrow()
            .as_ref()
            .is_some_and(|f| !f.release.load(Ordering::SeqCst))
    })
}
pub(super) fn held_poll() -> io::Result<Option<std::process::ExitStatus>> {
    if AUTH_FAULT.with(|f| f.borrow().as_ref().unwrap().unknown) {
        Err(io::Error::other("injected auth cleanup query failure"))
    } else {
        Ok(None)
    }
}
pub(super) fn record_reap() {
    AUTH_FAULT.with(|f| {
        if let Some(f) = f.borrow().as_ref() {
            assert!(
                HANDLER_INSTALLED.with(|h| h.get()),
                "console protection ended before reap"
            );
            let busy = bounded(
                command("auth-busy", &f.root)
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                    .unwrap(),
            );
            assert!(
                busy.status.success(),
                "guard must still be held at confirmed reap: {busy:?}"
            );
            f.reaped.store(true, Ordering::SeqCst);
        }
    });
}
struct RestoreCleanup(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for RestoreCleanup {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
struct KnownChild(OwnedHandle);
impl Drop for KnownChild {
    fn drop(&mut self) {
        if child_status(&self.0) == 259 {
            // SAFETY: handle cloned directly from our self-bounded fake Child.
            assert_ne!(
                unsafe {
                    windows_sys::Win32::System::Threading::TerminateProcess(
                        self.0.as_raw_handle(),
                        1,
                    )
                },
                0
            );
            let end = Instant::now() + Duration::from_secs(2);
            while child_status(&self.0) == 259 && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_ne!(child_status(&self.0), 259, "known fake cleanup deadline");
        }
    }
}
fn child_status(child: &OwnedHandle) -> u32 {
    let mut code = 0;
    // SAFETY: cloned handle from our direct Child, never reopened by PID.
    assert_ne!(
        unsafe {
            windows_sys::Win32::System::Threading::GetExitCodeProcess(
                child.as_raw_handle(),
                &mut code,
            )
        },
        0
    );
    code
}
fn wait_fault(root: &Path, unknown: bool, auth: bool) {
    use crate::{
        launch::admission::*,
        registry::{Provider, Registry},
    };
    use std::sync::{Arc, atomic::AtomicBool, mpsc};
    let registry = Registry::at(root.join("managed")).unwrap();
    registry
        .add("work".parse().unwrap(), Provider::Codex)
        .unwrap();
    let fake = root.join("fake.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &fake).unwrap();
    registry.bind(Provider::Codex, fake.clone(), false).unwrap();
    let digest = checked_artifact(&fake, registry.root()).unwrap().sha256;
    let release = Arc::new(AtomicBool::new(false));
    let reaped = Arc::new(AtomicBool::new(false));
    let restore = RestoreCleanup(release.clone());
    let (child_tx, child_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let worker_root = root.to_owned();
    let worker_reaped = reaped.clone();
    let worker = std::thread::spawn(move || {
        let row = ArtifactRow {
            provider: Provider::Codex,
            release: "invented",
            sha256: digest,
            install_kind: "native-fake",
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            operation: if auth {
                Operation::Status
            } else {
                Operation::Run
            },
            interface: "invented",
            backend: "invented",
            policy_revision: 1,
            argv: &["--exact", ENTRY, "--nocapture"],
            denied_env: &[],
            paths: &[],
            evidence: "unit-test-only",
        };
        let mut admitted = admit(
            &registry,
            row.operation,
            &"work".parse().unwrap(),
            vec![],
            std::slice::from_ref(&row),
            &[],
        )
        .unwrap();
        let guard = if auth {
            Some(acquire_auth_guard(registry.root(), admitted.snapshot.target.profile.id).unwrap())
        } else {
            None
        };
        revalidate(&admitted).unwrap();
        admitted.plan.env_delta.extend([
            ("LARPCC_WINDOWS_CASE".into(), "auth-fake".into()),
            ("LARPCC_WINDOWS_ROOT".into(), worker_root.as_os_str().into()),
        ]);
        AUTH_FAULT.with(|f| {
            *f.borrow_mut() = Some(AuthFault {
                release,
                reaped: worker_reaped.clone(),
                child: child_tx,
                unknown,
                root: worker_root,
            })
        });
        let result = dispatch(admitted, guard);
        if auth {
            assert!(
                worker_reaped.load(Ordering::SeqCst),
                "dispatch returned before confirmed reap"
            );
        }
        assert!(!HANDLER_INSTALLED.with(|h| h.get()));
        done_tx.send(result).unwrap();
    });
    let child = KnownChild(
        child_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("known child handle"),
    );
    ready(root);
    let start = Instant::now();
    // The ordinary cleanup limit must expire while real child and wrapper stay live.
    std::thread::sleep(Duration::from_millis(2200));
    assert!(start.elapsed() > Duration::from_secs(2));
    assert_eq!(child_status(&child.0), 259);
    assert!(!reaped.load(Ordering::SeqCst));
    if !auth {
        match done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ordinary bounded return")
        {
            Err(Error::Wait {
                source,
                cleanup_failed,
            }) => {
                assert_eq!(source.raw_os_error(), Some(6));
                assert!(cleanup_failed);
            }
            other => panic!("ordinary Run must retain bounded failure: {other:?}"),
        }
        worker.join().unwrap();
        assert!(!root.join("managed/auth-locks").exists());
        return; // KnownChild and RestoreCleanup bound fake cleanup on every path.
    }
    assert!(
        matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "wrapper returned or panicked before release"
    );
    let busy = bounded(
        command("auth-busy", root)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    );
    assert!(
        busy.status.success(),
        "independent same-ID contender: {busy:?}"
    );
    drop(restore); // Restore genuine handle termination/query; never authorize PID cleanup.
    match done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("auth cleanup/reap deadline")
    {
        Err(Error::Wait {
            source,
            cleanup_failed,
        }) => {
            assert_eq!(source.raw_os_error(), Some(6));
            assert!(
                !cleanup_failed,
                "confirmed reap must not report a running child"
            );
        }
        other => panic!("original wait failure must remain primary: {other:?}"),
    }
    worker.join().unwrap();
    assert!(reaped.load(Ordering::SeqCst));
    assert_ne!(child_status(&child.0), 259);
    let free = bounded(
        command("auth-free", root)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    );
    assert!(
        free.status.success(),
        "guard must release after reap: {free:?}"
    );
}
#[test]
fn stage3_auth_wait_failure_retains_guard_past_cleanup_deadline_until_reap() {
    for mode in ["auth-live", "auth-unknown"] {
        let root = fixture();
        let output = bounded(
            command(mode, root.path())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .unwrap(),
        );
        assert!(output.status.success(), "{mode}: {output:?}");
    }
}

#[test]
fn stage3_run_wait_failure_keeps_ordinary_bounded_cleanup() {
    for mode in ["run-live", "run-unknown"] {
        let root = fixture();
        let output = bounded(
            command(mode, root.path())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .unwrap(),
        );
        assert!(output.status.success(), "{mode}: {output:?}");
    }
}
