mod cli;
mod launch;
mod platform;
mod provider;
mod registry;
mod share;
fn main() -> std::process::ExitCode {
    cli::run()
}
