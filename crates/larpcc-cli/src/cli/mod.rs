mod json;
mod render;
use json::{execute_json, json_error, json_or_usage, json_success};

use crate::launch::admission::{self, Operation};
use crate::registry::{Error, ProfileName, Provider, Registry};
use clap::{Parser, Subcommand, builder::PossibleValuesParser};
use std::{
    io::{self, Write},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "larp",
    bin_name = "larp",
    version,
    about = "Named Claude Code and Codex account profiles",
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Emit one local metadata JSON envelope
    #[arg(long, global = true)]
    json: bool,
    /// Refuse if the registry revision has changed
    #[arg(long, global = true, value_parser = parse_revision)]
    expected_revision: Option<u64>,
    #[command(subcommand)]
    command: RootAction,
}

fn parse_revision(value: &str) -> Result<u64, &'static str> {
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err("expected a canonical decimal u64 revision");
    }
    value.parse().map_err(|_| "revision exceeds u64 range")
}

#[derive(Subcommand)]
enum RootAction {
    #[command(
        about = "Manage Claude Code and Codex profiles",
        arg_required_else_help = true,
        disable_help_subcommand = true,
        after_help = "Data directory: $LARPCC_HOME if set, otherwise $XDG_DATA_HOME/larpcc or ~/.local/share/larpcc (Linux), ~/Library/Application Support/larpcc (macOS), %LOCALAPPDATA%\\larpcc (Windows)."
    )]
    Cc {
        #[command(subcommand)]
        command: CcAction,
    },
}

#[derive(Subcommand)]
enum CcAction {
    /// Bind or open a verified desktop app (ChatGPT on macOS; Claude closed)
    #[command(arg_required_else_help = true, disable_help_subcommand = true)]
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Manage explicit skill packages and per-profile sharing
    #[command(arg_required_else_help = true, disable_help_subcommand = true)]
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Manage reference-only MCP definitions and per-profile switches
    #[command(arg_required_else_help = true, disable_help_subcommand = true)]
    Mcp {
        #[command(subcommand)]
        command: crate::share::mcp::cli::McpCommand,
    },
    #[command(
        about = "Manage native provider bindings",
        arg_required_else_help = true,
        disable_help_subcommand = true
    )]
    Provider {
        #[command(subcommand)]
        command: ProviderAction,
    },
    #[command(about = "Launch a profile (qualification pending; disabled in this build)", after_help = admission::ROUTING_NOTICE)]
    Run {
        name: ProfileName,
        #[arg(last = true)]
        args: Vec<std::ffi::OsString>,
    },
    #[command(about = "Authenticate a profile", after_help = admission::AUTH_HELP)]
    Login {
        name: ProfileName,
        #[arg(long, help = "Confirm the authentication operation")]
        yes: bool,
    },
    #[command(about = "Show native authentication status", after_help = admission::AUTH_HELP)]
    Status { name: ProfileName },
    #[command(about = "Log out a profile", after_help = admission::AUTH_HELP)]
    Logout {
        name: ProfileName,
        #[arg(long, help = "Confirm the authentication operation")]
        yes: bool,
    },
    #[command(
        about = "Manage profile records (no login, no provider launch)",
        arg_required_else_help = true,
        disable_help_subcommand = true
    )]
    Profile {
        #[command(subcommand)]
        command: ProfileAction,
    },
}

#[derive(Subcommand)]
enum AppCommand {
    Bind {
        #[arg(value_parser = PossibleValuesParser::new(Provider::ALL.map(Provider::as_str)).try_map(|s| s.parse::<Provider>().map_err(|_| "invalid provider")))]
        provider: Provider,
        path: std::path::PathBuf,
        #[arg(long = "version", value_parser=crate::registry::parse_recorded_version)]
        recorded_version: Option<String>,
        #[arg(long)]
        replace: bool,
    },
    Open {
        name: ProfileName,
    },
}

