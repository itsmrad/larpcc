use super::protocol::{decimal, digest};
use crate::{
    registry::ProfileName,
    share::skills::{MAX_BYTES, MAX_DEPTH, MAX_FILES},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{self, Read},
};

const PREFIX: &[u8] = b"LARPCC-SKILLS/1\n";
const MANIFEST_LIMIT: usize = 1024 * 1024;
const MANIFEST_LENGTH_BYTES: usize = 8;
pub(crate) const LIMIT: usize =
    PREFIX.len() + MANIFEST_LENGTH_BYTES + MANIFEST_LIMIT + MAX_BYTES as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Corrupt,
    Unsafe,
    TooLarge,
    Io,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: ProfileName,
    sha256: String,
    files: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    size: String,
}

fn read_exact(reader: &mut impl Read, bytes: &mut [u8]) -> Result<(), Error> {
    reader.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            Error::Corrupt
        } else {
            Error::Io
        }
    })
}

fn manifest_length(reader: &mut impl Read) -> Result<usize, Error> {
    let mut digits = Vec::with_capacity(MANIFEST_LENGTH_BYTES - 1);
    loop {
        let mut byte = [0];
        read_exact(reader, &mut byte)?;
        match byte[0] {
            b'\n' => break,
            b'0'..=b'9' if digits.len() < MANIFEST_LENGTH_BYTES - 1 => digits.push(byte[0]),
            b'0'..=b'9' => return Err(Error::TooLarge),
            _ => return Err(Error::Corrupt),
        }
    }
    let len = std::str::from_utf8(&digits)
        .ok()
        .and_then(decimal)
        .ok_or(Error::Corrupt)?;
    if len == 0 {
        return Err(Error::Corrupt);
    }
    if len > MANIFEST_LIMIT as u64 {
        return Err(Error::TooLarge);
    }
    Ok(len as usize)
}

fn file_sizes(manifest: &Manifest) -> Result<Vec<usize>, Error> {
    if manifest.files.len() > MAX_FILES {
        return Err(Error::TooLarge);
    }
    if !manifest.files.iter().any(|entry| entry.path == "SKILL.md")
        || manifest.files.windows(2).any(|p| p[0].path >= p[1].path)
    {
        return Err(Error::Unsafe);
    }
    let mut paths = BTreeMap::new();
    let mut directories = 0;
    let mut total = 0;
    let mut sizes = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        let size = decimal(&entry.size).ok_or(Error::Corrupt)?;
        if size > MAX_BYTES - total {
            return Err(Error::TooLarge);
        }
        total += size;
        sizes.push(size as usize);
        let components = entry.path.split('/');
        let depth = components.clone().count();
        if depth > MAX_DEPTH {
            return Err(Error::TooLarge);
        }
        let mut prefix_len = 0;
        for (index, component) in components.enumerate() {
            crate::share::skills::component(component).map_err(|_| Error::Unsafe)?;
            prefix_len += component.len();
            let prefix = &entry.path[..prefix_len];
            let directory = index + 1 != depth;
            match paths.entry(prefix.to_ascii_lowercase()) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert((prefix, directory));
                    if directory {
                        directories += 1;
                        if directories > MAX_FILES {
                            return Err(Error::TooLarge);
                        }
                    }
                }
                std::collections::btree_map::Entry::Occupied(slot)
                    if directory && *slot.get() == (prefix, true) => {}
                _ => return Err(Error::Unsafe),
            }
            prefix_len += 1;
        }
    }
    Ok(sizes)
}

fn hash_file(hash: &mut Sha256, path: &str, bytes: &[u8]) {
    hash.update((path.len() as u64).to_be_bytes());
    hash.update(path.as_bytes());
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

pub(crate) fn encode<'a>(
    name: &ProfileName,
    sha256: &str,
    files: impl Iterator<Item = (&'a str, &'a [u8])> + Clone,
) -> Result<Vec<u8>, Error> {
    let manifest = Manifest {
        name: name.clone(),
        sha256: sha256.into(),
        files: files
            .clone()
            .map(|(path, bytes)| Entry {
                path: path.into(),
                size: bytes.len().to_string(),
            })
            .collect(),
    };
    let sizes = file_sizes(&manifest)?;
    let bytes = serde_json::to_vec(&manifest).map_err(|_| Error::Corrupt)?;
    if bytes.len() > MANIFEST_LIMIT {
        return Err(Error::TooLarge);
    }
    let length = format!("{}\n", bytes.len());
    let mut frame =
        Vec::with_capacity(PREFIX.len() + length.len() + bytes.len() + sizes.iter().sum::<usize>());
    frame.extend_from_slice(PREFIX);
    frame.extend_from_slice(length.as_bytes());
    frame.extend_from_slice(&bytes);
    let mut hash = Sha256::new();
    for (path, bytes) in files {
        hash_file(&mut hash, path, bytes);
        frame.extend_from_slice(bytes);
    }
    if format!("{:x}", hash.finalize()) != sha256 {
        return Err(Error::Corrupt);
    }
    Ok(frame)
}

