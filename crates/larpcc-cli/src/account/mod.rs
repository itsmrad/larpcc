mod capture;
mod parser;

use crate::{
    launch::admission::{self, Admitted, ArtifactRow, Operation},
    registry::{
        Error, ProfileName, Registry,
        usage::data::{clock, graph},
    },
};
use serde_json::{Value, json};
use std::time::Duration;

pub(crate) struct StatusRow {
    artifact: ArtifactRow,
    parser: parser::Parser,
    deadline: Duration,
}

// No native status build has passed qualification.
pub(crate) const ROWS: &[StatusRow] = &[];
const DEADLINE: Duration = Duration::from_secs(10);

enum Account {
    #[cfg_attr(
        not(feature = "synthetic-launch"),
        allow(dead_code, reason = "no qualified row yet")
    )]
    SignedIn {
        email: Option<String>,
        plan: Option<String>,
    },
    #[cfg_attr(
        not(feature = "synthetic-launch"),
        allow(dead_code, reason = "no qualified row yet")
    )]
    SignedOut,
    Unknown,
    NotQualified,
    Unavailable,
}

impl Account {
    fn json(self) -> Value {
        let (state, email, plan) = match self {
            Self::SignedIn { email, plan } => ("signed_in", email, plan),
            Self::SignedOut => ("signed_out", None, None),
            Self::Unknown => ("unknown", None, None),
            Self::NotQualified => ("not_qualified", None, None),
            Self::Unavailable => ("unavailable", None, None),
        };
        json!({"state": state, "email": email, "plan": plan})
    }

    fn human(&self) -> String {
        match self {
            Self::SignedIn { email, plan } => {
                let mut line = String::from("Signed in");
                if let Some(email) = email {
                    line.push_str(" as ");
                    line.push_str(email);
                }
                if let Some(plan) = plan {
                    line.push_str(" · ");
                    line.push_str(plan);
                }
                line
            }
            Self::SignedOut => "Signed out".into(),
            Self::Unknown => "account: unknown".into(),
            Self::NotQualified => "account: not qualified for this provider build".into(),
            Self::Unavailable => "account: unavailable".into(),
        }
    }
}

pub(crate) struct Summary {
    metadata: Value,
    account: Account,
    version: Option<&'static str>,
    usage: Option<Value>,
    read_clock: i64,
}

impl Summary {
    pub(crate) fn json(self) -> Value {
        let mut metadata = self.metadata;
        json!({
            "profile": metadata["profile"].take(),
            "provider_version": self.version,
            "account": self.account.json(),
            "usage": self.usage,
            "diagnostics": metadata["diagnostics"].take(),
        })
    }

    pub(crate) fn human(&self) -> String {
        let profile = &self.metadata["profile"];
        let name = profile["name"].as_str().unwrap_or_default();
        let provider = profile["provider"].as_str().unwrap_or_default();
        let version = self.version.unwrap_or("version unknown");
        format!(
            "{name} ({provider} {version})\n{}\n{}",
            self.account.human(),
            graph::status_graph(self.usage.as_ref(), self.read_clock),
        )
    }
}

pub(crate) fn metadata_human(metadata: &Value) -> String {
    let profile = &metadata["profile"];
    let cli = &profile["surfaces"]["cli"];
    let binding = &metadata["detected_versions"][0]["binding"];
    let mut output = format!(
        "Name:      {}\nProvider:  {}\nID:        {}\nState dir: {}\nState:     {}\nCLI binding: {}\n",
        profile["name"].as_str().unwrap_or_default(),
        profile["provider"].as_str().unwrap_or_default(),
        profile["id"].as_str().unwrap_or_default(),
        cli["path"].as_str().unwrap_or_default(),
        cli["state"].as_str().unwrap_or_default(),
        binding["state"].as_str().unwrap_or_default(),
    );
    if let Some(path) = binding["path"].as_str() {
        output.push_str("Executable: ");
        output.push_str(path);
        output.push('\n');
    }
    output
}

enum Prepared<'a> {
    Observe(Box<Admitted<'a>>, &'a StatusRow),
    Complete(Account),
}

