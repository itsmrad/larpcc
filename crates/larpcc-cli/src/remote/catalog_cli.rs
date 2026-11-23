use super::{
    DispatchError, Error, Request, Session, host_cli::receipt, mcp_frame, protocol, skill_frame,
    transport,
};
use crate::registry::{
    self, ProfileName, Registry, catalog,
    catalog_sharing::Kind,
    hosts::{self, Route},
};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

#[derive(Subcommand)]
pub(crate) enum CatalogCommand {
    /// List catalog entries, copy digests and sharing flags
    List { kind: Kind },
    /// Set the flag used by explicit machine sync
    Share {
        kind: Kind,
        entry: ProfileName,
        #[arg(value_parser = ["on", "off"])]
        mode: String,
    },
    /// Apply a reference-only MCP definition to the selected machine
    Apply {
        #[arg(value_parser = ["mcp"])]
        kind: String,
        #[command(flatten)]
        input: McpInput,
    },
    /// Copy one approved catalog entry to one explicit destination
    Copy {
        kind: Kind,
        entry: ProfileName,
        #[arg(long)]
        from: Destination,
        #[arg(long)]
        to: Destination,
        #[arg(long, value_parser = revision)]
        expected_source_revision: u64,
        #[arg(long, value_parser = parse_digest)]
        sha256: String,
    },
    #[command(hide = true)]
    Export { kind: Kind, entry: ProfileName },
    #[command(hide = true)]
    Import {
        #[arg(value_parser = ["skills"])]
        kind: String,
        entry: ProfileName,
        #[arg(long, value_parser = parse_digest)]
        sha256: String,
    },
}

#[derive(Args)]
#[group(id = "mcp_input", multiple = true)]
pub(crate) struct McpInput {
    entry: ProfileName,
    #[arg(long, required_unless_present = "url", conflicts_with = "url")]
    command: Option<String>,
    #[arg(long = "arg", allow_hyphen_values = true, requires = "command")]
    args: Vec<String>,
    #[arg(long = "env-ref", requires = "command")]
    env_refs: Vec<String>,
    #[arg(long)]
    url: Option<String>,
    #[arg(long = "header-ref", requires = "url")]
    header_refs: Vec<String>,
}

impl McpInput {
    fn transport(&self, windows: bool) -> Result<crate::share::mcp::Transport, registry::Error> {
        Ok(crate::share::mcp::transport_for(
            self.command.as_deref(),
            &self.args,
            &self.env_refs,
            self.url.as_deref(),
            &self.header_refs,
            windows,
        )?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    Local,
    Remote(ProfileName),
}

impl std::str::FromStr for Destination {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "@local" {
            Ok(Self::Local)
        } else {
            ProfileName::parse_new(value)
                .map(Self::Remote)
                .map_err(|_| "invalid machine route")
        }
    }
}

impl Destination {
    fn route(&self, data: &hosts::Hosts) -> Result<Option<Route>, registry::Error> {
        match self {
            Self::Local => Ok(None),
            Self::Remote(name) => Route::from_snapshot(data, name).map(Some),
        }
    }
}

fn revision(value: &str) -> Result<u64, &'static str> {
    protocol::decimal(value).ok_or("expected a canonical decimal u64 revision")
}

fn parse_digest(value: &str) -> Result<String, &'static str> {
    protocol::digest(value)
        .then(|| value.to_owned())
        .ok_or("expected a lowercase SHA-256 digest")
}

const COPY_TIMEOUT: Error = Error {
    code: "remote_timeout",
    message: "Catalog preparation exceeded its deadline before dispatch or commit.",
    exit: 1,
};
const CATALOG_UNAVAILABLE: Error = Error {
    code: "catalog_unavailable",
    message: "The destination cannot use this MCP command or reference.",
    exit: 1,
};
const INVALID_COPY_ROUTE: Error = Error {
    code: "usage",
    message: "Catalog copy requires two different machine routes.",
    exit: 2,
};
const PRIVATE_COMMAND: Error = Error {
    code: "unsupported",
    message: "This operation is restricted to the remote catalog protocol.",
    exit: 2,
};
const REQUIRE_EXPECTED: Error = Error {
    code: "usage",
    message: "This operation requires --expected-revision.",
    exit: 2,
};
const LOCAL_SOURCE: Duration = Duration::from_secs(10);
const LOCAL_DESTINATION: Duration = Duration::from_secs(15);

pub(crate) enum Outcome {
    Data(u64, Value),
    Frame(Vec<u8>),
}

