mod address;
mod cancel;
mod catalog_cli;
mod host_cli;
pub(crate) mod mcp_frame;
mod protocol;
pub(crate) mod skill_frame;
mod transport;

pub(crate) use address::{Address, Host};
pub(crate) use cancel::Guard as CancellationGuard;
pub(crate) use catalog_cli::Outcome as CatalogOutcome;
pub(crate) use host_cli::HostCommand;
pub(crate) use protocol::Error;

use crate::registry::{self, hosts::Route};
use serde_json::Value;
use std::path::Path;
use transport::{Client, Output};

pub(crate) fn valid_word(value: &str) -> bool {
    address::word(value)
}

#[derive(Debug)]
pub(crate) enum DispatchError {
    Local(registry::Error),
    Remote(Error),
}

impl From<registry::Error> for DispatchError {
    fn from(error: registry::Error) -> Self {
        Self::Local(error)
    }
}

impl From<Error> for DispatchError {
    fn from(error: Error) -> Self {
        Self::Remote(error)
    }
}

const USAGE: Error = Error {
    code: "usage",
    message: "The remote request exceeds the supported limits or contains an invalid value.",
    exit: 2,
};

pub(crate) struct Request {
    args: Vec<String>,
    script: Vec<u8>,
}

impl Request {
    pub(crate) fn new(host: &Host, words: &[String], import: bool) -> Result<Self, Error> {
        Ok(Self {
            args: host.ssh_args().ok_or(USAGE)?,
            script: host.script(words, import).ok_or(USAGE)?,
        })
    }
}

pub(crate) struct Session<'a> {
    root: &'a Path,
    client: Option<Client>,
}

impl<'a> Session<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self { root, client: None }
    }

    fn spawn(
        &mut self,
        route: &Route,
        args: &[String],
    ) -> Result<std::process::Child, DispatchError> {
        if cancel::requested() {
            return Err(Error::transport(transport::Failure::Cancelled, false).into());
        }
        let spawn_version = |command: &mut std::process::Command| {
            route
                .through_spawn(self.root, || {
                    if cancel::requested() {
                        return Err(transport::Failure::Cancelled);
                    }
                    command.spawn().map_err(|_| transport::Failure::Unavailable)
                })?
                .map_err(|error| Error::transport(error, false).into())
        };
        let client = match &mut self.client {
            Some(client) => {
                client.refresh(spawn_version)?;
                client
            }
            None => self.client.insert(Client::discover(spawn_version)?),
        };
        route
            .through_spawn(self.root, || {
                if cancel::requested() {
                    return Err(transport::Failure::Cancelled);
                }
                client.spawn(&route.host, args)
            })?
            .map_err(|e| Error::transport(e, false).into())
    }

    fn probe(&mut self, route: &Route) -> Result<(), DispatchError> {
        let request = Request::new(&route.host, &["--json".into(), "--version".into()], false)?;
        let child = self.spawn(route, &request.args)?;
        let output = transport::collect(
            child,
            request.script,
            None,
            transport::OUTPUT_LIMIT,
            transport::READ,
        )
        .map_err(|e| Error::transport(e, false))?;
        protocol::probe(output)?;
        Ok(())
    }

    pub(crate) fn check(&mut self, route: &Route) -> Result<&'static str, registry::Error> {
        match self.probe(route) {
            Ok(()) => Ok("ok"),
            Err(DispatchError::Remote(error)) => Ok(error.check_state()),
            Err(DispatchError::Local(error)) => Err(error),
        }
    }

    pub(crate) fn json(
        &mut self,
        route: &Route,
        request: Request,
        command: &str,
        mutation: bool,
        payload: Option<Vec<u8>>,
    ) -> Result<Value, DispatchError> {
        let deadline = if mutation {
            transport::MUTATION
        } else {
            transport::READ
        };
        self.probe(route)?;
        let child = self.spawn(route, &request.args)?;
        let output = transport::collect(
            child,
            request.script,
            payload,
            transport::OUTPUT_LIMIT,
            deadline,
        )
        .map_err(|e| Error::transport(e, mutation))?;
        Ok(protocol::reply(output, command, mutation)?)
    }

    pub(crate) fn export(
        &mut self,
        route: &Route,
        request: Request,
        limit: usize,
    ) -> Result<Vec<u8>, DispatchError> {
        self.probe(route)?;
        let child = self.spawn(route, &request.args)?;
        let output = transport::collect(child, request.script, None, limit, transport::MUTATION)
            .map_err(|e| Error::transport(e, false))?;
        export_reply(output).map_err(Into::into)
    }
}

fn export_reply(output: Output) -> Result<Vec<u8>, Error> {
    let code = protocol::transport_exit(&output)?;
    if code == 0 && output.sent {
        return Ok(output.stdout);
    }
    if output.stdout.len() > transport::OUTPUT_LIMIT {
        return Err(protocol::OUTPUT);
    }
    Err(protocol::object_line(&output.stdout)
        .and_then(|value| protocol::confirmed_error(&value, "host.catalog.export", code))
        .unwrap_or(protocol::OUTPUT))
}
