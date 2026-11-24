use super::{FIVE_HOUR, Observation, WEEKLY, Window, clock, profile_directory, version};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, IgnoredAny, MapAccess, Visitor},
};
use std::{collections::HashSet, ffi::OsStr, fmt, io::Read, path::Path};

const SETTINGS_LIMIT: u64 = 1024 * 1024;
const CAPTURE_NAME: &str = "claude-statusline.json";
const VERIFIED: &str = "2.1.325";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Presence {
    Absent,
    Present,
    Indeterminate,
}

struct StatusLineKey(bool);

impl<'de> Deserialize<'de> for StatusLineKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = StatusLineKey;

            fn expecting(&self, writer: &mut fmt::Formatter<'_>) -> fmt::Result {
                writer.write_str("a settings object with unique keys")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Self::Value, A::Error> {
                let mut seen = HashSet::new();
                let mut present = false;
                while let Some(key) = entries.next_key::<String>()? {
                    present |= key == "statusLine";
                    if !seen.insert(key) {
                        return Err(de::Error::custom("duplicate settings key"));
                    }
                    entries.next_value::<IgnoredAny>()?;
                }
                Ok(StatusLineKey(present))
            }
        }
        deserializer.deserialize_map(Object)
    }
}

fn checked_settings(file: Option<crate::platform::CheckedObject>) -> Presence {
    let Some(file) = file else {
        return Presence::Absent;
    };
    let Ok(metadata) = file.file.metadata() else {
        return Presence::Indeterminate;
    };
    if metadata.len() > SETTINGS_LIMIT {
        return Presence::Indeterminate;
    }
    let mut text = String::new();
    if file
        .file
        .take(SETTINGS_LIMIT + 1)
        .read_to_string(&mut text)
        .is_err()
        || text.len() as u64 > SETTINGS_LIMIT
    {
        return Presence::Indeterminate;
    }
    match serde_json::from_str::<StatusLineKey>(&text) {
        Ok(StatusLineKey(true)) => Presence::Present,
        Ok(StatusLineKey(false)) => Presence::Absent,
        Err(_) => Presence::Indeterminate,
    }
}

pub(crate) fn profile_presence(root: &Path, id: u64) -> Presence {
    let opened = (|| -> Result<Presence, ()> {
        let cli = profile_directory(root, id)?
            .ok_or(())?
            .directory(OsStr::new("cli"), true)?
            .ok_or(())?;
        let presence = checked_settings(cli.file(OsStr::new("settings.json"), false)?);
        cli.recheck()?;
        Ok(presence)
    })();
    opened.unwrap_or(Presence::Indeterminate)
}

#[cfg(unix)]
pub(crate) fn launch_presence(root: &Path, id: u64, launch_dir: &Path) -> Presence {
    let profile = profile_presence(root, id);
    if profile != Presence::Absent {
        return profile;
    }
    let checked = super::files::Directory::project_chain(launch_dir)
        .and_then(|chain| project_presence(chain.iter().rev()));
    checked.unwrap_or(Presence::Indeterminate)
}

#[cfg(unix)]
fn project_presence<'a>(
    parents: impl Iterator<Item = &'a super::files::Directory>,
) -> Result<Presence, ()> {
    let mut present = false;
    for parent in parents {
        parent.recheck()?;
        let Some(directory) = parent.directory(OsStr::new(".claude"), false)? else {
            continue;
        };
        for name in ["settings.json", "settings.local.json"] {
            match checked_settings(directory.file(OsStr::new(name), false)?) {
                Presence::Indeterminate => return Err(()),
                Presence::Present => present = true,
                Presence::Absent => {}
            }
        }
        directory.recheck()?;
    }
    Ok(if present {
        Presence::Present
    } else {
        Presence::Absent
    })
}

#[derive(Deserialize)]
#[cfg(unix)]
struct Input {
    version: String,
    rate_limits: Option<InputLimits>,
}

#[derive(Deserialize)]
#[cfg(unix)]
struct InputLimits {
    five_hour: Option<InputWindow>,
    seven_day: Option<InputWindow>,
}