impl CatalogCommand {
    pub(crate) fn command(&self) -> &'static str {
        match self {
            Self::List { .. } => "host.catalog.list",
            Self::Share { .. } => "host.catalog.share",
            Self::Apply { .. } => "host.catalog.apply",
            Self::Copy { .. } => "host.catalog.copy",
            Self::Export { .. } => "host.catalog.export",
            Self::Import { .. } => "host.catalog.import",
        }
    }

    pub(crate) fn forward(
        &self,
        host: &super::Host,
        expected: Option<u64>,
    ) -> Result<(Request, bool), DispatchError> {
        let mut words = catalog_words();
        let mutation = match self {
            Self::List { kind } => {
                words.extend(["list".into(), kind.word().into()]);
                false
            }
            Self::Share { kind, entry, mode } => {
                words.extend([
                    "share".into(),
                    kind.word().into(),
                    entry.to_string(),
                    mode.clone(),
                ]);
                true
            }
            Self::Apply { input, .. } => {
                let transport = input.transport(false)?;
                words = mcp_frame::apply_words(host, &input.entry, &transport, expected)
                    .map_err(registry::Error::from)?;
                return Ok((Request::new(host, &words, false)?, true));
            }
            _ => return Err(PRIVATE_COMMAND.into()),
        };
        if let Some(revision) = expected {
            words.push(format!("--expected-revision={revision}"));
        }
        Ok((Request::new(host, &words, false)?, mutation))
    }

    pub(crate) fn public_forwardable(&self) -> bool {
        matches!(
            self,
            Self::List { .. } | Self::Share { .. } | Self::Apply { .. }
        )
    }

    pub(crate) fn execute(
        self,
        expected: Option<u64>,
        host_revision: Option<u64>,
        receiver: bool,
    ) -> Result<Outcome, DispatchError> {
        let private = matches!(self, Self::Export { .. } | Self::Import { .. });
        if private && !receiver || matches!(self, Self::Copy { .. }) && receiver {
            return Err(PRIVATE_COMMAND.into());
        }
        if private && expected.is_none() {
            return Err(REQUIRE_EXPECTED.into());
        }
        let registry = Registry::locate()?.expecting(expected);
        let (revision, data) = match self {
            Self::List { kind } => catalog::list(&registry, kind)?,
            Self::Share { kind, entry, mode } => {
                let (changed, revision) = catalog::share(&registry, kind, &entry, mode == "on")?;
                (revision, receipt(&entry, changed))
            }
            Self::Apply { input, .. } => {
                let transport = input.transport(cfg!(windows))?;
                let (changed, revision) =
                    catalog::apply_mcp(&registry, &input.entry, transport, None)?;
                (revision, receipt(&input.entry, changed))
            }
            Self::Copy {
                kind,
                entry,
                from,
                to,
                expected_source_revision,
                sha256,
            } => {
                let expected = expected.ok_or(REQUIRE_EXPECTED)?;
                return copy(
                    &registry,
                    Copy {
                        kind,
                        entry,
                        from,
                        to,
                        source_revision: expected_source_revision,
                        destination_revision: expected,
                        host_revision,
                        sha256,
                    },
                );
            }
            Self::Export { kind, entry } => {
                return Ok(Outcome::Frame(catalog::export(&registry, kind, &entry)?.0));
            }
            Self::Import { entry, sha256, .. } => {
                if io::stdin().is_terminal() || io::stdout().is_terminal() {
                    return Err(PRIVATE_COMMAND.into());
                }
                catalog::import_preflight(&registry, &entry, &sha256)?;
                let mut stdout = io::stdout().lock();
                stdout
                    .write_all(transport::READY)
                    .and_then(|()| stdout.flush())
                    .map_err(|source| registry::Error::Io {
                        action: "write import readiness",
                        path: "stdout".into(),
                        source,
                    })?;
                drop(stdout);
                let payload = skill_frame::decode(io::stdin().lock(), &entry, &sha256)
                    .map_err(catalog::frame_error)?;
                let (changed, revision) =
                    catalog::import_skills(&registry, &entry, &sha256, payload, None)?;
                (revision, receipt(&entry, changed))
            }
        };
        Ok(Outcome::Data(revision, data))
    }
}

