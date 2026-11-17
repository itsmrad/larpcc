//! In-process invented targets only. Not linked into the ordinary binary.
use super::*;
use crate::platform::FileIdentity;
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Process {
    pub pid: u32,
    pub birth: u64,
    pub executable: FileIdentity,
    pub argv: Vec<OsString>,
}
pub(crate) struct Inspection {
    pub processes: Vec<Process>,
    pub complete: bool,
}
/// Implementations must supply only candidate official executable argv, never env.
pub(crate) trait Runtime {
    fn inspect(&mut self) -> Result<Inspection, InspectionCode>;
    fn recheck(&mut self, process: &Process) -> Result<Process, InspectionCode>;
    fn focus(&mut self, process: &Process) -> Result<(), InspectionCode>;
    fn launch(&mut self, command: &Command) -> Result<(), InspectionCode>;
}
#[derive(Debug)]
pub(crate) struct Command {
    pub executable: &'static str,
    pub argv: Vec<OsString>,
    pub remove_env: Vec<OsString>,
    pub set_env: (&'static str, PathBuf),
}
pub(crate) fn command(
    target: &ValidatedDesktopTarget,
    env_names: &[OsString],
) -> Result<Command, InspectionCode> {
    if env_names.len() > 4096 {
        return Err(InspectionCode::Limit);
    }
    let mut remove_env = Vec::new();
    for name in env_names {
        let name_text = name.to_str().ok_or(InspectionCode::Malformed)?;
        if name_text.starts_with("CLAUDE")
            || name_text.starts_with("ANTHROPIC_")
            || name_text == "CODEX_HOME"
        {
            remove_env.push(name.clone());
        }
    }
    let mut data = OsString::from("--user-data-dir=");
    data.push(&target.snapshot.desktop.path);
    Ok(Command {
        executable: "/usr/bin/open",
        argv: vec![
            "-n".into(),
            "-a".into(),
            target.app.path.clone().into_os_string(),
            "--args".into(),
            data,
        ],
        remove_env,
        set_env: (
            match target.snapshot.provider {
                Provider::Claude => "CLAUDE_CONFIG_DIR",
                Provider::Codex => "CODEX_HOME",
            },
            target.snapshot.cli.path.clone(),
        ),
    })
}
fn matches(process: &Process, target: &ValidatedDesktopTarget) -> Result<bool, InspectionCode> {
    if process.pid == 0 || process.birth == 0 || process.argv.is_empty() {
        return Err(InspectionCode::Malformed);
    }
    if process.argv.len() > 256 {
        return Err(InspectionCode::Limit);
    }
    let mut found = None;
    let mut bytes = 0usize;
    let mut it = process.argv.iter();
    while let Some(arg) = it.next() {
        let arg = arg.to_str().ok_or(InspectionCode::Malformed)?;
        bytes = bytes.checked_add(arg.len()).ok_or(InspectionCode::Limit)?;
        if arg.contains('\0') || bytes > 32768 {
            return Err(InspectionCode::Limit);
        }
        let data = if arg == "--user-data-dir" {
            let next = it
                .next()
                .and_then(|v| v.to_str())
                .ok_or(InspectionCode::Malformed)?;
            bytes = bytes.checked_add(next.len()).ok_or(InspectionCode::Limit)?;
            if next.contains('\0') || bytes > 32768 {
                return Err(InspectionCode::Limit);
            }
            Some(next)
        } else {
            arg.strip_prefix("--user-data-dir=")
        };
        if let Some(data) = data {
            if found.is_some() || data.is_empty() {
                return Err(InspectionCode::Malformed);
            }
            found = Some(data == target.snapshot.desktop.path.as_os_str());
        }
    }
    if process.executable != target.app.executable.identity {
        return Err(InspectionCode::IdentityChanged);
    }
    Ok(found == Some(true))
}
fn inspect(
    runtime: &mut impl Runtime,
    target: &ValidatedDesktopTarget,
    deadline: Instant,
) -> Result<Option<Process>, InspectionCode> {
    let inspection = runtime.inspect()?;
    if Instant::now() >= deadline {
        return Err(InspectionCode::Timeout);
    }
    if !inspection.complete {
        return Err(InspectionCode::Truncated);
    }
    if inspection.processes.len() > 128 {
        return Err(InspectionCode::Limit);
    }
    let mut matched = None;
    for process in inspection.processes {
        if matches(&process, target)? {
            if matched.is_some() {
                return Err(InspectionCode::Malformed);
            }
            matched = Some(process);
        }
    }
    Ok(matched)
}
pub(crate) fn activity(runtime: &mut impl Runtime, target: &ValidatedDesktopTarget) -> Activity {
    if target.recheck().is_err() {
        return Activity::Unknown(InspectionCode::IdentityChanged);
    }
    match inspect(runtime, target, Instant::now() + Duration::from_millis(100)) {
        Ok(Some(_)) => Activity::KnownActive,
        Ok(None) => Activity::Inactive,
        Err(code) => Activity::Unknown(code),
    }
}
/// The harness supplies coherent CORE revalidation and invented process evidence.
/// No marker, CLI argument, environment value, or compiled qualification row unlocks it.
pub(crate) fn dispatch(
    target: &ValidatedDesktopTarget,
    env_names: &[OsString],
    revalidate: impl FnOnce(ProfileId, Provider, u64, Option<&str>) -> Result<(), Error>,
    runtime: &mut impl Runtime,
) -> Result<DesktopReceipt, Error> {
    let deadline = Instant::now() + Duration::from_millis(100);
    target.recheck()?;
    let command = command(target, env_names).map_err(|_| Error::AdmissionChanged)?;
    let process = inspect(runtime, target, deadline).map_err(|_| Error::AdmissionChanged)?;
    let s = &target.snapshot;
    revalidate(
        s.profile_id,
        s.provider,
        s.revision,
        s.recorded_version.as_deref(),
    )?;
    target.recheck()?;
    if Instant::now() >= deadline {
        return Err(Error::AdmissionChanged);
    }
    let outcome = if let Some(process) = process {
        let current = runtime
            .recheck(&process)
            .map_err(|_| Error::AdmissionChanged)?;
        if current != process || !matches(&current, target).map_err(|_| Error::AdmissionChanged)? {
            return Err(Error::AdmissionChanged);
        }
        if Instant::now() >= deadline {
            return Err(Error::AdmissionChanged);
        }
        runtime
            .focus(&current)
            .map_err(|_| Error::AdmissionChanged)?;
        "focused"
    } else {
        runtime
            .launch(&command)
            .map_err(|_| Error::AdmissionChanged)?;
        "launched"
    };
    Ok(DesktopReceipt {
        profile_id: s.profile_id,
        provider: s.provider,
        surface: "desktop",
        outcome,
        native_auth: "not_observed",
    })
}

pub(crate) fn fixture_app(path: PathBuf) -> Result<CheckedApp, Error> {
    let exe = path.join("Contents/MacOS/ChatGPT");
    Ok(CheckedApp {
        path,
        ancestry: platform::anchor(&exe)?,
        executable: platform::inspect(&exe, ObjectKind::File)?.ok_or(Error::ArtifactInvalid)?,
    })
}
pub(crate) fn fixture_directory(path: &Path) -> Result<CheckedDirectory, Error> {
    Ok(CheckedDirectory {
        path: path.into(),
        object: platform::inspect(path, ObjectKind::Directory)?.ok_or(Error::AdmissionChanged)?,
        ancestry: platform::anchor(path)?,
    })
}
impl CheckedApp {
    pub(crate) fn executable_identity(&self) -> FileIdentity {
        self.executable.identity.clone()
    }
}
#[cfg(unix)]
pub(crate) fn singleton_pid(path: &Path, hostname: &[u8]) -> Result<i32, InspectionCode> {
    super::singleton::read(
        &fixture_directory(path).map_err(|_| InspectionCode::IdentityChanged)?,
        hostname,
    )
    .map(|lock| lock.pid)
}
#[cfg(unix)]
pub(crate) fn singleton_parse(target: &[u8], hostname: &[u8]) -> Result<i32, InspectionCode> {
    super::singleton::parse(target, hostname)
}
