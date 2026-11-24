use super::{claude, files::Directory, profile_directory};
use crate::{
    aliases::{self, Artifact, Directories, DirectoryPin, Source},
    launch::admission::{Admitted, Operation},
    platform::{self, FileIdentity},
    registry::{Error, Provider, Registry},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const EXE: &str = ".larp-usage-hook";
const RECEIPT: &str = ".larp-usage-hook.receipt.json";
const RENDER: &str = "claude-settings.json";
const MANIFEST: &str = "claude-settings.owned.json";
// Each approved digest must prove Claude's precedence, shell and environment inheritance.
const APPROVED_STATUSLINE_IMAGES: &[[u8; 32]] = &[];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookReceipt {
    version: u32,
    root: PathBuf,
    directories: DirectoryPin,
    executable: Artifact,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderReceipt {
    version: u32,
    identity: FileIdentity,
    sha256: [u8; 32],
}

fn bounded(file: impl Read) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Drift)?;
    if bytes.len() > 4096 {
        return Err(Error::Drift);
    }
    Ok(bytes)
}

fn receipt(dirs: &Directories, root: &Path) -> Result<Option<HookReceipt>, Error> {
    let path = root.join("bin").join(RECEIPT);
    let Some(object) = dirs.read(&path)? else {
        return Ok(None);
    };
    let receipt: HookReceipt =
        serde_json::from_slice(&bounded(object.file)?).map_err(|_| Error::Drift)?;
    if receipt.version != 1
        || receipt.directories != dirs.pin()
        || !claude::safe_root(&receipt.root)
        || std::fs::canonicalize(&receipt.root).map_err(|_| Error::Drift)?
            != std::fs::canonicalize(root).map_err(|_| Error::Drift)?
        || receipt
            .executable
            .present(dirs, &root.join("bin").join(EXE))?
            != Some(true)
    {
        return Err(Error::Drift);
    }
    Ok(Some(receipt))
}

pub(crate) fn install(root: &Path) -> Result<(), Error> {
    if !claude::safe_root(root) {
        return Ok(());
    }
    Directory::root(root)
        .map_err(|_| Error::Drift)?
        .ok_or(Error::Drift)?
        .ensure_directory(c"bin")
        .map_err(|_| Error::Drift)?;
    let dirs = Directories::open(root)?;
    if receipt(&dirs, root)?.is_some() {
        return Ok(());
    }
    let executable = root.join("bin").join(EXE);
    let source = Source::current(root)?;
    let file = dirs.create(&executable)?;
    let image = Artifact {
        identity: file.identity.clone(),
        digest: source.sha256,
    };
    if let Err(error) = aliases::fill_launcher(file, &source) {
        let _ = dirs.remove(&executable, &image.identity);
        return Err(error);
    }
    let bytes = serde_json::to_vec(&HookReceipt {
        version: 1,
        root: root.into(),
        directories: dirs.pin(),
        executable: image,
    })
    .map_err(|_| Error::Drift)?;
    let mut receipt = dirs.create(&root.join("bin").join(RECEIPT))?.file;
    receipt
        .write_all(&bytes)
        .and_then(|_| receipt.sync_all())
        .map_err(|_| Error::Drift)?;
    dirs.sync()
}

fn ensure_render(directory: &Directory, expected: &[u8]) -> Result<(), Error> {
    let render = directory
        .file(OsStr::new(RENDER), true)
        .map_err(|_| Error::Drift)?;
    let manifest = directory
        .file(OsStr::new(MANIFEST), true)
        .map_err(|_| Error::Drift)?;
    match (render, manifest) {
        (None, None) => {
            let identity = directory
                .install(RENDER, expected)
                .map_err(|_| Error::Drift)?;
            let manifest = RenderReceipt {
                version: 1,
                identity,
                sha256: aliases::digest(expected),
            };
            let bytes = serde_json::to_vec(&manifest).map_err(|_| Error::Drift)?;
            directory
                .install(MANIFEST, &bytes)
                .map_err(|_| Error::Drift)?;
        }
        (Some(render), Some(manifest)) => {
            let manifest: RenderReceipt =
                serde_json::from_slice(&bounded(manifest.file)?).map_err(|_| Error::Drift)?;
            if manifest.version != 1
                || manifest.identity != render.identity
                || manifest.sha256 != aliases::digest(expected)
                || bounded(render.file)? != expected
            {
                return Err(Error::Drift);
            }
        }
        _ => return Err(Error::Drift),
    }
    directory.recheck().map_err(|_| Error::Drift)
}

