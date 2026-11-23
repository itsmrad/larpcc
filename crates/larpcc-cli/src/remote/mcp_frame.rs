use super::{Host, protocol::digest, transport::OUTPUT_LIMIT};
use crate::{
    registry::ProfileName,
    share::mcp::{self, HeaderRef, InputError, SecretRef, Transport},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PREFIX: &[u8] = b"LARPCC-MCP/1\n";
pub(crate) const LIMIT: usize = PREFIX.len() + OUTPUT_LIMIT;

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Definition {
    Stdio(Stdio),
    Http(Http),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stdio {
    name: ProfileName,
    command: String,
    args: Vec<String>,
    env_refs: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Http {
    name: ProfileName,
    url: String,
    header_refs: Vec<Header>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Header {
    header: String,
    env_ref: String,
}

fn env(reference: &SecretRef) -> Result<String, InputError> {
    match reference {
        SecretRef::Env { name } => Ok(name.clone()),
        SecretRef::Keychain { .. } => Err(InputError::SecretReference),
    }
}

impl Definition {
    fn new(name: &ProfileName, transport: &Transport) -> Result<Self, InputError> {
        mcp::server_name(name.as_str())?;
        Ok(match transport {
            Transport::Stdio {
                command,
                args,
                env: refs,
            } => Self::Stdio(Stdio {
                name: name.clone(),
                command: command.clone(),
                args: args.clone(),
                env_refs: refs.iter().map(env).collect::<Result<_, _>>()?,
            }),
            Transport::Http { url, headers } => Self::Http(Http {
                name: name.clone(),
                url: url.clone(),
                header_refs: headers
                    .iter()
                    .map(|h| {
                        Ok(Header {
                            header: h.header.clone(),
                            env_ref: env(&h.value)?,
                        })
                    })
                    .collect::<Result<_, _>>()?,
            }),
        })
    }

    fn name(&self) -> &ProfileName {
        match self {
            Self::Stdio(transport) => &transport.name,
            Self::Http(transport) => &transport.name,
        }
    }

    fn transport(self) -> Transport {
        match self {
            Self::Stdio(Stdio {
                command,
                args,
                env_refs,
                ..
            }) => Transport::Stdio {
                command,
                args,
                env: env_refs
                    .into_iter()
                    .map(|name| SecretRef::Env { name })
                    .collect(),
            },
            Self::Http(Http {
                url, header_refs, ..
            }) => Transport::Http {
                url,
                headers: header_refs
                    .into_iter()
                    .map(|h| HeaderRef {
                        header: h.header,
                        value: SecretRef::Env { name: h.env_ref },
                    })
                    .collect(),
            },
        }
    }

    fn body(&self) -> Result<Vec<u8>, InputError> {
        let mut bytes = serde_json::to_vec(self).map_err(|_| InputError::Usage)?;
        bytes.push(b'\n');
        if bytes.len() > OUTPUT_LIMIT {
            return Err(InputError::TooLarge);
        }
        Ok(bytes)
    }

    fn argv(&self, expected: Option<u64>) -> Vec<String> {
        let mut argv: Vec<_> = [
            "--json",
            "--remote-request",
            "cc",
            "host",
            "catalog",
            "apply",
            "mcp",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        argv.push(self.name().to_string());
        if let Some(expected) = expected {
            argv.push(format!("--expected-revision={expected}"));
        }
        match self {
            Self::Stdio(value) => {
                argv.push(format!("--command={}", value.command));
                argv.extend(value.args.iter().map(|arg| format!("--arg={arg}")));
                argv.extend(
                    value
                        .env_refs
                        .iter()
                        .map(|name| format!("--env-ref={name}")),
                );
            }
            Self::Http(value) => {
                argv.push(format!("--url={}", value.url));
                argv.extend(
                    value
                        .header_refs
                        .iter()
                        .map(|h| format!("--header-ref={}={}", h.header, h.env_ref)),
                );
            }
        }
        argv
    }
}

pub(crate) fn apply_words(
    host: &Host,
    name: &ProfileName,
    transport: &Transport,
    expected: Option<u64>,
) -> Result<Vec<String>, InputError> {
    let words = Definition::new(name, transport)?.argv(expected);
    host.script(&words, false).ok_or(InputError::TooLarge)?;
    Ok(words)
}

pub(crate) fn encode(
    name: &ProfileName,
    transport: &Transport,
) -> Result<(Vec<u8>, String), InputError> {
    transport.validate()?;
    let definition = Definition::new(name, transport)?;
    let maximum = Host {
        name: name.clone(),
        ssh: super::Address::config("host".into()).ok_or(InputError::Usage)?,
        larp: None,
        larpcc_home: None,
        identity_file: None,
    };
    maximum
        .script(&definition.argv(Some(u64::MAX)), false)
        .ok_or(InputError::TooLarge)?;
    let body = definition.body()?;
    let digest = format!("{:x}", Sha256::digest(&body));
    Ok(([PREFIX, &body].concat(), digest))
}

pub(crate) fn decode(
    bytes: &[u8],
    expected_name: &ProfileName,
    expected_digest: &str,
    remote_source: bool,
) -> Result<Transport, InputError> {
    let body = bytes.strip_prefix(PREFIX).ok_or(InputError::Usage)?;
    if body.len() > OUTPUT_LIMIT {
        return Err(InputError::TooLarge);
    }
    if !digest(expected_digest) || format!("{:x}", Sha256::digest(body)) != expected_digest {
        return Err(InputError::Usage);
    }
    let definition: Definition = serde_json::from_slice(body).map_err(|_| InputError::Usage)?;
    if definition.name() != expected_name || definition.body()? != body {
        return Err(InputError::Usage);
    }
    mcp::server_name(definition.name().as_str())?;
    let transport = definition.transport();
    transport.validate_for(!remote_source && cfg!(windows))?;
    Ok(transport)
}
