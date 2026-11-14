mod cli;
mod launch;
mod platform;
mod provider;
mod registry;
fn main() -> std::process::ExitCode {
    cli::run()
}
