mod cli;
#[cfg_attr(any(test, not(target_os = "macos")), allow(dead_code))]
mod desktop;
mod launch;
mod platform;
mod provider;
mod registry;
mod share;
fn main() -> std::process::ExitCode {
    cli::run()
}
