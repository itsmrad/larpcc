mod aliases;
mod cli;
#[cfg_attr(any(test, not(target_os = "macos")), allow(dead_code))]
mod desktop;
mod launch;
mod platform;
mod provider;
mod registry;
mod share;
fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    platform::pin_startup_image();
    // argv[0] classification precedes every parser and early gate.
    if let Some(code) = cli::gate() {
        return code;
    }
    cli::run()
}
