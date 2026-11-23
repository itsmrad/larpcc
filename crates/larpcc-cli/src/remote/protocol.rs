use super::transport::{Failure, Output};
use crate::registry::ProfileName;
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value};
use std::{collections::HashSet, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Error {
    pub code: &'static str,
    pub message: &'static str,
    pub exit: u8,
}

const fn operational(code: &'static str, message: &'static str) -> Error {
    Error {
        code,
        message,
        exit: 1,
    }
}

pub(crate) const OUTPUT: Error = operational(
    "remote_output",
    "The remote CLI returned an invalid response.",
);
pub(crate) const UNKNOWN: Error = operational(
    "remote_outcome_unknown",
    "The remote outcome is unknown. Refresh explicitly before retrying.",
);
const INCOMPATIBLE: Error = operational(
    "remote_incompatible",
    "The remote CLI or operating system is incompatible.",
);
const LARP_MISSING: Error = operational(
    "remote_larp_missing",
    "Larp was not found on the remote machine.",
);
const SSH_FAILED: Error = operational(
    "ssh_failed",
    "SSH failed. Verify connectivity, the host key and authentication outside Larp.",
);
const SSH_UNAVAILABLE: Error = operational(
    "ssh_unavailable",
    "No supported local OpenSSH client could be verified.",
);
const TIMEOUT: Error = operational("remote_timeout", "The SSH operation timed out.");
const TOO_LARGE: Error = operational("too_large", "Remote output exceeds the supported size.");

impl Error {
    pub(super) fn transport(failure: Failure, mutation: bool) -> Self {
        if mutation {
            return UNKNOWN;
        }
        match failure {
            Failure::Unavailable => SSH_UNAVAILABLE,
            Failure::Start => SSH_FAILED,
            Failure::Timeout => TIMEOUT,
            Failure::Cancelled => operational("remote_timeout", "The SSH operation was cancelled."),
            Failure::TooLarge => TOO_LARGE,
            Failure::Io => OUTPUT,
        }
    }

    pub(super) fn check_state(self) -> &'static str {
        match self.code {
            "ssh_unavailable" | "ssh_failed" | "remote_timeout" => "unreachable",
            "remote_larp_missing" => "larp_missing",
            _ => "incompatible",
        }
    }
}

// The Stage 13 receiver first ships in 0.1.1. Earlier 0.1.0 builds could not receive requests.
const COMPATIBLE: &[&str] = &["0.1.1"];

pub(super) fn probe(output: Output) -> Result<(), Error> {
    let code = transport_exit(&output)?;
    match (code, output.stdout.is_empty()) {
        (44, true) => return Err(LARP_MISSING),
        (45, true) => return Err(INCOMPATIBLE),
        (0, false) => (),
        _ => return Err(INCOMPATIBLE),
    }
    let valid = object_line(&output.stdout).is_some_and(|value| {
        success_shape(&value, "version")
            && value["revision"].is_null()
            && fields(&value["data"], &["version"])
            && value["data"]["version"]
                .as_str()
                .is_some_and(|v| COMPATIBLE.contains(&v))
    });
    if valid && output.sent {
        Ok(())
    } else {
        Err(INCOMPATIBLE)
    }
}

pub(super) fn transport_exit(output: &Output) -> Result<i32, Error> {
    if output.code == Some(255) {
        return Err(SSH_FAILED);
    }
    if !output.stderr.is_empty() {
        return Err(OUTPUT);
    }
    output.code.ok_or(OUTPUT)
}

pub(super) fn reply(output: Output, command: &str, mutation: bool) -> Result<Value, Error> {
    let invalid = if mutation { UNKNOWN } else { OUTPUT };
    let code = transport_exit(&output).map_err(|e| if mutation { UNKNOWN } else { e })?;
    let mut value = object_line(&output.stdout).ok_or(invalid)?;
    if code == 0 && output.sent && (command != "host.catalog.import" || output.ready) {
        if !success_shape(&value, command)
            || !success_data(command, &value["data"], &value["revision"])
        {
            return Err(invalid);
        }
        for field in ["warnings", "diagnostics"] {
            if let Some(rows) = value["data"].get_mut(field) {
                redact(rows);
            }
        }
        return Ok(value);
    }
    Err(confirmed_error(&value, command, code).ok_or(invalid)?)
}

