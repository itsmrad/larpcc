mod cli;
#[cfg(all(feature = "synthetic-launch", windows))]
#[path = "launch/types.rs"]
mod launch;
mod platform;
mod provider;
mod registry;
fn main() -> std::process::ExitCode {
    cli::run()
}