#[derive(Deserialize)]
#[cfg(unix)]
struct InputWindow {
    used_percentage: f64,
    resets_at: i64,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct CapturedWindow {
    used_percent: f64,
    resets_at: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    version: u32,
    captured_at: String,
    client_version: String,
    #[serde(deserialize_with = "required_option")]
    five_hour: Option<CapturedWindow>,
    #[serde(deserialize_with = "required_option")]
    weekly: Option<CapturedWindow>,
}

fn required_option<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

#[cfg(unix)]
fn normalize(input: &[u8], now: i64) -> Option<Capture> {
    let value: Input = serde_json::from_slice(input).ok()?;
    let limits = value.rate_limits?;
    if !version(&value.version) {
        return None;
    }
    let window = |value: Option<InputWindow>| -> Result<Option<CapturedWindow>, ()> {
        value
            .map(|value| {
                if !(clock::MIN_RESET..clock::MAX_RESET).contains(&value.resets_at) {
                    return Err(());
                }
                let used = super::percent(value.used_percentage).ok_or(())?;
                Ok(CapturedWindow {
                    used_percent: used as f64 / 10.0,
                    resets_at: value.resets_at,
                })
            })
            .transpose()
    };
    Some(Capture {
        version: 1,
        captured_at: clock::format(now),
        client_version: value.version,
        five_hour: window(limits.five_hour).ok()?,
        weekly: window(limits.seven_day).ok()?,
    })
}

fn read_capture(directory: &super::files::Directory) -> Result<Option<Capture>, ()> {
    let Some(file) = directory.file(OsStr::new(CAPTURE_NAME), true)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.file
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    directory.recheck()?;
    if bytes.len() > 4096 {
        return Err(());
    }
    let capture: Capture = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if capture.version != 1
        || !version(&capture.client_version)
        || !whole_seconds(&capture.captured_at)
    {
        return Err(());
    }
    Ok(Some(capture))
}

fn whole_seconds(value: &str) -> bool {
    value.len() == 20 && clock::parse(value).is_some()
}

pub(super) fn observe(root: &Path, id: u64, enabled: bool, read_clock: i64) -> Observation {
    let mut absent = Observation::missing("capture_not_installed", false);
    absent.diagnostics.insert("capture_not_installed");
    if !enabled {
        return absent;
    }
    if cfg!(windows) {
        absent.diagnostics.insert("unsupported_os");
        return absent;
    }
    if !safe_root(root) {
        absent.diagnostics.insert("unsafe_path");
        return absent;
    }
    let capture = (|| {
        let Some(profile) = profile_directory(root, id)? else {
            return Ok(None);
        };
        let Some(directory) = profile.directory(OsStr::new("usage"), true)? else {
            return Ok(None);
        };
        read_capture(&directory)
    })();
    let capture = match capture {
        Ok(Some(capture)) => capture,
        Ok(None) => return absent,
        Err(()) => {
            let mut row = Observation::missing("invalid_record", false);
            row.diagnostics.insert("invalid_record");
            return row;
        }
    };
    let captured = clock::parse(&capture.captured_at).expect("validated capture time");
    if captured > read_clock + 300 {
        let mut row = Observation::missing("invalid_record", false);
        row.diagnostics.insert("invalid_record");
        return row;
    }
    let reason = if capture.client_version == VERIFIED {
        "not_reported"
    } else {
        "unverified_version"
    };
    let mut row = Observation::missing(reason, false);
    row.source = Some("claude_statusline");
    row.captured_at = Some(captured);
    row.client_version = Some(capture.client_version);
    if reason == "not_reported" {
        for (output, input, length) in [
            (&mut row.windows.five_hour, capture.five_hour, FIVE_HOUR),
            (&mut row.windows.weekly, capture.weekly, WEEKLY),
        ] {
            if let Some(window) = input {
                *output = Window::from_record(
                    Some(window.used_percent),
                    Some(window.resets_at),
                    captured,
                    Some(length),
                    read_clock,
                );
                if output.reason == Some("invalid_record") {
                    row.diagnostics.insert("invalid_record");
                }
                if output.reason.is_none() && window.resets_at - captured > length + 300 {
                    row.diagnostics.insert("clock_skew");
                }
            }
        }
    }
    row
}

pub(crate) fn safe_root(root: &Path) -> bool {
    root.is_absolute()
        && root.to_str().is_some_and(|text| {
            text.bytes()
                .all(|byte| byte == b' ' || (byte.is_ascii_graphic() && byte != b'\''))
        })
}

#[cfg(unix)]
pub(crate) fn render(root: &Path, id: u64) -> Option<Vec<u8>> {
    if cfg!(windows) || !safe_root(root) || id == 0 {
        return None;
    }
    let hook = root.join("bin/.larp-usage-hook");
    #[derive(Serialize)]
    struct Render {
        #[serde(rename = "statusLine")]
        status_line: Command,
    }
    #[derive(Serialize)]
    struct Command {
        #[serde(rename = "type")]
        kind: &'static str,
        command: String,
        padding: u8,
    }
    serde_json::to_vec(&Render {
        status_line: Command {
            kind: "command",
            command: format!("'{}' claude-statusline {id}", hook.to_str()?),
            padding: 0,
        },
    })
    .ok()
}

#[cfg(unix)]
pub(crate) fn capture_stdin<G>(
    root: &Path,
    id: u64,
    input: std::os::fd::RawFd,
    guard: impl FnOnce() -> Option<G>,
) {
    if !safe_root(root) || id == 0 {
        return;
    }
    let Some(bytes) = bounded_stdin(input) else {
        return;
    };
    let captured = clock::now();
    let Some(capture) = normalize(&bytes, captured) else {
        return;
    };
    if let Some(_guard) = guard() {
        let _ = save(root, id, &capture, captured);
    }
}

#[cfg(unix)]
fn bounded_stdin(input: std::os::fd::RawFd) -> Option<Vec<u8>> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(2);
    let limit = 256 * 1024;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        let mut event = libc::pollfd {
            fd: input,
            events: libc::POLLIN,
            revents: 0,
        };
        // The single descriptor is borrowed from stdin for the life of the hook.
        let ready = unsafe { libc::poll(&mut event, 1, left.as_millis().max(1) as i32) };
        if ready <= 0 {
            if ready < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            return None;
        }
        if event.revents & (libc::POLLIN | libc::POLLHUP) == 0 {
            return None;
        }
        let read = buffer.len().min(limit + 1 - bytes.len());
        // Only the initialized prefix produced by read is copied.
        let count = unsafe { libc::read(input, buffer.as_mut_ptr().cast(), read) };
        if count < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        if Instant::now() > deadline {
            return None;
        }
        if count == 0 {
            return Some(bytes);
        }
        bytes.extend_from_slice(&buffer[..count as usize]);
        if bytes.len() > limit {
            return None;
        }
    }
}

