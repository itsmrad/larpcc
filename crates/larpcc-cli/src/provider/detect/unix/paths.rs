use super::fs::walk_parent;
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
};

struct Planned {
    path: PathBuf,
    parent: PathBuf,
    leaf: OsString,
    done: bool,
}

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
        let mut planned = Vec::with_capacity(paths.len());
        for path in paths {
            if !path.is_absolute()
                || path.file_name().is_none()
                || path.components().any(|part| part == Component::ParentDir)
            {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            self.insert(path);
            let path = normalize(path);
            let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?.to_owned();
            let leaf = path
                .file_name()
                .ok_or(io::ErrorKind::InvalidInput)?
                .to_owned();
            planned.push(Planned {
                path,
                parent,
                leaf,
                done: false,
            });
        }
        loop {
            let Some(index) = (0..planned.len()).find(|&index| {
                !planned[index].done && parent_walk_is_safe(&planned[index], &planned)
            }) else {
                // No remaining root can be walked without entering a leaf whose
                // parent identity is still unknown.
                if planned.iter().any(|root| !root.done) {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                break;
            };
            self.map_configured(&planned, index, &mut missing_edges, &within_budget)?;
            planned[index].done = true;
        }
        self.edges.extend(missing_edges);
        Ok(())
    }

    fn map_configured(
        &mut self,
        planned: &[Planned],
        index: usize,
        missing_edges: &mut Vec<((u64, u64), OsString)>,
        within_budget: &impl Fn() -> bool,
    ) -> io::Result<()> {
        let parent = planned[index].parent.clone();
        let name = planned[index].leaf.clone();
        let mut absent = None;
        let mapped = walk_parent(
            &parent,
            |part, directory| {
                within_budget()
                    && self.allows_child(part, directory)
                    && !blocks_unresolved_leaf(part, planned)
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
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Keep the first absent component closed for this scan if
                // another process creates it after the guarded lookup.
                missing_edges.push(absent.ok_or(error)??);
                Ok(())
            }
            Err(error) => Err(error),
        }
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
    #[cfg(not(target_os = "macos"))]
    {
        a == b
    }
    #[cfg(target_os = "macos")]
    {
        a == b || {
            let (a, b) = (a.as_bytes(), b.as_bytes());
            if a.is_ascii() && b.is_ascii() {
                a.eq_ignore_ascii_case(b)
            } else {
                macos_name_eq(a, b)
            }
        }
    }
}

fn parent_walk_is_safe(root: &Planned, planned: &[Planned]) -> bool {
    let mut walked = PathBuf::new();
    for component in root.parent.components() {
        walked.push(component);
        if blocks_unresolved_leaf(&walked, planned) {
            return false;
        }
    }
    true
}

/// A same-basename component whose configured parent is not resolved yet.
/// Strict ancestors are ordinary prefixes (`…/claude/…/claude`); every other
/// match can be an alias of the protected leaf, so the walk must not enter it.
fn blocks_unresolved_leaf(part: &Path, planned: &[Planned]) -> bool {
    let Some(name) = part.file_name() else {
        return false;
    };
    let part = comparable(part);
    planned.iter().any(|root| {
        if root.done || !same_name(name, &root.leaf) {
            return false;
        }
        let root_path = comparable(&root.path);
        part == root_path || !root_path.starts_with(&part)
    })
}

#[cfg(target_os = "macos")]
fn macos_name_eq(a: &[u8], b: &[u8]) -> bool {
    use std::ffi::c_void;
    type Cf = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithBytes(
            allocator: Cf,
            bytes: *const u8,
            len: isize,
            encoding: u32,
            external: u8,
        ) -> Cf;
        fn CFStringCompare(a: Cf, b: Cf, options: usize) -> isize;
        fn CFRelease(value: Cf);
    }
    const UTF8: u32 = 0x0800_0100;
    const CASE_INSENSITIVE: usize = 1;
    const NONLITERAL: usize = 16;
    // SAFETY: byte slices are live; CFString copies the supplied bytes. Null
    // conversions are ambiguous, and non-null Create results are released once.
    unsafe {
        let a = CFStringCreateWithBytes(std::ptr::null(), a.as_ptr(), a.len() as isize, UTF8, 0);
        let b = CFStringCreateWithBytes(std::ptr::null(), b.as_ptr(), b.len() as isize, UTF8, 0);
        let matches =
            a.is_null() || b.is_null() || CFStringCompare(a, b, CASE_INSENSITIVE | NONLITERAL) == 0;
        if !a.is_null() {
            CFRelease(a);
        }
        if !b.is_null() {
            CFRelease(b);
        }
        matches
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
