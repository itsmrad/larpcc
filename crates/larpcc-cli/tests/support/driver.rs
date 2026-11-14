#[path = "../../src/cli/mod.rs"]
mod cli;
#[path = "../../src/launch/mod.rs"]
mod launch;
#[path = "../../src/launch/legacy.rs"]
mod legacy;
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
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("stage3")) {
        return stage3();
    }
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
        .and_then(|r| r.admission_snapshot(&name, |_| true).map(|s| s.target))
        .and_then(|t| legacy::plan(&t, args, &std::env::vars_os().collect::<Vec<_>>()))
        .and_then(|p| legacy::execute(&p));
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

// Nonpackaged invented-row input. Ordinary larp never compiles this parser or row.
#[derive(Parser)]
struct Invented {
    #[arg(value_parser = ["run", "login", "status", "logout"])]
    operation: String,
    name: registry::ProfileName,
    #[arg(long)]
    sha256: String,
    #[arg(long, value_parser = ["claude", "codex"])]
    provider: String,
    #[arg(long)]
    yes: bool,
    #[arg(last = true)]
    args: Vec<OsString>,
}
fn stage3() -> std::process::ExitCode {
    use launch::admission::{self, ArtifactRow, Operation};
    let input = Invented::parse_from(
        std::iter::once(OsString::from("stage3")).chain(std::env::args_os().skip(2)),
    );
    let op = match input.operation.as_str() {
        "login" => Operation::Login,
        "status" => Operation::Status,
        "logout" => Operation::Logout,
        _ => Operation::Run,
    };
    let mut sha256 = [0; 32];
    if input.sha256.len() != 64 || !input.sha256.is_ascii() {
        eprintln!("invalid invented digest");
        return 2.into();
    }
    for (i, byte) in sha256.iter_mut().enumerate() {
        let Ok(value) = u8::from_str_radix(&input.sha256[i * 2..i * 2 + 2], 16) else {
            eprintln!("invalid invented digest");
            return 2.into();
        };
        *byte = value;
    }
    let row = ArtifactRow {
        provider: input.provider.parse().unwrap(),
        release: "invented-1",
        sha256,
        install_kind: "test-fixture",
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        operation: op,
        interface: "invented",
        backend: "invented-no-network",
        policy_revision: 1,
        argv: &[],
        denied_env: &["LARPCC_SYNTHETIC_DENY_"],
        paths: &[],
        evidence: "synthetic fixture only",
    };
    match admission::invoke(op, &input.name, input.args, input.yes, &[row]) {
        Ok(code) => std::process::exit(code as i32),
        Err(error) => {
            eprintln!("error: {error}");
            if let Some(hint) = error.hint() {
                eprintln!("hint: {hint}");
            }
            if matches!(error, registry::Error::ConfirmationRequired) {
                2.into()
            } else {
                1.into()
            }
        }
    }
}
