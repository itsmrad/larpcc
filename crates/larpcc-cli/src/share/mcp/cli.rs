//! `larp cc mcp ...` grammar. Add inputs are taken as raw strings and
//! validated by the MCP module so that no refused value is echoed by a parser.
use crate::registry::ProfileName;
use clap::Subcommand;
use std::fmt::Write;

#[derive(Clone, Subcommand)]
pub enum McpCommand {
    /// Add a reference-only server definition (disabled unless sync-all is on)
    #[command(group = clap::ArgGroup::new("transport").required(true).args(["command", "url"]))]
    Add {
        /// Server name: 1-32 characters from a-z, 0-9, '-', '_', starting with a letter
        name: String,
        /// Absolute path of a local server executable
        #[arg(long, value_name = "ABSOLUTE")]
        command: Option<std::ffi::OsString>,
        /// Only --stdio or --transport=stdio; pass as --arg=--stdio
        #[arg(long = "arg", value_name = "SAFE_ARG", allow_hyphen_values = true)]
        args: Vec<String>,
        /// Environment variable NAME passed by reference, never its value
        #[arg(long = "env-ref", value_name = "VAR")]
        env_refs: Vec<String>,
        /// https URL whose path is exactly /mcp or /sse
        #[arg(long, value_name = "SAFE_URL")]
        url: Option<String>,
        /// HEADER=ENV_VAR reference, never a header value
        #[arg(long = "header-ref", value_name = "HEADER=ENV_VAR")]
        header_refs: Vec<String>,
    },
    /// Remove a definition, its switches and its renders
    Remove { name: ProfileName },
    /// Enable a server for one profile or for all profiles
    Enable {
        name: ProfileName,
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        profile: Option<ProfileName>,
        #[arg(long)]
        all: bool,
    },
    /// Disable a server for one profile or for all profiles
    Disable {
        name: ProfileName,
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        profile: Option<ProfileName>,
        #[arg(long)]
        all: bool,
    },
    /// List definitions by reference name only
    List,
    /// Show the per-profile switch matrix
    Matrix,
    /// Inherit definitions on all current and future profiles (on) or stop (off)
    SyncAll {
        #[arg(value_parser = ["on", "off"])]
        mode: String,
    },
    /// Render all profiles or explicitly resume an interrupted transaction
    Sync,
}

pub enum Target {
    Profile(ProfileName),
    All,
}
pub enum McpAction {
    Add {
        name: String,
        command: Option<std::ffi::OsString>,
        args: Vec<String>,
        env_refs: Vec<String>,
        url: Option<String>,
        header_refs: Vec<String>,
    },
    Remove {
        name: ProfileName,
    },
    Switch {
        name: ProfileName,
        target: Target,
        enabled: bool,
    },
    List,
    Matrix,
    SyncAll(bool),
    Sync,
}
impl McpCommand {
    pub fn command(&self) -> &'static str {
        match self {
            Self::Add { .. } => "mcp.add",
            Self::Remove { .. } => "mcp.remove",
            Self::Enable { .. } => "mcp.enable",
            Self::Disable { .. } => "mcp.disable",
            Self::List => "mcp.list",
            Self::Matrix => "mcp.matrix",
            Self::SyncAll { .. } => "mcp.sync-all",
            Self::Sync => "mcp.sync",
        }
    }
    pub fn into_action(self) -> McpAction {
        let target = |profile: Option<ProfileName>, all: bool| match profile {
            Some(p) if !all => Target::Profile(p),
            _ => Target::All,
        };
        match self {
            Self::Add {
                name,
                command,
                args,
                env_refs,
                url,
                header_refs,
            } => McpAction::Add {
                name,
                command,
                args,
                env_refs,
                url,
                header_refs,
            },
            Self::Remove { name } => McpAction::Remove { name },
            Self::Enable { name, profile, all } => McpAction::Switch {
                name,
                target: target(profile, all),
                enabled: true,
            },
            Self::Disable { name, profile, all } => McpAction::Switch {
                name,
                target: target(profile, all),
                enabled: false,
            },
            Self::List => McpAction::List,
            Self::Matrix => McpAction::Matrix,
            Self::SyncAll { mode } => McpAction::SyncAll(mode == "on"),
            Self::Sync => McpAction::Sync,
        }
    }
}

/// Human output shows the same reference-only metadata as JSON.
pub fn human(data: &serde_json::Value) -> String {
    if let Some(rows) = data.get("servers").and_then(serde_json::Value::as_array) {
        let mut output = format!(
            "Sync all: {}\n",
            if data["sync_all"] == true {
                "on"
            } else {
                "off"
            }
        );
        if rows.is_empty() {
            output.push_str("No MCP servers.\n");
            return output;
        }
        output.push_str("NAME\tTRANSPORT\tSTATE\tREFERENCES\n");
        for row in rows {
            let refs: Vec<_> = row["secret_refs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    format!(
                        "{}:{}",
                        r["kind"].as_str().unwrap_or(""),
                        r["name"].as_str().unwrap_or("")
                    )
                })
                .collect();
            let _ = writeln!(
                output,
                "{}\t{}\t{}\t{}",
                row["name"].as_str().unwrap_or(""),
                row["transport"].as_str().unwrap_or(""),
                row["state"].as_str().unwrap_or(""),
                if refs.is_empty() {
                    "-".to_owned()
                } else {
                    refs.join(",")
                }
            );
        }
        return output;
    }
    if let Some(rows) = data.get("rows").and_then(serde_json::Value::as_array) {
        let mut output = "SERVER".to_owned();
        for p in data["profiles"].as_array().into_iter().flatten() {
            let _ = write!(output, "\t{}", p["name"].as_str().unwrap_or(""));
        }
        output.push('\n');
        for row in rows {
            output.push_str(row["name"].as_str().unwrap_or(""));
            for on in row["enabled"].as_array().into_iter().flatten() {
                output.push_str(if on == true { "\ton" } else { "\toff" });
            }
            output.push('\n');
        }
        return output;
    }
    let mut output = if data["changed"] == true {
        "MCP updated.\n".to_owned()
    } else {
        "MCP unchanged.\n".to_owned()
    };
    for w in data["warnings"].as_array().into_iter().flatten() {
        let _ = writeln!(output, "warning: {}", w["message"].as_str().unwrap_or(""));
    }
    output
}
