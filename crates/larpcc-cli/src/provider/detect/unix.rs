mod fs;
mod paths;
#[cfg(test)]
mod tests;

use self::{
    fs::walk,
    paths::{Exclusions, Kind},
};
use crate::{
    launch::admission::ROWS,
    platform::{FileIdentity, removal::Entries},
    provider::{evidence, version},
    registry::{Error, Provider, Registry, display},
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::{BTreeSet, HashSet},
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_CANDIDATES: usize = 48;
const MAX_DIRECTORIES: usize = 160;
const MAX_ENTRIES: usize = 2048;
const SCAN_TIME: Duration = Duration::from_secs(10);
const PROBE_TIME: Duration = Duration::from_secs(7);

pub(super) fn collect(registry: &Registry, provider: Option<Provider>) -> Result<Value, Error> {
    let home = absolute_env("HOME");
    let deadline = Instant::now() + SCAN_TIME;
    let mut scan = Scan {
        exclusions: Exclusions::new(home.as_deref()),
        deadline,
        limited: Cell::new(false),
        dirs: 0,
        entries: 0,
        checked: HashSet::new(),
        candidates: Vec::new(),
        skipped: BTreeSet::new(),
    };
    let mut protected = Vec::new();
    for name in ["CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        protected.extend(provider_home(name)?);
    }
    if let Some(home) = &home {
        for path in [
            ".config/Claude",
            ".config/ChatGPT",
            ".config/claude",
            ".config/chatgpt",
            "Library/Application Support/Claude",
            "Library/Application Support/ChatGPT",
            "Library/Containers/com.anthropic.claudefordesktop",
            "Library/Containers/com.openai.chat",
            "Library/Caches/com.anthropic.claudefordesktop",
            "Library/Caches/com.openai.chat",
        ] {
            protected.push(home.join(path));
        }
    }
    for name in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        if let Some(root) = absolute_env(name) {
            for app in [
                "Claude",
                "claude",
                "ChatGPT",
                "chatgpt",
                "Codex",
                "codex",
                "com.openai.chat",
                "com.openai.codex",
                "com.anthropic.claudefordesktop",
            ] {
                protected.push(root.join(app));
            }
        }
    }
    for path in &protected {
        scan.exclusions.insert(path);
    }
    for path in protected {
        scan.exclude_alias(&path);
    }
    if let Some(home) = &home {
        // Inspect names in the user's home only to account for renamed or
        // symlinked provider roots. No provider directory is opened.
        let hidden = |name: &OsStr| {
            let name = name.as_bytes().to_ascii_lowercase();
            name.starts_with(b".claude") || name.starts_with(b".codex")
        };
        if let Some(names) = scan.directory_names(home, hidden) {
            for path in names {
                scan.exclude_alias(&path);
            }
        } else {
            scan.limited.set(true);
        }
    }
    if scan.limited.get() {
        return Err(Error::ForbiddenSource);
    }
    let allowed = |path: &Path| scan.exclusions.allows(path);
    if [
        "profiles",
        "auth-locks",
        "registry.json",
        "registry.lock",
        "registry.json.tmp",
        "share-journal.json",
    ]
    .iter()
    .any(|name| !allowed(&registry.root().join(name)))
    {
        return Err(Error::ForbiddenSource);
    }
    // Registry path resolution must obey the same exclusions as search roots.
    match walk(registry.root(), allowed, false) {
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err(Error::ForbiddenSource),
    }
    let first = provider.unwrap_or(Provider::Claude);
    let mut bindings = vec![(first, registry.binding_path(first)?)];
    let snapshot = Registry::at(registry.root().into())?.expecting(registry.revision());
    if provider.is_none() {
        bindings.push((Provider::Codex, snapshot.binding_path(Provider::Codex)?));
    }
    for (provider, bound) in &bindings {
        if let Some(path) = bound {
            scan.offer(*provider, path.clone(), Kind::Unknown, Some(path));
        }
    }
    for (provider, bound) in &bindings {
        scan.locations(*provider, bound.as_deref(), home.as_deref());
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path).filter(|dir| dir.is_absolute()) {
            if !scan.start_directory() {
                break;
            }
            for (provider, bound) in &bindings {
                scan.offer(
                    *provider,
                    dir.join(provider.as_str()),
                    Kind::Unknown,
                    bound.as_deref(),
                );
            }
        }
    }
    let desktop = scan.desktop_apps(&bindings, home.as_deref());
    let result = scan.results(registry.root(), desktop)?;
    snapshot.binding_path(first)?;
    Ok(result)
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

fn provider_home(name: &str) -> Result<Option<PathBuf>, Error> {
    let Some(path) = std::env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    if path.is_absolute() {
        return Ok(Some(path));
    }
    let cwd = std::env::current_dir().map_err(|_| Error::ForbiddenSource)?;
    Ok(Some(cwd.join(path)))
}

