use super::{
    Extra, FIVE_HOUR, MAX_WINDOW, Observation, WEEKLY, Window, clock, files::Directory, version,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    time::{Duration, Instant},
};

const MIB: u64 = 1024 * 1024;
const SOURCE: &str = "codex_session_log";
const VERIFIED: &str = "0.169.0";

#[derive(Clone, Copy, Debug)]
enum Scan {
    Invalid,
    Limit,
}

impl From<()> for Scan {
    fn from(_: ()) -> Self {
        Self::Invalid
    }
}

struct Budget {
    deadline: Instant,
    entries: usize,
    days: usize,
    files: usize,
    bytes: u64,
}

impl Budget {
    fn new() -> Self {
        Self {
            deadline: Instant::now() + Duration::from_secs(2),
            entries: 0,
            days: 0,
            files: 0,
            bytes: 0,
        }
    }

    fn time(&self) -> Result<(), Scan> {
        if Instant::now() >= self.deadline {
            Err(Scan::Limit)
        } else {
            Ok(())
        }
    }

    fn names(
        &mut self,
        dir: &Directory,
        matches: impl Fn(&str) -> bool,
    ) -> (Vec<String>, Result<(), Scan>) {
        let mut names = Vec::new();
        let mut limit = Ok(());
        let walked = dir.names(|entry| {
            if self.entries == 2000 || self.time().is_err() {
                limit = Err(Scan::Limit);
                return false;
            }
            self.entries += 1;
            if let Some(name) = entry.to_str().filter(|name| matches(name)) {
                names.push(name.to_owned());
            }
            true
        });
        names.sort_unstable_by(|a, b| b.cmp(a));
        (names, walked.map_err(Scan::from).and(limit))
    }
}

fn digits(name: &str, count: usize, range: std::ops::RangeInclusive<u32>) -> bool {
    name.len() == count
        && name.bytes().all(|b| b.is_ascii_digit())
        && name.parse().is_ok_and(|value| range.contains(&value))
}

