use super::{Error, Profile, ProfileName, ProfileState, Provider, Registry, paths, store};
use crate::platform::{self, ObjectKind};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt, io::Read, io::Write, path::Path, time::Duration};

#[path = "../usage/mod.rs"]
pub(crate) mod data;

const SETTINGS: &str = "usage.json";
const NEXT: &str = "usage.json.next";
const JOURNAL: &str = "share-journal.json";
const STAGED_JOURNAL: &str = "usage.json.journal.tmp";
const LIMIT: u64 = 64 * 1024;
const CURRENT_WARNING: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    #[default]
    Inherit,
    Local,
    Live,
}

#[derive(Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct Live {
    enabled: bool,
    #[serde(deserialize_with = "required_option")]
    acknowledged_warning: Option<u32>,
}

#[derive(Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct Options {
    mode: Mode,
    #[serde(deserialize_with = "required_option")]
    plan_label: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    claude_statusline: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u32,
    mode: Mode,
    live: Live,
    #[serde(deserialize_with = "required_option")]
    refresh_interval_seconds: Option<u32>,
    #[serde(deserialize_with = "unique_profiles")]
    profiles: BTreeMap<String, Options>,
}

fn required_option<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

fn unique_profiles<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Options>, D::Error> {
    struct Unique;
    impl<'de> Visitor<'de> for Unique {
        type Value = BTreeMap<String, Options>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object with unique profile IDs")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Self::Value, A::Error> {
            let mut profiles = BTreeMap::new();
            while let Some((id, settings)) = entries.next_entry()? {
                if profiles.insert(id, settings).is_some() {
                    return Err(de::Error::custom("duplicate profile ID"));
                }
            }
            Ok(profiles)
        }
    }
    d.deserialize_map(Unique)
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            mode: Mode::Local,
            live: Live::default(),
            refresh_interval_seconds: None,
            profiles: BTreeMap::new(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Journal {
    UsageSettings {
        base: u64,
        target: u64,
        sha256: String,
    },
}

fn read(root: &Path, name: &str, limit: u64) -> Result<Option<Vec<u8>>, Error> {
    let path = root.join(name);
    let Some(parent) = platform::inspect(root, ObjectKind::Directory)? else {
        return Ok(None);
    };
    let Some(opened) = platform::alias_open_at(&parent, &path, ObjectKind::File, false)? else {
        return Ok(None);
    };
    platform::single_link(&opened.file)?;
    let mut bytes = Vec::new();
    opened
        .file
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| paths::io("read", &path, e))?;
    if bytes.len() as u64 > limit {
        return Err(Error::TooLarge);
    }
    Ok(Some(bytes))
}

fn create(root: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let path = root.join(name);
    let mut file = platform::create_file(&path)?.file;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| paths::io("write", &path, e))?;
    paths::sync(root)
}

