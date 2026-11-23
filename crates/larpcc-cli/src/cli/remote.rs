use super::{
    AliasCommand, CcAction, ProfileAction, SkillsCommand, json, json_error, json_or_usage,
    json_success, write_output,
};
use crate::{
    registry::{self, ProfileName, Registry, hosts::Route},
    remote::{
        CancellationGuard, CatalogOutcome, DispatchError, Error, HostCommand, Request, Session,
    },
    share::{mcp, mcp::cli::McpCommand},
};
use serde_json::Value;
use std::{
    io::{self, Write},
    process::ExitCode,
};

const UNSUPPORTED: Error = Error {
    code: "unsupported",
    message: "This command cannot be routed to a remote machine.",
    exit: 2,
};
const LOCAL_ONLY: Error = Error {
    code: "unsupported",
    message: "A remote receiver request must use JSON and local routing.",
    exit: 2,
};
const ARGUMENT: Error = Error {
    code: "usage",
    message: "The remote command requires a valid absolute path and explicit arguments.",
    exit: 2,
};

pub(super) fn routed(
    action: CcAction,
    host: Option<ProfileName>,
    expected: Option<u64>,
    host_revision: Option<u64>,
    receiver: bool,
    is_json: bool,
) -> ExitCode {
    let command = json::command_name(&action);
    if is_json
        && matches!(
            &action,
            CcAction::Run { .. } | CcAction::Login { .. } | CcAction::Logout { .. }
        )
    {
        return json_or_usage(
            true,
            command,
            "json_native_io",
            "Native operations do not support JSON output.",
        );
    }
    if receiver && (!is_json || host.is_some() || host_revision.is_some()) {
        return emit_failure(command, LOCAL_ONLY.into(), is_json);
    }
    if host.is_some() && matches!(action, CcAction::Run { .. }) {
        return emit_failure(
            command,
            registry::Error::QualificationPending.into(),
            is_json,
        );
    }
    let _cancellation =
        if !receiver && (host.is_some() || matches!(command, "host.check" | "host.catalog.copy")) {
            match CancellationGuard::install() {
                Ok(guard) => Some(guard),
                Err(_) => {
                    return emit_failure(
                        command,
                        Error {
                            code: "ssh_unavailable",
                            message: "Could not initialize SSH cancellation handling.",
                            exit: 1,
                        }
                        .into(),
                        is_json,
                    );
                }
            }
        } else {
            None
        };
    let result = if let Some(host) = host {
        forward(&action, command, &host, expected, host_revision, is_json).map(Reply::Forwarded)
    } else {
        match action {
            CcAction::Host {
                command: HostCommand::Catalog { command },
            } => {
                let copy = command.command() == "host.catalog.copy";
                if host_revision.is_some() && !copy {
                    Err(LOCAL_ONLY.into())
                } else {
                    command.execute(expected, host_revision, receiver).map(
                        |outcome| match outcome {
                            CatalogOutcome::Data(revision, data) => {
                                Reply::Local(Some(revision), data)
                            }
                            CatalogOutcome::Frame(frame) => Reply::Frame(frame),
                        },
                    )
                }
            }
            CcAction::Host { command } if !receiver && host_revision.is_none() => command
                .execute(expected)
                .map(|(revision, data)| Reply::Local(Some(revision), data)),
            CcAction::Doctor {
                name,
                detect_versions: false,
            } if host_revision.is_none() => (|| {
                let registry = Registry::locate()?.expecting(expected);
                let data = registry.metadata_json("doctor", name.as_ref())?;
                Ok(Reply::Local(registry.revision(), data))
            })(),
            action if receiver => match metadata_words(&action, true) {
                Ok(_) => return json::execute_json(action, expected),
                Err(error) => Err(error),
            },
            _ => Err(UNSUPPORTED.into()),
        }
    };
    match result {
        Ok(reply) => emit_reply(command, reply, is_json),
        Err(error) => emit_failure(command, error, is_json),
    }
}