#[derive(Clone, Subcommand)]
enum SkillsCommand {
    /// Copy one explicit package into the immutable managed store
    Add {
        path: std::path::PathBuf,
        #[arg(long, value_parser=ProfileName::parse_new)]
        name: ProfileName,
    },
    /// Remove a package after disabling all profile references
    Remove {
        name: ProfileName,
    },
    Enable {
        name: ProfileName,
        profile: ProfileName,
    },
    Disable {
        name: ProfileName,
        profile: ProfileName,
    },
    List,
    Matrix,
    /// Apply sharing changes or explicitly resume an interrupted transaction
    Sync,
}
impl SkillsCommand {
    fn action(&self) -> crate::registry::SkillsAction {
        self.clone().into_action()
    }
    fn into_action(self) -> crate::registry::SkillsAction {
        use crate::registry::SkillsAction as A;
        match self {
            Self::Add { path, name } => A::Add { source: path, name },
            Self::Remove { name } => A::Remove { name },
            Self::Enable { name, profile } => A::Switch {
                name,
                profile,
                enabled: true,
            },
            Self::Disable { name, profile } => A::Switch {
                name,
                profile,
                enabled: false,
            },
            Self::List => A::List,
            Self::Matrix => A::Matrix,
            Self::Sync => A::Sync,
        }
    }
}

#[derive(Subcommand)]
enum ProviderAction {
    #[command(about = "Bind an absolute native executable")]
    Bind {
        #[arg(value_parser = PossibleValuesParser::new(Provider::ALL.map(Provider::as_str)).try_map(|s| s.parse::<Provider>().map_err(|_| "invalid provider")))]
        provider: Provider,
        executable: std::path::PathBuf,
        #[arg(long, help = "Replace an existing provider binding")]
        replace: bool,
    },
    #[command(about = "List provider bindings")]
    List,
}

#[derive(Subcommand)]
enum ProfileAction {
    /// Enable or disable a profile surface (does not qualify native launch)
    Surface {
        name: ProfileName,
        #[arg(value_parser = ["cli", "desktop"])]
        surface: String,
        #[arg(value_parser = ["on", "off"])]
        enabled: String,
    },
    /// Register a profile and create its empty state directory
    Add {
        /// Profile name: 1-32 characters from a-z, 0-9, '-', '_', starting with a letter
        #[arg(value_parser = ProfileName::parse_new)]
        name: ProfileName,
        /// Provider this profile is for
        #[arg(long, value_name = "PROVIDER", value_parser = PossibleValuesParser::new(Provider::ALL.map(Provider::as_str)).try_map(|s| s.parse::<Provider>().map_err(|_| "invalid provider")))]
        provider: Provider,
    },
    /// List profiles
    List,
    /// Show one profile
    Show {
        /// Profile name
        name: ProfileName,
    },
    /// Rename a profile; its ID and state directory do not change
    Rename {
        /// Profile name
        name: ProfileName,
        /// New profile name
        #[arg(value_parser = ProfileName::parse_new)]
        new_name: ProfileName,
    },
    /// Remove a profile from the registry; its state directory is kept
    Forget {
        /// Profile name
        name: ProfileName,
        /// Confirm removing the profile from the registry
        #[arg(long)]
        yes: bool,
    },
}

use clap::builder::TypedValueParser;