struct Candidate {
    provider: Provider,
    path: PathBuf,
    resolved: PathBuf,
    identity: FileIdentity,
    size: u64,
    kind: Kind,
    bound: bool,
}

struct Scan {
    exclusions: Exclusions,
    deadline: Instant,
    limited: Cell<bool>,
    dirs: usize,
    entries: usize,
    checked: HashSet<(Provider, PathBuf)>,
    candidates: Vec<Candidate>,
    skipped: BTreeSet<(&'static str, PathBuf)>,
}

impl Scan {
    fn exclude_alias(&mut self, protected: &Path) {
        // Look up the root itself without opening it. Never make an exception
        // for an ancestor or a component reached below another protected root.
        let protected = paths::normalize(protected);
        let Some((parent, name)) = protected.parent().zip(protected.file_name()) else {
            return;
        };
        let Ok(parent) = walk(parent, |p| self.allow(p), false) else {
            return;
        };
        let leaf = parent.path.join(name);
        let guard = |p: &Path| self.time_left() && (p == leaf || self.exclusions.allows(p));
        if let Ok(entry) = walk(&leaf, guard, false) {
            self.exclusions.insert(&entry.path);
        }
    }

    fn time_left(&self) -> bool {
        let time_left = Instant::now() < self.deadline;
        if !time_left {
            self.limited.set(true);
        }
        time_left
    }

    fn allow(&self, path: &Path) -> bool {
        self.time_left() && self.exclusions.allows(path)
    }

    fn start_directory(&mut self) -> bool {
        if !self.time_left() || self.dirs == MAX_DIRECTORIES {
            self.limited.set(true);
            return false;
        }
        self.dirs += 1;
        true
    }

    fn names(&mut self, path: &Path) -> Vec<PathBuf> {
        self.directory_names(path, |_| true).unwrap_or_default()
    }

    fn directory_names(
        &mut self,
        path: &Path,
        retain: impl Fn(&OsStr) -> bool,
    ) -> Option<Vec<PathBuf>> {
        if !self.start_directory() {
            return None;
        }
        let entry = match walk(path, |p| self.allow(p), true) {
            Ok(entry) => entry,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
            Err(_) => return None,
        };
        let directory = entry.directory?;
        let mut entries = Entries::open(&directory).ok()?;
        let mut names = Vec::new();
        while self.time_left() {
            let name = match entries.next() {
                Ok(Some(name)) => name,
                Ok(None) => {
                    names.sort_unstable();
                    return Some(names);
                }
                Err(_) => return None,
            };
            if self.entries == MAX_ENTRIES {
                self.limited.set(true);
                break;
            }
            self.entries += 1;
            let name = std::ffi::OsStr::from_bytes(name.to_bytes());
            if retain(name) {
                names.push(path.join(name));
            }
        }
        // Discard an incomplete enumeration so filesystem order cannot select its candidates.
        None
    }

    fn offer(&mut self, provider: Provider, path: PathBuf, kind: Kind, bound: Option<&Path>) {
        if !self.time_left() || super::super::lexical(&path).is_err() {
            return;
        }
        let Some(entry) = self.locate(provider, &path) else {
            return;
        };
        if !entry.executable() || !self.checked.insert((provider, path.clone())) {
            return;
        }
        if self.candidates.len() == MAX_CANDIDATES {
            self.limited.set(true);
            return;
        }
        let identity = stat_identity(&entry.stat);
        let kind = match infer_kind(&entry.path) {
            Kind::Unknown => kind,
            inferred => inferred,
        };
        self.candidates.push(Candidate {
            bound: bound == Some(path.as_path()),
            provider,
            path,
            resolved: entry.path,
            identity,
            size: u64::try_from(entry.stat.st_size).unwrap_or_default(),
            kind,
        });
    }

    fn locate(&mut self, provider: Provider, path: &Path) -> Option<fs::Entry> {
        let inside_home = Cell::new(self.exclusions.inside_provider_home(path));
        let entry = if inside_home.get() {
            None
        } else {
            walk(
                path,
                |part| {
                    let inside = self.exclusions.inside_provider_home(part);
                    inside_home.set(inside);
                    !inside && self.allow(part)
                },
                false,
            )
            .ok()
        };
        if inside_home.get() {
            if self.skipped.len() < MAX_CANDIDATES {
                self.skipped.insert((provider.as_str(), path.to_owned()));
            } else {
                self.limited.set(true);
            }
        }
        entry
    }

