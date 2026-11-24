//! Local onboarding discovery. Filesystem access starts at a held directory
//! and follows links one component at a time, after checking exclusions.
#[cfg(unix)]
mod unix;

use crate::registry::{Error, Provider, Registry};
use serde_json::Value;

pub(crate) const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos"));
pub(crate) const UNSUPPORTED_MESSAGE: &str = "Provider detection is supported on macOS and Linux.";

pub(crate) fn collect(registry: &Registry, provider: Option<Provider>) -> Result<Value, Error> {
    #[cfg(unix)]
    return unix::collect(registry, provider);
    #[cfg(not(unix))]
    {
        let _ = (registry, provider);
        Err(Error::Unsupported)
    }
}

pub(crate) fn human(data: &Value) -> String {
    let mut output = String::new();
    for row in data["executables"].as_array().into_iter().flatten() {
        use std::fmt::Write;
        let provider = row["provider"].as_str().unwrap_or_default();
        let path = row["path"].as_str().unwrap_or_default();
        let kind = row["install_kind"].as_str().unwrap_or_default();
        let version = row["version"].as_str().unwrap_or("unknown");
        let bindable = if row["bindable"] == true {
            "bindable"
        } else {
            "not bindable"
        };
        let bound = if row["bound"] == true { ", bound" } else { "" };
        let qualified = if row["qualified"] == true {
            ", qualified"
        } else {
            ""
        };
        let _ = writeln!(
            output,
            "{provider}: {path} ({kind}, {version}, {bindable}{bound}{qualified})"
        );
        if let Some(reason) = row["bind_reason"].as_str() {
            let _ = writeln!(output, "  {reason}");
        }
    }
    for row in data["desktop_apps"].as_array().into_iter().flatten() {
        use std::fmt::Write;
        let path = row["path"].as_str().unwrap_or_default();
        let version = row["version"].as_str().unwrap_or("unknown");
        let requirement = row["code_requirement"].as_str().unwrap_or_default();
        let _ = writeln!(output, "{path} ({version}, code requirement {requirement})");
    }
    for row in data["skipped"].as_array().into_iter().flatten() {
        use std::fmt::Write;
        let path = row["path"].as_str().unwrap_or_default();
        let _ = writeln!(output, "{path} (skipped: inside provider home)");
    }
    if output.is_empty() {
        output.push_str("No provider installations found.\n");
    }
    if data["limited"] == true {
        output.push_str("Scan limit reached. Results may be incomplete.\n");
    }
    output
}
