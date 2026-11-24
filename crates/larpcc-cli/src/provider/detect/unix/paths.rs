use super::fs::walk_parent;
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
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
    edges: Vec<((u64, u64), OsString)>,
    claude_install: Option<PathBuf>,
}

impl Exclusions {
    pub(super) fn new(home: Option<&Path>) -> Self {
        Self {
            roots: Vec::new(),
            edges: Vec::new(),
            claude_install: home.map(|p| comparable(&p.join(".local/share/claude"))),
        }
    }

    pub(super) fn register(
        &mut self,
        paths: &[PathBuf],
        within_budget: impl Fn() -> bool,
    ) -> io::Result<()> {
        let mut missing_edges = Vec::new();
        for path in paths {
            if !path.is_absolute()
                || path.file_name().is_none()
                || path.components().any(|part| part == Component::ParentDir)
            {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            self.insert(path);
        }
        for path in paths {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(io::ErrorKind::InvalidInput.into());
            };
            let mut absent = None;
            let mapped = walk_parent(
                parent,
                |part, directory| {
                    within_budget()
                    && self.allows_child(part, directory)
                    // Until all aliases are known, do not enter any unresolved
                    // protected basename while mapping another root's parent.
                    && !part.file_name().is_some_and(|leaf| paths.iter().any(|path| {
                        path.file_name().is_some_and(|name| same_name(name, leaf))
                    }))
                },
                |part, directory| {
                    absent = Some(edge(directory, part));
                },
            );
            match mapped {
                Ok(entry) => {
                    let directory = entry.directory.ok_or(io::ErrorKind::NotADirectory)?;
                    let physical = entry.path.join(name);
                    self.edges.push(edge(&directory, &physical)?);
                    self.insert(&physical);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // Keep the first absent component closed for this scan if
                    // another process creates it after the guarded lookup.
                    missing_edges.push(absent.ok_or(error)??);
                }
                Err(error) => return Err(error),
            }
        }
        self.edges.extend(missing_edges);
        Ok(())
    }

    pub(super) fn allows_child(&self, path: &Path, parent: Option<&File>) -> bool {
        self.allows(path) && !self.denied_child(path, parent)
    }

    pub(super) fn denied_child(&self, path: &Path, parent: Option<&File>) -> bool {
        let Some((name, parent)) = path.file_name().zip(parent) else {
            return false;
        };
        self.edges.iter().any(|(id, denied)| {
            same_name(name, denied)
                && parent
                    .metadata()
                    .map_or(true, |meta| (meta.dev(), meta.ino()) == *id)
        })
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
        Path::new(OsStr::from_bytes(&folded)).to_owned()
    } else {
        path.to_owned()
    }
}

fn same_name(a: &OsStr, b: &OsStr) -> bool {
    if cfg!(target_os = "macos") {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        // Close on Unicode spellings whose filesystem equivalence we cannot
        // establish without inspecting the child.
        !a.is_ascii() || !b.is_ascii() || a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn edge(directory: &File, path: &Path) -> io::Result<((u64, u64), OsString)> {
    let meta = directory.metadata()?;
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    Ok(((meta.dev(), meta.ino()), name.to_owned()))
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
