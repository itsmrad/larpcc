#[path = "../../src/account/mod.rs"]
mod account;
#[path = "../../src/aliases.rs"]
#[allow(dead_code)]
mod aliases;
#[path = "../../src/cli/mod.rs"]
mod cli;
#[path = "../../src/desktop/mod.rs"]
#[allow(dead_code)]
mod desktop;
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
#[path = "../../src/share/mod.rs"]
mod share;
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
    #[cfg(windows)]
    platform::pin_startup_image();
    if let Some(spec) = std::env::var_os("LARPCC_SYNTHETIC_VERSION_DESCRIPTORS") {
        provider::version::synthetic_descriptors(invented_descriptors(&spec.to_string_lossy()));
    }
    let mut args = std::env::args_os();
    let runner = format!("larpcc-synthetic{}", std::env::consts::EXE_SUFFIX);
    if args
        .next()
        .as_deref()
        .and_then(|arg| std::path::Path::new(arg).file_name())
        != Some(std::ffi::OsStr::new(&runner))
    {
        return cli::gate_with_rows(alias_rows).unwrap_or_else(cli::run);
    }
    if args.next().as_deref() == Some(std::ffi::OsStr::new("stage05-install")) {
        return match install_alias() {
            Ok(()) => 0.into(),
            Err(error) => cli::operational_error(error),
        };
    }
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("stage3")) {
        return stage3();
    }
    #[cfg(unix)]
    if let Some(digest) = std::env::var("LARPCC_STAGE11_FAKE_DIGEST")
        .ok()
        .and_then(|value| invented_digest(&value))
    {
        use launch::admission::{Operation, synthetic_usage_rows};
        use registry::Provider;
        synthetic_usage_rows(
            [Provider::Codex, Provider::Claude]
                .into_iter()
                .map(|provider| invented_row(provider, Operation::Usage, digest))
                .collect(),
        );
    }
    if let Some(digest) = std::env::var("LARPCC_STAGE14_FAKE_DIGEST")
        .ok()
        .and_then(|value| invented_digest(&value))
    {
        return cli::run_with_status_rows(&[account::synthetic_row(digest)]);
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

mod alias_args;

fn install_alias() -> Result<(), registry::Error> {
    use registry::{ProfileName, Registry, Surface};
    let source = std::env::args_os()
        .nth(2)
        .ok_or(registry::Error::AliasInvalid)?;
    let source = std::path::Path::new(&source);
    Registry::locate()?.alias_add_with(
        ProfileName::parse_new("codex-45").unwrap(),
        &ProfileName::parse_new("claude-work").unwrap(),
        Surface::Cli,
        None,
        |root| aliases::Source::open(source, root),
    )?;
    Ok(())
}

fn alias_rows(
    forwarded: &[OsString],
) -> Result<std::borrow::Cow<'static, [launch::admission::ArtifactRow]>, registry::Error> {
    use launch::admission::Operation;
    let digest = std::env::var("LARPCC_SYNTHETIC_ALIAS_SHA256")
        .ok()
        .and_then(|value| invented_digest(&value))
        .ok_or(registry::Error::ArtifactInvalid)?;
    // Check the arguments handed off by the alias separately from the invented row.
    assert!(
        forwarded
            .iter()
            .map(OsString::as_os_str)
            .eq(alias_args::ARGS.iter().map(std::ffi::OsStr::new))
    );
    let mut row = invented_row(registry::Provider::Codex, Operation::Run, digest);
    row.argv = alias_args::ARGS;
    Ok(std::borrow::Cow::Owned(vec![row]))
}

// Nonpackaged invented-row input. Ordinary larp never compiles this parser or row.
#[derive(Parser)]
struct Invented {
    #[arg(long, value_parser = ["setup", "wait", "cancel"])]
    failure: Option<String>,
    #[arg(long)]
    observation: Option<std::path::PathBuf>,
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
    use launch::admission::{self, Operation};
    let input = Invented::parse_from(
        std::iter::once(OsString::from("stage3")).chain(std::env::args_os().skip(2)),
    );
    let op = match input.operation.as_str() {
        "login" => Operation::Login,
        "status" => Operation::Status,
        "logout" => Operation::Logout,
        _ => Operation::Run,
    };
    let Some(sha256) = invented_digest(&input.sha256) else {
        eprintln!("invalid invented digest");
        return 2.into();
    };
    let row = invented_row(input.provider.parse().unwrap(), op, sha256);
    #[cfg(unix)]
    let before_signals = observe::observe().unwrap();
    let before_env = std::env::vars_os().collect::<Vec<_>>();
    if let Some(failure) = &input.failure {
        #[cfg(unix)]
        platform::synthetic_auth_fault(match failure.as_str() {
            "setup" => 1,
            "wait" => 2,
            _ => 3,
        });
        #[cfg(windows)]
        {
            eprintln!("unsupported native failure seam: {failure}");
            return 2.into();
        }
    }
    let result = admission::invoke(op, &input.name, input.args, input.yes, &[row]);
    if let Some(path) = &input.observation {
        #[cfg(unix)]
        let signals_unchanged = before_signals == observe::observe().unwrap();
        #[cfg(windows)]
        let signals_unchanged = true;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer(file, &serde_json::json!({"signals_unchanged":signals_unchanged,"env_unchanged":before_env==std::env::vars_os().collect::<Vec<_>>()})).unwrap();
    }
    match result {
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

// Stage 08 invented descriptors: `provider|sha256|argv|prefix|suffix`, `;`-separated.
// Leaked once per test process; the shipped table stays empty.
fn invented_descriptors(spec: &str) -> Vec<provider::version::VersionDescriptor> {
    let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    spec.split(';')
        .map(|entry| {
            let fields: Vec<_> = entry.split('|').collect();
            assert_eq!(fields.len(), 5, "invented descriptor fields");
            let provider: registry::Provider = fields[0].parse().unwrap();
            let argv: &'static [&'static str] = Box::leak(vec![leak(fields[2])].into_boxed_slice());
            provider::version::VersionDescriptor {
                provider,
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                sha256: invented_digest(fields[1]).expect("invented digest"),
                argv,
                home_env: provider::version::home_env(provider),
                prefix: leak(fields[3]),
                suffix: leak(fields[4]),
                evidence: "synthetic fixture only",
            }
        })
        .collect()
}

fn invented_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.is_ascii() {
        return None;
    }
    let mut sha256 = [0; 32];
    for (i, byte) in sha256.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(sha256)
}

fn invented_row(
    provider: registry::Provider,
    op: launch::admission::Operation,
    sha256: [u8; 32],
) -> launch::admission::ArtifactRow {
    use launch::admission::{ArtifactRow, Operation};
    ArtifactRow {
        provider,
        release: "invented-1",
        sha256,
        install_kind: "test-fixture",
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        operation: op,
        interface: "invented",
        backend: "invented-no-network",
        policy_revision: 1,
        argv: match op {
            Operation::Run => &[],
            Operation::Login => &["invented-login"],
            Operation::Status => &["invented-status"],
            Operation::Logout => &["invented-logout"],
            #[cfg(unix)]
            Operation::Usage => launch::admission::usage_argv(provider),
            #[cfg(windows)]
            Operation::Usage => unreachable!(),
        },
        denied_env: &["LARPCC_SYNTHETIC_DENY_*"],
        paths: &[],
        evidence: "synthetic fixture only",
    }
}
