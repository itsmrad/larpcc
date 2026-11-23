use crate::registry::{DirStatus, ProfileEntry, ProfileName};
use std::fmt::Write;

fn state(status: &DirStatus) -> &'static str {
    match status {
        DirStatus::Ok => "ok",
        DirStatus::Missing => "missing",
        DirStatus::Unsafe(_) => "unsafe",
    }
}

pub fn added(entry: &ProfileEntry) -> String {
    format!(
        "Added profile {} ({}, id {}).\nState directory: {}\n",
        entry.profile.name,
        entry.profile.provider,
        entry.profile.id,
        crate::registry::display::path(&entry.state_dir)
    )
}
pub fn renamed(from: &ProfileName, entry: &ProfileEntry) -> String {
    format!(
        "Renamed profile {from} to {} (id {}).\nState directory unchanged: {}\n",
        entry.profile.name,
        entry.profile.id,
        crate::registry::display::path(&entry.state_dir)
    )
}
pub fn forgotten(entry: &ProfileEntry) -> String {
    format!(
        "Forgot profile {} (id {}).\nState directory kept: {}\n",
        entry.profile.name,
        entry.profile.id,
        crate::registry::display::path(&entry.state_dir)
    )
}
pub fn show(entry: &ProfileEntry) -> String {
    let status = match &entry.status {
        DirStatus::Unsafe(reason) => format!("unsafe: {reason}"),
        status => state(status).into(),
    };
    format!(
        "Name:      {}\nProvider:  {}\nID:        {}\nState dir: {}\nState:     {status}\n",
        entry.profile.name,
        entry.profile.provider,
        entry.profile.id,
        crate::registry::display::path(&entry.state_dir)
    )
}
pub fn list(mut entries: Vec<ProfileEntry>) -> String {
    if entries.is_empty() {
        return "No profiles.\n".into();
    }
    entries.sort_by(|a, b| a.profile.name.as_str().cmp(b.profile.name.as_str()));
    let mut rows = vec![[
        "NAME".into(),
        "PROVIDER".into(),
        "ID".into(),
        "STATE".into(),
    ]];
    rows.extend(entries.iter().map(|e| {
        [
            e.profile.name.as_str().to_owned(),
            e.profile.provider.to_string(),
            e.profile.id.to_string(),
            state(&e.status).into(),
        ]
    }));
    let widths: [usize; 3] =
        std::array::from_fn(|i| rows.iter().map(|r| r[i].len()).max().unwrap_or(0));
    let mut output = String::new();
    for row in rows {
        let _ = writeln!(
            output,
            "{:<w0$}  {:<w1$}  {:<w2$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2]
        );
    }
    output
}

pub fn bound(
    provider: crate::registry::Provider,
    path: &std::path::Path,
    outcome: crate::registry::BindOutcome,
) -> String {
    use crate::registry::{BindOutcome, display::path as display};
    match outcome {
        BindOutcome::Bound => format!("Bound {provider} to {}.\n", display(path)),
        BindOutcome::Unchanged => format!("{provider} is already bound to {}.\n", display(path)),
        BindOutcome::Replaced { old } => format!(
            "Rebound {provider} from {} to {}.\n",
            display(&old),
            display(path)
        ),
    }
}
pub fn bindings(
    entries: Vec<(crate::registry::Provider, crate::registry::BindingState)>,
) -> String {
    use crate::registry::{BindingState, display::path};
    let mut rows = vec![[
        "PROVIDER".to_owned(),
        "STATE".to_owned(),
        "EXECUTABLE".to_owned(),
    ]];
    rows.extend(entries.into_iter().map(|(p, s)| match s {
        BindingState::Unbound => [p.to_string(), "unbound".into(), "-".into()],
        BindingState::Ok(b) => [p.to_string(), "ok".into(), path(&b.executable)],
        BindingState::Invalid(b, r) => {
            [p.to_string(), format!("invalid: {r}"), path(&b.executable)]
        }
    }));
    let w0 = rows.iter().map(|r| r[0].len()).max().unwrap_or(0);
    let w1 = rows.iter().map(|r| r[1].len()).max().unwrap_or(0);
    let mut result = String::new();
    for r in rows {
        let _ = writeln!(result, "{:<w0$}  {:<w1$}  {}", r[0], r[1], r[2]);
    }
    result
}

pub fn alias(root: &std::path::Path, add: bool, data: &serde_json::Value) -> String {
    let bin = crate::registry::display::path(&root.join("bin"));
    if let Some(aliases) = data["aliases"].as_array() {
        if aliases.is_empty() {
            return "No aliases.\n".into();
        }
        let mut out = String::new();
        for a in aliases {
            let _ = writeln!(
                out,
                "{}\tprofile id {}\t{}\t{}\t{}",
                a["name"].as_str().unwrap_or_default(),
                a["profile_id"].as_str().unwrap_or_default(),
                a["surface"].as_str().unwrap_or_default(),
                a["state"].as_str().unwrap_or_default(),
                a["path"].as_str().unwrap_or_default()
            );
        }
        return out;
    }
    let name = data["name"].as_str().unwrap_or_default();
    let id = data["profile_id"].as_str().unwrap_or_default();
    if !add {
        return format!("Removed alias {name} (profile id {id}).\n");
    }
    format!(
        "Added alias {name} for profile id {id} in {bin}.\nLarp CC does not edit PATH or shell files. To use it, add {bin} to PATH yourself; it only shadows a same-named command when it precedes that command in PATH.\n"
    )
}

pub(super) fn skills(data: &serde_json::Value) -> String {
    if let Some(rows) = data.get("skills").and_then(serde_json::Value::as_array) {
        if rows.is_empty() {
            return "No shared skills.\n".into();
        }
        let mut output = "NAME\tSTATE\tSHA256\n".to_owned();
        for row in rows {
            let _ = writeln!(
                output,
                "{}\t{}\t{}",
                row["name"].as_str().unwrap_or(""),
                row["state"].as_str().unwrap_or(""),
                row["sha256"].as_str().unwrap_or("")
            );
        }
        return output;
    }
    if let Some(rows) = data.get("rows").and_then(serde_json::Value::as_array) {
        let mut output = "SKILL".to_owned();
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
    if data["changed"] == true {
        "Skills updated.\n".into()
    } else {
        "Skills unchanged.\n".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Profile, Provider};
    fn entry(name: &str, id: u64, status: DirStatus) -> ProfileEntry {
        ProfileEntry {
            profile: Profile {
                id: serde_json::from_str(&id.to_string()).unwrap(),
                name: ProfileName::parse(name).unwrap(),
                provider: Provider::Claude,
                surfaces: None,
                state: None,
                deletion_transaction: None,
            },
            state_dir: "root/profiles/1".into(),
            status,
        }
    }
    #[test]
    fn sorted_columns_and_all_directory_states() {
        assert_eq!(list(vec![]), "No profiles.\n");
        assert_eq!(
            list(vec![
                entry("work", 100, DirStatus::Missing),
                entry("a", 2, DirStatus::Unsafe("reason".into())),
                entry("longer", 3, DirStatus::Ok)
            ]),
            "NAME    PROVIDER  ID   STATE\na       claude    2    unsafe\nlonger  claude    3    ok\nwork    claude    100  missing\n"
        );
        assert_eq!(
            show(&entry(
                "work",
                1,
                DirStatus::Unsafe("is not a directory".into())
            )),
            "Name:      work\nProvider:  claude\nID:        1\nState dir: root/profiles/1\nState:     unsafe: is not a directory\n"
        );
    }
}