pub(super) fn confirmed_error(value: &Value, command: &str, code: i32) -> Option<Error> {
    if !fields(value, &["schema", "command", "ok", "error"])
        || value["schema"].as_u64() != Some(1)
        || value["ok"].as_bool() != Some(false)
        || !fields(&value["error"], &["code", "message"])
        || !value["error"]["message"].is_string()
    {
        return None;
    }
    let reason = value["error"]["code"].as_str()?;
    if value["command"] != command
        && !(value["command"] == "parse" && reason == "usage" && code == 2)
    {
        return None;
    }
    let (reason, message, exit) = match (reason, code) {
        ("usage", 2) => ("usage", "The remote CLI refused the arguments.", 2),
        ("unsupported", 2) => ("unsupported", "The remote operation is unsupported.", 2),
        ("unsupported", 1) if command.starts_with("mcp.") || command == "host.catalog.apply" => (
            "unsupported",
            "The remote MCP transport or adapter is unsupported.",
            1,
        ),
        ("confirmation_required", 2) => (
            "confirmation_required",
            "The remote operation requires confirmation.",
            2,
        ),
        ("json_native_io", 2) => (
            "json_native_io",
            "Native operations do not support JSON output.",
            2,
        ),
        ("secret_literal", 1) => ("secret_literal", "A secret literal was refused.", 1),
        ("secret_reference", 1 | 2) => (
            "secret_reference",
            "A secret reference was refused.",
            code as u8,
        ),
        ("not_found", 1) => ("not_found", "The requested remote record was not found.", 1),
        ("conflict", 1) => (
            "conflict",
            "The remote revision or record conflicts. Refresh and confirm again.",
            1,
        ),
        ("busy", 1) => ("busy", "The remote registry is busy.", 1),
        ("unsafe_path", 1) => ("unsafe_path", "The remote path could not be verified.", 1),
        ("forbidden_source", 1) => (
            "forbidden_source",
            "The remote skill source is forbidden.",
            1,
        ),
        ("drift", 1) => (
            "drift",
            "Owned remote content or filesystem identity has changed.",
            1,
        ),
        ("qualification_pending", 1) => (
            "qualification_pending",
            "The operation is disabled pending qualification.",
            1,
        ),
        ("too_large", 1) => (
            "too_large",
            "The remote operation exceeded a size limit.",
            1,
        ),
        ("recovery_required", 1) => (
            "recovery_required",
            "The remote outcome is unknown until explicit recovery.",
            1,
        ),
        ("io", 1) => ("io", "Remote filesystem I/O could not be completed.", 1),
        ("corrupt", 1) => ("corrupt", "Remote data is invalid.", 1),
        ("newer_schema", 1) => (
            "newer_schema",
            "The remote registry schema is unsupported.",
            1,
        ),
        ("exhausted", 1) => ("exhausted", "Remote registry counters are exhausted.", 1),
        _ => return None,
    };
    Some(Error {
        code: reason,
        message,
        exit,
    })
}

fn success_shape(value: &Value, command: &str) -> bool {
    fields(value, &["schema", "command", "ok", "revision", "data"])
        && value["schema"].as_u64() == Some(1)
        && value["command"] == command
        && value["ok"].as_bool() == Some(true)
        && nullable(&value["revision"], |v| {
            v.as_str().and_then(decimal).is_some()
        })
}

pub(super) fn object_line(bytes: &[u8]) -> Option<Value> {
    let object = bytes.strip_suffix(b"\n")?;
    if object.first() != Some(&b'{') || object.last() != Some(&b'}') || object.contains(&b'\n') {
        return None;
    }
    serde_json::from_slice::<Unique>(object).ok().map(|v| v.0)
}

