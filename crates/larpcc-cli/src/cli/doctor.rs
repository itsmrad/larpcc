//! `doctor [PROFILE] [--detect-versions]`: the frozen `status` shape. Without the
//! flag it is local metadata only and starts no child.
use crate::{
    provider::version::{self, Detection},
    registry::{Error, ProfileName, Provider, Registry},
};
use serde_json::{Value, json};
use std::fmt::Write;

pub(super) fn data(
    registry: &Registry,
    profile: Option<&ProfileName>,
    detect: bool,
) -> Result<Value, Error> {
    let mut data = match profile {
        // The frozen `status` metadata shape, shared with `status --no-account`.
        Some(name) => registry.with_status_metadata(name, None, |_| Ok(()))?.0,
        None => {
            let bindings = registry.metadata_json("provider.list", None)?;
            json!({"profile":null,"detected_versions":bindings["bindings"],"native_auth":"not_observed","native_rows":[],"diagnostics":[]})
        }
    };
    if !detect {
        return Ok(data);
    }
    // Each executable comes from the same revision the metadata reported, selected
    // per provider so no other binding is validated or read.
    let snapshot = Registry::at(registry.root().to_owned())?.expecting(registry.revision());
    let mut diagnostics = Vec::new();
    for entry in data["detected_versions"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if entry["surface"] != "cli" || entry["binding"]["state"] != "ok" {
            continue;
        }
        let Some(provider) = entry["provider"]
            .as_str()
            .and_then(|p| p.parse::<Provider>().ok())
        else {
            continue;
        };
        let detection = match snapshot.binding_path(provider)? {
            Some(executable) => version::detect(provider, &executable, registry.root())?,
            None => Detection::Failed,
        };
        match detection {
            Detection::Detected(version) => entry["binding"]["detected_version"] = json!(version),
            Detection::Unapproved => diagnostics.push((
                "version_unapproved",
                format!("No reviewed version probe matches the {provider} executable."),
            )),
            Detection::Failed => diagnostics.push((
                "version_probe_failed",
                format!("The {provider} version probe returned no accepted version."),
            )),
        }
    }
    diagnostics.sort();
    data["diagnostics"] = diagnostics
        .into_iter()
        .map(|(code, message)| json!({"code":code,"message":message}))
        .collect();
    Ok(data)
}

pub(super) fn human(data: &Value, detect: bool) -> String {
    let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
    let mut out = String::new();
    let profile = &data["profile"];
    if profile.is_null() {
        out.push_str("Profile: all bindings\n");
    } else {
        let _ = writeln!(
            out,
            "Profile: {} (ID {}, {})",
            text(&profile["name"]),
            text(&profile["id"]),
            text(&profile["provider"])
        );
        for surface in ["cli", "desktop"] {
            let s = &profile["surfaces"][surface];
            let _ = writeln!(
                out,
                "  {surface} home: {}{} {}",
                text(&s["state"]),
                if s["enabled"] == true {
                    ""
                } else {
                    " (disabled)"
                },
                text(&s["path"])
            );
        }
    }
    for entry in data["detected_versions"].as_array().into_iter().flatten() {
        let b = &entry["binding"];
        let _ = write!(
            out,
            "{} {}: {}",
            text(&entry["provider"]),
            text(&entry["surface"]),
            text(&b["state"])
        );
        if !b["path"].is_null() {
            let _ = write!(out, " {}", text(&b["path"]));
        }
        out.push('\n');
        if b["state"] == "unbound" {
            continue;
        }
        let detected = if !b["detected_version"].is_null() {
            text(&b["detected_version"])
        } else if detect && entry["surface"] == "cli" && b["state"] == "ok" {
            "unknown".into()
        } else {
            "not probed".into()
        };
        let _ = writeln!(out, "  detected version: {detected}");
        if !b["recorded_version"].is_null() {
            let _ = writeln!(
                out,
                "  recorded label: {} (unverified)",
                text(&b["recorded_version"])
            );
        }
    }
    out.push_str("Native auth: not observed\nNative rows: none (launch qualification pending)\n");
    let diagnostics = data["diagnostics"].as_array().cloned().unwrap_or_default();
    if diagnostics.is_empty() {
        out.push_str("Diagnostics: none\n");
    } else {
        out.push_str("Diagnostics:\n");
        for d in diagnostics {
            let _ = writeln!(out, "  {}: {}", text(&d["code"]), text(&d["message"]));
        }
    }
    out
}