fn forward(
    action: &CcAction,
    command: &str,
    host: &ProfileName,
    expected: Option<u64>,
    host_revision: Option<u64>,
    is_json: bool,
) -> Result<Value, DispatchError> {
    let metadata = match action {
        CcAction::Host {
            command: HostCommand::Catalog { command },
        } if command.public_forwardable() => None,
        _ => Some(metadata_words(action, is_json)?),
    };
    if let Some((words, _)) = &metadata {
        if !words.iter().all(|word| crate::remote::valid_word(word)) {
            return Err(ARGUMENT.into());
        }
    }
    let registry = Registry::locate()?;
    let route = Route::resolve(registry.root(), host_revision, host)?;
    let (request, mutation) = if let Some((mut words, mutation)) = metadata {
        if let Some(expected) = expected {
            words.push(format!("--expected-revision={expected}"));
        }
        (Request::new(&route.host, &words, false)?, mutation)
    } else if let CcAction::Host {
        command: HostCommand::Catalog { command },
    } = action
    {
        command.forward(&route.host, expected)?
    } else {
        return Err(UNSUPPORTED.into());
    };
    Session::new(registry.root()).json(&route, request, command, mutation, None)
}

fn metadata_words(action: &CcAction, is_json: bool) -> Result<(Vec<String>, bool), DispatchError> {
    let mut words = strings(&["--json", "--remote-request", "cc"]);
    let mutation = match action {
        CcAction::Profile { command } => {
            words.push("profile".into());
            match command {
                ProfileAction::List => {
                    words.push("list".into());
                    false
                }
                ProfileAction::Show { name } => {
                    words.extend(strings(&["show", name.as_str()]));
                    false
                }
                ProfileAction::Add { name, provider } => {
                    words.extend(strings(&[
                        "add",
                        name.as_str(),
                        "--provider",
                        provider.as_str(),
                    ]));
                    true
                }
                ProfileAction::Rename { name, new_name } => {
                    words.extend(strings(&["rename", name.as_str(), new_name.as_str()]));
                    true
                }
                ProfileAction::Forget { name, yes: true } => {
                    words.extend(strings(&["forget", name.as_str(), "--yes"]));
                    true
                }
                ProfileAction::Forget { .. } => {
                    return Err(Error {
                        code: "usage",
                        message: "This operation requires --yes.",
                        exit: 2,
                    }
                    .into());
                }
                ProfileAction::Surface {
                    name,
                    surface,
                    enabled,
                } => {
                    words.extend(strings(&["surface", name.as_str(), surface, enabled]));
                    true
                }
            }
        }
        CcAction::Alias { command } => {
            if let Some(message) = command.usage() {
                return Err(Error {
                    code: "usage",
                    message,
                    exit: 2,
                }
                .into());
            }
            words.push("alias".into());
            match command {
                AliasCommand::List => {
                    words.push("list".into());
                    false
                }
                AliasCommand::Add {
                    name,
                    profile,
                    desktop,
                    shadow,
                } => {
                    if *desktop {
                        return Err(UNSUPPORTED.into());
                    }
                    words.extend(strings(&["add", name.as_str(), profile.as_str()]));
                    if *shadow {
                        words.push("--shadow".into());
                    }
                    true
                }
                AliasCommand::Remove { name } => {
                    words.extend(strings(&["remove", name.as_str()]));
                    true
                }
            }
        }
        CcAction::Skills { command } => {
            words.push("skills".into());
            match command {
                SkillsCommand::List | SkillsCommand::Matrix => {
                    words.push(
                        if matches!(command, SkillsCommand::List) {
                            "list"
                        } else {
                            "matrix"
                        }
                        .into(),
                    );
                    false
                }
                SkillsCommand::Add { path, name } => {
                    let path = path
                        .to_str()
                        .filter(|path| path.starts_with('/'))
                        .ok_or(ARGUMENT)?;
                    words.extend(strings(&["add", path, "--name", name.as_str()]));
                    true
                }
                SkillsCommand::Remove { name } => {
                    words.extend(strings(&["remove", name.as_str()]));
                    true
                }
                SkillsCommand::Enable { name, profile }
                | SkillsCommand::Disable { name, profile } => {
                    let mode = if matches!(command, SkillsCommand::Enable { .. }) {
                        "enable"
                    } else {
                        "disable"
                    };
                    words.extend(strings(&[mode, name.as_str(), profile.as_str()]));
                    true
                }
                SkillsCommand::Sync => {
                    words.push("sync".into());
                    true
                }
            }
        }
        CcAction::Mcp { command } => {
            words.push("mcp".into());
            mcp_words(&mut words, command)?
        }
        CcAction::Doctor {
            name,
            detect_versions: false,
        } => {
            words.push("doctor".into());
            if let Some(name) = name {
                words.push(name.to_string());
            }
            false
        }
        CcAction::Status { name } if is_json => {
            words.extend(strings(&["status", name.as_str()]));
            false
        }
        _ => return Err(UNSUPPORTED.into()),
    };
    Ok((words, mutation))
}

