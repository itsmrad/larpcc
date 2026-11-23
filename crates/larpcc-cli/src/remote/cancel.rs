use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

static REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) fn requested() -> bool {
    REQUESTED.load(Ordering::Relaxed)
}

pub(crate) struct Guard {
    #[cfg(unix)]
    previous: [libc::sighandler_t; 3],
}

#[cfg(unix)]
const SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

#[cfg(unix)]
extern "C" fn request(_signal: libc::c_int) {
    REQUESTED.store(true, Ordering::Relaxed);
}

#[cfg(windows)]
unsafe extern "system" fn request(control: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if matches!(control, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        REQUESTED.store(true, Ordering::Relaxed);
        return 1;
    }
    0
}

impl Guard {
    pub(crate) fn install() -> io::Result<Self> {
        REQUESTED.store(false, Ordering::Relaxed);
        #[cfg(unix)]
        {
            let mut previous = [libc::SIG_DFL; SIGNALS.len()];
            for (index, signal) in SIGNALS.into_iter().enumerate() {
                // SAFETY: each signal permits a handler. The handler uses only a lock-free atomic.
                let handler =
                    unsafe { libc::signal(signal, request as *const () as libc::sighandler_t) };
                if handler == libc::SIG_ERR {
                    let error = io::Error::last_os_error();
                    for (&signal, &handler) in SIGNALS[..index].iter().zip(&previous) {
                        // SAFETY: restores a disposition returned by signal for this signal.
                        unsafe { libc::signal(signal, handler) };
                    }
                    return Err(error);
                }
                previous[index] = handler;
            }
            Ok(Self { previous })
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
            // SAFETY: the callback has the required system ABI and static lifetime.
            if unsafe { SetConsoleCtrlHandler(Some(request), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {})
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        #[cfg(unix)]
        for (signal, handler) in SIGNALS.into_iter().zip(self.previous) {
            // SAFETY: restores the previous disposition for this signal.
            unsafe { libc::signal(signal, handler) };
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
            // SAFETY: unregisters the callback installed by Guard::install.
            unsafe { SetConsoleCtrlHandler(Some(request), 0) };
        }
    }
}
