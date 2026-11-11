#![allow(dead_code)]
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub struct Fixture {
    pub temp: tempfile::TempDir,
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("managed");
        for dir in ["home", "xdg", "local", "app", "claude", "codex"] {
            let path = temp.path().join(dir);
            fs::create_dir(&path).unwrap();
            for file in [
                ".credentials.json",
                "auth.json",
                ".claude.json",
                "config.toml",
            ] {
                fs::write(
                    path.join(file),
                    b"synthetic sentinel; never real account data\n",
                )
                .unwrap();
            }
        }
        Self { temp, root }
    }
    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_larp"));
        cmd.args(args).env("LARPCC_HOME", &self.root);
        for (var, dir) in [
            ("HOME", "home"),
            ("XDG_DATA_HOME", "xdg"),
            ("LOCALAPPDATA", "local"),
            ("APPDATA", "app"),
            ("CLAUDE_CONFIG_DIR", "claude"),
            ("CODEX_HOME", "codex"),
        ] {
            cmd.env(var, self.temp.path().join(dir));
        }
        cmd
    }
    pub fn run(&self, args: &[&str], code: i32) -> Output {
        let output = self.command(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "args={args:?}\nstdout={}\nstderr={}",
            stdout(&output),
            stderr(&output)
        );
        if code == 0 {
            assert!(output.stderr.is_empty(), "{}", stderr(&output));
        } else {
            assert!(output.stdout.is_empty(), "{}", stdout(&output));
        }
        output
    }
    pub fn profile(&self, args: &[&str], code: i32) -> Output {
        self.run(&[&["cc", "profile"][..], args].concat(), code)
    }
    pub fn install_registry(&self, bytes: &[u8]) {
        fs::create_dir(&self.root).unwrap();
        private(&self.root, true);
        fs::write(self.root.join("registry.json"), bytes).unwrap();
        private(&self.root.join("registry.json"), false);
    }
    // Exact bytes are stronger than a digest; record sizes and Unix modes as well.
    pub fn outside_snapshot(&self) -> BTreeMap<PathBuf, (u64, Vec<u8>, u32)> {
        fn walk(path: &Path, root: &Path, out: &mut BTreeMap<PathBuf, (u64, Vec<u8>, u32)>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path == root {
                    continue;
                }
                let meta = fs::symlink_metadata(&path).unwrap();
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    meta.permissions().mode()
                };
                #[cfg(not(unix))]
                let mode = u32::from(meta.permissions().readonly());
                let bytes = if meta.is_file() {
                    fs::read(&path).unwrap()
                } else {
                    vec![]
                };
                out.insert(path.clone(), (meta.len(), bytes, mode));
                if meta.is_dir() {
                    walk(&path, root, out);
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.temp.path(), &self.root, &mut out);
        out
    }
}
pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}
pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}
pub fn private(path: &Path, dir: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if dir { 0o700 } else { 0o600 }),
        )
        .unwrap();
    }
    #[cfg(not(unix))]
    let _ = (path, dir);
}
