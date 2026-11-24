mod account;
mod aliases;
mod cli;
#[cfg_attr(any(test, not(target_os = "macos")), allow(dead_code))]
mod desktop;
mod launch;
mod platform;
mod provider;
mod registry;
mod share;
#[cfg(windows)]
fn main() -> std::process::ExitCode {
    cli::start_windows(std::env::args_os(), std::env::current_exe, || {
        platform::pin_startup_image();
        run()
    })
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    run()
}

fn run() -> std::process::ExitCode {
    // argv[0] classification precedes every parser and early gate.
    if let Some(code) = cli::gate() {
        return code;
    }
    cli::run()
}