fn uuid(name: &str) -> bool {
    name.len() == 36
        && name.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

fn rollout(name: &str) -> bool {
    let Some(body) = name
        .strip_prefix("rollout-")
        .and_then(|s| s.strip_suffix(".jsonl"))
    else {
        return false;
    };
    let Some((timestamp, suffix)) = body.split_at_checked(19) else {
        return false;
    };
    let Some(ids) = suffix.strip_prefix('-') else {
        return false;
    };
    if timestamp.as_bytes()[13] != b'-' || timestamp.as_bytes()[16] != b'-' || !timestamp.is_ascii()
    {
        return false;
    }
    let stamp = format!(
        "{}:{}:{}Z",
        &timestamp[..13],
        &timestamp[14..16],
        &timestamp[17..19]
    );
    clock::parse(&stamp).is_some()
        && ids.split_once('_').map_or_else(
            || uuid(ids),
            |(thread, rollout)| uuid(thread) && uuid(rollout),
        )
}

#[derive(Deserialize)]
struct Line<T> {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    payload: Option<T>,
}

#[derive(Deserialize)]
struct EventKind {
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct Meta {
    cli_version: Option<String>,
}

#[derive(Deserialize)]
struct Event {
    rate_limits: Option<Limits>,
}

#[derive(Deserialize)]
struct Limits {
    limit_id: Option<String>,
    primary: Option<LimitWindow>,
    secondary: Option<LimitWindow>,
    plan_type: Option<String>,
}

#[derive(Deserialize)]
struct LimitWindow {
    used_percent: Option<f64>,
    resets_at: Option<i64>,
    window_minutes: Option<i64>,
}

struct Record {
    timestamp: (i64, u32),
    client: Option<String>,
    limits: Limits,
}

fn event_time(value: &str) -> Option<(i64, u32)> {
    let seconds = clock::parse(value)?;
    let nanos = value.split_once('.').map_or(0, |(_, fraction)| {
        let digits = fraction.trim_end_matches('Z');
        digits.parse::<u32>().unwrap_or(0) * 10_u32.pow(9 - digits.len() as u32)
    });
    Some((seconds, nanos))
}

fn read_line(
    input: &mut impl BufRead,
    buffer: &mut Vec<u8>,
    budget: &mut Budget,
    max: u64,
) -> Result<bool, Scan> {
    buffer.clear();
    let cap = max.min(40 * MIB - budget.bytes);
    if cap == 0 {
        return Err(Scan::Limit);
    }
    let mut limited = input.take(cap);
    while limited.limit() != 0 {
        budget.time()?;
        let chunk = limited.fill_buf().map_err(|_| Scan::Invalid)?;
        if chunk.is_empty() {
            return Ok(false);
        }
        let newline = chunk.iter().position(|&b| b == b'\n');
        let count = newline.map_or(chunk.len(), |i| i + 1);
        buffer.extend_from_slice(&chunk[..count]);
        limited.consume(count);
        budget.bytes += count as u64;
        if newline.is_some() {
            budget.time()?;
            return Ok(true);
        }
    }
    Err(Scan::Limit)
}

fn scan_file(
    file: &mut File,
    budget: &mut Budget,
    records: &mut BTreeMap<String, Record>,
    diagnostics: &mut BTreeSet<&'static str>,
) -> Result<(), Scan> {
    let size = file.metadata().map_err(|_| Scan::Invalid)?.len();
    let first_cap = size.min(MIB).min(40 * MIB - budget.bytes);
    let mut input = BufReader::new(file.take(first_cap));
    let mut buffer = Vec::new();
    if !read_line(&mut input, &mut buffer, budget, MIB)? {
        return Ok(());
    }
    let first = buffer.len() as u64;
    budget.bytes += first_cap - input.get_ref().limit() - first;
    let client = match serde_json::from_slice::<Line<serde::de::IgnoredAny>>(&buffer) {
        Ok(line) if line.kind == "session_meta" => serde_json::from_slice::<Line<Meta>>(&buffer)
            .map_err(|_| Scan::Invalid)?
            .payload
            .and_then(|meta| meta.cli_version)
            .filter(|value| version(value)),
        Ok(_) => None,
        Err(_) => {
            diagnostics.insert("invalid_record");
            None
        }
    };
    let tail = size.saturating_sub(4 * MIB);
    let file = input.into_inner().into_inner();
    let offset = tail.max(first);
    let mut skip_fragment = false;
    if tail > first {
        diagnostics.insert("scan_limit");
        budget.time()?;
        if budget.bytes == 40 * MIB {
            return Err(Scan::Limit);
        }
        file.seek(SeekFrom::Start(tail - 1))
            .map_err(|_| Scan::Invalid)?;
        let mut previous = [0];
        file.read_exact(&mut previous).map_err(|_| Scan::Invalid)?;
        budget.bytes += 1;
        skip_fragment = previous[0] != b'\n';
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| Scan::Invalid)?;
    let tail_cap = (size - offset).min(40 * MIB - budget.bytes);
    let mut input = BufReader::new(file.take(tail_cap));
    if skip_fragment && !read_line(&mut input, &mut buffer, budget, 4 * MIB)? {
        return Ok(());
    }
    for _ in 0..19_999 {
        if !read_line(&mut input, &mut buffer, budget, 4 * MIB)? {
            return Ok(());
        }
        budget.time()?;
        let header = match serde_json::from_slice::<Line<EventKind>>(&buffer) {
            Ok(header) => header,
            Err(_) => {
                diagnostics.insert("invalid_record");
                continue;
            }
        };
        if header.kind != "event_msg"
            || header.payload.and_then(|p| p.kind).as_deref() != Some("token_count")
        {
            continue;
        }
        let Some(timestamp) = header.timestamp.as_deref().and_then(event_time) else {
            diagnostics.insert("invalid_record");
            continue;
        };
        let limits = match serde_json::from_slice::<Line<Event>>(&buffer) {
            Ok(line) => line.payload.and_then(|event| event.rate_limits),
            Err(_) => {
                diagnostics.insert("invalid_record");
                continue;
            }
        };
        budget.time()?;
        if let Some(mut limits) = limits {
            let bucket = limits
                .limit_id
                .take()
                .unwrap_or_else(|| "codex".to_string());
            if records
                .get(&bucket)
                .is_none_or(|current| current.timestamp <= timestamp)
            {
                records.insert(
                    bucket,
                    Record {
                        timestamp,
                        client: client.clone(),
                        limits,
                    },
                );
            }
        }
    }
    match input.fill_buf() {
        Ok([]) => Ok(()),
        Ok(_) => Err(Scan::Limit),
        Err(_) => Err(Scan::Invalid),
    }
}

fn scan(
    cli: &Directory,
    budget: &mut Budget,
    records: &mut BTreeMap<String, Record>,
    diagnostics: &mut BTreeSet<&'static str>,
) -> Result<(), Scan> {
    let Some(sessions) = cli.directory(OsStr::new("sessions"), false)? else {
        return Ok(());
    };
    let (years, read_years) = budget.names(&sessions, |name| digits(name, 4, 1970..=9999));
    for year in years {
        let Some(year) = sessions.directory(OsStr::new(&year), false)? else {
            continue;
        };
        let (months, read_months) = budget.names(&year, |name| digits(name, 2, 1..=12));
        for month in months {
            let Some(month) = year.directory(OsStr::new(&month), false)? else {
                continue;
            };
            let (days, read_days) = budget.names(&month, |name| digits(name, 2, 1..=31));
            for day in days {
                budget.time()?;
                if budget.days == 62 {
                    return Err(Scan::Limit);
                }
                budget.days += 1;
                let Some(day) = month.directory(OsStr::new(&day), false)? else {
                    continue;
                };
                let (names, read_names) = budget.names(&day, rollout);
                for name in names {
                    budget.time()?;
                    if budget.files == 8 {
                        return Err(Scan::Limit);
                    }
                    budget.files += 1;
                    let name = OsStr::new(&name);
                    let Some(mut file) = day.file(name, false)? else {
                        continue;
                    };
                    let mut found = BTreeMap::new();
                    let outcome = scan_file(&mut file.file, budget, &mut found, diagnostics);
                    for dir in [cli, &sessions, &year, &month, &day] {
                        dir.recheck()?;
                    }
                    if !day
                        .file(name, false)?
                        .is_some_and(|after| after.identity == file.identity)
                    {
                        return Err(Scan::Invalid);
                    }
                    for (bucket, record) in found {
                        if records
                            .get(&bucket)
                            .is_none_or(|current| current.timestamp < record.timestamp)
                        {
                            records.insert(bucket, record);
                        }
                    }
                    outcome?;
                }
                read_names?;
            }
            read_days?;
        }
        read_months?;
    }
    read_years
}

fn category(minutes: Option<i64>) -> Option<&'static str> {
    let minutes = minutes?;
    for (base, label) in [
        (300, "5h"),
        (1440, "daily"),
        (10080, "weekly"),
        (43200, "monthly"),
        (525600, "annual"),
    ] {
        if (base * 95..=base * 105).contains(&(minutes.checked_mul(100)?)) {
            return Some(label);
        }
    }
    None
}

fn local_plan(plan: &str) -> Option<&'static str> {
    [
        "free",
        "go",
        "plus",
        "pro",
        "prolite",
        "promax",
        "team",
        "self_serve_business_prolite",
        "self_serve_business_usage_based",
        "business",
        "ent26",
        "enterprise_cbp_automation",
        "enterprise_cbp_usage_based",
        "enterprise",
        "edu",
        "edu_plus",
        "edu_pro",
        "sci",
        "sci_plus",
    ]
    .into_iter()
    .find(|&allowed| allowed == plan)
}