fn retire(root: &Path, name: &str) -> Result<(), Error> {
    let path = root.join(name);
    let Some(parent) = platform::inspect(root, ObjectKind::Directory)? else {
        return Ok(());
    };
    let Some(opened) = platform::alias_open_at(&parent, &path, ObjectKind::File, false)? else {
        return Ok(());
    };
    let identity = opened.identity;
    drop(opened.file);
    platform::alias_unlink_at(&parent, &path, &identity)?;
    paths::sync(root)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn parse_settings(bytes: &[u8]) -> Result<Settings, Error> {
    let settings: Settings = serde_json::from_slice(bytes).map_err(|_| Error::RecoveryRequired)?;
    if settings.version != 1
        || settings.mode == Mode::Inherit
        || settings
            .live
            .acknowledged_warning
            .is_some_and(|v| !(1..=1000).contains(&v))
        || settings
            .refresh_interval_seconds
            .is_some_and(|v| !(300..=86400).contains(&v))
        || (settings.mode == Mode::Live && !settings.live.enabled)
        || settings.profiles.iter().any(|(id, options)| {
            !id.parse::<u64>()
                .is_ok_and(|value| value != 0 && value.to_string() == *id)
                || options
                    .plan_label
                    .as_deref()
                    .is_some_and(|label| !data::valid_label(label))
                || (options.mode == Mode::Live && !settings.live.enabled)
        })
    {
        return Err(Error::RecoveryRequired);
    }
    Ok(settings)
}

fn load(root: &Path) -> Result<Settings, Error> {
    read(root, SETTINGS, LIMIT)
        .map_err(|_| Error::RecoveryRequired)?
        .map_or_else(|| Ok(Settings::default()), |bytes| parse_settings(&bytes))
}

/// Runs while acquiring the existing registry lock, before its caller checks a revision.
pub(super) fn recover(root: &Path) -> Result<(), Error> {
    if !crate::share::pending(root) {
        retire(root, NEXT).map_err(|_| Error::RecoveryRequired)?;
        retire(root, STAGED_JOURNAL).map_err(|_| Error::RecoveryRequired)?;
        return Ok(());
    }
    // Other transaction kinds retain their existing explicit recovery paths.
    let Ok(Some(bytes)) = read(root, JOURNAL, 1024) else {
        return Ok(());
    };
    let Ok(Journal::UsageSettings {
        base,
        target,
        sha256,
    }) = serde_json::from_slice(&bytes)
    else {
        return Ok(());
    };
    let recover = || {
        if base.checked_add(1) != Some(target)
            || sha256.len() != 64
            || !sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::RecoveryRequired);
        }
        let data = store::load_unchecked(root)?;
        if data.revision == base {
            retire(root, NEXT)?;
        } else if data.revision == target {
            let next = read(root, NEXT, LIMIT)?;
            let staged = next.is_some();
            let content = match next {
                Some(bytes) => bytes,
                None => read(root, SETTINGS, LIMIT)?.ok_or(Error::RecoveryRequired)?,
            };
            if digest(&content) != sha256 || parse_settings(&content).is_err() {
                return Err(Error::RecoveryRequired);
            }
            if staged {
                promote(root)?;
            } else {
                paths::sync(root)?;
            }
        } else {
            return Err(Error::RecoveryRequired);
        }
        retire(root, STAGED_JOURNAL)?;
        retire(root, JOURNAL)
    };
    recover().map_err(|_| Error::RecoveryRequired)
}

fn promote(root: &Path) -> Result<(), Error> {
    read(root, SETTINGS, LIMIT)?;
    store::replace(&root.join(NEXT), &root.join(SETTINGS))?;
    paths::sync(root)
}

fn persist(root: &Path, mut data: store::Data, settings: &Settings) -> Result<u64, Error> {
    let base = data.revision;
    let target = base.checked_add(1).ok_or(Error::Exhausted)?;
    data.revision = target;
    let registry = store::serialize(&data)?;
    let bytes = serde_json::to_vec(settings).map_err(|_| Error::RecoveryRequired)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::TooLarge);
    }
    let journal = Journal::UsageSettings {
        base,
        target,
        sha256: digest(&bytes),
    };
    let journal = serde_json::to_vec(&journal).map_err(|_| Error::RecoveryRequired)?;
    create(root, NEXT, &bytes)?;
    create(root, STAGED_JOURNAL, &journal)?;
    store::replace(&root.join(STAGED_JOURNAL), &root.join(JOURNAL))?;
    paths::sync(root)?;
    store::persist(root, &registry, &mut false, &mut false)?;
    promote(root)?;
    retire(root, JOURNAL)?;
    Ok(target)
}

fn active(profile: &Profile) -> bool {
    profile
        .state
        .is_none_or(|state| state == ProfileState::Active)
}

fn selected<'a>(data: &'a store::Data, name: &ProfileName) -> Result<&'a Profile, Error> {
    let profile = data
        .profiles
        .iter()
        .find(|profile| &profile.name == name)
        .ok_or_else(|| Error::NotFound { name: name.clone() })?;
    if !active(profile) {
        return Err(Error::RecoveryRequired);
    }
    Ok(profile)
}

fn profiles(data: &store::Data) -> Vec<&Profile> {
    let mut profiles: Vec<_> = data
        .profiles
        .iter()
        .filter(|profile| active(profile))
        .collect();
    profiles.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    profiles
}

pub(crate) enum Mutation {
    PlanLabel(ProfileName, Option<String>),
    ClaudeStatusline(ProfileName, bool),
    Mode(Option<ProfileName>, Mode),
    Interval(Option<u32>),
}

