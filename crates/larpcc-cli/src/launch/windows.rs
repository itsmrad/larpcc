use crate::registry::Error;
use std::io;
#[cfg(test)]
use windows_sys::Win32::Foundation::SetLastError;

unsafe extern "system" fn console_handler(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    i32::from(event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT)
}
#[cfg(test)]
thread_local! {
    static FAULT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static REMOVALS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static HANDLER_INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAILED_CHILD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
fn set_handler(add: bool) -> bool {
    #[cfg(test)]
    {
        if !add {
            REMOVALS.with(|n| n.set(n.get() + 1));
        }
        if add && FAULT.with(|f| f.get() == 1) {
            return false;
        }
    }
    // SAFETY: static handler remains valid throughout the process lifetime.
    let success = unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(
            Some(console_handler),
            i32::from(add),
        ) != 0
    };
    #[cfg(test)]
    {
        if success {
            HANDLER_INSTALLED.with(|h| h.set(add));
        }
        // Inject failure after real removal so test processes leave no real handler installed.
        if !add && FAULT.with(|f| matches!(f.get(), 2 | 5 | 6)) {
            // SAFETY: thread-local deterministic error for the injected failed API result.
            unsafe {
                SetLastError(6);
            }
            return false;
        }
    }
    success
}

fn wait_child(child: &mut std::process::Child) -> io::Result<std::process::ExitStatus> {
    #[cfg(test)]
    if FAULT.with(|f| matches!(f.get(), 3..=5)) {
        FAILED_CHILD.with(|p| p.set(child.id()));
        return Err(io::Error::from_raw_os_error(6));
    }
    child.wait()
}
fn poll_cleanup(child: &mut std::process::Child) -> io::Result<Option<std::process::ExitStatus>> {
    #[cfg(test)]
    if FAULT.with(|f| f.get() == 4) {
        return Err(io::Error::other("injected cleanup query failure"));
    }
    child.try_wait()
}
pub fn spawn_wait(plan: &crate::launch::LaunchPlan) -> Result<u32, Error> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // SAFETY: static handler performs no allocation/IO and is valid until removal.
    if !set_handler(true) {
        return Err(Error::Console {
            action: "register console handler",
            source: io::Error::last_os_error(),
        });
    }
    let mut command = Command::new(&plan.executable);
    command
        .args(&plan.argv)
        .envs(plan.env_delta.iter().cloned())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(cwd) = &plan.cwd {
        command.current_dir(cwd);
    }
    let result = match command.spawn() {
        Err(source) => Err(Error::Launch {
            path: plan.executable.clone(),
            source,
        }),
        Ok(mut child) => match wait_child(&mut child) {
            Ok(status) => status
                .code()
                .map(|code| code as u32)
                .ok_or_else(|| Error::Wait {
                    source: io::Error::other("child status is unavailable"),
                    cleanup_failed: false,
                }),
            Err(source) => {
                let _ = child.kill();
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut reaped = false;
                while Instant::now() < deadline {
                    if matches!(poll_cleanup(&mut child), Ok(Some(_))) {
                        reaped = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(Error::Wait {
                    source,
                    cleanup_failed: !reaped,
                })
            }
        },
    };
    // SAFETY: removes exactly our registered static handler, after known child completion/cleanup.
    // Preserve a primary spawn/wait error; standalone removal failure is reported below.
    let removed = set_handler(false);
    if !removed && result.is_ok() {
        return Err(Error::Console {
            action: "unregister console handler",
            source: io::Error::last_os_error(),
        });
    }
    result
}

#[cfg(test)]
#[path = "../../tests/fixtures/windows/lifecycle.rs"]
mod lifecycle;