fn window(
    limit: &LimitWindow,
    record: &Record,
    length: Option<i64>,
    read_clock: i64,
    diagnostics: &mut BTreeSet<&'static str>,
) -> Window {
    if record.client.as_deref() != Some(VERIFIED) {
        return Window::missing(length, "unverified_version");
    }
    if limit.window_minutes.is_some() && length.is_none() {
        diagnostics.insert("invalid_record");
        return Window::missing(length, "invalid_record");
    }
    let captured = record.timestamp.0;
    let result = Window::from_record(
        limit.used_percent,
        limit.resets_at,
        captured,
        length,
        read_clock,
    );
    if result.reason == Some("invalid_record") {
        diagnostics.insert("invalid_record");
    }
    if result.reason.is_none()
        && limit
            .resets_at
            .zip(length)
            .is_some_and(|(r, w)| r - captured > w + 300)
    {
        diagnostics.insert("clock_skew");
    }
    result
}

pub(super) fn observe(cli: &Directory, read_clock: i64) -> Observation {
    let mut records = BTreeMap::new();
    let mut diagnostics = BTreeSet::new();
    match scan(cli, &mut Budget::new(), &mut records, &mut diagnostics) {
        Err(Scan::Limit) => {
            diagnostics.insert("scan_limit");
        }
        Err(Scan::Invalid) => {
            diagnostics.insert("invalid_record");
        }
        Ok(()) => {}
    }
    let newest = records
        .values()
        .filter(|record| record.timestamp.0 <= read_clock + 300)
        .max_by_key(|record| record.timestamp);
    if records
        .values()
        .any(|record| record.timestamp.0 > read_clock + 300)
    {
        diagnostics.insert("invalid_record");
    }
    let Some(newest) = newest else {
        let reason = if diagnostics.contains("invalid_record") {
            "invalid_record"
        } else {
            "not_captured"
        };
        let mut row = Observation::missing(reason, true);
        row.diagnostics = diagnostics;
        return row;
    };
    let codex = records.get("codex");
    let mut row = Observation::missing(
        if codex.unwrap_or(newest).client.as_deref() == Some(VERIFIED) {
            "not_reported"
        } else {
            "unverified_version"
        },
        true,
    );
    row.source = Some(SOURCE);
    row.client_version = newest.client.clone();
    row.plan = codex
        .filter(|record| {
            record.client.as_deref() == Some(VERIFIED) && record.timestamp.0 <= read_clock + 300
        })
        .and_then(|record| record.limits.plan_type.as_deref())
        .and_then(local_plan);
    let main = |name| {
        codex.and_then(|record| {
            [&record.limits.primary, &record.limits.secondary]
                .into_iter()
                .flatten()
                .find(|limit| category(limit.window_minutes) == Some(name))
                .map(|limit| (record, limit))
        })
    };
    let five_hour = main("5h");
    let weekly = main("weekly");
    for (main, output, seconds) in [
        (five_hour, &mut row.windows.five_hour, FIVE_HOUR),
        (weekly, &mut row.windows.weekly, WEEKLY),
    ] {
        if let Some((record, limit)) = main {
            *output = window(limit, record, Some(seconds), read_clock, &mut diagnostics);
        }
    }
    let mut extras = Vec::new();
    let mut other = 0;
    for (bucket, record) in &records {
        let codex = bucket == "codex";
        if !codex {
            other += 1;
            if other > 999 {
                diagnostics.insert("extra_truncated");
                continue;
            }
        }
        for (limit, fallback) in [
            (&record.limits.primary, None),
            (&record.limits.secondary, Some("secondary")),
        ] {
            let Some(limit) = limit else { continue };
            if [five_hour, weekly]
                .into_iter()
                .flatten()
                .any(|(_, main)| std::ptr::eq(limit, main))
            {
                continue;
            }
            let category = category(limit.window_minutes);
            let length = limit
                .window_minutes
                .and_then(|minutes| minutes.checked_mul(60))
                .filter(|seconds| (1..=MAX_WINDOW).contains(seconds));
            let bucket = if codex {
                "Codex".to_owned()
            } else {
                format!("Other limit {other}")
            };
            let duration = category
                .map(str::to_owned)
                .or_else(|| length.map(|seconds| format!("{}m", seconds / 60)))
                .or_else(|| fallback.map(str::to_owned));
            let label =
                duration.map_or_else(|| bucket.clone(), |duration| format!("{bucket} {duration}"));
            extras.push(Extra {
                label,
                window: window(limit, record, length, read_clock, &mut diagnostics),
            });
        }
    }
    extras.sort_by(|a, b| {
        a.label.cmp(&b.label).then_with(|| {
            a.window
                .window_seconds
                .unwrap_or(i64::MAX)
                .cmp(&b.window.window_seconds.unwrap_or(i64::MAX))
        })
    });
    let mut labels = BTreeSet::new();
    extras.retain(|entry| {
        let keep = labels.insert(entry.label.clone());
        if !keep {
            diagnostics.insert("extra_duplicate");
        }
        keep
    });
    if extras.len() > 8 {
        diagnostics.insert("extra_truncated");
        extras.truncate(8);
    }
    let captured = [&row.windows.five_hour, &row.windows.weekly]
        .into_iter()
        .chain(extras.iter().map(|entry| &entry.window))
        .filter_map(|window| window.captured_at.as_deref())
        .max()
        .and_then(clock::parse);
    row.captured_at = captured.or(Some(newest.timestamp.0));
    row.windows.extra = Some(extras);
    row.diagnostics = diagnostics;
    row
}