impl Registry {
    pub(crate) fn check_usage_revision(&self) -> Result<(), Error> {
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        self.observed.set(Some(data.revision));
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn usage_capture_lock(
        &self,
        id: u64,
    ) -> Result<Option<(std::fs::File, Vec<platform::CheckedObject>)>, Error> {
        let root = self.root();
        if platform::inspect(&root.join("registry.lock"), ObjectKind::File)?.is_none() {
            return Ok(None);
        }
        let lock = store::admission_lock(root)?;
        let data = store::load(root)?;
        let selected = data.profiles.iter().any(|profile| {
            profile.id.get() == id
                && profile.provider == Provider::Claude
                && profile.state == Some(ProfileState::Active)
                && profile.surfaces.is_some_and(|surfaces| surfaces.cli)
        });
        let enabled = selected && self.claude_usage_enabled(id)?;
        Ok(enabled.then_some(lock))
    }

    #[cfg(unix)]
    pub(crate) fn claude_usage_enabled(&self, id: u64) -> Result<bool, Error> {
        Ok(load(self.root())?
            .profiles
            .get(&id.to_string())
            .is_some_and(|options| options.claude_statusline))
    }

    pub(crate) fn usage_settings(&self) -> Result<Value, Error> {
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        self.observed.set(Some(data.revision));
        let settings = load(self.root())?;
        let rows: Vec<_> = profiles(&data).into_iter().map(|profile| {
            let defaults = Options::default();
            let options = settings.profiles.get(&profile.id.to_string()).unwrap_or(&defaults);
            json!({
                "profile_id": profile.id.to_string(),
                "name": &profile.name,
                "provider": profile.provider,
                "mode": options.mode,
                "plan_label": &options.plan_label,
                "claude_statusline": (profile.provider == Provider::Claude).then_some(options.claude_statusline),
            })
        }).collect();
        Ok(json!({
            "mode": settings.mode,
            "live": {
                "state": if settings.live.enabled { "needs_acknowledgement" } else { "off" },
                "enabled": settings.live.enabled,
                "acknowledged_warning": settings.live.acknowledged_warning.filter(|&warning|
                    !settings.live.enabled && warning <= CURRENT_WARNING),
                "current_warning": CURRENT_WARNING,
                "verified": false,
            },
            "refresh_interval_seconds": settings.refresh_interval_seconds,
            "profiles": rows,
        }))
    }

    pub(crate) fn usage_rows(
        &self,
        name: Option<&ProfileName>,
        force_local: bool,
        read_clock: i64,
    ) -> Result<Value, Error> {
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        self.observed.set(Some(data.revision));
        let selected = match name {
            Some(name) => vec![selected(&data, name)?],
            None => profiles(&data),
        };
        let settings = load(self.root())?;
        let rows: Vec<_> = selected
            .into_iter()
            .map(|profile| read_row(self.root(), profile, &settings, force_local, read_clock))
            .collect();
        Ok(json!({
            "requested_mode": if force_local { "local" } else { "settings" },
            "generated_at": data::clock::format(read_clock),
            "profiles": rows,
        }))
    }

    pub(crate) fn local_usage_row(
        &self,
        profile: &Profile,
        read_clock: i64,
    ) -> Result<Option<Value>, Error> {
        if !active(profile) {
            return Ok(None);
        }
        let settings = load(self.root())?;
        let row = read_row(self.root(), profile, &settings, true, read_clock);
        Ok((!row["source"].is_null() || !row["plan"].is_null()).then_some(row))
    }

    pub(crate) fn change_usage(&self, mutation: Mutation) -> Result<Value, Error> {
        let root = self.root();
        let _lock = store::lock(root, Duration::from_secs(10))?;
        let data = store::load(root)?;
        self.observed.set(Some(data.revision));
        store::precondition(&data, self.expected)?;
        store::require_v3(&data)?;
        if let Mutation::PlanLabel(_, Some(label)) = &mutation
            && !data::valid_label(label)
        {
            return Err(Error::UsagePlanLabel);
        }
        let mut settings = load(root)?;
        let before = serde_json::to_vec(&settings).map_err(|_| Error::RecoveryRequired)?;
        let name = match &mutation {
            Mutation::PlanLabel(name, _) | Mutation::ClaudeStatusline(name, _) => Some(name),
            Mutation::Mode(name, _) => name.as_ref(),
            Mutation::Interval(_) => None,
        };
        let profile = name.map(|name| selected(&data, name)).transpose()?;
        let receipt = profile.map(|profile| (profile.id.to_string(), profile.name.clone()));
        match mutation {
            Mutation::PlanLabel(_, label) => {
                settings
                    .profiles
                    .entry(receipt.as_ref().ok_or(Error::RecoveryRequired)?.0.clone())
                    .or_default()
                    .plan_label = label;
            }
            Mutation::ClaudeStatusline(_, enable) => {
                let profile = profile.ok_or(Error::RecoveryRequired)?;
                if profile.provider != Provider::Claude {
                    return Err(Error::UsageUnsupported);
                }
                if enable {
                    use data::claude::{Presence, profile_presence};
                    match profile_presence(root, profile.id.get()) {
                        Presence::Present => return Err(Error::StatuslineExists),
                        Presence::Indeterminate => return Err(Error::StatuslineUnverifiable),
                        Presence::Absent => {}
                    }
                    #[cfg(unix)]
                    data::hook::install(root)?;
                }
                settings
                    .profiles
                    .entry(profile.id.to_string())
                    .or_default()
                    .claude_statusline = enable;
            }
            Mutation::Mode(_, Mode::Live) => return Err(Error::LiveDisabled),
            Mutation::Mode(_, mode) => {
                if let Some(profile) = profile {
                    settings
                        .profiles
                        .entry(profile.id.to_string())
                        .or_default()
                        .mode = mode;
                } else if mode == Mode::Inherit {
                    return Err(Error::UsageMode);
                } else {
                    settings.mode = mode;
                }
            }
            Mutation::Interval(interval) => {
                if interval.is_some_and(|value| !(300..=86400).contains(&value)) {
                    return Err(Error::UsageInterval);
                }
                settings.refresh_interval_seconds = interval;
            }
        }
        let active_ids: std::collections::BTreeSet<_> = profiles(&data)
            .into_iter()
            .map(|profile| profile.id.to_string())
            .collect();
        settings
            .profiles
            .retain(|id, options| active_ids.contains(id) && *options != Options::default());
        let changed =
            before != serde_json::to_vec(&settings).map_err(|_| Error::RecoveryRequired)?;
        if changed {
            self.observed.set(Some(persist(root, data, &settings)?));
        }
        let (id, name) = receipt.unzip();
        Ok(json!({"changed": changed, "profile_id": id, "name": name, "warnings": []}))
    }
}

fn read_row(
    root: &Path,
    profile: &Profile,
    settings: &Settings,
    force_local: bool,
    read_clock: i64,
) -> Value {
    let defaults = Options::default();
    let options = settings
        .profiles
        .get(&profile.id.to_string())
        .unwrap_or(&defaults);
    let mode = if options.mode == Mode::Inherit {
        settings.mode
    } else {
        options.mode
    };
    if !force_local && mode == Mode::Live {
        data::disabled_row(profile, options.plan_label.as_deref())
    } else {
        json!(data::local_row(
            root,
            profile,
            options.plan_label.as_deref(),
            options.claude_statusline,
            read_clock,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[cfg(unix)]
    type Temp = tempfile::TempDir;
    #[cfg(windows)]
    type Temp = platform::fixture_private::PrivateTempDir;

    fn fixture() -> (Temp, Registry) {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
        #[cfg(unix)]
        let temp = {
            use std::os::unix::fs::PermissionsExt;
            tempfile::Builder::new()
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir_in(parent)
                .unwrap()
        };
        #[cfg(windows)]
        let temp = platform::fixture_private::tempdir(&parent).unwrap();
        let registry = Registry::at(temp.path().join("managed")).unwrap();
        registry
            .add("claude-work".parse().unwrap(), Provider::Claude)
            .unwrap();
        registry
            .add("codex-45".parse().unwrap(), Provider::Codex)
            .unwrap();
        (temp, registry)
    }

    fn new_log(root: &Path, rows: &[(&str, Value)]) {
        let mut day = root.join("profiles/2/cli");
        for level in ["sessions", "2026", "11", "21"] {
            day.push(level);
            if platform::inspect(&day, ObjectKind::Directory)
                .unwrap()
                .is_none()
            {
                platform::create_directory(&day).unwrap();
            }
        }
        let file =
            day.join("rollout-2026-11-21T12-00-00-01234567-89ab-cdef-0123-456789abcdef.jsonl");
        if file.exists() {
            std::fs::remove_file(&file).unwrap();
        }
        let mut file = platform::create_file(&file).unwrap().file;
        writeln!(
            file,
            "{}",
            json!({"type":"session_meta","payload":{"cli_version":"0.169.0"}})
        )
        .unwrap();
        for (timestamp, limits) in rows {
            writeln!(
                file,
                "{}",
                json!({
                    "timestamp":timestamp, "type":"event_msg",
                    "payload":{"type":"token_count","rate_limits":limits},
                })
            )
            .unwrap();
        }
    }

    fn epoch(text: &str) -> i64 {
        data::clock::parse(text).unwrap()
    }

    #[test]
    fn stage11_json_contract_local_and_status_usage() {
        let (_temp, registry) = fixture();
        let root = registry.root();
        let examples: Vec<Value> = serde_json::from_str(include_str!(
            "../../../../docs/contracts/cli-v1.examples.json"
        ))
        .unwrap();
        let fixture = examples
            .iter()
            .find(|example| {
                example["command"] == "usage" && example["data"]["requested_mode"] == "settings"
            })
            .unwrap();
        let status = examples
            .iter()
            .find(|example| {
                example["command"] == "status.account" && example["data"]["usage"].is_object()
            })
            .unwrap();
        let read_clock = epoch("2026-11-21T12:30:00Z");
        let extra = json!({"limit_id":"redacted-x","primary":{
            "used_percent":10.0,"window_minutes":300,"resets_at":epoch("2026-11-21T14:00:00Z")
        }});
        let expired = json!({"limit_id":"redacted-y","primary":{
            "used_percent":90.0,"window_minutes":300,"resets_at":epoch("2026-11-21T11:00:00Z")
        }});
        let codex = |reset| {
            json!({"plan_type":"pro",
                "primary":{"used_percent":30.0,"window_minutes":300,"resets_at":epoch(reset)},
                "secondary":{"used_percent":75.0,"window_minutes":10080,
                    "resets_at":epoch("2026-11-24T23:59:59Z")}
            })
        };
        new_log(
            root,
            &[
                ("2026-11-21T10:00:00Z", extra.clone()),
                ("2026-11-21T10:30:00Z", expired),
                ("2026-11-21T12:00:00Z", codex("2026-11-21T14:00:00Z")),
            ],
        );
        let actual = registry.usage_rows(None, false, read_clock).unwrap();
        assert_eq!(
            serde_json::to_vec(&actual).unwrap(),
            serde_json::to_vec(&fixture["data"]).unwrap()
        );
        new_log(
            root,
            &[
                ("2026-11-21T10:00:00Z", extra),
                ("2026-11-21T12:00:00Z", codex("2026-11-21T12:20:00Z")),
            ],
        );
        let profile = registry.get(&"codex-45".parse().unwrap()).unwrap().profile;
        let actual = registry
            .local_usage_row(&profile, read_clock)
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_vec(&actual).unwrap(),
            serde_json::to_vec(&status["data"]["usage"]).unwrap()
        );
    }

    #[test]
    fn stage11_closed_stored_live_intent_and_forced_local() {
        let (_temp, registry) = fixture();
        let root = registry.root();
        let settings = Settings {
            mode: Mode::Live,
            live: Live {
                enabled: true,
                acknowledged_warning: Some(1),
            },
            ..Settings::default()
        };
        create(root, SETTINGS, &serde_json::to_vec(&settings).unwrap()).unwrap();
        let settings = registry.usage_settings().unwrap();
        assert_eq!(settings["live"]["state"], "needs_acknowledgement");
        let read_clock = epoch("2026-11-21T12:30:00Z");
        let row = |force_local| registry.usage_rows(None, force_local, read_clock).unwrap();
        assert_eq!(
            row(false)["profiles"][1]["windows"]["weekly"]["reason"],
            "live_disabled"
        );
        assert_eq!(
            row(false)["profiles"][1]["reset_credits"]["state"],
            "not_recorded"
        );
        assert_eq!(row(true)["profiles"][1]["mode"], "local");
        assert_eq!(
            row(true)["profiles"][1]["reset_credits"]["state"],
            "unsupported"
        );
    }
}
