// Test-only driver; never selected by ordinary larp dispatch.
#[path = "../../src/cli/mod.rs"]
mod cli;
#[path = "../../src/launch/mod.rs"]
mod launch;
#[path = "../../src/platform/mod.rs"]
mod platform;
#[path = "../../src/registry/mod.rs"]
mod registry;
fn main() -> std::process::ExitCode {
    cli::run()
}
