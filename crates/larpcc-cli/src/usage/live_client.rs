use super::{Extra, FIVE_HOUR, MAX_WINDOW, Observation, ResetCredits, WEEKLY, Window, clock, row};
use crate::{
    launch::{LaunchPlan, admission},
    platform,
    registry::{Error, Profile, Provider, Registry},
};
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    process::{ChildStdin, ChildStdout, Stdio},
    time::{Duration, Instant},
};

pub(crate) const BUDGET: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(10);
const CAP: usize = 1024 * 1024;
const UNAVAILABLE: &str = "live_unavailable";
const SIGN_IN: &str = "sign_in_again";

pub(crate) fn read(
    registry: &Registry,
    profile: &Profile,
    label: Option<&str>,
    revision: u64,
    now: i64,
    deadline: Instant,
) -> Result<Value, Error> {
    if Instant::now() >= deadline {
        return Ok(super::missing_live_row(profile, label, UNAVAILABLE));
    }
    let admitted = admission::admit(
        registry,
        admission::Operation::Usage,
        &profile.name,
        vec![],
        admission::usage_rows(),
        &platform::usage_env_keys(),
    )?;
    if admitted.snapshot.revision != revision {
        return Err(Error::AdmissionChanged);
    }
    let id = admitted.snapshot.target.profile.id;
    let guard = admission::acquire_auth_guard(registry.root(), id)?;
    let mut plan = admitted.plan.clone();
    plan.cwd = Some(admitted.root.clone());
    let response = probe(
        &plan,
        profile.provider,
        deadline.min(Instant::now() + TIMEOUT),
        || {
            admission::revalidate_guard(registry.root(), id, &guard)?;
            admission::revalidate(&admitted)
        },
    )?;
    let (observation, count) = match response.and_then(|value| parse(profile.provider, &value, now))
    {
        Ok(result) => result,
        Err(reason) => {
            let mut observation = Observation::missing(reason, false);
            observation.diagnostics.insert(reason);
            (observation, None)
        }
    };

    let mut result = row(profile, label, "live", observation);
    result.reset_credits = credits(count);
    Ok(json!(result))
}

fn credits(count: Option<u32>) -> ResetCredits {
    let (state, expires_state) = match count {
        Some(0) => ("none", "none"),
        Some(_) => ("available", "unsupported"),
        None => ("not_recorded", "not_recorded"),
    };
    ResetCredits {
        state,
        count,
        expires_state,
        expires_at: None,
    }
}

fn probe(
    plan: &LaunchPlan,
    provider: Provider,
    deadline: Instant,
    revalidate: impl FnOnce() -> Result<(), Error>,
) -> Result<Result<Value, &'static str>, Error> {
    let Ok((mut command, signals)) = platform::usage_command(plan) else {
        return Ok(Err(UNAVAILABLE));
    };
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    revalidate()?;
    if Instant::now() >= deadline || signals.pending().is_some() {
        return Ok(Err(UNAVAILABLE));
    }
    let Ok(mut child) = command.spawn() else {
        return Ok(Err(UNAVAILABLE));
    };
    let pid = child.id() as libc::pid_t;
    let response = (|| {
        let mut wire = Wire {
            input: nonblocking(child.stdin.take().ok_or(UNAVAILABLE)?)?,
            output: nonblocking(child.stdout.take().ok_or(UNAVAILABLE)?)?,
            pending: Vec::new(),
            received: 0,
            deadline,
            signals: &signals,
        };
        match provider {
            Provider::Codex => {
                wire.send(json!({"id": 1, "method": "initialize", "params": {
                    "clientInfo": {"name": "larpcc", "version": env!("CARGO_PKG_VERSION")},
                    "capabilities": {"experimentalApi": true}
                }}))?;
                wire.reply(provider, 1)?.as_object().ok_or(UNAVAILABLE)?;
                wire.send(json!({"method": "initialized"}))?;
                wire.send(json!({"id": 2, "method": "account/rateLimits/read", "params": null}))?;
            }
            Provider::Claude => {
                wire.send(json!({"type":"control_request","request_id":"1",
                    "request":{"subtype":"initialize"}}))?;
                wire.reply(provider, 1)?.as_object().ok_or(UNAVAILABLE)?;
                wire.send(json!({"type":"control_request","request_id":"2",
                    "request":{"subtype":"get_usage","skip_behaviors":true}}))?;
            }
        }
        wire.reply(provider, 2)
    })();
    // The unreaped direct child is the session leader, so its group ID cannot be reused.
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    platform::cancel_reap(pid);
    if let Some(signal) = signals.pending() {
        signals.finish()?;
        platform::terminate_by_signal(signal)?;
    }
    Ok(response)
}