pub(crate) fn decode(
    mut input: impl Read,
    expected_name: &ProfileName,
    expected_sha256: &str,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let mut prefix = [0; PREFIX.len()];
    read_exact(&mut input, &mut prefix)?;
    if prefix != PREFIX {
        return Err(Error::Corrupt);
    }
    let len = manifest_length(&mut input)?;
    let mut bytes = vec![0; len];
    read_exact(&mut input, &mut bytes)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| Error::Corrupt)?;
    if manifest.name != *expected_name
        || !digest(&manifest.sha256)
        || manifest.sha256 != expected_sha256
    {
        return Err(Error::Corrupt);
    }
    let sizes = file_sizes(&manifest)?;
    let mut hash = Sha256::new();
    let mut files = Vec::with_capacity(sizes.len());
    for (entry, size) in manifest.files.into_iter().zip(sizes) {
        let mut bytes = vec![0; size];
        read_exact(&mut input, &mut bytes)?;
        hash_file(&mut hash, &entry.path, &bytes);
        files.push((entry.path, bytes));
    }
    let mut trailing = Vec::with_capacity(1);
    if input
        .take(1)
        .read_to_end(&mut trailing)
        .map_err(|_| Error::Io)?
        != 0
        || format!("{:x}", hash.finalize()) != expected_sha256
    {
        return Err(Error::Corrupt);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name() -> ProfileName {
        "codex-45".parse().unwrap()
    }

    #[test]
    fn stage13_skill_transfer_round_trip_truncated_trailing_and_digest() {
        let files: [(&str, &[u8]); 2] =
            [("SKILL.md", b"fixture"), ("support/resource", b"\xff\x00")];
        let mut hash = Sha256::new();
        for (path, bytes) in files {
            hash_file(&mut hash, path, bytes);
        }
        let sha = format!("{:x}", hash.finalize());
        let frame = encode(&name(), &sha, files.into_iter()).unwrap();
        let received = decode(frame.as_slice(), &name(), &sha).unwrap();
        assert_eq!(
            received,
            files.map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
        );
        assert_eq!(
            decode(&frame[..frame.len() - 1], &name(), &sha),
            Err(Error::Corrupt)
        );
        let mut trailing = frame.clone();
        trailing.push(b'x');
        assert_eq!(
            decode(trailing.as_slice(), &name(), &sha),
            Err(Error::Corrupt)
        );
        let mut damaged = frame;
        *damaged.last_mut().unwrap() ^= 1;
        assert_eq!(
            decode(damaged.as_slice(), &name(), &sha),
            Err(Error::Corrupt)
        );
    }

    #[test]
    fn stage13_skill_transfer_validates_complete_manifest_before_bytes() {
        let sha = "b".repeat(64);
        let test = |files: &[(&str, &str)], error| {
            let manifest = Manifest {
                name: name(),
                sha256: sha.clone(),
                files: files
                    .iter()
                    .map(|(path, size)| Entry {
                        path: (*path).into(),
                        size: (*size).into(),
                    })
                    .collect(),
            };
            let bytes = serde_json::to_vec(&manifest).unwrap();
            let frame = [PREFIX, format!("{}\n", bytes.len()).as_bytes(), &bytes].concat();
            assert_eq!(
                decode(frame.as_slice(), &name(), &sha),
                Err(error),
                "{files:?}"
            );
        };
        for path in [
            "a/.git/ignore",
            "a/auth.json",
            "a/secret.PEM",
            "a/id_fixture",
            "a/../test",
            "a/é",
            "a/NUL.txt",
        ] {
            test(&[("SKILL.md", "1"), (path, "9")], Error::Unsafe);
        }
        test(
            &[("SKILL.md", "1"), ("Test/a", "1"), ("test/b", "1")],
            Error::Unsafe,
        );
        test(
            &[("SKILL.md", "1"), ("test", "1"), ("test/child", "1")],
            Error::Unsafe,
        );
        test(&[("SKILL.md", "1"), ("SKILL.md", "1")], Error::Unsafe);
        test(&[("a/readme", "1")], Error::Unsafe);
        test(&[("SKILL.md", "16777217")], Error::TooLarge);
        test(&[("SKILL.md", "01")], Error::Corrupt);
        for fields in [
            format!(r#"{{"name":"codex-45","name":"codex-45","sha256":"{sha}","files":[]}}"#),
            format!(
                r#"{{"name":"codex-45","sha256":"{sha}","files":[{{"path":"SKILL.md","size":"0","type":"symlink"}}]}}"#
            ),
        ] {
            let frame = [
                PREFIX,
                format!("{}\n", fields.len()).as_bytes(),
                fields.as_bytes(),
            ]
            .concat();
            assert_eq!(decode(frame.as_slice(), &name(), &sha), Err(Error::Corrupt));
        }
    }
}
