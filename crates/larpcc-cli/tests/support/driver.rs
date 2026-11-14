#[path = "../../src/cli/mod.rs"]
mod cli;
#[path = "../../src/launch/mod.rs"]
mod launch;
#[cfg(unix)]
#[path = "unix_observe.rs"]
mod observe;
#[path = "../../src/platform/mod.rs"]
mod platform;
#[path = "../../src/provider/mod.rs"]
mod provider;
#[path = "../../src/registry/mod.rs"]
mod registry;
use clap::{Parser, Subcommand};
use std::{
    ffi::OsString,
    io::{self, Write},
};
#[derive(Parser)]
#[command(
    name = "larpcc-synthetic",
    about = "Test-only provisional synthetic launch driver",
    disable_help_subcommand = true
)]
struct Driver {
    #[command(subcommand)]
    command: Root,
}
#[derive(Subcommand)]
enum Root {
    Cc {
        #[command(subcommand)]
        command: Action,
    },
}
#[derive(Subcommand)]
enum Action {
    Run {
        name: registry::ProfileName,
        #[arg(last = true)]
        args: Vec<OsString>,
    },
}
fn main() -> std::process::ExitCode {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("cc"))
        && std::env::args_os().nth(2).as_deref() != Some(std::ffi::OsStr::new("run"))
    {
        return cli::run();
    }
    let driver = Driver::parse();
    let Root::Cc {
        command: Action::Run { name, args },
    } = driver.command;
    let observing = std::env::var_os("LARPCC_SYNTHETIC_OBSERVE_FAILURE").is_some();
    let before_env = if observing {
        Some(std::env::vars_os().collect::<Vec<_>>())
    } else {
        None
    };
    #[cfg(unix)]
    let before_signals = if observing {
        observe::observe().ok()
    } else {
        None
    };
    let result = registry::Registry::locate()
        .and_then(|r| r.launch_target(&name))
        .and_then(|t| launch::plan(&t, args, &std::env::vars_os().collect::<Vec<_>>()))
        .and_then(|p| launch::execute(&p));
    match result {
        Ok(code) => std::process::exit(code as i32),
        Err(error) => {
            if observing {
                let env_unchanged = before_env == Some(std::env::vars_os().collect::<Vec<_>>());
                #[cfg(unix)]
                let signals_unchanged = before_signals == observe::observe().ok();
                #[cfg(not(unix))]
                let signals_unchanged = true;
                println!(
                    "{}",
                    serde_json::json!({"env_unchanged":env_unchanged,"signals_unchanged":signals_unchanged})
                );
            }
            let mut err = io::stderr().lock();
            let _ = writeln!(err, "error: {error}");
            if let Some(hint) = error.hint() {
                let _ = writeln!(err, "hint: {hint}");
            }
            std::process::exit(1);
        }
    }
}
