mod render;

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
    #[command(subcommand)]
    command: RootAction,
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
enum ProfileAction {
    /// Register a profile and create its empty state directory
    Add {
        /// Profile name: 1-32 characters from a-z, 0-9, '-', '_', starting with a letter
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
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(mut error) => {
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
    let RootAction::Cc {
        command: CcAction::Profile { command },
    } = cli.command;
    let registry = match Registry::locate() {
        Ok(registry) => registry,
        Err(error) => return operational_error(error),
    };
    // Confirmation is read-only, including when the selected profile is missing.
    if let ProfileAction::Forget { name, yes: false } = &command {
        return match registry.get(name) {
            Ok(entry) => {
                let _ = writeln!(
                    io::stderr().lock(),
                    "error: refusing to forget profile {name} without --yes\nhint: the registry entry would be removed; the state directory {} is kept and this version cannot attach it to a new profile",
                    entry.state_dir.display()
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
    let _ = writeln!(stderr, "error: {error}");
    if let Some(hint) = error.hint() {
        let _ = writeln!(stderr, "hint: {hint}");
    }
    ExitCode::FAILURE
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