fn nonblocking<T: AsRawFd>(pipe: T) -> Result<T, &'static str> {
    let fd = pipe.as_raw_fd();
    // SAFETY: the owned pipe keeps this descriptor live.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(UNAVAILABLE);
    }
    Ok(pipe)
}

struct Wire<'a> {
    input: ChildStdin,
    output: ChildStdout,
    pending: Vec<u8>,
    received: usize,
    deadline: Instant,
    signals: &'a platform::AuthSignals,
}

impl Wire<'_> {
    fn check(&self) -> Result<(), &'static str> {
        if Instant::now() >= self.deadline || self.signals.pending().is_some() {
            return Err(UNAVAILABLE);
        }
        Ok(())
    }

    fn send(&mut self, frame: Value) -> Result<(), &'static str> {
        let line = format!("{frame}\n");
        let mut bytes = line.as_bytes();
        while !bytes.is_empty() {
            self.check()?;
            match self.input.write(bytes) {
                Ok(0) => return Err(UNAVAILABLE),
                Ok(n) => bytes = &bytes[n..],
                Err(error) => retry(error)?,
            }
        }
        Ok(())
    }

    fn reply(&mut self, provider: Provider, id: u64) -> Result<Value, &'static str> {
        let request_id = id.to_string();
        let mut scanned = 0;
        loop {
            self.check()?;
            if let Some(end) = self.pending[scanned..].iter().position(|&b| b == b'\n') {
                let end = scanned + end;
                let frame = serde_json::from_slice::<Value>(&self.pending[..end])
                    .map_err(|_| UNAVAILABLE)?;
                self.pending.drain(..=end);
                scanned = 0;
                match provider {
                    Provider::Codex if frame["id"].as_u64() == Some(id) => {
                        if let Some(error) = frame.get("error") {
                            return Err(failure(error["code"].as_i64(), error["message"].as_str()));
                        }
                        return frame.get("result").cloned().ok_or(UNAVAILABLE);
                    }
                    Provider::Claude if frame["type"] == "control_response" => {
                        let body = &frame["response"];
                        if body["request_id"] == request_id {
                            return match body["subtype"].as_str() {
                                Some("success") => body.get("response").cloned().ok_or(UNAVAILABLE),
                                Some("error") => Err(failure(None, body["error"].as_str())),
                                _ => Err(UNAVAILABLE),
                            };
                        }
                    }
                    _ => {}
                }
                continue;
            }
            scanned = self.pending.len();
            let mut chunk = [0; 8192];
            let size = chunk.len().min(CAP - self.received + 1);
            match self.output.read(&mut chunk[..size]) {
                Ok(0) => return Err(UNAVAILABLE),
                Ok(n) => {
                    self.received += n;
                    if self.received > CAP {
                        return Err(UNAVAILABLE);
                    }
                    self.pending.extend_from_slice(&chunk[..n]);
                }
                Err(error) => retry(error)?,
            }
        }
    }
}

fn retry(error: io::Error) -> Result<(), &'static str> {
    match error.kind() {
        io::ErrorKind::Interrupted => Ok(()),
        io::ErrorKind::WouldBlock => {
            std::thread::sleep(Duration::from_millis(2));
            Ok(())
        }
        _ => Err(UNAVAILABLE),
    }
}

