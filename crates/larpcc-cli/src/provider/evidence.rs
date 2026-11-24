//! Stage 08 bound-executable evidence (stage 14 Phase 2 step 1). Reads only the
//! bound executable through the trusted-path, owner/ACL/identity and bounded-hash
//! machinery. Executes nothing, reads no profile home or account data, writes nothing.
use crate::{
    launch::admission,
    platform::FileIdentity,
    registry::{BindingState, Error, Provider, Registry, display},
};
use serde_json::{Value, json};
use std::{fs::File, path::Path};

pub(crate) struct Evidence {
    provider: Provider,
    path: std::path::PathBuf,
    resolved: std::path::PathBuf,
    identity: FileIdentity,
    owner: Option<u32>,
    mode: Option<u32>,
    size: u64,
    sha256: [u8; 32],
    cdhash: Option<Vec<u8>>,
}

/// The registry snapshot (with `--expected-revision`) selects the binding; the
/// same revision is required again after hashing.
pub(crate) fn collect(registry: &Registry, provider: Provider) -> Result<Evidence, Error> {
    let executable = binding(registry, provider)?;
    let evidence = collect_with(provider, &executable, registry.root(), |_| {})?;
    let again = Registry::at(registry.root().to_owned())?.expecting(registry.revision());
    if binding(&again, provider)? != executable {
        return Err(Error::Conflict);
    }
    Ok(evidence)
}

fn binding(registry: &Registry, provider: Provider) -> Result<std::path::PathBuf, Error> {
    match registry
        .bindings()?
        .into_iter()
        .find(|(p, _)| *p == provider)
        .map(|(_, state)| state)
    {
        Some(BindingState::Ok(binding)) => Ok(binding.executable),
        Some(BindingState::Invalid(binding, reason)) => Err(Error::InvalidExecutable {
            provider,
            path: binding.executable,
            reason,
        }),
        Some(BindingState::Unbound) | None => Err(Error::Unbound { provider }),
    }
}

