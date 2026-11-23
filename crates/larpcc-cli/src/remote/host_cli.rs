use super::{Address, DispatchError, Error, Host, Session};
use crate::registry::{ProfileName, Registry, hosts};
use clap::Subcommand;
use serde_json::{Value, json};

#[derive(Subcommand)]
pub(crate) enum HostCommand {
    /// Add an SSH route to the local host store
    #[command(group = clap::ArgGroup::new("address").required(true).args(["ssh", "ssh_config_host"]))]
    Add {
        #[arg(value_parser = ProfileName::parse_new)]
        name: ProfileName,
        #[arg(long, value_name = "USER@HOST[:PORT]")]
        ssh: Option<String>,
        #[arg(long, value_name = "ALIAS")]
        ssh_config_host: Option<String>,
        #[arg(long, value_name = "REMOTE_PATH")]
        larp: Option<String>,
        #[arg(long, value_name = "REMOTE_PATH")]
        larpcc_home: Option<String>,
        #[arg(long, requires = "ssh", value_name = "LOCAL_PATH")]
        identity_file: Option<String>,
    },
    /// List locally configured SSH routes
    List,
    /// Show a locally configured SSH route
    Show { name: ProfileName },
    /// Remove the local route record
    Remove {
        name: ProfileName,
        #[arg(long)]
        yes: bool,
    },
    /// Explicitly check the remote Larp version
    Check { name: ProfileName },
    /// List, share or copy catalog entries between machines
    Catalog {
        #[command(subcommand)]
        command: super::catalog_cli::CatalogCommand,
    },
}

impl HostCommand {
    pub(crate) fn command(&self) -> &'static str {
        match self {
            Self::Add { .. } => "host.add",
            Self::List => "host.list",
            Self::Show { .. } => "host.show",
            Self::Remove { .. } => "host.remove",
            Self::Check { .. } => "host.check",
            Self::Catalog { command } => command.command(),
        }
    }

    pub(crate) fn execute(self, expected: Option<u64>) -> Result<(u64, Value), DispatchError> {
        let registry = Registry::locate()?;
        let root = registry.root();
        Ok(match self {
            Self::Add {
                name,
                ssh,
                ssh_config_host,
                larp,
                larpcc_home,
                identity_file,
            } => {
                let ssh = match (ssh, ssh_config_host) {
                    (Some(value), None) => Address::direct(value),
                    (None, Some(value)) => Address::config(value),
                    _ => None,
                }
                .ok_or(INVALID_ADDRESS)?;
                let host = Host {
                    name: name.clone(),
                    ssh,
                    larp,
                    larpcc_home,
                    identity_file,
                };
                if !host.valid() {
                    return Err(INVALID_ADDRESS.into());
                }
                (hosts::add(root, expected, host)?, receipt(&name, true))
            }
            Self::List => hosts::with_snapshot(root, expected, |data| {
                Ok((data.revision(), json!({"hosts":data.hosts})))
            })?,
            Self::Show { name } => hosts::with_snapshot(root, expected, |data| {
                Ok((data.revision(), json!({"host":data.get(&name)?})))
            })?,
            Self::Remove { name, yes: true } => {
                (hosts::remove(root, expected, &name)?, receipt(&name, true))
            }
            Self::Remove { yes: false, .. } => {
                return Err(Error {
                    code: "usage",
                    message: "This operation requires --yes.",
                    exit: 2,
                }
                .into());
            }
            Self::Check { name } => {
                let route = hosts::Route::resolve(root, expected, &name)?;
                let status = Session::new(root).check(&route)?;
                (route.revision, json!({"name":name,"status":status}))
            }
            Self::Catalog { .. } => unreachable!("catalog operations are dispatched separately"),
        })
    }
}

const INVALID_ADDRESS: Error = Error {
    code: "usage",
    message: "Invalid SSH addressing mode, destination or path.",
    exit: 2,
};

pub(super) fn receipt(name: &ProfileName, changed: bool) -> Value {
    json!({"name":name,"profile_id":null,"changed":changed,"warnings":[]})
}