// Value normally accepts duplicate keys. Reject them at every depth before validation.
struct Unique(Value);

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Unique;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Unique, E> {
                Ok(Unique(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Unique, E> {
                Ok(Unique(Value::from(value)))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Unique, E> {
                Ok(Unique(Value::from(value)))
            }
            fn visit_f64<E: de::Error>(self, _value: f64) -> Result<Unique, E> {
                Err(E::custom("unexpected float"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Unique, E> {
                Ok(Unique(Value::from(value)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut input: A) -> Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(Unique(value)) = input.next_element()? {
                    values.push(value);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut input: A) -> Result<Unique, A::Error> {
                let mut values = Map::new();
                while let Some((key, Unique(value))) = input.next_entry()? {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate key"));
                    }
                }
                Ok(Unique(Value::Object(values)))
            }
        }
        input.deserialize_any(Visitor)
    }
}

pub(super) fn decimal(value: &str) -> Option<u64> {
    let number: u64 = value.parse().ok()?;
    (number.to_string() == value).then_some(number)
}

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn fields(value: &Value, names: &[&str]) -> bool {
    value.as_object().is_some_and(|map| {
        map.len() == names.len() && names.iter().all(|name| map.contains_key(*name))
    })
}

fn id(value: &Value) -> bool {
    value.as_str().and_then(decimal).is_some_and(|id| id != 0)
}

fn name(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| ProfileName::parse_new(s).is_ok())
}

fn nullable(value: &Value, check: impl FnOnce(&Value) -> bool) -> bool {
    value.is_null() || check(value)
}

fn one_of(value: &Value, choices: &[&str]) -> bool {
    value.as_str().is_some_and(|s| choices.contains(&s))
}

const PROVIDERS: &[&str] = &["claude", "codex"];
const SURFACES: &[&str] = &["cli", "desktop"];

fn array<'a>(value: &'a Value, check: impl FnMut(&'a Value) -> bool) -> bool {
    value.as_array().is_some_and(|rows| rows.iter().all(check))
}

fn named_rows<'a>(value: &'a Value, check: impl FnMut(&'a Value) -> bool) -> bool {
    array(value, check)
        && value
            .as_array()
            .unwrap()
            .windows(2)
            .all(|p| p[0]["name"].as_str() < p[1]["name"].as_str())
}

fn unique_ids(value: &Value) -> bool {
    let mut ids = HashSet::new();
    array(value, |row| ids.insert(row["id"].as_str()))
}

fn label(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        (1..=128).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
    })
}

fn binding(value: &Value) -> bool {
    fields(
        value,
        &[
            "path",
            "recorded_version",
            "detected_version",
            "state",
            "qualified",
        ],
    ) && nullable(&value["path"], Value::is_string)
        && nullable(&value["recorded_version"], label)
        && nullable(&value["detected_version"], |v| {
            v.as_str().is_some_and(super::address::word)
        })
        && value["qualified"].as_bool() == Some(false)
        && match value["state"].as_str() {
            Some("unbound") => {
                value["path"].is_null()
                    && value["recorded_version"].is_null()
                    && value["detected_version"].is_null()
            }
            Some("ok" | "invalid") => value["path"].is_string(),
            _ => false,
        }
}

fn surface(value: &Value) -> bool {
    fields(value, &["enabled", "path", "state"])
        && value["enabled"].is_boolean()
        && value["path"].is_string()
        && one_of(&value["state"], &["ok", "missing", "unsafe"])
}

fn alias(value: &Value) -> bool {
    fields(value, &["name", "profile_id", "surface", "path", "state"])
        && name(&value["name"])
        && id(&value["profile_id"])
        && one_of(&value["surface"], SURFACES)
        && value["path"].is_string()
        && one_of(&value["state"], &["ok", "missing", "drift", "incompatible"])
}

fn column(value: &Value) -> bool {
    id(&value["id"]) && name(&value["name"]) && one_of(&value["provider"], PROVIDERS)
}

fn profile(value: &Value) -> bool {
    fields(
        value,
        &[
            "id",
            "name",
            "provider",
            "state",
            "surfaces",
            "aliases",
            "desktop_binding",
        ],
    ) && column(value)
        && one_of(&value["state"], &["active", "deleting"])
        && fields(&value["surfaces"], SURFACES)
        && SURFACES.iter().all(|s| surface(&value["surfaces"][*s]))
        && named_rows(&value["aliases"], |a| {
            alias(a) && a["profile_id"] == value["id"]
        })
        && binding(&value["desktop_binding"])
}

fn profile_list(value: &Value) -> bool {
    let mut aliases = HashSet::new();
    fields(value, &["profiles"])
        && named_rows(&value["profiles"], profile)
        && unique_ids(&value["profiles"])
        && array(&value["profiles"], |p| {
            array(&p["aliases"], |a| aliases.insert(a["name"].as_str()))
        })
}

fn switches(value: &Value) -> bool {
    let mut last = 0;
    array(value, |item| {
        let current = item["profile_id"].as_str().and_then(decimal).unwrap_or(0);
        let valid = current > last
            && fields(item, &["profile_id", "enabled"])
            && item["enabled"].is_boolean();
        last = current;
        valid
    })
}

fn mcp(value: &Value) -> bool {
    fields(
        value,
        &["name", "transport", "secret_refs", "switches", "state"],
    ) && name(&value["name"])
        && one_of(&value["transport"], &["stdio", "http"])
        && switches(&value["switches"])
        && one_of(&value["state"], &["ok", "drift", "invalid"])
        && array(&value["secret_refs"], |r| {
            fields(r, &["kind", "name"])
                && one_of(&r["kind"], &["env", "keychain"])
                && r["name"].as_str().is_some_and(super::address::word)
        })
        && value["secret_refs"]
            .as_array()
            .unwrap()
            .windows(2)
            .all(|p| {
                let key = |i: usize| (p[i]["kind"].as_str(), p[i]["name"].as_str());
                key(0) < key(1)
            })
}

fn skills(value: &Value) -> bool {
    fields(value, &["name", "sha256", "switches", "state"])
        && name(&value["name"])
        && value["sha256"].as_str().is_some_and(digest)
        && switches(&value["switches"])
        && one_of(&value["state"], &["ok", "drift", "invalid"])
}

fn matrix(value: &Value) -> bool {
    fields(value, &["profiles", "rows"])
        && named_rows(&value["profiles"], |c| {
            fields(c, &["id", "name", "provider"]) && column(c)
        })
        && unique_ids(&value["profiles"])
        && named_rows(&value["rows"], |row| {
            fields(row, &["name", "enabled"])
                && name(&row["name"])
                && array(&row["enabled"], Value::is_boolean)
                && row["enabled"].as_array().unwrap().len()
                    == value["profiles"].as_array().unwrap().len()
        })
}

fn notices(value: &Value) -> bool {
    array(value, |d| {
        fields(d, &["code", "message"]) && d["code"].is_string() && d["message"].is_string()
    }) && value.as_array().unwrap().windows(2).all(|p| {
        let key = |i: usize| (p[i]["code"].as_str(), p[i]["message"].as_str());
        key(0) <= key(1)
    })
}

fn redact(value: &mut Value) {
    if let Some(rows) = value.as_array_mut() {
        for row in &mut *rows {
            let (code, message) = match row["code"].as_str() {
                Some("keychain_retained") => {
                    ("keychain_retained", "A Keychain reference was retained.")
                }
                _ => ("remote_notice", "The remote CLI reported a notice."),
            };
            row["code"] = Value::from(code);
            row["message"] = Value::from(message);
        }
        rows.sort_by(|a, b| a["code"].as_str().cmp(&b["code"].as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            code: Some(code),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
            sent: true,
            ready: false,
        }
    }

    fn line(value: Value) -> Vec<u8> {
        let mut result = serde_json::to_vec(&value).unwrap();
        result.push(b'\n');
        result
    }

    #[test]
    fn stage13_protocol_probe_classification_and_exact_version() {
        let compatible = br#"{"schema":1,"command":"version","ok":true,"revision":null,"data":{"version":"0.1.1"}}"#;
        let valid = [compatible.as_slice(), b"\n"].concat();
        assert_eq!(probe(output(0, &valid, b"")), Ok(()));
        assert_eq!(probe(output(44, b"", b"")), Err(LARP_MISSING));
        assert_eq!(probe(output(45, b"", b"")), Err(INCOMPATIBLE));
        assert_eq!(probe(output(44, b"unexpected", b"")), Err(INCOMPATIBLE));
        assert_eq!(probe(output(0, &valid, b"untrusted banner")), Err(OUTPUT));
        assert_eq!(
            probe(output(255, &valid, b"untrusted host key refusal")),
            Err(SSH_FAILED)
        );
        for version in ["0.0.9", "0.1.0", "0.1.2", "v0.1.1", "00.1.1", "0.1.1-dev"] {
            let response = line(
                json!({"schema":1,"command":"version","ok":true,"revision":null,"data":{"version":version}}),
            );
            assert_eq!(probe(output(0, &response, b"")), Err(INCOMPATIBLE));
        }
    }

    #[test]
    fn stage13_protocol_untrusted_reply_and_exit_validation() {
        let list = json!({"schema":1,"command":"profile.list","ok":true,"revision":"0","data":{"profiles":[]}});
        let valid = line(list.clone());
        assert_eq!(
            reply(output(0, &valid, b""), "profile.list", false),
            Ok(list)
        );
        for invalid in [
            valid[..valid.len() - 1].to_vec(),
            [&valid[..], b"\n"].concat(),
            [&valid[..], b"{}\n"].concat(),
            b"\xff\n".to_vec(),
            b"{\"schema\":2,\"command\":\"profile.list\",\"ok\":true,\"revision\":\"0\",\"data\":{\"profiles\":[]}}\n".to_vec(),
            b"{\"schema\":1,\"command\":\"profile.list\",\"ok\":true,\"revision\":\"0\",\"data\":{\"profiles\":[],\"profiles\":[]}}\n".to_vec(),
            line(json!({"schema":1,"command":"alias.list","ok":true,"revision":"0","data":{"aliases":[]}})),
            line(json!({"schema":1,"command":"profile.list","ok":true,"revision":"18446744073709551616","data":{"profiles":[]}})),
            line(json!({"schema":1,"command":"profile.list","ok":true,"revision":"0","data":{"profiles":[],"extra":false}})),
        ] {
            assert_eq!(reply(output(0, &invalid, b""), "profile.list", false), Err(OUTPUT));
        }
        assert_eq!(
            reply(output(1, &valid, b""), "profile.list", false),
            Err(OUTPUT)
        );
        for stderr in [
            b"banner with sentinel".as_slice(),
            b"post-quantum warning with sentinel",
        ] {
            assert_eq!(
                reply(output(0, &valid, stderr), "profile.list", false),
                Err(OUTPUT)
            );
            assert_eq!(
                reply(output(0, &valid, stderr), "profile.add", true),
                Err(UNKNOWN)
            );
        }
        for code in [
            "ssh_failed",
            "ssh_future",
            "remote_timeout",
            "remote_future",
            "catalog_unavailable",
        ] {
            let forged = line(
                json!({"schema":1,"command":"profile.add","ok":false,"error":{"code":code,"message":"sentinel"}}),
            );
            assert_eq!(
                reply(output(1, &forged, b""), "profile.add", true),
                Err(UNKNOWN)
            );
        }
        let conflict = line(
            json!({"schema":1,"command":"profile.add","ok":false,"error":{"code":"conflict","message":"sentinel"}}),
        );
        let safe = reply(output(1, &conflict, b""), "profile.add", true).unwrap_err();
        assert_eq!((safe.code, safe.exit), ("conflict", 1));
        assert!(!safe.message.contains("sentinel"));
        let parse = line(
            json!({"schema":1,"command":"parse","ok":false,"error":{"code":"usage","message":"sentinel"}}),
        );
        assert_eq!(
            reply(output(2, &parse, b""), "profile.add", true)
                .unwrap_err()
                .code,
            "usage"
        );
    }

    #[test]
    fn stage13_protocol_fixture_shapes_and_semantics() {
        let fixtures: Vec<Value> = serde_json::from_str(include_str!(
            "../../../../docs/contracts/cli-v1.examples.json"
        ))
        .unwrap();
        let mut checked = 0;
        for example in fixtures {
            let Some(command) = example["command"].as_str() else {
                continue;
            };
            if example["ok"] != true
                || !matches!(
                    command,
                    "profile.list"
                        | "profile.show"
                        | "alias.list"
                        | "mcp.list"
                        | "mcp.matrix"
                        | "skills.list"
                        | "skills.matrix"
                        | "status"
                        | "doctor"
                        | "profile.add"
                        | "host.catalog.list"
                        | "host.catalog.share"
                        | "host.catalog.apply"
                )
            {
                continue;
            }
            assert!(
                reply(output(0, &line(example.clone()), b""), command, false).is_ok(),
                "{command}"
            );
            checked += 1;
        }
        assert!(checked > 0);
        let matrix = json!({"schema":1,"command":"skills.matrix","ok":true,"revision":"2","data":{
           "profiles":[{"id":"1","name":"claude-work","provider":"claude"}],
           "rows":[{"name":"codex-45","enabled":[]}]
        }});
        assert_eq!(
            reply(output(0, &line(matrix), b""), "skills.matrix", false),
            Err(OUTPUT)
        );
        let rec = json!({"schema":1,"command":"profile.add","ok":true,"revision":"1","data":{
            "changed":true,"profile_id":"1","name":"claude-work","warnings":[{"code":"sentinel","message":"sentinel"}]
        }});
        let safe = reply(output(0, &line(rec), b""), "profile.add", true).unwrap();
        assert!(!safe.to_string().contains("sentinel"));
    }
}

fn status(value: &Value, command: &str) -> bool {
    let detections = &value["detected_versions"];
    fields(
        value,
        &[
            "profile",
            "detected_versions",
            "native_auth",
            "native_rows",
            "diagnostics",
        ],
    ) && (profile(&value["profile"]) || (command == "doctor" && value["profile"].is_null()))
        && value["native_auth"] == "not_observed"
        && value["native_rows"].as_array().is_some_and(Vec::is_empty)
        && notices(&value["diagnostics"])
        && array(detections, |d| {
            fields(d, &["provider", "surface", "binding"])
                && one_of(&d["provider"], PROVIDERS)
                && one_of(&d["surface"], SURFACES)
                && binding(&d["binding"])
                && (value["profile"].is_null() || d["provider"] == value["profile"]["provider"])
        })
        && detections.as_array().unwrap().windows(2).all(|p| {
            let key = |i: usize| (p[i]["provider"].as_str(), p[i]["surface"] == "desktop");
            key(0) < key(1)
        })
}

fn receipt(value: &Value, command: &str, revision: &Value) -> bool {
    if revision.is_null()
        || !fields(value, &["changed", "profile_id", "name", "warnings"])
        || !value["changed"].is_boolean()
        || !notices(&value["warnings"])
    {
        return false;
    }
    match command {
        "profile.add" | "profile.rename" | "profile.forget" | "profile.delete"
        | "profile.recover" | "profile.surface" | "alias.add" | "alias.remove"
        | "skills.enable" | "skills.disable" => name(&value["name"]) && id(&value["profile_id"]),
        "mcp.enable" | "mcp.disable" => name(&value["name"]) && nullable(&value["profile_id"], id),
        "skills.add"
        | "skills.remove"
        | "mcp.add"
        | "mcp.remove"
        | "host.catalog.share"
        | "host.catalog.apply"
        | "host.catalog.import" => name(&value["name"]) && value["profile_id"].is_null(),
        "profile.migrate" | "skills.sync" | "mcp.sync" | "mcp.sync-all" => {
            value["name"].is_null() && value["profile_id"].is_null()
        }
        _ => false,
    }
}

fn success_data(command: &str, value: &Value, revision: &Value) -> bool {
    match command {
        "profile.list" => profile_list(value),
        "profile.show" => profile(value),
        "alias.list" => fields(value, &["aliases"]) && named_rows(&value["aliases"], alias),
        "mcp.list" => {
            fields(value, &["sync_all", "servers"])
                && value["sync_all"].is_boolean()
                && named_rows(&value["servers"], mcp)
        }
        "skills.list" => fields(value, &["skills"]) && named_rows(&value["skills"], skills),
        "mcp.matrix" | "skills.matrix" => matrix(value),
        "status" | "doctor" => status(value, command),
        "host.catalog.list" => {
            !revision.is_null()
                && fields(value, &["kind", "entries"])
                && one_of(&value["kind"], &["mcp", "skills"])
                && named_rows(&value["entries"], |entry| {
                    fields(entry, &["name", "all_machines", "content_sha256"])
                        && name(&entry["name"])
                        && entry["all_machines"].is_boolean()
                        && nullable(&entry["content_sha256"], |v| v.as_str().is_some_and(digest))
                })
                && value["entries"].as_array().unwrap().len() <= 256
        }
        _ => receipt(value, command, revision),
    }
}
