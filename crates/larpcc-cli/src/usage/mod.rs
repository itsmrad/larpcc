pub(crate) mod claude;
pub(crate) mod cli;
pub(crate) mod clock;
mod codex;
mod files;
pub(crate) mod graph;
#[cfg(unix)]
pub(crate) mod hook;

use crate::registry::Profile;
use serde::Serialize;
use std::collections::BTreeSet;
use std::{ffi::OsStr, path::Path};

pub(super) const FIVE_HOUR: i64 = 18_000;
pub(super) const WEEKLY: i64 = 604_800;
pub(super) const MAX_WINDOW: i64 = 31_622_400;

#[derive(Serialize)]
struct Pace {
    elapsed_percent: f64,
    reserve_percent: f64,
    runs_out_at: Option<String>,
    lasts_until_reset: Option<bool>,
}

fn pace(used: i64, reset: i64, captured: i64, length: i64) -> Option<Pace> {
    let left = reset - captured;
    if left <= 0 || left > length + 300 {
        return None;
    }
    let elapsed = (length - left).max(0);
    let rounded = |numerator: i64| {
        let magnitude = (numerator.abs() * 2 + length) / (2 * length);
        (magnitude * numerator.signum()) as f64 / 10.0
    };
    let runout = if used == 0 {
        Some(reset)
    } else if elapsed == 0 {
        None
    } else {
        Some((captured + (1000 - used) * elapsed / used).min(reset))
    };
    Some(Pace {
        elapsed_percent: rounded(1000 * elapsed),
        reserve_percent: rounded(1000 * elapsed - used * length),
        runs_out_at: runout.map(clock::format),
        lasts_until_reset: runout.map(|instant| instant == reset),
    })
}

#[derive(Serialize)]
struct Window {
    used_percent: Option<f64>,
    resets_at: Option<String>,
    captured_at: Option<String>,
    window_seconds: Option<i64>,
    reason: Option<&'static str>,
    pace: Option<Pace>,
}

#[derive(Serialize)]
struct Diagnostic {
    code: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct Extra {
    label: String,
    #[serde(flatten)]
    window: Window,
}

#[derive(Serialize)]
struct Windows {
    five_hour: Window,
    weekly: Window,
    extra: Option<Vec<Extra>>,
}

struct Observation {
    source: Option<&'static str>,
    captured_at: Option<i64>,
    client_version: Option<String>,
    plan: Option<&'static str>,
    windows: Windows,
    diagnostics: BTreeSet<&'static str>,
}

impl Observation {
    fn missing(reason: &'static str, extras: bool) -> Self {
        Self {
            source: None,
            captured_at: None,
            client_version: None,
            plan: None,
            windows: Windows {
                five_hour: Window::missing(Some(FIVE_HOUR), reason),
                weekly: Window::missing(Some(WEEKLY), reason),
                extra: extras.then(Vec::new),
            },
            diagnostics: BTreeSet::new(),
        }
    }
}

fn diagnostic(code: &'static str) -> Diagnostic {
    let message = match code {
        "capture_not_installed" => "Claude usage capture is off or needs a qualified Larp launch.",
        "clock_skew" => "The usage record and reset time disagree beyond clock tolerance.",
        "extra_duplicate" => "Duplicate extra usage windows were omitted.",
        "extra_truncated" => "Extra usage windows were omitted after the output limit.",
        "invalid_record" => "A local usage record was invalid or unsafe.",
        "scan_limit" => "Only the bounded set of recent usage records was scanned.",
        "unsafe_path" => "The root path cannot be safely quoted for the usage hook.",
        "unsupported_os" => "Claude usage capture is unavailable on this operating system.",
        _ => "Usage is not available right now.",
    };
    Diagnostic { code, message }
}

fn profile_directory(root: &Path, id: u64) -> Result<Option<files::Directory>, ()> {
    let Some(root) = files::Directory::root(root)? else {
        return Ok(None);
    };
    let Some(profiles) = root.directory(OsStr::new("profiles"), true)? else {
        return Ok(None);
    };
    profiles.directory(OsStr::new(&id.to_string()), true)
}

#[derive(Serialize)]
struct Plan<'a> {
    label: &'a str,
    source: &'static str,
}

#[derive(Serialize)]
struct ResetCredits {
    state: &'static str,
    count: Option<u32>,
    expires_state: &'static str,
    expires_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Row<'a> {
    profile_id: String,
    name: &'a str,
    provider: crate::registry::Provider,
    mode: &'static str,
    source: Option<&'static str>,
    captured_at: Option<String>,
    client_version: Option<String>,
    plan: Option<Plan<'a>>,
    windows: Windows,
    reset_credits: ResetCredits,
    diagnostics: Vec<Diagnostic>,
}

pub(crate) fn local_row<'a>(
    root: &Path,
    profile: &'a Profile,
    label: Option<&'a str>,
    claude_capture: bool,
    read_clock: i64,
) -> Row<'a> {
    use crate::registry::Provider;
    let observation = match profile.provider {
        Provider::Claude => claude::observe(root, profile.id.get(), claude_capture, read_clock),
        Provider::Codex => {
            let cli = profile_directory(root, profile.id.get())
                .and_then(|profile| {
                    profile
                        .map(|dir| dir.directory(OsStr::new("cli"), true))
                        .transpose()
                })
                .map(Option::flatten);
            match cli {
                Ok(Some(cli)) => codex::observe(&cli, read_clock),
                Ok(None) => Observation::missing("not_captured", true),
                Err(()) => {
                    let mut row = Observation::missing("invalid_record", true);
                    row.diagnostics.insert("invalid_record");
                    row
                }
            }
        }
    };
    row(profile, label, "local", observation)
}

