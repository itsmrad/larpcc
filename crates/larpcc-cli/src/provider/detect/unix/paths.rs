use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Native,
    Homebrew,
    Npm,
    Bun,
    Unknown,
}

impl Kind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Homebrew => "homebrew",
            Self::Npm => "npm",
            Self::Bun => "bun",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) struct Exclusions {
    roots: Vec<PathBuf>,
    claude_install: Option<PathBuf>,
}

impl Exclusions {
    pub(super) fn new(home: Option<&Path>) -> Self {
        Self {
            roots: Vec::new(),
            claude_install: home.map(|p| comparable(&p.join(".local/share/claude"))),
        }
    }

    pub(super) fn insert(&mut self, path: &Path) {
        let path = comparable(&normalize(path));
        if !self.roots.contains(&path) {
            self.roots.push(path);
        }
    }

    pub(super) fn allows(&self, path: &Path) -> bool {
        !["/proc", "/sys", "/dev"]
            .iter()
            .any(|p| path.starts_with(p))
            && !self.inside_provider_home(path)
    }

    pub(super) fn inside_provider_home(&self, path: &Path) -> bool {
        let path = comparable(path);
        if self.roots.iter().any(|root| path.starts_with(root)) {
            return true;
        }
        let in_native_versions = self
            .claude_install
            .as_ref()
            .is_some_and(|install| path == *install || path.starts_with(install.join("versions")));
        let mut parent: Option<Vec<u8>> = None;
        for name in path.iter() {
            let name = name.as_bytes().to_ascii_lowercase();
            if name.starts_with(b".claude") || name.starts_with(b".codex") {
                return true;
            }
            let parent_name: Option<&[u8]> = parent.as_deref();
            let state_dir = matches!(
                parent_name,
                Some(
                    b"application support"
                        | b"containers"
                        | b"group containers"
                        | b"caches"
                        | b"preferences"
                        | b"saved application state"
                        | b"httpstorages"
                        | b"webkit"
                        | b"logs"
                        | b".config"
                        | b".cache"
                        | b"share"
                        | b"state"
                )
            );
            if state_dir
                && (name.starts_with(b"com.anthropic")
                    || name.starts_with(b"com.openai")
                    || matches!(name.as_slice(), b"claude" | b"chatgpt" | b"codex")
                        && !(name == b"claude" && in_native_versions))
            {
                return true;
            }
            parent = Some(name);
        }
        false
    }
}

fn comparable(path: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        let folded = path.as_os_str().as_bytes().to_ascii_lowercase();
        let path = Path::new(OsStr::from_bytes(&folded));
        // The writable data volume also exposes /Users via this spelling.
        if let Ok(suffix) = path.strip_prefix("/system/volumes/data") {
            return Path::new("/").join(suffix);
        }
        path.to_owned()
    } else {
        path.to_owned()
    }
}

pub(super) fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => (),
            other => normalized.push(other),
        }
    }
    normalized
}