fn prepare<'a>(
    registry: &Registry,
    name: &ProfileName,
    metadata: &Value,
    rows: &'a [StatusRow],
    artifacts: &'a [ArtifactRow],
) -> Result<Prepared<'a>, Error> {
    let complete = |state| Ok(Prepared::Complete(state));
    if !artifacts
        .iter()
        .any(|row| row.operation == Operation::Status)
    {
        return complete(Account::NotQualified);
    }
    let cli = &metadata["profile"]["surfaces"]["cli"];
    if cli["enabled"] != true
        || cli["state"] != "ok"
        || metadata["profile"]["state"] != "active"
        || metadata["detected_versions"][0]["binding"]["state"] == "unbound"
    {
        return complete(Account::Unavailable);
    }
    let keys = std::env::vars_os().map(|(key, _)| key).collect::<Vec<_>>();
    let admitted =
        match admission::admit(registry, Operation::Status, name, vec![], artifacts, &keys) {
            Ok(admitted) => admitted,
            Err(
                Error::QualificationPending
                | Error::ArtifactInvalid
                | Error::InvalidExecutable { .. },
            ) => {
                // Stage 08 has no approved version probe descriptor.
                return complete(Account::NotQualified);
            }
            Err(Error::Unbound { .. } | Error::HomeUnavailable { .. } | Error::Unsafe { .. }) => {
                return complete(Account::Unavailable);
            }
            Err(error) => return Err(error),
        };
    let mut matching = rows.iter().filter(|row| {
        row.artifact.operation == admitted.row.operation
            && row.artifact.provider == admitted.row.provider
            && row.artifact.os == admitted.row.os
            && row.artifact.arch == admitted.row.arch
            && row.artifact.sha256 == admitted.row.sha256
    });
    let Some(row) = matching.next() else {
        return complete(Account::NotQualified);
    };
    if matching.next().is_some()
        || crate::registry::parse_recorded_version(row.artifact.release).is_err()
    {
        return complete(Account::NotQualified);
    }
    Ok(Prepared::Observe(Box::new(admitted), row))
}

pub(crate) fn status(
    registry: &Registry,
    name: &ProfileName,
    expected_id: Option<u64>,
    json: bool,
    rows: &[StatusRow],
) -> Result<Summary, Error> {
    let read_clock = clock::now();
    let artifacts: Vec<_> = rows.iter().map(|row| row.artifact.clone()).collect();
    let (metadata, (prepared, usage)) =
        registry.with_status_profile_metadata(name, expected_id, |metadata, profile| {
            let prepared = prepare(registry, name, metadata, rows, &artifacts)?;
            let usage = registry.local_usage_row(profile, read_clock).ok().flatten();
            Ok((prepared, usage))
        })?;
    finish(metadata, prepared, json, usage, read_clock)
}

fn finish(
    metadata: Value,
    prepared: Prepared<'_>,
    json: bool,
    usage: Option<Value>,
    read_clock: i64,
) -> Result<Summary, Error> {
    let (admitted, row) = match prepared {
        Prepared::Observe(admitted, row) => (admitted, row),
        Prepared::Complete(account) => {
            return Ok(Summary {
                metadata,
                account,
                version: None,
                usage,
                read_clock,
            });
        }
    };
    let id = admitted.snapshot.target.profile.id;
    let guard = admission::acquire_auth_guard(&admitted.root, id).map_err(|error| match error {
        Error::AdmissionChanged => Error::Conflict,
        error => error,
    })?;
    let revalidate = || {
        admission::revalidate_guard(&admitted.root, id, &guard).map_err(|_| Error::Conflict)?;
        admission::revalidate(&admitted).map_err(|_| Error::Conflict)
    };
    if !json {
        eprintln!("{}", admission::ROUTING_NOTICE);
        eprintln!("{}", admission::AUTH_NOTICE);
    }
    let captured = capture::run(&admitted.plan, DEADLINE.min(row.deadline), revalidate);
    let version = Some(admitted.row.release);
    let account = match captured {
        Ok(capture::Captured::Cancelled(signal)) => {
            drop(guard);
            drop(admitted);
            capture::propagate(signal)?;
            return Err(Error::Conflict);
        }
        captured => {
            revalidate()?;
            match captured? {
                capture::Captured::Complete(exit, stdout, stderr) => {
                    (row.parser)(exit.code(), &stdout, &stderr)
                }
                capture::Captured::Incomplete => Account::Unknown,
                capture::Captured::Cancelled(_) => unreachable!(),
            }
        }
    };
    Ok(Summary {
        metadata,
        account,
        version,
        usage,
        read_clock,
    })
}

#[cfg(feature = "synthetic-launch")]
#[allow(
    dead_code,
    reason = "only the unpackaged synthetic driver injects a row"
)]
pub(crate) fn synthetic_row(sha256: [u8; 32]) -> StatusRow {
    use crate::registry::Provider;

    StatusRow {
        artifact: ArtifactRow {
            provider: Provider::Codex,
            release: "0.0.14-fixture",
            sha256,
            install_kind: "native-fixture",
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            operation: Operation::Status,
            interface: "invented-status-v1",
            backend: "no-auth-fixture",
            policy_revision: 1,
            argv: &["login", "status"],
            denied_env: &[],
            paths: &[],
            evidence: "test only",
        },
        parser: parser::invented,
        deadline: Duration::from_secs(4),
    }
}
