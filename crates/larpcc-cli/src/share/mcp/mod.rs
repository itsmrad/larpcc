//! Stage 07 MCP definitions: strict catalog model, reference-only input
//! validation and deterministic provider renders. This module is pure: it never
//! reads environment values, keychains, provider configuration or the network,
//! and it never starts an MCP server. Registry transactions live in `registry/mcp.rs`.
pub mod cli;

use crate::registry::{ProfileId, ProfileName, Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;
pub const MAX_SERVERS: usize = 256;
pub const MAX_PROFILES: usize = 256;
pub const MAX_RENDER_BYTES: usize = 1024 * 1024;
pub const MAX_TRANSACTION_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_REFS: usize = 16;
pub const MAX_COMMAND_BYTES: usize = 1024;
/// Checked provider name limit for `<catalog-name>__p<ID>` (both adapters).
pub const MAX_RENDER_NAME: usize = 64;
/// The only stdio arguments admitted until a reviewed adapter amendment.
pub const ALLOWED_ARGS: [&str; 2] = ["--stdio", "--transport=stdio"];

/// Safe refusal classes. None carries or formats the refused value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    Usage,
    SecretLiteral,
    SecretReference,
    Unsupported,
    UnsafePath,
    TooLarge,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecretRef {
    Env {
        name: String,
    },
    /// Serializable for future adapters; the CLI never creates, resolves or
    /// reads keychain items, and every current renderer refuses it.
    Keychain {
        name: String,
    },
}
impl SecretRef {
    fn valid(&self) -> bool {
        match self {
            Self::Env { name } => env_name(name),
            Self::Keychain { name } => {
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            }
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Env { .. } => "env",
            Self::Keychain { .. } => "keychain",
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Env { name } | Self::Keychain { name } => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderRef {
    pub header: String,
    pub value: SecretRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<SecretRef>,
    },
    Http {
        url: String,
        headers: Vec<HeaderRef>,
    },
}
impl Transport {
    /// Public transport class; `/sse` endpoints are reported as http.
    pub fn class(&self) -> &'static str {
        match self {
            Self::Stdio { .. } => "stdio",
            Self::Http { .. } => "http",
        }
    }
    /// Reference names only, sorted by kind then name, without duplicates.
    pub fn references(&self) -> Vec<&SecretRef> {
        let mut refs: Vec<_> = match self {
            Self::Stdio { env, .. } => env.iter().collect(),
            Self::Http { headers, .. } => headers.iter().map(|h| &h.value).collect(),
        };
        refs.sort_by(|a, b| (a.kind(), a.name()).cmp(&(b.kind(), b.name())));
        refs.dedup();
        refs
    }
    fn sse(&self) -> bool {
        matches!(self, Self::Http { url, .. } if url.ends_with("/sse"))
    }
    /// Revalidate a persisted definition with the same rules as `mcp add`,
    /// except command existence, which is checked only when it is added.
    pub fn validate(&self) -> Result<(), InputError> {
        match self {
            Self::Stdio { command, args, env } => {
                command_syntax(command)?;
                if args.len() > ALLOWED_ARGS.len()
                    || args.iter().any(|a| !ALLOWED_ARGS.contains(&a.as_str()))
                    || args.iter().collect::<BTreeSet<_>>().len() != args.len()
                    || env.len() > MAX_REFS
                    || env.iter().any(|r| !r.valid())
                    || env.windows(2).any(|w| w[0] >= w[1])
                {
                    return Err(InputError::Usage);
                }
            }
            Self::Http { url, headers } => {
                url_syntax(url)?;
                if headers.len() > MAX_REFS
                    || headers
                        .iter()
                        .any(|h| !header_name(&h.header) || !h.value.valid())
                    || headers.windows(2).any(|w| {
                        w[0].header.to_ascii_lowercase() >= w[1].header.to_ascii_lowercase()
                    })
                {
                    return Err(InputError::Usage);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Switch {
    pub profile_id: ProfileId,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub name: ProfileName,
    pub transport: Transport,
    /// Explicit per-profile-ID switches sorted by ID. A missing ID (only after
    /// an interrupted profile hook) inherits `sync_all` until the next commit.
    pub switches: Vec<Switch>,
}
impl Server {
    pub fn switch(&self, id: ProfileId) -> Option<bool> {
        self.switches
            .iter()
            .find(|s| s.profile_id == id)
            .map(|s| s.enabled)
    }
    pub fn set(&mut self, id: ProfileId, enabled: bool) {
        match self.switches.iter_mut().find(|s| s.profile_id == id) {
            Some(s) => s.enabled = enabled,
            None => {
                self.switches.push(Switch {
                    profile_id: id,
                    enabled,
                });
                self.switches.sort_by_key(|s| s.profile_id);
            }
        }
    }
}

/// `mcp.json`: strict version 1, bounded, separate from other catalogs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u64,
    pub revision: u64,
    pub sync_all: bool,
    pub servers: Vec<Server>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            sync_all: false,
            servers: vec![],
        }
    }
}
impl Catalog {
    /// Structural validation; registry cross references are checked by `registry::mcp`.
    pub fn validate(&self) -> Result<(), InputError> {
        if self.version != 1 || self.servers.len() > MAX_SERVERS {
            return Err(InputError::Usage);
        }
        for w in self.servers.windows(2) {
            if w[0].name >= w[1].name {
                return Err(InputError::Usage);
            }
        }
        for s in &self.servers {
            if ProfileName::parse_new(s.name.as_str()).is_err()
                || s.switches.len() > MAX_PROFILES
                || s.switches.iter().any(|w| w.profile_id.get() == 0)
                || s.switches
                    .windows(2)
                    .any(|w| w[0].profile_id >= w[1].profile_id)
            {
                return Err(InputError::Usage);
            }
            s.transport.validate()?;
        }
        Ok(())
    }
    pub fn find(&self, name: &ProfileName) -> Option<&Server> {
        self.servers.iter().find(|s| &s.name == name)
    }
}

// ---------------------------------------------------------------------------
// Input validation. Refusals never echo or format the refused value.

fn env_name(value: &str) -> bool {
    let b = value.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}
fn header_name(value: &str) -> bool {
    let b = value.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
}

/// Conservative ambiguity screen for the few free-form identifier fields.
/// It is NOT a secret detector and makes no completeness claim; the boundary
/// is that no field accepts an arbitrary literal value at all.
pub fn token_like(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    const PREFIXES: [&str; 14] = [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "xox",
        "akia",
        "aiza",
        "eyj",
        "-----begin",
    ];
    if PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return true;
    }
    value
        .split(|c: char| !(c.is_ascii_alphanumeric() || "+/=_-".contains(c)))
        .any(|run| {
            let len = run.len();
            let lower = run.bytes().any(|b| b.is_ascii_lowercase());
            let upper = run.bytes().any(|b| b.is_ascii_uppercase());
            let digits = run.bytes().filter(u8::is_ascii_digit).count();
            let classes = [lower, upper, digits > 0]
                .into_iter()
                .filter(|c| *c)
                .count();
            let hex = run.bytes().all(|b| b.is_ascii_hexdigit());
            (len >= 20 && classes == 3)
                || (len >= 24 && digits >= 6 && classes >= 2)
                || (len >= 32 && hex && digits > 0)
        })
}

fn command_syntax(command: &str) -> Result<(), InputError> {
    if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
        return Err(InputError::Usage);
    }
    if command.chars().any(char::is_control) {
        return Err(InputError::Usage);
    }
    let path = std::path::Path::new(command);
    if !path.is_absolute() {
        return Err(InputError::Usage);
    }
    #[cfg(windows)]
    if command.starts_with(r"\\") || command.starts_with("//") {
        // UNC and device namespaces are not validated local file paths.
        return Err(InputError::UnsafePath);
    }
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    if command.ends_with(separators) {
        return Err(InputError::Usage);
    }
    for (i, part) in command.split(separators).enumerate() {
        if i == 0 {
            // Unix leading empty component, or a Windows drive prefix.
            continue;
        }
        if part.is_empty() || part == "." || part == ".." {
            return Err(InputError::Usage);
        }
        // `$`/`%` would be expanded by native config readers; `=`/quotes and
        // token-shaped components can carry values. All refuse without echo.
        if part.contains(['$', '%', '=', '"', '\'', '`'])
            || part.starts_with('-')
            || token_like(part)
        {
            return Err(InputError::SecretLiteral);
        }
    }
    Ok(())
}

fn url_syntax(url: &str) -> Result<(), InputError> {
    if url.len() > 2048 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(InputError::Usage);
    }
    let Some(rest) = url.strip_prefix("https://") else {
        // Plain/loopback http and other schemes need a reviewed adapter.
        return Err(InputError::Unsupported);
    };
    // Anything that can carry a value beyond origin and fixed path is refused.
    if rest.contains(['@', '?', '#', '%', '$', '\\', '"', '\'', '`']) {
        return Err(InputError::SecretLiteral);
    }
    let (authority, path) = rest
        .find('/')
        .map(|i| rest.split_at(i))
        .ok_or(InputError::Usage)?;
    if path != "/mcp" && path != "/sse" {
        return Err(InputError::SecretLiteral);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if let Some(port) = port
        && (port.is_empty()
            || port.len() > 5
            || !port.bytes().all(|b| b.is_ascii_digit())
            || port.starts_with('0')
            || port.parse::<u32>().map_or(true, |p| p > 65_535))
    {
        return Err(InputError::Usage);
    }
    if host.is_empty() || host.len() > 253 {
        return Err(InputError::Usage);
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(InputError::Usage);
        }
        if token_like(label) {
            return Err(InputError::SecretLiteral);
        }
    }
    Ok(())
}

/// Validate `mcp add` input into one canonical transport. Exactly one of
/// `command`/`url` is required; the CLI parser also enforces that.
pub fn transport(
    command: Option<&str>,
    args: &[String],
    env_refs: &[String],
    url: Option<&str>,
    header_refs: &[String],
) -> Result<Transport, InputError> {
    match (command, url) {
        (Some(command), None) => {
            if !header_refs.is_empty() {
                return Err(InputError::Usage);
            }
            command_syntax(command)?;
            for arg in args {
                if !ALLOWED_ARGS.contains(&arg.as_str()) {
                    // Arbitrary literals are refused whatever they look like.
                    return Err(InputError::SecretLiteral);
                }
            }
            if args.iter().collect::<BTreeSet<_>>().len() != args.len() {
                return Err(InputError::Usage);
            }
            if env_refs.len() > MAX_REFS {
                return Err(InputError::TooLarge);
            }
            let mut env = Vec::new();
            for value in env_refs {
                env.push(env_reference(value)?);
            }
            env.sort();
            if env.windows(2).any(|w| w[0] == w[1]) {
                return Err(InputError::Usage);
            }
            Ok(Transport::Stdio {
                command: command.into(),
                args: args.to_vec(),
                env,
            })
        }
        (None, Some(url)) => {
            if !args.is_empty() || !env_refs.is_empty() {
                return Err(InputError::Usage);
            }
            url_syntax(url)?;
            if header_refs.len() > MAX_REFS {
                return Err(InputError::TooLarge);
            }
            let mut headers = Vec::new();
            for value in header_refs {
                let Some((header, env)) = value.split_once('=') else {
                    return Err(if token_like(value) || value.contains([' ', ':']) {
                        InputError::SecretLiteral
                    } else {
                        InputError::SecretReference
                    });
                };
                if !header_name(header) {
                    return Err(if header.contains([' ', ':']) || token_like(header) {
                        InputError::SecretLiteral
                    } else {
                        InputError::SecretReference
                    });
                }
                if token_like(header) {
                    return Err(InputError::SecretLiteral);
                }
                headers.push(HeaderRef {
                    header: header.into(),
                    value: env_reference(env)?,
                });
            }
            headers.sort_by_key(|h| h.header.to_ascii_lowercase());
            if headers
                .windows(2)
                .any(|w| w[0].header.eq_ignore_ascii_case(&w[1].header))
            {
                return Err(InputError::Usage);
            }
            Ok(Transport::Http {
                url: url.into(),
                headers,
            })
        }
        _ => Err(InputError::Usage),
    }
}

fn env_reference(value: &str) -> Result<SecretRef, InputError> {
    // `NAME=value`, spaces, `${...}` or token-shaped text are literal values.
    if value.contains(['=', ' ', ':', '$']) || token_like(value) {
        return Err(InputError::SecretLiteral);
    }
    if !env_name(value) {
        return Err(InputError::SecretReference);
    }
    Ok(SecretRef::Env { name: value.into() })
}

/// A command must name an existing local regular file. Only metadata is read.
pub fn command_exists(command: &str) -> Result<(), InputError> {
    match std::fs::metadata(command) {
        Ok(meta) if meta.is_file() => Ok(()),
        _ => Err(InputError::UnsafePath),
    }
}

pub fn name_ambiguous(name: &str) -> bool {
    token_like(name)
}

// ---------------------------------------------------------------------------
// Rendering: pure, deterministic, reference-only.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    Unsupported,
    SecretReference,
    TooLarge,
    Collision,
}