fn collect_with(
    provider: Provider,
    path: &Path,
    root: &Path,
    before_read: impl FnOnce(&File),
) -> Result<Evidence, Error> {
    let (resolved, identity) = super::validate(provider, path, root)?;
    // Hash the held handle; the stored spelling is re-resolved after the read.
    let artifact = admission::checked_artifact_with(path, root, before_read)?;
    if artifact.identity != identity {
        return Err(Error::ArtifactInvalid);
    }
    let meta = artifact
        .file
        .metadata()
        .map_err(|_| Error::ArtifactInvalid)?;
    #[cfg(unix)]
    let (owner, mode) = {
        use std::os::unix::fs::MetadataExt;
        (Some(meta.uid()), Some(meta.mode() & 0o7777))
    };
    #[cfg(not(unix))]
    let (owner, mode) = {
        let _ = &meta;
        (None, None)
    };
    #[cfg(target_os = "macos")]
    let cdhash = crate::desktop::code_directory_hash(&resolved).ok();
    #[cfg(not(target_os = "macos"))]
    let cdhash = None;
    // The code-signature read is path based: the same spelling must still resolve
    // to the same checked object afterwards.
    let (after, current) = super::validate(provider, path, root)?;
    if after != resolved || current != identity {
        return Err(Error::ArtifactInvalid);
    }
    Ok(Evidence {
        provider,
        path: path.to_owned(),
        resolved,
        identity,
        owner,
        mode,
        size: artifact.size,
        sha256: artifact.sha256,
        cdhash,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Evidence {
    fn identity_json(&self) -> Value {
        match &self.identity {
            #[cfg(unix)]
            FileIdentity::Unix { dev, ino } => {
                json!({"kind":"unix","dev":dev.to_string(),"ino":ino.to_string()})
            }
            #[cfg(windows)]
            FileIdentity::Windows { volume, id } => {
                json!({"kind":"windows","volume":volume.to_string(),"file_id":hex(id)})
            }
        }
    }
    pub(crate) fn json(&self) -> Value {
        json!({
            "provider": self.provider,
            "path": display::path(&self.path),
            "resolved_path": display::path(&self.resolved),
            "identity": self.identity_json(),
            "owner_uid": self.owner.map(|u| u.to_string()),
            "mode": self.mode.map(|m| format!("{m:04o}")),
            "size": self.size.to_string(),
            "sha256": hex(&self.sha256),
            "cdhash": self.cdhash.as_deref().map(hex),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        })
    }
    pub(crate) fn human(&self) -> String {
        let identity = match &self.identity {
            #[cfg(unix)]
            FileIdentity::Unix { dev, ino } => format!("unix dev {dev} inode {ino}"),
            #[cfg(windows)]
            FileIdentity::Windows { volume, id } => {
                format!("windows volume {volume} file {}", hex(id))
            }
        };
        let or = |v: Option<String>| v.unwrap_or_else(|| "unavailable".into());
        format!(
            "Provider: {}\nPath: {}\nResolved path: {}\nIdentity: {identity}\nOwner UID: {}\nMode: {}\nSize: {} bytes\nSHA-256: {}\nCode directory hash: {}\nPlatform: {} {}\nNothing was executed and no profile or account data was read.\n",
            self.provider,
            display::path(&self.path),
            display::path(&self.resolved),
            or(self.owner.map(|u| u.to_string())),
            or(self.mode.map(|m| format!("{m:04o}"))),
            self.size,
            hex(&self.sha256),
            or(self.cdhash.as_deref().map(hex)),
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn private_dir() -> tempfile::TempDir {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap()
    }
    // A minimal native header passes the magic check; nothing is ever executed.
    fn native(path: &Path, tail: &[u8]) {
        #[cfg(target_os = "linux")]
        let mut bytes = b"\x7fELF".to_vec();
        #[cfg(target_os = "macos")]
        let mut bytes = 0xfeedfacfu32.to_be_bytes().to_vec();
        bytes.extend_from_slice(tail);
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn stage08_evidence_exact_identity_size_digest() {
        use sha2::Digest;
        let temp = private_dir();
        let exe = temp.path().join("fake-codex");
        native(&exe, b"invented artifact bytes");
        let root = temp.path().join("managed");
        let e = collect_with(Provider::Codex, &exe, &root, |_| {}).unwrap();
        let bytes = std::fs::read(&exe).unwrap();
        let meta = std::fs::metadata(&exe).unwrap();
        use std::os::unix::fs::MetadataExt;
        let value = e.json();
        assert_eq!(value["size"], bytes.len().to_string());
        assert_eq!(value["sha256"], hex(&sha2::Sha256::digest(&bytes)));
        assert_eq!(value["identity"]["dev"], meta.dev().to_string());
        assert_eq!(value["identity"]["ino"], meta.ino().to_string());
        assert_eq!(value["owner_uid"], meta.uid().to_string());
        assert_eq!(value["mode"], "0700");
        assert_eq!(value["resolved_path"], exe.to_str().unwrap());
        #[cfg(not(target_os = "macos"))]
        assert_eq!(value["cdhash"], Value::Null);
    }
    #[test]
    fn stage08_evidence_refuses_symlink_swap_between_check_and_hash() {
        let temp = private_dir();
        let (a, b) = (temp.path().join("a"), temp.path().join("b"));
        native(&a, b"first");
        native(&b, b"second");
        let link = temp.path().join("codex");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        let root = temp.path().join("managed");
        let swapped = temp.path().join("swap");
        let result = collect_with(Provider::Codex, &link, &root, |_| {
            std::os::unix::fs::symlink(&b, &swapped).unwrap();
            std::fs::rename(&swapped, &link).unwrap();
        });
        assert!(
            matches!(result, Err(Error::ArtifactInvalid)),
            "swap accepted"
        );
    }
    #[test]
    fn stage08_evidence_refuses_untrusted_path() {
        let temp = private_dir();
        let exe = temp.path().join("codex");
        native(&exe, b"bytes");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o722)).unwrap();
        let root = temp.path().join("managed");
        assert!(matches!(
            collect_with(Provider::Codex, &exe, &root, |_| {}),
            Err(Error::InvalidExecutable { .. })
        ));
    }
}