fn failure(code: Option<i64>, message: Option<&str>) -> &'static str {
    if matches!(code, Some(401 | 403))
        || message.is_some_and(|text| {
            let text = text.to_ascii_lowercase();
            [
                "not signed in",
                "not logged in",
                "authentication required",
                "please run /login",
            ]
            .iter()
            .any(|reason| text.contains(reason))
        })
    {
        SIGN_IN
    } else {
        UNAVAILABLE
    }
}

fn parse(
    provider: Provider,
    reply: &Value,
    now: i64,
) -> Result<(Observation, Option<u32>), &'static str> {
    let mut result = Observation::missing("not_reported", true);
    let extras = result.windows.extra.as_mut().ok_or(UNAVAILABLE)?;
    let count = match provider {
        Provider::Codex => {
            let limits = reply["rateLimitsByLimitId"]
                .get("codex")
                .filter(|value| !value.is_null())
                .unwrap_or(&reply["rateLimits"]);
            if !limits.is_object()
                || limits
                    .get("limitId")
                    .is_some_and(|id| !id.is_null() && id != "codex")
            {
                return Err(UNAVAILABLE);
            }
            let mut found = false;
            for key in ["primary", "secondary"] {
                let entry = &limits[key];
                if entry.is_null() {
                    continue;
                }
                if entry.get("usedPercent").is_none() {
                    return Err(UNAVAILABLE);
                }
                found = true;
                let minutes = entry["windowDurationMins"].as_i64().ok_or(UNAVAILABLE)?;
                let length = minutes
                    .checked_mul(60)
                    .filter(|seconds| (1..=MAX_WINDOW).contains(seconds))
                    .ok_or(UNAVAILABLE)?;
                let reset = optional(&entry["resetsAt"], Value::as_i64)?;
                let window = measured(&entry["usedPercent"], reset, length, now)?;
                match length {
                    FIVE_HOUR => result.windows.five_hour = window,
                    WEEKLY => result.windows.weekly = window,
                    _ => extras.push(Extra {
                        label: format!("Other limit {} {minutes}m", extras.len() + 1),
                        window,
                    }),
                }
            }
            if !found {
                return Err(UNAVAILABLE);
            }
            result.plan = limits["planType"]
                .as_str()
                .and_then(super::codex::live_plan);
            reply["rateLimitResetCredits"]["availableCount"].as_u64()
        }
        Provider::Claude => {
            if reply["rate_limits_available"] == false {
                return Err(UNAVAILABLE);
            }
            let limits = &reply["rate_limits"];
            if !limits.is_object()
                || !["five_hour", "seven_day", "model_scoped"]
                    .iter()
                    .any(|key| limits.get(key).is_some())
            {
                return Err(UNAVAILABLE);
            }
            for (key, length, target) in [
                ("five_hour", FIVE_HOUR, &mut result.windows.five_hour),
                ("seven_day", WEEKLY, &mut result.windows.weekly),
            ] {
                if !limits[key].is_null() {
                    *target = claude_window(&limits[key], length, now)?;
                }
            }
            if !limits["model_scoped"].is_null() {
                let models = limits["model_scoped"].as_array().ok_or(UNAVAILABLE)?;
                for (i, entry) in models.iter().enumerate() {
                    if entry["utilization"].as_f64().is_none() {
                        continue;
                    }
                    let window = claude_window(entry, WEEKLY, now)?;
                    let name = entry["display_name"]
                        .as_str()
                        .filter(|name| safe_name(name))
                        .map_or_else(
                            || format!("Other limit {}", (i + 1).min(999)),
                            str::to_owned,
                        );
                    extras.push(Extra {
                        label: format!("{name} weekly"),
                        window,
                    });
                }
            }
            reply["rate_limit_reset_credits"]["available_count"].as_u64()
        }
    };
    extras.sort_by(|a, b| a.label.cmp(&b.label));
    extras.dedup_by(|later, earlier| {
        if later.label == earlier.label {
            result.diagnostics.insert("extra_duplicate");
            true
        } else {
            false
        }
    });
    if extras.len() > 8 {
        extras.truncate(8);
        result.diagnostics.insert("extra_truncated");
    }
    if [&result.windows.five_hour, &result.windows.weekly]
        .into_iter()
        .chain(extras.iter().map(|extra| &extra.window))
        .any(|window| {
            window.used_percent.is_some()
                && window
                    .resets_at
                    .as_deref()
                    .and_then(clock::parse)
                    .zip(window.window_seconds)
                    .is_some_and(|(reset, length)| reset - now > length + 300)
        })
    {
        result.diagnostics.insert("clock_skew");
    }
    result.source = Some("live_endpoint");
    result.captured_at = Some(now);
    Ok((
        result,
        count
            .filter(|&value| value <= 1000)
            .map(|value| value as u32),
    ))
}