pub fn run() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let early_json = args
        .iter()
        .skip(1)
        .take_while(|a| *a != "--")
        .any(|a| a == "--json");
    let expected_present = args.iter().skip(1).take_while(|a| *a != "--").any(|a| {
        a == "--expected-revision"
            || a.to_str()
                .is_some_and(|s| s.starts_with("--expected-revision="))
    });
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(mut error) => {
            use clap::error::ErrorKind;
            let informational = matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            if informational && expected_present {
                return json_or_usage(
                    early_json,
                    "parse",
                    "usage",
                    "Help and version do not accept a revision precondition.",
                );
            }
            if early_json {
                if informational {
                    let (command, data) = if error.kind() == ErrorKind::DisplayVersion {
                        (
                            "version",
                            serde_json::json!({"version":env!("CARGO_PKG_VERSION")}),
                        )
                    } else {
                        ("help", serde_json::json!({"text":error.to_string()}))
                    };
                    return json_success(command, None, data);
                }
                return json_error("parse", "usage", "Invalid command or arguments.", 2);
            }
            escape_error_values(&mut error);
            if !error.use_stderr() {
                return write_output(
                    error.to_string().as_bytes(),
                    &mut io::stdout().lock(),
                    &mut io::stderr().lock(),
                );
            }
            let _ = write!(io::stderr().lock(), "{error}");
            return ExitCode::from(2);
        }
    };
    let RootAction::Cc { command } = cli.command;
    if cli.json {
        return execute_json(command, cli.expected_revision);
    }
    let expected = cli.expected_revision;
    let command = match command {
        CcAction::App { command } => {
            if let Err(e) = crate::desktop::preflight() {
                return operational_error(e);
            }
            return match Registry::locate().and_then(|r| {
                let r = r.expecting(expected);
                match command {
                    AppCommand::Bind {
                        provider,
                        path,
                        recorded_version,
                        replace,
                    } => r
                        .bind_desktop(provider, path, recorded_version, replace)
                        .map(|_| "Desktop binding saved.\n".to_owned()),
                    AppCommand::Open { name } => r
                        .open_desktop(&name, false)
                        .map(|receipt| format!("Desktop {}.\n", receipt.outcome)),
                }
            }) {
                Ok(output) => write_output(
                    output.as_bytes(),
                    &mut io::stdout().lock(),
                    &mut io::stderr().lock(),
                ),
                Err(e) => operational_error(e),
            };
        }
        CcAction::Skills { command } => {
            return match Registry::locate()
                .and_then(|r| r.expecting(expected).skills(command.into_action()))
            {
                Ok(data) => write_output(
                    render::skills(&data).as_bytes(),
                    &mut io::stdout().lock(),
                    &mut io::stderr().lock(),
                ),
                Err(error) => operational_error(error),
            };
        }
        CcAction::Mcp { command } => {
            return match Registry::locate()
                .and_then(|r| r.expecting(expected).mcp(command.into_action()))
            {
                Ok(data) => write_output(
                    crate::share::mcp::cli::human(&data).as_bytes(),
                    &mut io::stdout().lock(),
                    &mut io::stderr().lock(),
                ),
                Err(error) => operational_error(error),
            };
        }
        CcAction::Run { name, args } => return operation(Operation::Run, name, args, false),
        CcAction::Login { name, yes } => return operation(Operation::Login, name, vec![], yes),
        CcAction::Status { name } => return operation(Operation::Status, name, vec![], true),
        CcAction::Logout { name, yes } => return operation(Operation::Logout, name, vec![], yes),
        CcAction::Provider { command } => {
            return match Registry::locate().and_then(|r| match command {
                ProviderAction::List => r.expecting(expected).bindings().map(render::bindings),
                ProviderAction::Bind {
                    provider,
                    executable,
                    replace,
                } => r
                    .expecting(expected)
                    .bind(provider, executable.clone(), replace)
                    .map(|outcome| render::bound(provider, &executable, outcome)),
            }) {
                Ok(output) => write_output(
                    output.as_bytes(),
                    &mut io::stdout().lock(),
                    &mut io::stderr().lock(),
                ),
                Err(error) => operational_error(error),
            };
        }
        CcAction::Profile { command } => command,
    };
    let registry = match Registry::locate() {
        Ok(registry) => registry.expecting(expected),
        Err(error) => return operational_error(error),
    };
    // Confirmation is read-only, including when the selected profile is missing.
    if let ProfileAction::Forget { name, yes: false } = &command {
        return match registry.get(name) {
            Ok(entry) => {
                let _ = writeln!(
                    io::stderr().lock(),
                    "error: refusing to forget profile {name} without --yes\nhint: the registry entry would be removed; the state directory {} is kept and this version cannot attach it to a new profile",
                    crate::registry::display::path(&entry.state_dir)
                );
                ExitCode::from(2)
            }
            Err(error) => operational_error(error),
        };
    }
    match execute(command, &registry) {
        Ok(output) => write_output(
            output.as_bytes(),
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        ),
        Err(error) => operational_error(error),
    }
}

