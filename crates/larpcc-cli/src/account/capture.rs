use crate::{launch::LaunchPlan, registry::Error};
use std::{
    io::{self, Read},
    process::{Child, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const STREAM_CAP: usize = 16 * 1024;

pub(super) enum Captured {
    Complete(ExitStatus, Vec<u8>, Vec<u8>),
    Incomplete,
    Cancelled(i32),
}

pub(super) fn run(
    plan: &LaunchPlan,
    budget: Duration,
    revalidate: impl FnOnce() -> Result<(), Error>,
) -> Result<Captured, Error> {
    let (mut command, signals) = os::prepare(plan)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = revalidate().and_then(|()| {
        if let Some(signal) = signals.pending() {
            return Ok(Captured::Cancelled(signal));
        }
        let deadline = Instant::now() + budget;
        command
            .spawn()
            .map_err(|source| Error::Launch {
                path: plan.executable.clone(),
                source,
            })
            .and_then(|mut child| supervise(&mut child, &signals, deadline))
    });
    let pending = signals.pending();
    signals.finish()?;
    match pending {
        Some(signal) => Ok(Captured::Cancelled(signal)),
        None => result,
    }
}

fn supervise(
    child: &mut Child,
    signals: &os::Signals,
    deadline: Instant,
) -> Result<Captured, Error> {
    let mut status = None;
    let result = drain(child, signals, deadline, &mut status);
    if status.is_none() {
        os::cancel_reap(child);
    }
    result.map_err(|source| Error::Wait {
        source,
        cleanup_failed: false,
    })
}

fn drain(
    child: &mut Child,
    signals: &os::Signals,
    deadline: Instant,
    status: &mut Option<ExitStatus>,
) -> io::Result<Captured> {
    let mut stdout = os::prepare_pipe(child.stdout.take().ok_or_else(missing_pipe)?)?;
    let mut stderr = os::prepare_pipe(child.stderr.take().ok_or_else(missing_pipe)?)?;
    let mut out = Stream::default();
    let mut err = Stream::default();
    loop {
        if let Some(signal) = signals.pending() {
            return Ok(Captured::Cancelled(signal));
        }
        if Instant::now() >= deadline {
            return Ok(Captured::Incomplete);
        }
        if !out.step(|buf| os::read_pipe(&mut stdout, buf))?
            || !err.step(|buf| os::read_pipe(&mut stderr, buf))?
        {
            return Ok(Captured::Incomplete);
        }
        if status.is_none() {
            *status = child.try_wait()?;
        }
        if let Some(exit) = *status
            && out.eof
            && err.eof
        {
            return Ok(Captured::Complete(exit, out.bytes, err.bytes));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn missing_pipe() -> io::Error {
    io::Error::other("status capture pipe is unavailable")
}

#[derive(Default)]
struct Stream {
    bytes: Vec<u8>,
    eof: bool,
}
impl Stream {
    // Read at most one byte past the cap to distinguish an exact-cap result.
    fn step(&mut self, read: impl FnOnce(&mut [u8]) -> io::Result<usize>) -> io::Result<bool> {
        if self.eof {
            return Ok(true);
        }
        let mut chunk = [0; 4096];
        let remaining = STREAM_CAP - self.bytes.len();
        let length = chunk.len().min(remaining + 1);
        match read(&mut chunk[..length]) {
            Ok(0) => self.eof = true,
            Ok(n) if n > remaining => return Ok(false),
            Ok(n) => self.bytes.extend_from_slice(&chunk[..n]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
        Ok(true)
    }
}

#[cfg(unix)]
mod os {
    use super::*;
    use std::os::fd::AsRawFd;

    pub(super) use crate::platform::AuthSignals as Signals;
    pub(super) use crate::platform::auth_capture_command as prepare;

    pub(super) fn cancel_reap(child: &mut Child) {
        crate::platform::cancel_reap(child.id() as libc::pid_t);
    }

    pub(super) fn prepare_pipe<T: AsRawFd>(pipe: T) -> io::Result<T> {
        // SAFETY: the owned pipe remains live during both descriptor operations.
        let fd = pipe.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pipe)
    }
    pub(super) fn read_pipe(pipe: &mut impl Read, bytes: &mut [u8]) -> io::Result<usize> {
        pipe.read(bytes)
    }

    pub(super) fn propagate(signal: i32) -> Result<(), Error> {
        crate::platform::terminate_by_signal(signal).map(|_| ())
    }
}

#[cfg(windows)]
mod os {
    use super::*;
    use std::{
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::Command,
        sync::atomic::{AtomicBool, Ordering},
    };
    use windows_sys::Win32::{
        Foundation::{ERROR_BROKEN_PIPE, STATUS_CONTROL_C_EXIT},
        System::{
            Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler},
            Pipes::PeekNamedPipe,
            Threading::CREATE_NO_WINDOW,
        },
    };

    static CANCELLED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn cancel(event: u32) -> i32 {
        if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
            CANCELLED.store(true, Ordering::SeqCst);
            return 1;
        }
        0
    }

    pub(super) struct Signals;
    impl Signals {
        pub(super) fn pending(&self) -> Option<i32> {
            CANCELLED
                .load(Ordering::SeqCst)
                .then_some(STATUS_CONTROL_C_EXIT)
        }
        pub(super) fn finish(self) -> Result<(), Error> {
            // SAFETY: remove the same static handler after the direct child was reaped.
            if unsafe { SetConsoleCtrlHandler(Some(cancel), 0) } == 0 {
                return Err(Error::Console {
                    action: "remove status cancellation handler",
                    source: io::Error::last_os_error(),
                });
            }
            Ok(())
        }
    }

    pub(super) fn prepare(plan: &LaunchPlan) -> Result<(Command, Signals), Error> {
        let mut command = Command::new(&plan.executable);
        command
            .creation_flags(CREATE_NO_WINDOW)
            .args(&plan.argv)
            .envs(plan.env_delta.iter().cloned());
        if let Some(cwd) = &plan.cwd {
            command.current_dir(cwd);
        }
        CANCELLED.store(false, Ordering::SeqCst);
        // SAFETY: the handler is static and only writes a lock-free atomic.
        if unsafe { SetConsoleCtrlHandler(Some(cancel), 1) } == 0 {
            return Err(Error::Console {
                action: "install status cancellation handler",
                source: io::Error::last_os_error(),
            });
        }
        Ok((command, Signals))
    }

    pub(super) use crate::launch::windows::auth_cancel_reap as cancel_reap;

    pub(super) fn prepare_pipe<T>(pipe: T) -> io::Result<T> {
        Ok(pipe)
    }
    pub(super) fn read_pipe(
        pipe: &mut (impl Read + AsRawHandle),
        bytes: &mut [u8],
    ) -> io::Result<usize> {
        use std::ptr::null_mut;
        let mut available = 0;
        // SAFETY: the handle is the owned read end, and available is writable.
        let ok = unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                Ok(0)
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let length = bytes.len().min(available as usize);
        pipe.read(&mut bytes[..length])
    }
    pub(super) fn propagate(signal: i32) -> Result<(), Error> {
        std::process::exit(signal)
    }
}

pub(super) fn propagate(signal: i32) -> Result<(), Error> {
    os::propagate(signal)
}
