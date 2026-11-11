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
        entry.state_dir.display()
    )
}
pub fn renamed(from: &ProfileName, entry: &ProfileEntry) -> String {
    format!(
        "Renamed profile {from} to {} (id {}).\nState directory unchanged: {}\n",
        entry.profile.name,
        entry.profile.id,
        entry.state_dir.display()
    )
}
pub fn forgotten(entry: &ProfileEntry) -> String {
    format!(
        "Forgot profile {} (id {}).\nState directory kept: {}\n",
        entry.profile.name,
        entry.profile.id,
        entry.state_dir.display()
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
        entry.state_dir.display()
    )
}
pub fn list(mut entries: Vec<ProfileEntry>) -> String {
    if entries.is_empty() {
        return "No profiles.\n".into();
    }
    entries.sort_by(|a, b| a.profile.name.cmp(&b.profile.name));
    let mut rows = vec![[
        "NAME".into(),
        "PROVIDER".into(),
        "ID".into(),
        "STATE".into(),
    ]];
    rows.extend(entries.iter().map(|e| {
        [
            e.profile.name.to_string(),
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