// Clap echoes user values independently of the domain parser's escaped message.
fn escape_error_values(error: &mut clap::Error) {
    use clap::error::ContextValue;
    let replacements: Vec<_> = error
        .context()
        .filter_map(|(kind, value)| {
            let escaped = match value {
                ContextValue::String(value) => {
                    ContextValue::String(value.chars().flat_map(char::escape_debug).collect())
                }
                ContextValue::Strings(values) => ContextValue::Strings(
                    values
                        .iter()
                        .map(|v| v.chars().flat_map(char::escape_debug).collect())
                        .collect(),
                ),
                _ => return None,
            };
            Some((kind, escaped))
        })
        .collect();
    for (kind, value) in replacements {
        error.insert(kind, value);
    }
}

fn execute(command: ProfileAction, registry: &Registry) -> Result<String, Error> {
    Ok(match command {
        ProfileAction::Surface {
            name,
            surface,
            enabled,
        } => {
            let (entry, _) = registry.surface(
                name,
                if surface == "cli" {
                    crate::registry::Surface::Cli
                } else {
                    crate::registry::Surface::Desktop
                },
                enabled == "on",
            )?;
            render::show(&entry)
        }
        ProfileAction::Add { name, provider } => render::added(&registry.add(name, provider)?),
        ProfileAction::List => render::list(registry.list()?),
        ProfileAction::Show { name } => render::show(&registry.get(&name)?),
        ProfileAction::Rename { name, new_name } => {
            render::renamed(&name, &registry.rename(&name, new_name)?)
        }
        ProfileAction::Forget { name, .. } => render::forgotten(&registry.forget(&name)?),
    })
}

fn operational_error(error: Error) -> ExitCode {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(
        stderr,
        "error: {}",
        crate::registry::display::text(&error.to_string())
    );
    if let Some(hint) = error.hint() {
        let _ = writeln!(stderr, "hint: {}", crate::registry::display::text(&hint));
    }
    if matches!(
        error,
        Error::AlreadyBound { .. } | Error::InvalidVersionLabel | Error::McpInvalid
    ) {
        ExitCode::from(2)
    } else {
        ExitCode::FAILURE
    }
}

fn write_output(output: &[u8], stdout: &mut impl Write, stderr: &mut impl Write) -> ExitCode {
    match stdout.write_all(output).and_then(|()| stdout.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error.kind() != io::ErrorKind::BrokenPipe {
                let _ = writeln!(stderr, "error: cannot write output: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn operation(
    op: Operation,
    name: ProfileName,
    args: Vec<std::ffi::OsString>,
    yes: bool,
) -> ExitCode {
    match admission::invoke(op, &name, args, yes, admission::ROWS) {
        Ok(code) => std::process::exit(code as i32),
        Err(Error::ConfirmationRequired) => {
            let _ = operational_error(Error::ConfirmationRequired);
            ExitCode::from(2)
        }
        Err(error) => operational_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stdout_failure_is_reported_without_panicking() {
        struct Fails(io::ErrorKind);
        impl Write for Fails {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(self.0.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(self.0.into())
            }
        }
        for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::StorageFull] {
            let mut stderr = Vec::new();
            assert_eq!(
                write_output(b"output", &mut Fails(kind), &mut stderr),
                ExitCode::FAILURE
            );
            if kind == io::ErrorKind::BrokenPipe {
                assert!(stderr.is_empty());
            } else {
                assert!(
                    String::from_utf8(stderr)
                        .unwrap()
                        .starts_with("error: cannot write output: ")
                );
            }
        }
    }
}