/// Called only under the registry lock, after an exact future Claude Run row admits this flag.
fn prepare(root: &Path, id: u64, launch_dir: &Path) -> Result<Option<PathBuf>, Error> {
    let Some(render) = claude::render(root, id) else {
        return Ok(None);
    };
    if claude::launch_presence(root, id, launch_dir) != claude::Presence::Absent {
        return Ok(None);
    }
    let profile = profile_directory(root, id)
        .map_err(|_| Error::Drift)?
        .ok_or(Error::Drift)?;
    let directory = profile
        .ensure_directory(c"usage")
        .map_err(|_| Error::Drift)?;
    let directories = Directories::open(root)?;
    if receipt(&directories, root)?.is_none() {
        return Err(Error::Drift);
    }
    ensure_render(&directory, &render)?;
    Ok(Some(directory.path().join(RENDER)))
}

pub(crate) fn attach_to_launch(
    registry: &Registry,
    admitted: &mut Admitted<'_>,
) -> Result<(), Error> {
    let row = admitted.row;
    if row.operation != Operation::Run
        || row.provider != Provider::Claude
        || !APPROVED_STATUSLINE_IMAGES.contains(&row.sha256)
    {
        return Ok(());
    }
    if admitted
        .plan
        .argv
        .iter()
        .any(|arg| arg == "--settings" || arg.as_encoded_bytes().starts_with(b"--settings="))
    {
        return Ok(());
    }
    let _lock = registry.admission_lock()?;
    let snapshot = &admitted.snapshot;
    let target = &snapshot.target;
    let fresh = registry.launch_target(&target.profile.name)?;
    if fresh.revision != snapshot.revision
        || fresh.target != *target
        || fresh.homes != snapshot.homes
    {
        return Err(Error::AdmissionChanged);
    }
    let id = target.profile.id.get();
    if registry.claude_usage_enabled(id)?
        && let Some(launch_dir) = admitted
            .plan
            .cwd
            .clone()
            .or_else(|| std::env::current_dir().ok())
        && let Some(path) = prepare(registry.root(), id, &launch_dir)?
    {
        admitted.plan.argv.push("--settings".into());
        admitted.plan.argv.push(path.into_os_string());
    }
    Ok(())
}

fn args(args: &[OsString]) -> Option<u64> {
    let [action, id] = args else { return None };
    if action != "claude-statusline" {
        return None;
    }
    let text = id.to_str()?;
    let id: u64 = text.parse().ok()?;
    (id != 0 && id.to_string() == text).then_some(id)
}

fn is_hook(argv0: &OsStr) -> bool {
    Path::new(argv0).file_name() == Some(OsStr::new(EXE))
}

pub(crate) fn dispatch_if_hook(argv0: &OsStr) -> Option<std::process::ExitCode> {
    if !is_hook(argv0) && !std::env::current_exe().is_ok_and(|image| is_hook(image.as_os_str())) {
        return None;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind(dispatch);
    Some(std::process::ExitCode::SUCCESS)
}

fn dispatch() {
    let argv: Vec<_> = std::env::args_os().skip(1).take(3).collect();
    let Some(id) = args(&argv) else { return };
    let checked = (|| {
        let image = std::env::current_exe().ok()?;
        if !is_hook(image.as_os_str()) {
            return None;
        }
        let bin = image.parent()?;
        if bin.file_name()? != "bin" {
            return None;
        }
        let root = bin.parent()?;
        let directories = Directories::open(root).ok()?;
        let receipt = receipt(&directories, root).ok()??;
        if platform::running_identity(&image).ok()? != receipt.executable.identity {
            return None;
        }
        let expected = receipt
            .root
            .join("profiles")
            .join(id.to_string())
            .join("cli");
        if std::env::var_os("CLAUDE_CONFIG_DIR").as_deref() != Some(expected.as_os_str()) {
            return None;
        }
        Some((receipt.root, directories))
    })();
    if let Some((root, _directories)) = checked {
        claude::capture_stdin(&root, id, libc::STDIN_FILENO, || {
            let registry = Registry::at(root.clone()).ok()?;
            registry.usage_capture_lock(id).ok()?
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage11_local_claude_hook_owned_render_and_drift() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
        let temp = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let root = temp.path().join("root with space");
        let registry = Registry::at(root.clone()).unwrap();
        registry
            .add("claude-work".parse().unwrap(), Provider::Claude)
            .unwrap();
        let profile = profile_directory(&root, 1).unwrap().unwrap();
        let directory = profile.ensure_directory(c"usage").unwrap();
        let bytes = claude::render(&root, 1).unwrap();
        let expected = format!(
            "{{\"statusLine\":{{\"type\":\"command\",\"command\":\"'{}/bin/.larp-usage-hook' claude-statusline 1\",\"padding\":0}}}}",
            root.display(),
        );
        assert_eq!(bytes, expected.as_bytes());
        assert!(claude::render(&root.join("cannot'quote"), 1).is_none());
        ensure_render(&directory, &bytes).unwrap();
        ensure_render(&directory, &bytes).unwrap();
        let settings = directory.path().join(RENDER);
        fs::write(&settings, b"drift").unwrap();
        assert!(matches!(
            ensure_render(&directory, &bytes),
            Err(Error::Drift)
        ));
        assert_eq!(fs::read(&settings).unwrap(), b"drift");
    }
}