    fn locations(&mut self, provider: Provider, bound: Option<&Path>, home: Option<&Path>) {
        let command = provider.as_str();
        if let Some(home) = home {
            self.offer(
                provider,
                home.join(".local/bin").join(command),
                Kind::Native,
                bound,
            );
            if provider == Provider::Codex
                && let Some(binary) = codex_release_binary()
            {
                self.offer(
                    provider,
                    home.join(".local/bin").join(binary),
                    Kind::Native,
                    bound,
                );
            }
            self.offer(
                provider,
                home.join(".bun/bin").join(command),
                Kind::Bun,
                bound,
            );
            for dir in [
                ".npm-global/bin",
                ".volta/bin",
                ".local/share/fnm/aliases/default/bin",
                "Library/Application Support/fnm/aliases/default/bin",
            ] {
                self.offer(provider, home.join(dir).join(command), Kind::Npm, bound);
            }
            for (dir, bin) in [
                (".nvm/versions/node", "bin"),
                (".volta/tools/image/node", "bin"),
                (".local/share/fnm/node-versions", "installation/bin"),
                (".fnm/node-versions", "installation/bin"),
                (
                    "Library/Application Support/fnm/node-versions",
                    "installation/bin",
                ),
            ] {
                for version in self.names(&home.join(dir)) {
                    self.offer(provider, version.join(bin).join(command), Kind::Npm, bound);
                }
            }
            if provider == Provider::Claude {
                for version in self.names(&home.join(".local/share/claude/versions")) {
                    self.offer(provider, version.clone(), Kind::Native, bound);
                    self.offer(provider, version.join(command), Kind::Native, bound);
                }
            }
        }
        for prefix in ["/opt/homebrew", "/usr/local"] {
            self.homebrew_locations(Path::new(prefix), provider, bound);
        }
    }

    fn homebrew_locations(&mut self, prefix: &Path, provider: Provider, bound: Option<&Path>) {
        let command = provider.as_str();
        self.offer(
            provider,
            prefix.join("bin").join(command),
            Kind::Unknown,
            bound,
        );
        let formula = if provider == Provider::Claude {
            "claude-code"
        } else {
            command
        };
        for version in self.names(&prefix.join("Cellar").join(formula)) {
            self.offer(
                provider,
                version.join("bin").join(command),
                Kind::Homebrew,
                bound,
            );
        }
        for version in self.names(&prefix.join("Caskroom").join(formula)) {
            self.offer(provider, version.join(command), Kind::Homebrew, bound);
            if provider == Provider::Codex
                && let Some(binary) = codex_release_binary()
            {
                self.offer(provider, version.join(binary), Kind::Homebrew, bound);
            }
            self.offer(
                provider,
                version.join("bin").join(command),
                Kind::Homebrew,
                bound,
            );
        }
    }

    fn desktop_apps(
        &mut self,
        bindings: &[(Provider, Option<PathBuf>)],
        home: Option<&Path>,
    ) -> Vec<Value> {
        #[cfg(target_os = "macos")]
        {
            let dirs = std::iter::once(PathBuf::from("/Applications"))
                .chain(home.map(|home| home.join("Applications")));
            let mut rows = Vec::new();
            let mut seen = HashSet::new();
            for directory in dirs {
                for &(provider, _) in bindings {
                    let name = match provider {
                        Provider::Claude => "Claude.app",
                        Provider::Codex => "ChatGPT.app",
                    };
                    let path = directory.join(name);
                    let Some(entry) = self.locate(provider, &path) else {
                        continue;
                    };
                    if entry.stat.st_mode & libc::S_IFMT == libc::S_IFDIR
                        && seen.insert(stat_identity(&entry.stat))
                    {
                        let confined = fs::confined_bundle(&entry.path, |p| self.allow(p));
                        rows.push(crate::desktop::detect_app(
                            provider,
                            &path,
                            &entry.path,
                            confined,
                        ));
                    }
                }
            }
            rows
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (bindings, home);
            Vec::new()
        }
    }

