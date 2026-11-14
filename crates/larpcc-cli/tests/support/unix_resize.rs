use std::io;
static RESIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
extern "C" fn resized(_: libc::c_int) {
    RESIZED.store(true, std::sync::atomic::Ordering::Relaxed);
}
pub fn install_resize() -> io::Result<()> {
    // SAFETY: zeroed sigaction is initialized before registration and handler only stores a lock-free atomic bool.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = resized as *const () as usize;
    action.sa_flags = libc::SA_RESTART;
    // SAFETY: action and its empty mask are initialized and SIGWINCH is valid.
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
        || unsafe { libc::sigaction(libc::SIGWINCH, &action, std::ptr::null_mut()) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn resized_size() -> io::Result<Option<(u16, u16)>> {
    if !RESIZED.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return Ok(None);
    }
    // SAFETY: winsize is output storage, and ioctl writes only this fixed-size native structure.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: stdin is the test PTY slave; TIOCGWINSZ accepts writable winsize storage.
    if unsafe { libc::ioctl(0, libc::TIOCGWINSZ, &mut size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Some((size.ws_row, size.ws_col)))
}
