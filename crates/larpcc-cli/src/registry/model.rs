use super::NameError;
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}
impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Codex];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}
impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl FromStr for Provider {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            _ => Err(()),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileId(u64);
impl ProfileId {
    pub(super) fn new(value: u64) -> Self {
        Self(value)
    }
    pub fn get(self) -> u64 {
        self.0
    }
}
impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileName(String);
impl ProfileName {
    pub fn parse(s: &str) -> Result<Self, NameError> {
        if s.is_empty()
            || s.len() > 32
            || !s.as_bytes()[0].is_ascii_lowercase()
            || !s
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(NameError::Invalid { input: s.into() });
        }
        if matches!(s, "con" | "prn" | "aux" | "nul")
            || (s.len() == 4
                && (s.starts_with("com") || s.starts_with("lpt"))
                && matches!(s.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(NameError::Reserved { input: s.into() });
        }
        Ok(Self(s.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for ProfileName {
    type Err = NameError;
    fn from_str(s: &str) -> Result<Self, NameError> {
        Self::parse(s)
    }
}
impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for ProfileName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: ProfileId,
    pub name: ProfileName,
    pub provider: Provider,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirStatus {
    Ok,
    Missing,
    Unsafe(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileEntry {
    pub profile: Profile,
    pub state_dir: PathBuf,
    pub status: DirStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub provider: Provider,
    pub executable: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingState {
    Unbound,
    Ok(Binding),
    Invalid(Binding, String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindOutcome {
    Bound,
    Unchanged,
    Replaced { old: PathBuf },
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "Launch target belongs to the separate synthetic driver."
)]
#[cfg(feature = "synthetic-launch")]
pub struct LaunchTarget {
    pub profile: Profile,
    pub state_dir: PathBuf,
    pub stored_executable: PathBuf,
    pub resolved_executable: PathBuf,
    pub identity: crate::platform::FileIdentity,
}