fn mcp_words(words: &mut Vec<String>, command: &McpCommand) -> Result<bool, DispatchError> {
    Ok(match command {
        McpCommand::List | McpCommand::Matrix => {
            words.push(
                if matches!(command, McpCommand::List) {
                    "list"
                } else {
                    "matrix"
                }
                .into(),
            );
            false
        }
        McpCommand::Add {
            name,
            command,
            args,
            env_refs,
            url,
            header_refs,
        } => {
            mcp::server_name(name).map_err(registry::Error::from)?;
            let command = command
                .as_ref()
                .map(|path| path.to_str().ok_or(ARGUMENT))
                .transpose()?;
            mcp::transport_for(command, args, env_refs, url.as_deref(), header_refs, false)
                .map_err(registry::Error::from)?;
            words.extend(strings(&["add", name]));
            if let Some(command) = command {
                words.push(format!("--command={command}"));
            }
            if let Some(url) = url {
                words.push(format!("--url={url}"));
            }
            words.extend(args.iter().map(|arg| format!("--arg={arg}")));
            words.extend(env_refs.iter().map(|name| format!("--env-ref={name}")));
            words.extend(
                header_refs
                    .iter()
                    .map(|reference| format!("--header-ref={reference}")),
            );
            true
        }
        McpCommand::Remove { name } => {
            words.extend(strings(&["remove", name.as_str()]));
            true
        }
        McpCommand::Enable { name, profile, all } | McpCommand::Disable { name, profile, all } => {
            let mode = if matches!(command, McpCommand::Enable { .. }) {
                "enable"
            } else {
                "disable"
            };
            words.extend(strings(&[mode, name.as_str()]));
            if *all {
                words.push("--all".into());
            } else if let Some(profile) = profile {
                words.push(profile.to_string());
            }
            true
        }
        McpCommand::SyncAll { mode } => {
            words.extend(strings(&["sync-all", mode]));
            true
        }
        McpCommand::Sync => {
            words.push("sync".into());
            true
        }
    })
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).into()).collect()
}

enum Reply {
    Local(Option<u64>, Value),
    Forwarded(Value),
    Frame(Vec<u8>),
}

fn emit_reply(command: &str, reply: Reply, is_json: bool) -> ExitCode {
    let data = match reply {
        Reply::Local(revision, data) if is_json => return json_success(command, revision, data),
        Reply::Forwarded(value) if is_json => return json::emit(value, 0),
        Reply::Frame(frame) => {
            let mut output = io::stdout().lock();
            return if output
                .write_all(&frame)
                .and_then(|()| output.flush())
                .is_ok()
            {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        Reply::Local(_, data) => data,
        Reply::Forwarded(mut value) => value["data"].take(),
    };
    let data = crate::registry::display::text(&data.to_string());
    write_output(
        format!("{data}\n").as_bytes(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

fn emit_failure(command: &str, error: DispatchError, is_json: bool) -> ExitCode {
    match error {
        DispatchError::Local(error) if is_json => json::failure(command, error),
        DispatchError::Local(error) => {
            let (_, message, exit) = json::failure_details(error);
            let _ = writeln!(io::stderr().lock(), "error: {message}");
            ExitCode::from(exit)
        }
        DispatchError::Remote(error) if is_json => {
            json_error(command, error.code, error.message, error.exit)
        }
        DispatchError::Remote(error) => {
            let _ = writeln!(io::stderr().lock(), "error: {}", error.message);
            ExitCode::from(error.exit)
        }
    }
}
