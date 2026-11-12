mod cli;
#[cfg(feature = "synthetic-launch")]
#[path = "launch/types.rs"]
mod launch;
mod platform;
mod registry;
fn main() -> std::process::ExitCode {
    cli::run()
}