#[cfg(unix)]
fn save(root: &Path, id: u64, capture: &Capture, captured: i64) -> Result<(), ()> {
    let profile = profile_directory(root, id)?.ok_or(())?;
    let directory = profile.ensure_directory(c"usage")?;
    if let Some(previous) = read_capture(&directory)?
        && previous.client_version == capture.client_version
        && previous.five_hour == capture.five_hour
        && previous.weekly == capture.weekly
        && clock::parse(&previous.captured_at)
            .is_some_and(|previous| (0..60).contains(&(captured - previous)))
    {
        return Ok(());
    }
    let bytes = serde_json::to_vec(capture).map_err(|_| ())?;
    directory.replace_capture(&bytes, captured)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::registry::{Provider, Registry};
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    fn fixture() -> (tempfile::TempDir, Registry) {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
        let temp = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let registry = Registry::at(temp.path().join("managed")).unwrap();
        registry
            .add("claude-work".parse().unwrap(), Provider::Claude)
            .unwrap();
        (temp, registry)
    }

    #[test]
    fn stage11_local_claude_hook_capture_redaction_and_throttle() {
        let (_temp, registry) = fixture();
        let root = registry.root();
        let time = clock::parse("2026-11-21T12:30:00Z").unwrap();
        let mut input = serde_json::json!({
            "version":"2.1.325",
            "transcript_path":"private transcript sentinel",
            "rate_limits":{
                "five_hour":{"used_percentage":31.25,"resets_at":time+3600},
                "spend_limit": "private spend sentinel",
            }
        });
        let captured = |input: &serde_json::Value, at| {
            normalize(&serde_json::to_vec(input).unwrap(), at).unwrap()
        };
        save(root, 1, &captured(&input, time), time).unwrap();
        let path = root.join("profiles/1/usage/claude-statusline.json");
        let usage = path.parent().unwrap();
        let first = fs::read_to_string(&path).unwrap();
        assert!(!first.contains("private"));
        assert!(first.contains("\"used_percent\":31.3"));
        assert!(first.contains("\"weekly\":null"));
        save(root, 1, &captured(&input, time + 10), time + 10).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), first);
        assert!(normalize(br#"{"version":"2.1.325"}"#, time).is_none());
        assert!(normalize(br#"{"version":"2.1.325","rate_limits":{"five_hour":{"used_percentage":NaN,"resets_at":0}}}"#, time).is_none());
        let stale = usage.join(format!(".claude-statusline.next-{}-42", time - 70));
        let unrelated = usage.join(format!(".claude-statusline.next-{}-+42", time - 70));
        for temp in [&stale, &unrelated] {
            fs::write(temp, b"interrupted capture").unwrap();
            fs::set_permissions(temp, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), first);
        input["rate_limits"]["five_hour"]["used_percentage"] = serde_json::json!(50.0);
        save(root, 1, &captured(&input, time + 10), time + 10).unwrap();
        assert_ne!(fs::read_to_string(&path).unwrap(), first);
        assert!(!stale.exists() && unrelated.exists());
        let observed = observe(root, 1, true, time + 10);
        assert_eq!(observed.windows.five_hour.used_percent, Some(50.0));
        assert_eq!(observed.windows.weekly.reason, Some("not_reported"));
        assert_eq!(
            observe(root, 1, false, time + 10).windows.five_hour.reason,
            Some("capture_not_installed")
        );
    }

    #[test]
    fn stage11_local_claude_hook_bounds_stalled_stdin() {
        use std::{
            os::fd::AsRawFd,
            time::{Duration, Instant},
        };
        let (input, _writer) = std::io::pipe().unwrap();
        let started = Instant::now();
        assert!(bounded_stdin(input.as_raw_fd()).is_none());
        assert!(started.elapsed() >= Duration::from_millis(1900));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn stage11_statusline_check_parent_walk_confined_to_fixture() {
        let (temp, registry) = fixture();
        let project = temp.path().join("project");
        let leaf = project.join("leaf");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&leaf).unwrap();
        assert_eq!(profile_presence(registry.root(), 1), Presence::Absent);
        let chain = super::super::files::Directory::project_chain(&leaf).unwrap();
        let parents = || {
            chain
                .iter()
                .rev()
                .take_while(|dir| dir.path().starts_with(temp.path()))
        };
        assert_eq!(project_presence(parents()).unwrap(), Presence::Absent);
        let settings = project.join(".claude");
        fs::create_dir(&settings).unwrap();
        let file = settings.join("settings.local.json");
        fs::write(&file, br#"{"statusLine":{"private":"ignored"}}"#).unwrap();
        assert_eq!(project_presence(parents()).unwrap(), Presence::Present);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(project_presence(parents()).is_err());
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink("unread-target", &file).unwrap();
        assert!(project_presence(parents()).is_err());
        let too_deep = leaf.join("x/".repeat(65));
        assert!(super::super::files::Directory::project_chain(&too_deep).is_err());
    }
}
