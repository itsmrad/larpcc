use std::io;
pub fn observe() -> io::Result<serde_json::Value> {
    // SAFETY: sigset_t is valid as zero-initialized output storage for sigprocmask.
    let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: a null new mask queries only; mask is writable storage.
    if unsafe { libc::sigprocmask(0, std::ptr::null(), &mut mask) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut dispositions = Vec::new();
    for signal in [
        libc::SIGINT,
        libc::SIGTERM,
        libc::SIGHUP,
        libc::SIGQUIT,
        libc::SIGPIPE,
        libc::SIGTSTP,
    ] {
        // SAFETY: sigaction is initialized output storage, not interpreted before success.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: querying action only; signal is a known valid signal and output is writable.
        if unsafe { libc::sigaction(signal, std::ptr::null(), &mut action) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let disposition = if action.sa_sigaction == libc::SIG_IGN {
            "ignored"
        } else if action.sa_sigaction == libc::SIG_DFL {
            "default"
        } else {
            "handler"
        };
        // SAFETY: mask is initialized by successful sigprocmask; signal is valid.
        let blocked = unsafe { libc::sigismember(&mask, signal) } == 1;
        dispositions
            .push(serde_json::json!({"signal":signal,"disposition":disposition,"blocked":blocked}));
    }
    // SAFETY: getppid and getpgrp have no arguments or memory preconditions.
    let (ppid, pgrp) = unsafe { (libc::getppid(), libc::getpgrp()) };
    let mut open_descriptors = Vec::new();
    for fd in 3..64 {
        // SAFETY: F_GETFD queries an integer descriptor without dereferencing user memory.
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0 {
            open_descriptors.push(fd);
        }
    }
    Ok(
        serde_json::json!({"ppid":ppid,"pgrp":pgrp,"signals":dispositions,"open_descriptors":open_descriptors}),
    )
}