/// Whether the exact provider adapter can represent this definition without
/// materializing a secret. Desktop surfaces have no adapter at all.
pub fn supported(provider: Provider, server: &Server) -> Result<(), RenderError> {
    if server
        .transport
        .references()
        .iter()
        .any(|r| matches!(r, SecretRef::Keychain { .. }))
    {
        return Err(RenderError::SecretReference);
    }
    if provider == Provider::Codex && server.transport.sse() {
        // Codex supports stdio and streamable HTTP only; never map SSE.
        return Err(RenderError::Unsupported);
    }
    Ok(())
}

pub fn render_name(name: &ProfileName, id: ProfileId) -> Result<String, RenderError> {
    let value = format!("{}__p{}", name.as_str(), id.get());
    if value.len() > MAX_RENDER_NAME {
        return Err(RenderError::TooLarge);
    }
    Ok(value)
}

pub fn file_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude.json",
        Provider::Codex => "codex-overrides.json",
    }
}

/// Render the profile-owned file for the servers enabled on one profile.
/// Claude: a separate `--mcp-config` JSON using `${VAR}` references.
/// Codex: a separate overrides description (key/value pairs a reviewed
/// adapter may translate into exact `-c` argv) using native `env_vars` and
/// `env_http_headers` reference fields. Neither reads or merges native config.
pub fn render(
    provider: Provider,
    id: ProfileId,
    servers: &[&Server],
) -> Result<Vec<u8>, RenderError> {
    use serde_json::{Map, Value, json};
    let mut names = BTreeSet::new();
    let mut sorted: Vec<_> = servers.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut claude = Map::new();
    let mut overrides = Vec::new();
    for server in sorted {
        supported(provider, server)?;
        let name = render_name(&server.name, id)?;
        if !names.insert(name.clone()) {
            return Err(RenderError::Collision);
        }
        match (&server.transport, provider) {
            (Transport::Stdio { command, args, env }, Provider::Claude) => {
                let env: Map<_, _> = env
                    .iter()
                    .map(|r| (r.name().to_owned(), json!(format!("${{{}}}", r.name()))))
                    .collect();
                claude.insert(
                    name,
                    json!({"type":"stdio","command":command,"args":args,"env":env}),
                );
            }
            (Transport::Http { url, headers }, Provider::Claude) => {
                let headers: Map<_, _> = headers
                    .iter()
                    .map(|h| (h.header.clone(), json!(format!("${{{}}}", h.value.name()))))
                    .collect();
                let kind = if server.transport.sse() {
                    "sse"
                } else {
                    "http"
                };
                claude.insert(name, json!({"type":kind,"url":url,"headers":headers}));
            }
            (Transport::Stdio { command, args, env }, Provider::Codex) => {
                let key = |field: &str| format!("mcp_servers.{name}.{field}");
                overrides.push(json!({"key":key("command"),"value":command}));
                overrides.push(json!({"key":key("args"),"value":args}));
                overrides.push(json!({"key":key("env_vars"),"value":env.iter().map(SecretRef::name).collect::<Vec<_>>()}));
            }
            (Transport::Http { url, headers }, Provider::Codex) => {
                let key = |field: &str| format!("mcp_servers.{name}.{field}");
                overrides.push(json!({"key":key("url"),"value":url}));
                let headers: Map<_, _> = headers
                    .iter()
                    .map(|h| (h.header.clone(), Value::from(h.value.name())))
                    .collect();
                overrides.push(json!({"key":key("env_http_headers"),"value":headers}));
            }
        }
    }
    let value = match provider {
        Provider::Claude => json!({"mcpServers": claude}),
        Provider::Codex => {
            json!({"version":1,"format":"codex-config-overrides","overrides":overrides})
        }
    };
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|_| RenderError::TooLarge)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_RENDER_BYTES {
        return Err(RenderError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn stage07_secrets_input_positions_refuse_without_value() {
        let ok = transport(
            Some("/usr/local/bin/files-mcp"),
            &s(&["--stdio"]),
            &s(&["FILES_TOKEN"]),
            None,
            &[],
        );
        assert!(ok.is_ok());
        for arg in ["--token=abc", "sk-live-123", "--verbose", "x"] {
            assert_eq!(
                transport(Some("/usr/bin/x"), &s(&[arg]), &[], None, &[]),
                Err(InputError::SecretLiteral)
            );
        }
        for env in ["TOKEN=abc", "ghp_0123456789abcdefABCDEF", "A B"] {
            assert_eq!(
                transport(Some("/usr/bin/x"), &[], &s(&[env]), None, &[]),
                Err(InputError::SecretLiteral)
            );
        }
        for env in ["", "1BAD", "bad-name"] {
            assert_eq!(
                transport(Some("/usr/bin/x"), &[], &s(&[env]), None, &[]),
                Err(InputError::SecretReference)
            );
        }
        for url in [
            "https://user:pw@example.com/mcp",
            "https://example.com/mcp?token=x",
            "https://example.com/mcp#x",
            "https://example.com/%41/mcp",
            "https://example.com/other",
            "https://Ab3dEf9hIj2kLm4nOp6q.example.com/mcp",
        ] {
            assert_eq!(
                transport(None, &[], &[], Some(url), &[]),
                Err(InputError::SecretLiteral),
                "{url}"
            );
        }
        assert_eq!(
            transport(None, &[], &[], Some("http://localhost/mcp"), &[]),
            Err(InputError::Unsupported)
        );
        assert_eq!(
            transport(
                None,
                &[],
                &[],
                Some("https://example.com/mcp"),
                &s(&["Authorization=Bearer abc"])
            ),
            Err(InputError::SecretLiteral)
        );
        assert_eq!(
            transport(
                None,
                &[],
                &[],
                Some("https://example.com/mcp"),
                &s(&["Authorization="])
            ),
            Err(InputError::SecretReference)
        );
        assert_eq!(
            transport(
                None,
                &[],
                &[],
                Some("https://example.com/mcp"),
                &s(&["Authorization"])
            ),
            Err(InputError::SecretReference)
        );
        assert!(
            transport(
                None,
                &[],
                &[],
                Some("https://docs.example.com:8443/sse"),
                &s(&["Authorization=DOCS_TOKEN"])
            )
            .is_ok()
        );
    }
    #[test]
    fn stage07_render_names_are_checked() {
        let name = ProfileName::parse_new("example").unwrap();
        let id: ProfileId = serde_json::from_str("18446744073709551615").unwrap();
        assert_eq!(
            render_name(&name, id).unwrap(),
            "example__p18446744073709551615"
        );
        const { assert!(MAX_RENDER_NAME >= 32 + 3 + 20) };
    }
}
