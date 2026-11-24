use super::*;
use serde_json::{Value, json};

fn emit(value: Value, code: u8) -> ExitCode {
    let mut bytes = match serde_json::to_vec(&value) {
        Ok(b) => b,
        Err(_) => return ExitCode::FAILURE,
    };
    bytes.push(b'\n');
    if bytes.len() > 1_048_576 {
        return json_error(
            value["command"].as_str().unwrap_or("parse"),
            "too_large",
            "Output exceeds the supported size.",
            1,
        );
    }
    if io::stdout()
        .lock()
        .write_all(&bytes)
        .and_then(|()| io::stdout().lock().flush())
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::from(code)
}
pub(super) fn json_success(command: &str, revision: Option<u64>, data: Value) -> ExitCode {
    emit(
        json!({"schema":1,"command":command,"ok":true,"revision":revision.map(|r| r.to_string()),"data":data}),
        0,
    )
}
pub(super) fn json_error(command: &str, code: &str, message: &str, exit: u8) -> ExitCode {
    emit(
        json!({"schema":1,"command":command,"ok":false,"error":{"code":code,"message":message}}),
        exit,
    )
}
pub(super) fn json_or_usage(is_json: bool, command: &str, code: &str, message: &str) -> ExitCode {
    if is_json {
        json_error(command, code, message, 2)
    } else {
        let _ = writeln!(io::stderr().lock(), "error: {message}");
        ExitCode::from(2)
    }
}
fn failure_details(error: Error) -> (&'static str, &'static str, u8) {
    match error {
        Error::UsagePlanLabel => ("usage", "Provide a valid plan label or --clear.", 2),
        Error::UsageMode => ("usage", "Inherit requires --profile.", 2),
        Error::UsageInterval => (
            "usage",
            "Refresh interval must be off or 300–86400 seconds.",
            2,
        ),
        Error::UsageUnsupported => ("unsupported", "Only Claude supports statusLine capture.", 1),
        Error::UsageRemoteUnsupported => (
            "unsupported",
            "This usage operation is unavailable on remote requests.",
            2,
        ),
        Error::UsageConfirmation => (
            "confirmation_required",
            "Enabling LIVE usage requires --acknowledge-risk.",
            2,
        ),
        Error::StatuslineExists => (
            "statusline_exists",
            "This profile already has its own statusLine; LarpCC will not replace it.",
            1,
        ),
        Error::StatuslineUnverifiable => (
            "statusline_unverifiable",
            "LarpCC could not confirm that this profile has no statusLine.",
            1,
        ),
        Error::LiveDisabled => (
            "live_disabled",
            "LIVE usage is off; enable it in settings first.",
            1,
        ),
        Error::InvalidVersionLabel => {
            ("usage", "Recorded version label is invalid or too long.", 2)
        }
        Error::ForbiddenSource => (
            "forbidden_source",
            "Skill sources inside managed or vendor homes are forbidden.",
            1,
        ),
        Error::Io { .. } => ("io", "Local filesystem I/O could not be completed.", 1),
        Error::Drift => (
            "drift",
            "Owned content or filesystem identity has changed.",
            1,
        ),
        Error::SkillNotFound => ("not_found", "Skill was not found.", 1),
        Error::McpNotFound => ("not_found", "MCP server was not found.", 1),
        Error::McpInvalid => ("usage", "MCP definition input is invalid.", 2),
        Error::SecretLiteral => (
            "secret_literal",
            "Literal values are refused; use references only.",
            1,
        ),
        Error::SecretReference => (
            "secret_reference",
            "A reference is missing, invalid or unsupported.",
            1,
        ),
        Error::Unsupported => (
            "unsupported",
            "This MCP transport or adapter is unsupported.",
            1,
        ),
        Error::AliasNotFound => ("not_found", "Alias was not found.", 1),
        Error::AliasInvalid => (
            "drift",
            "Alias launcher, receipt or mapping cannot be verified.",
            1,
        ),
        Error::AliasImageUnverified => (
            "unsafe_path",
            "Cannot verify the loaded Larp image for alias creation.",
            1,
        ),
        Error::Conflict | Error::NameTaken { .. } => {
            ("conflict", "Registry revision or references conflict.", 1)
        }
        Error::AlreadyBound { .. } => ("conflict", "Rebinding requires --replace.", 2),
        Error::NotFound { .. } => ("not_found", "Profile was not found.", 1),
        Error::RecoveryRequired => (
            "recovery_required",
            "Explicit profile migration or recovery is required.",
            1,
        ),
        Error::QualificationPending => (
            "qualification_pending",
            "Provider operation is disabled for this combination.",
            1,
        ),
        Error::Busy { .. } | Error::AuthBusy => ("busy", "Profile or registry is busy.", 1),
        Error::TooLarge => (
            "too_large",
            "Output or registry exceeds the supported size.",
            1,
        ),
        Error::NewerSchema { .. } => ("newer_schema", "Registry schema is unsupported.", 1),
        Error::Corrupt { .. } => ("corrupt", "Registry metadata is invalid.", 1),
        Error::InvalidExecutable { .. } => ("unsafe_path", "Provider executable is invalid.", 1),
        Error::Unbound { .. } => ("not_found", "Provider is not bound.", 1),
        Error::Exhausted => ("exhausted", "Registry counters are exhausted.", 1),
        _ => (
            "unsafe_path",
            "The requested local operation could not be completed safely.",
            1,
        ),
    }
}
pub(super) fn failure(command: &str, error: Error) -> ExitCode {
    if command == "status.account" && matches!(error, Error::AuthBusy) {
        return json_error(
            command,
            "busy",
            "Profile authentication operation is busy.",
            1,
        );
    }
    let (code, message, exit) = failure_details(error);
    json_error(command, code, message, exit)
}
fn receipt(entry: &crate::registry::ProfileEntry) -> Value {
    json!({"changed":true,"profile_id":entry.profile.id.to_string(),"name":entry.profile.name,"warnings":[]})
}
pub(super) fn execute_json(
    action: CcAction,
    expected: Option<u64>,
    read_clock: i64,
    remote: bool,
) -> ExitCode {
    let command = match &action {
        CcAction::Usage(_) => "usage",
        CcAction::Settings { command } => command.discriminator(),
        CcAction::App {
            command: AppCommand::Bind { .. },
        } => "app.bind",
        CcAction::App { .. } => "app.open",
        CcAction::Skills { command } => command.action().command(),
        CcAction::Mcp { command } => command.command(),
        CcAction::Alias { command } => command.command(),
        CcAction::Run { .. } => "run",
        CcAction::Login { .. } => "login",
        CcAction::Logout { .. } => "logout",
        CcAction::Status { .. } => unreachable!(),
        CcAction::Provider {
            command: ProviderAction::List,
        } => "provider.list",
        CcAction::Provider {
            command: ProviderAction::Evidence { .. },
        } => "provider.evidence",
        CcAction::Doctor { .. } => "doctor",
        CcAction::Provider { .. } => "provider.bind",
        CcAction::Profile { command } => match command {
            ProfileAction::PlanLabel(_) => "profile.plan-label",
            ProfileAction::Add { .. } => "profile.add",
            ProfileAction::List => "profile.list",
            ProfileAction::Show { .. } => "profile.show",
            ProfileAction::Rename { .. } => "profile.rename",
            ProfileAction::Forget { .. } => "profile.forget",
            ProfileAction::Surface { .. } => "profile.surface",
            ProfileAction::Delete { .. } => "profile.delete",
            ProfileAction::Recover { .. } => "profile.recover",
            ProfileAction::Migrate => "profile.migrate",
        },
    };
    if matches!(
        action,
        CcAction::Run { .. }
            | CcAction::Login { .. }
            | CcAction::Logout { .. }
            | CcAction::Profile {
                command: ProfileAction::Delete { logout: true, .. }
            }
    ) {
        return json_error(
            command,
            "json_native_io",
            "Native operations do not support JSON output.",
            2,
        );
    }
    if matches!(
        action,
        CcAction::Profile {
            command: ProfileAction::Forget { yes: false, .. }
                | ProfileAction::Delete { yes: false, .. }
                | ProfileAction::Recover { yes: false, .. }
        }
    ) {
        return json_error(command, "usage", "This operation requires --yes.", 2);
    }
    if let CcAction::Alias { command } = &action
        && let Some(message) = command.usage()
    {
        return json_error(command.command(), "usage", message, 2);
    }
    if matches!(action, CcAction::App { .. })
        && let Err(e) = crate::desktop::preflight()
    {
        return failure(command, e);
    }
    if matches!(
        action,
        CcAction::App {
            command: AppCommand::Open { .. }
        }
    ) && let Err(e) = crate::desktop::open_preflight()
    {
        return failure(command, e);
    }
    let r = match Registry::locate() {
        Ok(r) => r.expecting(expected),
        Err(e) => return failure(command, e),
    };
    let result = match action {
        CcAction::Usage(args) => args.execute(&r, read_clock, remote),
        CcAction::Settings { command } => command.execute(&r, remote),
        CcAction::App {command:AppCommand::Bind {provider,path,recorded_version,replace}} => r.bind_desktop(provider,path,recorded_version,replace).map(|changed| json!({"changed":changed,"profile_id":null,"name":null,"warnings":[]})),
        CcAction::App {command:AppCommand::Open {name}} => r.open_desktop(&name,true).map(|receipt| json!({"profile_id":receipt.profile_id.to_string(),"provider":receipt.provider,"surface":receipt.surface,"outcome":receipt.outcome,"native_auth":receipt.native_auth})),
        CcAction::Skills {command} => r.skills(command.into_action()),
        CcAction::Mcp {command} => r.mcp(command.into_action()),
        CcAction::Alias {command} => command.execute(&r),
        CcAction::Status { .. } => unreachable!(),
        CcAction::Provider {command:ProviderAction::List} => r.metadata_json(command,None),
        CcAction::Provider {command:ProviderAction::Evidence {provider}} => crate::provider::evidence::collect(&r,provider).map(|e| e.json()),
        CcAction::Doctor {profile,detect_versions} => super::doctor::data(&r,profile.as_ref(),detect_versions),
        CcAction::Provider {command:ProviderAction::Bind {provider,executable,replace}} => r.bind(provider,executable,replace).map(|outcome| json!({"changed":outcome != crate::registry::BindOutcome::Unchanged,"profile_id":null,"name":null,"warnings":[]})),
        CcAction::Profile {command: action} => match action {
            ProfileAction::PlanLabel(args) => args.execute(&r),
            ProfileAction::List => r.metadata_json(command,None),
            ProfileAction::Show {name} => r.metadata_json(command,Some(&name)),
            ProfileAction::Add {name,provider} => r.add(name,provider).map(|e| receipt(&e)),
            ProfileAction::Rename {name,new_name} => r.rename(&name,new_name).map(|e| receipt(&e)),
            ProfileAction::Forget {name,..} => r.forget(&name).map(|e| receipt(&e)),
            ProfileAction::Delete {name,..} => r.delete_profile(name.clone(),false).map(|id| json!({"changed":true,"profile_id":id.to_string(),"name":name,"warnings":[{"code":"keychain_retained","message":super::KEYCHAIN_RETAINED}]})),
            ProfileAction::Recover {name,..} => r.delete_profile(name.clone(),true).map(|id| json!({"changed":true,"profile_id":id.to_string(),"name":name,"warnings":[]})),
            ProfileAction::Migrate => r.migrate_profiles().map(|changed| json!({"changed":changed,"profile_id":null,"name":null,"warnings":[]})),
            ProfileAction::Surface {name,surface,enabled} => r.surface(name,if surface=="cli" {crate::registry::Surface::Cli}else{crate::registry::Surface::Desktop},enabled=="on").map(|(e,changed)| { let mut data=receipt(&e); data["changed"]=json!(changed); data }),
        },
        _ => unreachable!("native operations refused above"),
    };
    match result {
        Ok(data) => json_success(command, r.revision(), data),
        Err(e) => failure(command, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage09_app_bind_conflict_exit_and_redacted_messages() {
        let (code, message, exit) = failure_details(Error::AlreadyBound {
            provider: Provider::Codex,
            old: "private-path".into(),
        });
        assert_eq!((code, exit), ("conflict", 2));
        assert!(!message.contains("private-path"));
        assert_eq!(failure_details(Error::Conflict).2, 1);
        assert_eq!(failure_details(Error::InvalidVersionLabel).2, 2);
    }
    #[test]
    fn stage06_descriptor_exhaustion_has_redacted_io_envelope_code() {
        #[cfg(unix)]
        let code = libc::EMFILE;
        #[cfg(windows)]
        let code = 4;
        let (name, message, exit) = failure_details(Error::Io {
            action: "test",
            path: "private-home".into(),
            source: std::io::Error::from_raw_os_error(code),
        });
        assert_eq!((name, exit), ("io", 1));
        assert!(!message.contains("private-home"));
    }
}
