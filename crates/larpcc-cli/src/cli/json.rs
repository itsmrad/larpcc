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
fn failure(command: &str, error: Error) -> ExitCode {
    let (code, message, exit) = match error {
        Error::Drift => (
            "drift",
            "Owned content or filesystem identity has changed.",
            1,
        ),
        Error::SkillNotFound => ("not_found", "Skill was not found.", 1),
        Error::AliasNotFound => ("not_found", "Alias was not found.", 1),
        Error::AliasInvalid => (
            "drift",
            "Alias launcher, receipt or mapping cannot be verified.",
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
        Error::Exhausted => ("exhausted", "Registry counters are exhausted.", 1),
        _ => (
            "unsafe_path",
            "The requested local operation could not be completed safely.",
            1,
        ),
    };
    json_error(command, code, message, exit)
}
fn receipt(entry: &crate::registry::ProfileEntry) -> Value {
    json!({"changed":true,"profile_id":entry.profile.id.to_string(),"name":entry.profile.name,"warnings":[]})
}
pub(super) fn execute_json(action: CcAction, expected: Option<u64>) -> ExitCode {
    let command = match &action {
        CcAction::App {
            command: AppCommand::Bind { .. },
        } => "app.bind",
        CcAction::App { .. } => "app.open",
        CcAction::Skills { command } => command.action().command(),
        CcAction::Alias { command } => command.command(),
        CcAction::Run { .. } => "run",
        CcAction::Login { .. } => "login",
        CcAction::Logout { .. } => "logout",
        CcAction::Status { .. } => "status",
        CcAction::Provider {
            command: ProviderAction::List,
        } => "provider.list",
        CcAction::Provider { .. } => "provider.bind",
        CcAction::Profile { command } => match command {
            ProfileAction::Add { .. } => "profile.add",
            ProfileAction::List => "profile.list",
            ProfileAction::Show { .. } => "profile.show",
            ProfileAction::Rename { .. } => "profile.rename",
            ProfileAction::Forget { .. } => "profile.forget",
            ProfileAction::Surface { .. } => "profile.surface",
        },
    };
    if matches!(
        action,
        CcAction::Run { .. } | CcAction::Login { .. } | CcAction::Logout { .. }
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
    let r = match Registry::locate() {
        Ok(r) => r.expecting(expected),
        Err(e) => return failure(command, e),
    };
    let result = match action {
        CcAction::App {command:AppCommand::Bind {provider,path,recorded_version,replace}} => r.bind_desktop(provider,path,recorded_version,replace).map(|changed| json!({"changed":changed,"profile_id":null,"name":null,"warnings":[]})),
        CcAction::App {command:AppCommand::Open {name}} => r.open_desktop(&name,true).map(|receipt| json!({"profile_id":receipt.profile_id.to_string(),"provider":receipt.provider,"surface":receipt.surface,"outcome":receipt.outcome,"native_auth":receipt.native_auth})),
        CcAction::Skills {command} => r.skills(command.into_action()),
        CcAction::Alias {command} => command.execute(&r),
        CcAction::Status {name} => r.metadata_json(command,Some(&name)),
        CcAction::Provider {command:ProviderAction::List} => r.metadata_json(command,None),
        CcAction::Provider {command:ProviderAction::Bind {provider,executable,replace}} => r.bind(provider,executable,replace).map(|outcome| json!({"changed":outcome != crate::registry::BindOutcome::Unchanged,"profile_id":null,"name":null,"warnings":[]})),
        CcAction::Profile {command: action} => match action {
            ProfileAction::List => r.metadata_json(command,None),
            ProfileAction::Show {name} => r.metadata_json(command,Some(&name)),
            ProfileAction::Add {name,provider} => r.add(name,provider).map(|e| receipt(&e)),
            ProfileAction::Rename {name,new_name} => r.rename(&name,new_name).map(|e| receipt(&e)),
            ProfileAction::Forget {name,..} => r.forget(&name).map(|e| receipt(&e)),
            ProfileAction::Surface {name,surface,enabled} => r.surface(name,if surface=="cli" {crate::registry::Surface::Cli}else{crate::registry::Surface::Desktop},enabled=="on").map(|(e,changed)| { let mut data=receipt(&e); data["changed"]=json!(changed); data }),
        },
        _ => unreachable!("native operations refused above"),
    };
    match result {
        Ok(data) => json_success(command, r.revision(), data),
        Err(e) => failure(command, e),
    }
}