    fn results(mut self, root: &Path, desktop_apps: Vec<Value>) -> Result<Value, Error> {
        let candidates = std::mem::take(&mut self.candidates);
        let mut rows = Vec::new();
        for candidate in candidates {
            if !self.time_left() {
                break;
            }
            let artifact =
                evidence::collect_candidate(candidate.provider, &candidate.path, root, &|p| {
                    self.allow(p)
                })
                .and_then(|e| {
                    if e.matches(&candidate.resolved, &candidate.identity) {
                        Ok(e)
                    } else {
                        Err(Error::ArtifactInvalid)
                    }
                });
            let qualified = artifact.as_ref().is_ok_and(|e| {
                ROWS.iter().any(|row| {
                    row.provider == candidate.provider
                        && row.os == std::env::consts::OS
                        && row.arch == std::env::consts::ARCH
                        && row.sha256 == e.digest()
                })
            });
            rows.push((candidate, qualified, artifact));
        }
        rows.sort_by(|(a, aq, ae), (b, bq, be)| {
            (
                !a.bound,
                !*aq,
                ae.is_err(),
                a.kind,
                a.provider.as_str(),
                &a.path,
            )
                .cmp(&(
                    !b.bound,
                    !*bq,
                    be.is_err(),
                    b.kind,
                    b.provider.as_str(),
                    &b.path,
                ))
        });
        let mut seen = HashSet::new();
        rows.retain(|(c, ..)| seen.insert((c.provider, c.identity.clone())));
        rows.sort_by(|(a, aq, _), (b, bq, _)| {
            (!a.bound, !*aq, a.kind, a.provider.as_str(), &a.path).cmp(&(
                !b.bound,
                !*bq,
                b.kind,
                b.provider.as_str(),
                &b.path,
            ))
        });
        let executables: Result<Vec<_>, Error> = rows
            .into_iter()
            .map(|(c, qualified, artifact)| {
                let detection = match artifact.as_ref().map(evidence::Evidence::digest) {
                    Ok(digest) if version::approved(c.provider, digest) => {
                        let probe_guard = |path: &Path| {
                            if self.deadline.saturating_duration_since(Instant::now()) < PROBE_TIME
                            {
                                self.limited.set(true);
                                return false;
                            }
                            self.allow(path)
                        };
                        if probe_guard(&c.path) {
                            version::detect_candidate(
                                c.provider,
                                &c.path,
                                root,
                                digest,
                                &probe_guard,
                            )?
                        } else {
                            version::Detection::Failed
                        }
                    }
                    _ => version::Detection::Unapproved,
                };
                let (version, diagnostic) = match detection {
                    version::Detection::Detected(v) => (Some(v), None),
                    version::Detection::Unapproved => {
                        (Some("unapproved".into()), Some("version_unapproved"))
                    }
                    version::Detection::Failed => (None, Some("version_probe_failed")),
                };
                let FileIdentity::Unix { dev, ino } = c.identity;
                let evidence = artifact
                    .as_ref()
                    .map(evidence::Evidence::json)
                    .unwrap_or(Value::Null);
                let bind_reason = artifact.as_ref().err().map(|error| match error {
                    Error::InvalidExecutable { reason, .. } | Error::Unsafe { reason, .. } => {
                        reason.as_str()
                    }
                    _ => "executable evidence could not be verified",
                });
                Ok(json!({
                    "provider": c.provider,
                    "path": display::path(&c.path),
                    "resolved_path": display::path(&c.resolved),
                    "identity": {"kind":"unix","dev":dev.to_string(),"ino":ino.to_string()},
                    "size": c.size.to_string(),
                    "install_kind": c.kind.label(),
                    "bindable": artifact.is_ok(),
                    "bind_reason": bind_reason,
                    "qualified": qualified,
                    "bound": c.bound,
                    "sha256": evidence["sha256"],
                    "cdhash": evidence["cdhash"],
                    "version": version,
                    "version_diagnostic": diagnostic,
                }))
            })
            .collect();
        Ok(json!({
            "executables": executables?,
            "desktop_apps": desktop_apps,
            "skipped": self.skipped.iter().map(|(provider, path)| {
                json!({"provider": provider, "path": display::path(path), "reason": "inside provider home"})
            }).collect::<Vec<_>>(),
            "limited": self.limited.get(),
        }))
    }
}

fn codex_release_binary() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "aarch64") => Some("codex-aarch64-unknown-linux-musl"),
        ("linux", "x86_64") => Some("codex-x86_64-unknown-linux-musl"),
        ("macos", "aarch64") => Some("codex-aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("codex-x86_64-apple-darwin"),
        _ => None,
    }
}

fn infer_kind(path: &Path) -> Kind {
    if path.starts_with("/opt/homebrew")
        || path.starts_with("/usr/local/Cellar")
        || path.starts_with("/usr/local/Caskroom")
    {
        return Kind::Homebrew;
    }
    if path.components().any(|p| {
        matches!(
            p.as_os_str().to_str(),
            Some(".nvm" | ".volta" | ".fnm" | "fnm" | ".npm-global" | "node_modules")
        )
    }) {
        return Kind::Npm;
    }
    if path.components().any(|p| p.as_os_str() == ".bun") {
        return Kind::Bun;
    }
    if path.components().any(|p| p.as_os_str() == ".local")
        && (path.parent().is_some_and(|p| p.ends_with(".local/bin"))
            || path.ancestors().any(|p| p.ends_with("claude/versions")))
    {
        return Kind::Native;
    }
    Kind::Unknown
}

#[allow(clippy::unnecessary_cast, reason = "stat field widths vary by OS")]
fn stat_identity(stat: &libc::stat) -> FileIdentity {
    FileIdentity::Unix {
        dev: stat.st_dev as u64,
        ino: stat.st_ino as u64,
    }
}