fn catalog_words() -> Vec<String> {
    ["--json", "--remote-request", "cc", "host", "catalog"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

struct Copy {
    kind: Kind,
    entry: ProfileName,
    from: Destination,
    to: Destination,
    source_revision: u64,
    destination_revision: u64,
    host_revision: Option<u64>,
    sha256: String,
}

fn copy(registry: &Registry, spec: Copy) -> Result<Outcome, DispatchError> {
    if spec.from == spec.to {
        return Err(INVALID_COPY_ROUTE.into());
    }
    let root = registry.root();
    let (source, destination) = hosts::with_snapshot(root, spec.host_revision, |data| {
        Ok((spec.from.route(&data)?, spec.to.route(&data)?))
    })?;
    let mut session = Session::new(root);
    let frame = if let Some(route) = &source {
        let mut words = catalog_words();
        words.extend([
            "export".into(),
            spec.kind.word().into(),
            spec.entry.to_string(),
            format!("--expected-revision={}", spec.source_revision),
        ]);
        let limit = match spec.kind {
            Kind::Mcp => mcp_frame::LIMIT,
            Kind::Skills => skill_frame::LIMIT,
        };
        session.export(route, Request::new(&route.host, &words, false)?, limit)?
    } else {
        let started = Instant::now();
        let source = Registry::at(root.into())?.expecting(Some(spec.source_revision));
        let (frame, digest) = catalog::export(&source, spec.kind, &spec.entry)?;
        if started.elapsed() > LOCAL_SOURCE {
            return Err(COPY_TIMEOUT.into());
        }
        if digest != spec.sha256 {
            return Err(registry::Error::Conflict.into());
        }
        frame
    };
    let deadline = (destination.is_none()).then(|| Instant::now() + LOCAL_DESTINATION);
    let (changed, revision) = match spec.kind {
        Kind::Mcp => {
            let definition = mcp_frame::decode(&frame, &spec.entry, &spec.sha256, source.is_some())
                .map_err(|error| match error {
                    crate::share::mcp::InputError::TooLarge => registry::Error::TooLarge,
                    _ => catalog::frame_error(skill_frame::Error::Corrupt),
                })?;
            if let Some(route) = &destination {
                let words = mcp_frame::apply_words(
                    &route.host,
                    &spec.entry,
                    &definition,
                    Some(spec.destination_revision),
                )
                .map_err(registry::Error::from)?;
                let reply = session
                    .json(
                        route,
                        Request::new(&route.host, &words, false)?,
                        "host.catalog.apply",
                        true,
                        None,
                    )
                    .map_err(map_unavailable)?;
                from_receipt(&reply, &spec.entry)?
            } else {
                catalog::apply_mcp(registry, &spec.entry, definition, deadline)
                    .map_err(|error| map_unavailable(error.into()))?
            }
        }
        Kind::Skills => {
            let files = skill_frame::decode(frame.as_slice(), &spec.entry, &spec.sha256)
                .map_err(catalog::frame_error)?;
            if let Some(route) = &destination {
                drop(files);
                let mut words = catalog_words();
                words.extend([
                    "import".into(),
                    "skills".into(),
                    spec.entry.to_string(),
                    format!("--expected-revision={}", spec.destination_revision),
                    format!("--sha256={}", spec.sha256),
                ]);
                let reply = session.json(
                    route,
                    Request::new(&route.host, &words, true)?,
                    "host.catalog.import",
                    true,
                    Some(frame),
                )?;
                from_receipt(&reply, &spec.entry)?
            } else {
                catalog::import_skills(registry, &spec.entry, &spec.sha256, files, deadline)?
            }
        }
    };
    Ok(Outcome::Data(revision, receipt(&spec.entry, changed)))
}

fn from_receipt(value: &Value, entry: &ProfileName) -> Result<(bool, u64), Error> {
    if value["data"]["name"] != entry.as_str() {
        return Err(protocol::UNKNOWN);
    }
    let changed = value["data"]["changed"]
        .as_bool()
        .ok_or(protocol::UNKNOWN)?;
    let revision = value["revision"]
        .as_str()
        .and_then(protocol::decimal)
        .ok_or(protocol::UNKNOWN)?;
    Ok((changed, revision))
}

fn map_unavailable(error: DispatchError) -> DispatchError {
    match error {
        DispatchError::Remote(e) if matches!(e.code, "not_found" | "secret_reference") => {
            CATALOG_UNAVAILABLE.into()
        }
        DispatchError::Local(
            registry::Error::NotFound { .. } | registry::Error::SecretReference,
        ) => CATALOG_UNAVAILABLE.into(),
        other => other,
    }
}
