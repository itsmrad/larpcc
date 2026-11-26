use crate::registry::{
    Error, ProfileName, Registry,
    usage::{Mode, Mutation},
};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::Value;
use std::fmt::Write;

pub(crate) const WINDOWS_UNSUPPORTED: &str =
    "Usage reads and settings changes are unavailable on Windows.";

pub(crate) fn empty_settings() -> Value {
    serde_json::json!({
        "mode": "local",
        "live": {"state":"off", "enabled":false, "acknowledged_warning":null,
            "current_warning": super::super::CURRENT_WARNING, "verified":false},
        "refresh_interval_seconds": null,
        "profiles": [],
    })
}

#[derive(Args)]
pub(crate) struct ReadUsage {
    /// Profile name. Omit to list all active profiles.
    profile: Option<ProfileName>,
    /// Read local usage observations.
    #[arg(long, conflicts_with = "live")]
    local: bool,
    /// Request usage from the profile’s qualified official client.
    #[arg(long)]
    live: bool,
}

#[derive(Subcommand)]
pub(crate) enum SettingsCommand {
    /// Show usage settings or apply a usage setting.
    #[command(disable_help_subcommand = true)]
    Usage {
        #[command(subcommand)]
        command: Option<UsageSetting>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Switch {
    On,
    Off,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum ModeArg {
    Local,
    Live,
    Inherit,
}

#[derive(Subcommand)]
pub(crate) enum LiveSetting {
    /// Ask the profile's qualified official client for limits using its stored sign-in.
    /// Provider restrictions are possible. LIVE remains unverified against real accounts.
    On {
        #[arg(long)]
        acknowledge_risk: bool,
    },
    Off,
}

#[derive(Subcommand)]
pub(crate) enum UsageSetting {
    /// Change consent for LIVE usage through qualified official clients.
    Live {
        #[command(subcommand)]
        command: LiveSetting,
    },
    /// Set the installation or profile usage mode.
    Mode {
        mode: ModeArg,
        #[arg(long)]
        profile: Option<ProfileName>,
    },
    /// Record the usage refresh interval as off or 300–86400 seconds.
    Interval { value: String },
    /// Opt a Claude profile in or out of LOCAL statusLine capture.
    ClaudeStatusline {
        state: Switch,
        #[arg(long)]
        profile: ProfileName,
    },
}

#[derive(Args)]
pub(crate) struct PlanLabel {
    profile: ProfileName,
    label: Option<String>,
    #[arg(long)]
    clear: bool,
}

impl PlanLabel {
    pub(crate) fn execute(self, registry: &Registry) -> Result<Value, Error> {
        match (self.label, self.clear) {
            (Some(label), false) => {
                registry.change_usage(Mutation::PlanLabel(self.profile, Some(label)))
            }
            (None, true) => registry.change_usage(Mutation::PlanLabel(self.profile, None)),
            _ => Err(Error::UsagePlanLabel),
        }
    }
}

impl ReadUsage {
    pub(crate) fn execute(
        self,
        registry: &Registry,
        clock: i64,
        remote: bool,
    ) -> Result<Value, Error> {
        if self.live {
            if remote {
                return Err(Error::UsageRemoteUnsupported);
            }
            return registry.read_usage(self.profile.as_ref(), false, true, clock);
        }
        registry.usage_rows(self.profile.as_ref(), self.local || remote, clock)
    }
}

impl SettingsCommand {
    pub(crate) fn discriminator(&self) -> &'static str {
        let Self::Usage { command } = self;
        match command {
            None => "settings.usage",
            Some(UsageSetting::Live { .. }) => "settings.usage.live",
            Some(UsageSetting::Mode { .. }) => "settings.usage.mode",
            Some(UsageSetting::Interval { .. }) => "settings.usage.interval",
            Some(UsageSetting::ClaudeStatusline { .. }) => "settings.usage.claude-statusline",
        }
    }

    pub(crate) fn execute(self, registry: &Registry, remote: bool) -> Result<Value, Error> {
        let Self::Usage { command } = self;
        let Some(command) = command else {
            return registry.usage_settings();
        };
        if remote {
            return Err(Error::UsageRemoteUnsupported);
        }
        let mutation = match command {
            UsageSetting::Live {
                command:
                    LiveSetting::On {
                        acknowledge_risk: false,
                    },
            } => {
                return Err(Error::UsageConfirmation);
            }
            UsageSetting::Live { command } => {
                Mutation::Live(matches!(command, LiveSetting::On { .. }))
            }
            UsageSetting::Mode { mode, profile } => Mutation::Mode(
                profile,
                match mode {
                    ModeArg::Local => Mode::Local,
                    ModeArg::Live => Mode::Live,
                    ModeArg::Inherit => Mode::Inherit,
                },
            ),
            UsageSetting::Interval { value } => Mutation::Interval(match value.as_str() {
                "off" => None,
                other if !other.is_empty() && other.bytes().all(|byte| byte.is_ascii_digit()) => {
                    Some(other.parse().map_err(|_| Error::UsageInterval)?)
                }
                _ => return Err(Error::UsageInterval),
            }),
            UsageSetting::ClaudeStatusline { state, profile } => {
                Mutation::ClaudeStatusline(profile, matches!(state, Switch::On))
            }
        };
        registry.change_usage(mutation)
    }
}

pub(crate) fn human(command: &str, data: &Value) -> String {
    let mut text = String::new();
    if command == "usage" {
        let Some(rows) = data["profiles"].as_array() else {
            return text;
        };
        if rows.is_empty() {
            return "No profiles.\n".into();
        }
        for row in rows {
            let name = row["name"].as_str().unwrap_or_default();
            let mode = row["mode"].as_str().unwrap_or_default();
            let _ = writeln!(text, "{name} ({mode})");
            if let Some(label) = row["plan"]["label"].as_str() {
                let _ = writeln!(text, "  Plan {label}");
            }
            if let Some(captured) = row["captured_at"].as_str() {
                let _ = writeln!(text, "  As of {captured}");
            }
            let window = |text: &mut String, label: &str, value: &Value| {
                let _ = write!(text, "  {label}  ");
                if let Some(percent) = value["used_percent"].as_f64() {
                    let _ = write!(text, "{percent:.1}% used");
                    if let Some(reset) = value["resets_at"].as_str() {
                        let _ = write!(text, ", resets {reset}");
                    }
                    let _ = writeln!(text);
                } else if value["reason"] == "reset_since_capture" {
                    let _ = writeln!(text, "Reset due");
                } else {
                    let _ = writeln!(text, "Not available");
                }
            };
            window(&mut text, "5h", &row["windows"]["five_hour"]);
            window(&mut text, "Weekly", &row["windows"]["weekly"]);
            if let Some(extras) = row["windows"]["extra"].as_array() {
                for extra in extras {
                    window(
                        &mut text,
                        extra["label"].as_str().unwrap_or_default(),
                        extra,
                    );
                }
            }
        }
    } else if command == "settings.usage" {
        let mode = data["mode"].as_str().unwrap_or_default();
        let live = data["live"]["state"].as_str().unwrap_or_default();
        let _ = writeln!(text, "Default mode {mode}\nLIVE {live}");
        let interval = data["refresh_interval_seconds"]
            .as_u64()
            .map_or_else(|| "off".to_owned(), |value| format!("{value}s"));
        let _ = writeln!(text, "Refresh interval {interval}");
        if let Some(profiles) = data["profiles"].as_array() {
            for profile in profiles {
                let name = profile["name"].as_str().unwrap_or_default();
                let mode = profile["mode"].as_str().unwrap_or_default();
                let _ = write!(text, "{name} ({mode})");
                if let Some(label) = profile["plan_label"].as_str() {
                    let _ = write!(text, ", plan {label}");
                }
                if let Some(capture) = profile["claude_statusline"].as_bool() {
                    let _ = write!(
                        text,
                        ", Claude capture {}",
                        if capture { "on" } else { "off" }
                    );
                }
                let _ = writeln!(text);
            }
        }
    } else if data["changed"] == true {
        text.push_str("Usage settings saved.\n");
    } else {
        text.push_str("Usage settings unchanged.\n");
    }
    if let Some(warnings) = data["warnings"].as_array() {
        for message in warnings
            .iter()
            .filter_map(|warning| warning["message"].as_str())
        {
            let _ = writeln!(text, "{message}");
        }
    }
    text
}
