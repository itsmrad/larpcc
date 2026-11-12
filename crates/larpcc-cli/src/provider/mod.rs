use crate::registry::{Error, Provider};
use std::path::{Component, Path};
pub fn lexical(path: &Path) -> Result<(), &'static str> {
    let value = path.to_str().ok_or("must be Unicode")?;
    if value.contains('\0') {
        return Err("contains a NUL character");
    }
    if !path.is_absolute() {
        return Err("must be an absolute path");
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err("must not contain '..'");
    }
    #[cfg(unix)]
    if !value.starts_with('/')
        || value.starts_with("//")
        || value.ends_with('/')
        || value[1..]
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err("must be normalized");
    }
    #[cfg(windows)]
    {
        let bytes = value.as_bytes();
        if bytes.len() < 4
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || bytes[2] != b'\\'
        {
            return Err("must be a local disk path");
        }
        if value.contains('/')
            || value[3..].split('\\').any(|c| {
                c.is_empty()
                    || c == "."
                    || c == ".."
                    || c.ends_with([' ', '.'])
                    || c.contains(':')
                    || reserved(c)
            })
        {
            return Err("must be normalized");
        }
    }
    if value.len() > 4096 {
        return Err("exceeds 4096 bytes");
    }
    Ok(())
}
#[cfg(windows)]
fn reserved(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}
pub fn validate(
    provider: Provider,
    path: &Path,
    root: &Path,
) -> Result<(std::path::PathBuf, crate::platform::FileIdentity), Error> {
    let invalid = |reason: String| Error::InvalidExecutable {
        provider,
        path: path.into(),
        reason,
    };
    input(provider, path, root)?;
    crate::platform::executable(path, root).map_err(|e| match e {
        Error::Unsafe { reason, .. } => invalid(reason),
        Error::ManagedMode { .. } => invalid("is writable by group or other users".into()),
        Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            invalid("does not exist".into())
        }
        other => other,
    })
}
pub fn input(provider: Provider, path: &Path, root: &Path) -> Result<(), Error> {
    let invalid = |reason: &str| Error::InvalidExecutable {
        provider,
        path: path.into(),
        reason: reason.into(),
    };
    lexical(path).map_err(invalid)?;
    if crate::platform::contains_path(path, root)? {
        return Err(invalid("is inside the Larp CC data directory"));
    }
    Ok(())
}