pub(crate) fn disabled_row(profile: &Profile, label: Option<&str>) -> serde_json::Value {
    let mut observation = Observation::missing("live_disabled", false);
    observation.diagnostics.insert("live_disabled");
    let mut row = row(profile, label, "live", observation);
    if profile.provider == crate::registry::Provider::Codex {
        row.reset_credits.state = "not_recorded";
        row.reset_credits.expires_state = "not_recorded";
    }
    serde_json::json!(row)
}

fn row<'a>(
    profile: &'a Profile,
    label: Option<&'a str>,
    mode: &'static str,
    observation: Observation,
) -> Row<'a> {
    Row {
        profile_id: profile.id.to_string(),
        name: profile.name.as_str(),
        provider: profile.provider,
        mode,
        source: observation.source,
        captured_at: observation.captured_at.map(clock::format),
        client_version: observation.client_version,
        plan: label
            .filter(|label| valid_label(label))
            .map(|label| Plan {
                label,
                source: "user",
            })
            .or_else(|| {
                observation.plan.map(|label| Plan {
                    label,
                    source: "client_local",
                })
            }),
        windows: observation.windows,
        reset_credits: ResetCredits {
            state: "unsupported",
            count: None,
            expires_state: "unsupported",
            expires_at: None,
        },
        diagnostics: observation
            .diagnostics
            .into_iter()
            .map(diagnostic)
            .collect(),
    }
}

impl Window {
    fn missing(length: Option<i64>, reason: &'static str) -> Self {
        Self {
            used_percent: None,
            resets_at: None,
            captured_at: None,
            window_seconds: length,
            reason: Some(reason),
            pace: None,
        }
    }

    fn from_record(
        used: Option<f64>,
        reset: Option<i64>,
        captured: i64,
        length: Option<i64>,
        read_clock: i64,
    ) -> Self {
        let used = match used {
            Some(value) => match percent(value) {
                Some(used) => Some(used),
                None => return Self::missing(length, "invalid_record"),
            },
            None => None,
        };
        if captured > read_clock + 300
            || reset.is_some_and(|value| !(clock::MIN_RESET..clock::MAX_RESET).contains(&value))
        {
            return Self::missing(length, "invalid_record");
        }
        if let Some(reset) = reset.filter(|&value| value <= read_clock) {
            let mut window = Self::missing(length, "reset_since_capture");
            window.resets_at = Some(clock::format(reset));
            return window;
        }
        Self {
            used_percent: used.map(|value| value as f64 / 10.0),
            resets_at: reset.map(clock::format),
            captured_at: used.map(|_| clock::format(captured)),
            window_seconds: length,
            reason: match (used, reset) {
                (Some(_), Some(_)) => None,
                (Some(_), None) => Some("reset_time_unknown"),
                _ => Some("not_reported"),
            },
            pace: used
                .zip(reset)
                .zip(length)
                .and_then(|((used, reset), length)| pace(used, reset, captured, length)),
        }
    }
}

fn percent(value: f64) -> Option<i64> {
    (0.0..=1000.0)
        .contains(&value)
        .then(|| (value.min(100.0) * 10.0).round() as i64)
}

fn version(value: &str) -> bool {
    let mut parts = value.split('.');
    [4, 4, 6].into_iter().all(|max| {
        parts.next().is_some_and(|part| {
            !part.is_empty() && part.len() <= max && part.bytes().all(|b| b.is_ascii_digit())
        })
    }) && parts.next().is_none()
}

pub(crate) fn valid_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    (1..=32).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && !label.ends_with(' ')
        && !label.contains("  ")
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b" ._+/()-".contains(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn stage11_pace() {
        let vectors: Vec<Value> = serde_json::from_str(include_str!(
            "../../../../docs/contracts/usage-pace-vectors.json"
        ))
        .unwrap();
        for vector in vectors {
            let actual = (|| {
                pace(
                    percent(vector["used_percent"].as_f64()?)?,
                    clock::parse(vector["resets_at"].as_str()?)?,
                    clock::parse(vector["captured_at"].as_str()?)?,
                    vector["window_seconds"].as_i64()?,
                )
            })();
            assert_eq!(json!(actual), vector["expect"], "{}", vector["name"]);
        }
    }

    #[test]
    fn stage11_read_clock_and_timestamp() {
        let capture = clock::parse("2026-11-21T12:00:00.123Z").unwrap();
        let reset = capture + 60;
        let row = Window::from_record(Some(31.25), Some(reset), capture, Some(FIVE_HOUR), reset);
        assert_eq!(row.reason, Some("reset_since_capture"));
        assert_eq!(row.resets_at.as_deref(), Some("2026-11-21T12:01:00Z"));
        assert!(row.used_percent.is_none() && row.captured_at.is_none() && row.pace.is_none());
        assert!(clock::parse("2100-02-29T00:00:00Z").is_none());
        assert_eq!(
            clock::format(clock::parse("2000-02-29T00:00:00Z").unwrap()),
            "2000-02-29T00:00:00Z"
        );
    }
}
