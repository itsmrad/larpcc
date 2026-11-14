#[allow(dead_code)]
#[path = "../fixtures/windows/private.rs"]
mod private;
use std::{
    io::{self, Write},
    path::PathBuf,
};
pub fn prepare() -> io::Result<()> {
    let home = PathBuf::from(
        std::env::var_os("USERPROFILE")
            .ok_or_else(|| io::Error::other("missing Windows user parent"))?,
    );
    use std::os::windows::ffi::OsStrExt;
    let units = home.as_os_str().encode_wide().collect::<Vec<_>>();
    if units.len() < 3
        || !matches!(units[0], 67 | 99)
        || units[1] != 58
        || units[2] != 92
        || !home.is_absolute()
    {
        return Err(io::Error::other(
            "CI fixtures require trusted C: user ancestry",
        ));
    }
    let mut component = PathBuf::new();
    for part in home.components() {
        component.push(part);
        if matches!(part, std::path::Component::Prefix(_)) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&component)?;
        use std::os::windows::fs::MetadataExt;
        if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "CI user ancestry contains a reparse point or invalid type",
            ));
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut created = None;
    for counter in 0..1000 {
        let path = home.join(format!(
            "larpcc-native-{}-{stamp}-{counter}",
            std::process::id()
        ));
        match private::create_directory(&path) {
            Ok(()) => {
                created = Some(path);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let parent =
        created.ok_or_else(|| io::Error::other("CI private parent collisions exhausted"))?;
    let github_env = std::env::var_os("GITHUB_ENV")
        .ok_or_else(|| io::Error::other("missing CI environment output"))?;
    let mut output = std::fs::OpenOptions::new().append(true).open(github_env)?;
    writeln!(output, "LARPCC_TEST_PARENT={}", parent.display())?;
    writeln!(
        output,
        "CARGO_TARGET_DIR={}",
        parent.join("default-target").display()
    )?;
    println!("Protected synthetic fixture parent: {}", parent.display());
    Ok(())
}