fn safe_name(name: &str) -> bool {
    (1..=24).contains(&name.len())
        && !name.trim().is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .-".contains(&b))
}

fn optional<T>(
    value: &Value,
    parse: impl FnOnce(&Value) -> Option<T>,
) -> Result<Option<T>, &'static str> {
    if value.is_null() {
        Ok(None)
    } else {
        parse(value).map(Some).ok_or(UNAVAILABLE)
    }
}

fn claude_window(entry: &Value, length: i64, now: i64) -> Result<Window, &'static str> {
    if entry.get("utilization").is_none() {
        return Err(UNAVAILABLE);
    }
    let reset = optional(&entry["resets_at"], |value| clock::parse(value.as_str()?))?;
    measured(&entry["utilization"], reset, length, now)
}

fn measured(
    value: &Value,
    reset: Option<i64>,
    length: i64,
    now: i64,
) -> Result<Window, &'static str> {
    let used = optional(value, Value::as_f64)?;
    Ok(Window::from_record(used, reset, now, Some(length), now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage11_live_codex_keeps_valid_window_when_reset_is_out_of_range() {
        let now = clock::parse("2026-11-26T08:00:00Z").unwrap();
        let (observation, _) = parse(
            Provider::Codex,
            &json!({"rateLimits": {
                "primary": {"usedPercent": 31, "windowDurationMins": 300, "resetsAt": now + 3600},
                "secondary": {"usedPercent": 40, "windowDurationMins": 10080, "resetsAt": clock::MAX_RESET}
            }}),
            now,
        )
        .unwrap();
        assert_eq!(observation.windows.five_hour.used_percent, Some(31.0));
        assert_eq!(observation.windows.weekly.reason, Some("invalid_record"));
        assert_eq!(observation.windows.weekly.used_percent, None);
    }

    #[test]
    fn stage11_live_claude_normalizes_each_window_independently() {
        let now = clock::parse("2026-11-26T08:00:00Z").unwrap();
        let (observation, _) = parse(
            Provider::Claude,
            &json!({"rate_limits": {
                "five_hour": {"utilization": 101},
                "seven_day": {"utilization": 25},
                "model_scoped": [{"display_name": "Fable", "utilization": 1001}]
            }}),
            now,
        )
        .unwrap();
        assert_eq!(observation.windows.five_hour.used_percent, Some(100.0));
        assert_eq!(observation.windows.weekly.used_percent, Some(25.0));
        let extras = observation.windows.extra.unwrap();
        assert_eq!(extras[0].label, "Fable weekly");
        assert_eq!(extras[0].window.reason, Some("invalid_record"));
        assert_eq!(extras[0].window.used_percent, None);
    }

    #[test]
    fn stage11_live_model_name_is_bounded_and_ascii() {
        for name in ["Fable", "Fable 1.0-Pro", "123456789012345678901234"] {
            assert!(safe_name(name), "{name:?}");
        }
        for name in [
            "",
            " ",
            "1234567890123456789012345",
            "Fable/Pro",
            "Fable\n",
            "Fable\u{200b}",
            "Fablé",
            "Fable@Pro",
        ] {
            assert!(!safe_name(name), "{name:?}");
        }
    }
}
