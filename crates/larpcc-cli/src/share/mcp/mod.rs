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
    fn validate(&self) -> Result<(), InputError> {
        match self {
            Self::Env { name } => validate_env(name),
            Self::Keychain { name } if token_like(name) => Err(InputError::SecretLiteral),
            Self::Keychain { name }
                if !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) =>
            {
                Ok(())
            }
            Self::Keychain { .. } => Err(InputError::SecretReference),
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
                if args.iter().any(|a| !ALLOWED_ARGS.contains(&a.as_str())) {
                    return Err(InputError::SecretLiteral);
                }
                if env.len() > MAX_REFS {
                    return Err(InputError::TooLarge);
                }
                for reference in env {
                    reference.validate()?;
                }
                if args.iter().collect::<BTreeSet<_>>().len() != args.len()
                    || env.windows(2).any(|w| w[0] >= w[1])
                {
                    return Err(InputError::Usage);
                }
            }
            Self::Http { url, headers } => {
                url_syntax(url)?;
                if headers.len() > MAX_REFS {
                    return Err(InputError::TooLarge);
                }
                for h in headers {
                    if h.header.contains([' ', ':']) || token_like(&h.header) {
                        return Err(InputError::SecretLiteral);
                    }
                    if !header_name(&h.header) {
                        return Err(InputError::SecretReference);
                    }
                    h.value.validate()?;
                }
                if headers
                    .windows(2)
                    .any(|w| w[0].header.to_ascii_lowercase() >= w[1].header.to_ascii_lowercase())
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
            server_name(s.name.as_str())?;
            if s.switches.len() > MAX_PROFILES
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
    token_prefix(value) || high_entropy(value)
}
/// Known credential prefixes only.
fn token_prefix(value: &str) -> bool {
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
    PREFIXES.iter().any(|p| lower.starts_with(p))
}
fn high_entropy(value: &str) -> bool {
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

fn credential_component(part: &str) -> bool {
    let lower = part.to_ascii_lowercase();
    if [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghs_",
        "ghu_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxa-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
        "-----begin",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
        || lower == "bearer"
        || lower
            .strip_prefix("bearer")
            .is_some_and(|s| s.starts_with([' ', '-', '_', '.']))
    {
        return true;
    }
    let base64url = |b: u8| b.is_ascii_alphanumeric() || b"-_".contains(&b);
    if part.starts_with("eyJ")
        && part
            .split('.')
            .take(3)
            .filter(|s| s.len() >= 8 && s.bytes().all(base64url))
            .count()
            == 3
    {
        return true;
    }
    part.split('.').any(|segment| {
        if segment.len() == 36
            && segment.split('-').map(str::len).eq([8, 4, 4, 4, 12])
            && segment.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
        {
            return false;
        }
        let body = segment.trim_end_matches('=');
        let aws = body.len() == 20
            && matches!(&body.as_bytes()[..4], b"AKIA" | b"ASIA")
            && body
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
        let google = body.len() == 39 && body.starts_with("AIza") && body.bytes().all(base64url);
        let base64 = body.len() >= 20
            && segment.len() - body.len() <= 2
            && body.bytes().all(|b| base64url(b) || b == b'+')
            && body.bytes().any(|b| b.is_ascii_lowercase())
            && body.bytes().any(|b| b.is_ascii_uppercase())
            && (segment.ends_with('=') || body.bytes().any(|b| !b.is_ascii_alphabetic()));
        aws || google || base64
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
        if credential_component(part) {
            return Err(InputError::SecretLiteral);
        }
        // Keep native config readers from expanding an environment reference.
        if part.contains(['$', '%']) {
            return Err(InputError::Usage);
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
    let transport = match (command, url) {
        (Some(command), None) => {
            if !header_refs.is_empty() {
                return Err(InputError::Usage);
            }
            let mut env: Vec<_> = env_refs
                .iter()
                .map(|name| SecretRef::Env { name: name.clone() })
                .collect();
            env.sort();
            Transport::Stdio {
                command: command.into(),
                args: args.to_vec(),
                env,
            }
        }
        (None, Some(url)) => {
            if !args.is_empty() || !env_refs.is_empty() {
                return Err(InputError::Usage);
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
                headers.push(HeaderRef {
                    header: header.into(),
                    value: SecretRef::Env { name: env.into() },
                });
            }
            headers.sort_by_key(|h| h.header.to_ascii_lowercase());
            Transport::Http {
                url: url.into(),
                headers,
            }
        }
        _ => return Err(InputError::Usage),
    };
    transport.validate()?;
    Ok(transport)
}

fn validate_env(value: &str) -> Result<(), InputError> {
    // `NAME=value`, spaces, `${...}` or token-shaped text are literal values.
    if value.contains(['=', ' ', ':', '$']) || token_like(value) {
        return Err(InputError::SecretLiteral);
    }
    if !env_name(value) {
        return Err(InputError::SecretReference);
    }
    Ok(())
}

/// A command must name an existing local regular file. Only metadata is read.
pub fn command_exists(command: &str) -> Result<(), InputError> {
    match std::fs::metadata(command) {
        Ok(meta) if meta.is_file() => Ok(()),
        _ => Err(InputError::UnsafePath),
    }
}

pub fn server_name(name: &str) -> Result<ProfileName, InputError> {
    if token_like(name) {
        return Err(InputError::SecretLiteral);
    }
    ProfileName::parse_new(name).map_err(|_| InputError::Usage)
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
    const EXE: &str = if cfg!(windows) {
        r"C:\bin\files-mcp.exe"
    } else {
        "/usr/local/bin/files-mcp"
    };
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn stage07_secrets_input_positions_refuse_without_value() {
        let ok = transport(Some(EXE), &s(&["--stdio"]), &s(&["FILES_TOKEN"]), None, &[]);
        assert!(ok.is_ok());
        // Hashed/GUID path components are ordinary local paths; credential prefixes are not.
        let sep = if cfg!(windows) { r"\" } else { "/" };
        let root = if cfg!(windows) { "C:" } else { "" };
        let hashed = format!(
            "{root}{sep}nix{sep}store{sep}0123abcd4567efgh8901ijkl2345mnop-files{sep}bin{sep}files-mcp"
        );
        let guid = format!("{root}{sep}tmp{sep}4bbdac98-b7d0-457c-9168-fb946cea1607{sep}files-mcp");
        for path in [hashed, guid] {
            assert!(
                transport(Some(&path), &[], &[], None, &[]).is_ok(),
                "{path}"
            );
        }
        let dollar = format!("{root}{sep}opt{sep}${{HOME}}{sep}files-mcp");
        assert_eq!(
            transport(Some(&dollar), &[], &[], None, &[]),
            Err(InputError::Usage)
        );
        for arg in ["--token=abc", "sk-live-123", "--verbose", "x"] {
            assert_eq!(
                transport(Some(EXE), &s(&[arg]), &[], None, &[]),
                Err(InputError::SecretLiteral)
            );
        }
        for env in ["TOKEN=abc", "ghp_0123456789abcdefABCDEF", "A B"] {
            assert_eq!(
                transport(Some(EXE), &[], &s(&[env]), None, &[]),
                Err(InputError::SecretLiteral)
            );
        }
        for env in ["", "1BAD", "bad-name"] {
            assert_eq!(
                transport(Some(EXE), &[], &s(&[env]), None, &[]),
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
