mod cli;
// shortcut: seed contracts are unused, CLI worker must remove both allowances when wiring.
#[allow(dead_code, unused_imports)]
mod registry;
fn main() -> std::process::ExitCode {
    cli::run()
}
