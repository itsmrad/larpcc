use super::*;
use super::{
    paths,
    store::{self, Data, LIMIT, Mutation},
};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::{
    cell::RefCell,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
type Phase = (String, PathBuf);
thread_local! { static PHASES: RefCell<(Vec<Phase>, Option<Phase>)> = const { RefCell::new((Vec::new(),None)) }; }
pub(super) fn phase(action: &str, path: &Path) -> Result<(), Error> {
    PHASES.with(|state| {
        let mut state = state.borrow_mut();
        state.0.push((action.into(), path.into()));
        if state
            .1
            .as_ref()
            .is_some_and(|(a, p)| a == action && p == path)
        {
            Err(paths::io(
                "write",
                path,
                io::Error::other("synthetic failure"),
            ))
        } else {
            Ok(())
        }
    })
}
fn name(s: &str) -> ProfileName {
    ProfileName::parse(s).unwrap()
}
#[cfg(windows)]
use crate::platform::fixture_private as windows_private;
#[cfg(windows)]
type FixtureTemp = windows_private::PrivateTempDir;
#[cfg(unix)]
type FixtureTemp = tempfile::TempDir;
fn fixture() -> (FixtureTemp, Registry) {
    let parent = std::env::var_os("LARPCC_TEST_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    #[cfg(unix)]
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
        .unwrap();
    #[cfg(windows)]
    let temp = windows_private::tempdir(&parent).unwrap();
    let registry = Registry::at(temp.path().join("root")).unwrap();
    (temp, registry)
}
fn fixture_file(path: &Path, bytes: &[u8]) {
    #[cfg(unix)]
    {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(windows)]
    windows_private::create_file(path, bytes).unwrap();
}
fn add(r: &Registry, s: &str) -> ProfileEntry {
    r.add(name(s), Provider::Claude).unwrap()
}
fn initialize(r: &Registry) {
    add(r, "initial");
}
fn bytes(r: &Registry) -> Vec<u8> {
    fs::read(r.root().join("registry.json")).unwrap()
}
fn write(r: &Registry, s: impl AsRef<[u8]>) {
    fs::write(r.root().join("registry.json"), s).unwrap();
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
    fn visit(root: &Path, p: &Path, out: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
        let m = fs::symlink_metadata(p).unwrap();
        #[cfg(unix)]
        let mode = m.mode();
        #[cfg(not(unix))]
        let mode = 0;
        let b = if m.file_type().is_symlink() {
            fs::read_link(p)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else if m.is_file() {
            fs::read(p).unwrap()
        } else {
            Vec::new()
        };
        out.push((p.strip_prefix(root).unwrap().into(), b, mode));
        if m.is_dir() {
            let mut children = fs::read_dir(p)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for c in children {
                visit(root, &c, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out
}
#[test]
fn r01_names() {
    for s in [
        "a",
        "a1",
        "work-1",
        "w_x",
        "abcdefghijklmnopqrstuvwxyz123456",
        "com0",
        "console",
    ] {
        assert!(ProfileName::parse(s).is_ok(), "{s:?}");
    }
    for s in [
        "",
        "abcdefghijklmnopqrstuvwxyz1234567",
        "A",
        "Work",
        "1a",
        "-a",
        "_a",
        "a b",
        "a/b",
        "a\\b",
        ".",
        "..",
        "../x",
        "é",
        "ａ",
        "a\n",
        "a\u{1b}",
    ] {
        assert!(
            matches!(ProfileName::parse(s), Err(NameError::Invalid { .. })),
            "{s:?}"
        );
    }
    for s in ["con", "nul", "aux", "prn", "com1", "com9", "lpt1", "lpt9"] {
        assert!(matches!(
            ProfileName::parse(s),
            Err(NameError::Reserved { .. })
        ));
    }
}
#[test]
fn r02_provider() {
    for p in Provider::ALL {
        assert_eq!(p.to_string().parse(), Ok(p));
    }
    for s in ["Claude", "CLAUDE", ""] {
        assert!(s.parse::<Provider>().is_err());
    }
}
#[test]
fn r03_exact_serialization() {
    let expected=b"{\n  \"version\": 1,\n  \"revision\": 2,\n  \"next_id\": 3,\n  \"profiles\": [\n    {\n      \"id\": 1,\n      \"name\": \"work\",\n      \"provider\": \"claude\"\n    },\n    {\n      \"id\": 2,\n      \"name\": \"personal\",\n      \"provider\": \"codex\"\n    }\n  ]\n}\n";
    assert_eq!(
        store::serialize(&store::parse(Path::new("registry.json"), expected).unwrap()).unwrap(),
        expected
    );
}
#[test]
fn r04_invalid_unchanged() {
    let (_t, r) = fixture();
    initialize(&r);
    let valid = String::from_utf8(bytes(&r)).unwrap();
    let mut invalid = vec![
        "not json".into(),
        "".into(),
        "{".into(),
        format!("\u{feff}{valid}"),
        format!("{valid} trailing"),
        "[]".into(),
        valid.replace("\"version\": 2,", ""),
        valid.replace("\"version\": 2", "\"version\": 0"),
        valid.replace("\"version\": 2", "\"version\": \"1\""),
        valid.replace("\"version\": 2", "\"version\": 1.0"),
        valid.replace("\"version\": 2", "\"version\": 1, \"extra\": 4"),
        valid.replace("\"id\": 1", "\"id\": 1, \"extra\": 4"),
        valid.replace("\"id\": 1", "\"id\": 1, \"id\": 1"),
        valid.replace("\"version\": 2", "\"version\": 1, \"version\": 1"),
        valid.replace("\"id\": 1", "\"id\": 0"),
        valid.replace("\"id\": 1", "\"id\": 2"),
        valid.replace("\"id\": 1", "\"id\": \"../x\""),
        valid.replace("initial", "Bad"),
        valid.replace("claude", "unknown"),
        " ".repeat(LIMIT + 1),
    ];
    for field in ["revision", "next_id", "profiles"] {
        let mut value: serde_json::Value = serde_json::from_str(&valid).unwrap();
        value.as_object_mut().unwrap().remove(field);
        invalid.push(value.to_string());
    }
    for field in ["id", "name", "provider"] {
        let mut value: serde_json::Value = serde_json::from_str(&valid).unwrap();
        value["profiles"][0].as_object_mut().unwrap().remove(field);
        invalid.push(value.to_string());
    }
    for profiles in [
        r#"[{"id":1,"name":"a","provider":"claude"},{"id":1,"name":"b","provider":"claude"}]"#,
        r#"[{"id":2,"name":"a","provider":"claude"},{"id":1,"name":"b","provider":"claude"}]"#,
        r#"[{"id":1,"name":"a","provider":"claude"},{"id":2,"name":"a","provider":"claude"}]"#,
    ] {
        invalid.push(format!(
            r#"{{"version":1,"revision":0,"next_id":3,"profiles":{profiles}}}"#
        ));
    }
    for input in invalid {
        write(&r, &input);
        assert!(
            matches!(r.list(), Err(Error::Corrupt { .. })),
            "{input:.100}"
        );
        assert!(matches!(
            r.add(name("new"), Provider::Claude),
            Err(Error::Corrupt { .. })
        ));
        assert_eq!(bytes(&r), input.as_bytes());
    }
}
#[test]
fn r05_newer() {
    let (_t, r) = fixture();
    initialize(&r);
    write(&r, r#"{"version":3,"future":true}"#);
    let before = bytes(&r);
    assert!(matches!(
        r.add(name("x"), Provider::Codex),
        Err(Error::NewerSchema { found: 3, .. })
    ));
    assert_eq!(before, bytes(&r));
}
#[test]
fn r06_missing_read() {
    let (_t, r) = fixture();
    assert!(r.list().unwrap().is_empty());
    assert!(matches!(r.get(&name("a")), Err(Error::NotFound { .. })));
    assert!(!r.root().exists());
}
#[test]
fn r07_first_add() {
    let (_t, r) = fixture();
    let e = add(&r, "work");
    assert_eq!(e.profile.id.get(), 1);
    assert_eq!(e.status, DirStatus::Ok);
    let data = store::load(r.root()).unwrap();
    assert_eq!((data.revision, data.next_id), (1, 2));
    #[cfg(unix)]
    {
        for p in [r.root().to_owned(), r.root().join("profiles"), e.state_dir] {
            assert_eq!(fs::metadata(p).unwrap().mode() & 0o777, 0o700);
        }
        for p in ["registry.json", "registry.lock"] {
            assert_eq!(
                fs::metadata(r.root().join(p)).unwrap().mode() & 0o777,
                0o600
            );
        }
    }
}
#[test]
fn r08_lifecycle() {
    let (_t, r) = fixture();
    let a = add(&r, "work");
    assert!(matches!(
        r.rename(&name("work"), name("work")),
        Err(Error::NameTaken { .. })
    ));
    let b = r.rename(&name("work"), name("job")).unwrap();
    assert_eq!(a.state_dir, b.state_dir);
    assert_eq!(a.profile.id, b.profile.id);
    let f = r.forget(&name("job")).unwrap();
    assert_eq!(f, b);
    assert!(b.state_dir.exists());
    let c = add(&r, "job");
    assert_eq!(c.profile.id.get(), 2);
    let data = store::load(r.root()).unwrap();
    assert_eq!((data.revision, data.next_id), (4, 3));
}
#[test]
fn r09_orphan() {
    let (_t, r) = fixture();
    paths::mkdir(r.root()).unwrap();
    paths::mkdir(&r.root().join("profiles")).unwrap();
    paths::mkdir(&r.root().join("profiles/1")).unwrap();
    fixture_file(&r.root().join("profiles/1/keep"), b"unchanged");
    let before = snapshot(&r.root().join("profiles/1"));
    assert_eq!(add(&r, "a").profile.id.get(), 2);
    assert_eq!(before, snapshot(&r.root().join("profiles/1")));
}
#[test]
fn r10_stale_temp() {
    let (t, r) = fixture();
    initialize(&r);
    let temp = r.root().join("registry.json.tmp");
    fixture_file(&temp, b"garbage");
    add(&r, "b");
    assert!(!temp.exists());
    #[cfg(unix)]
    {
        let outside = t.path().join("outside");
        fs::write(&outside, b"unchanged").unwrap();
        symlink(&outside, &temp).unwrap();
        assert!(r.add(name("c"), Provider::Claude).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"unchanged");
        assert!(temp.is_symlink());
    }
    #[cfg(not(unix))]
    let _ = t;
}
#[cfg(unix)]
#[test]
fn r11_unsafe_paths() {
    for (target, kind, read_fails) in [
        ("", "symlink", true),
        ("", "file", true),
        ("", "mode", true),
        ("registry.json", "mode", true),
        ("registry.json", "symlink", true),
        ("registry.lock", "symlink", true),
        ("profiles", "symlink", true),
        ("profiles", "mode", true),
    ] {
        let (t, r) = fixture();
        initialize(&r);
        let p = if target.is_empty() {
            r.root().to_owned()
        } else {
            r.root().join(target)
        };
        if kind == "mode" {
            fs::set_permissions(
                &p,
                fs::Permissions::from_mode(if target == "registry.json" {
                    0o644
                } else {
                    0o755
                }),
            )
            .unwrap();
        } else {
            let moved = t.path().join("moved");
            fs::rename(&p, &moved).unwrap();
            if kind == "file" {
                fs::write(&p, b"file").unwrap();
            } else {
                symlink(&moved, &p).unwrap();
            }
        }
        let before = snapshot(t.path());
        assert!(
            matches!(
                r.add(name("b"), Provider::Claude),
                Err(Error::Unsafe { .. } | Error::ManagedMode { .. })
            ),
            "{target}/{kind}"
        );
        if read_fails {
            assert!(matches!(
                r.list(),
                Err(Error::Unsafe { .. } | Error::ManagedMode { .. })
            ));
        }
        assert_eq!(before, snapshot(t.path()));
    }
}
#[test]
fn r12_status() {
    for kind in [
        "missing",
        "file",
        #[cfg(unix)]
        "mode",
        #[cfg(unix)]
        "symlink",
    ] {
        let (t, r) = fixture();
        let e = add(&r, "a");
        fs::remove_dir(&e.state_dir).unwrap();
        match kind {
            "file" => fixture_file(&e.state_dir, b"file"),
            #[cfg(unix)]
            "mode" => {
                paths::mkdir(&e.state_dir).unwrap();
                fs::set_permissions(&e.state_dir, fs::Permissions::from_mode(0o755)).unwrap();
            }
            #[cfg(unix)]
            "symlink" => symlink(t.path(), &e.state_dir).unwrap(),
            _ => (),
        };
        let s = r.get(&name("a")).unwrap().status;
        match kind {
            "missing" => assert_eq!(s, DirStatus::Missing),
            "file" => assert_eq!(s, DirStatus::Unsafe("is not a directory".into())),
            #[cfg(unix)]
            "mode" => assert_eq!(
                s,
                DirStatus::Unsafe("has mode 0755; group and other access must be removed".into())
            ),
            #[cfg(unix)]
            "symlink" => assert_eq!(
                s,
                DirStatus::Unsafe("is a symbolic link or reparse point".into())
            ),
            _ => (),
        };
        r.rename(&name("a"), name("b")).unwrap();
        r.forget(&name("b")).unwrap();
        let _ = t;
    }
    #[cfg(unix)]
    {
        let mut identities = std::collections::HashMap::new();
        assert_eq!(paths::duplicate(&mut identities, (3, 4), 1), None);
        assert_eq!(
            paths::duplicate(&mut identities, (3, 4), 2),
            Some("shares a filesystem identity with profile 1".into())
        );
    }
}
#[test]
fn r13_foreign() {
    let (_t, r) = fixture();
    paths::mkdir(r.root()).unwrap();
    fixture_file(&r.root().join("notes.txt"), b"keep");
    let before = snapshot(r.root());
    assert!(r.list().unwrap().is_empty());
    assert!(matches!(
        r.add(name("a"), Provider::Claude),
        Err(Error::Unsafe { .. } | Error::ManagedMode { .. })
    ));
    assert_eq!(before, snapshot(r.root()));
}
#[test]
fn r14_threads() {
    for same in [false, true] {
        let (_t, r) = fixture();
        let count = if same { 8 } else { 16 };
        let handles = (0..count)
            .map(|i| {
                let root = r.root().to_owned();
                std::thread::spawn(move || {
                    Registry::at(root).unwrap().add(
                        name(&if same { "same".into() } else { format!("a{i}") }),
                        Provider::Claude,
                    )
                })
            })
            .collect::<Vec<_>>();
        let mut ok = 0;
        for h in handles {
            match h.join().unwrap() {
                Ok(_) => ok += 1,
                Err(Error::NameTaken { .. }) if same => (),
                e => panic!("{e:?}"),
            }
        }
        assert_eq!(ok, if same { 1 } else { 16 });
        let d = store::load(r.root()).unwrap();
        assert_eq!(d.revision, ok);
        assert_eq!(d.profiles.len(), ok as usize);
        assert_eq!(
            fs::read_dir(r.root().join("profiles")).unwrap().count(),
            ok as usize
        );
    }
}
#[test]
fn r15_busy() {
    let (t, r) = fixture();
    initialize(&r);
    let f = paths::options()
        .open(r.root().join("registry.lock"))
        .unwrap();
    // Windows byte-range locks prevent snapshot() from reading registry.lock while held.
    let before = snapshot(t.path());
    let registry_before = bytes(&r);
    f.lock().unwrap();
    let timeout = Duration::from_millis(200);
    let started = std::time::Instant::now();
    assert!(matches!(
        store::mutate(
            r.root(),
            Mutation::Add(name("b"), Provider::Claude),
            timeout
        ),
        Err(Error::Busy { .. })
    ));
    assert!(started.elapsed() >= timeout);
    assert_eq!(registry_before, bytes(&r));
    drop(f);
    assert_eq!(before, snapshot(t.path()));
}
#[test]
fn r16_roots() {
    let resolve = |pairs: &[(&str, &str)]| {
        paths::resolve(|k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).into())
        })
        .and_then(Registry::at)
    };
    let (_t, r) = fixture();
    let root = r.root().to_str().unwrap();
    assert_eq!(resolve(&[("LARPCC_HOME", root)]).unwrap().root(), r.root());
    for s in ["relative", "/tmp/../bad"] {
        assert!(matches!(
            resolve(&[("LARPCC_HOME", s)]),
            Err(Error::InvalidRoot { .. })
        ));
    }
    assert!(matches!(resolve(&[]), Err(Error::NoDataDir)));
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        assert_eq!(
            resolve(&[
                ("LARPCC_HOME", ""),
                ("XDG_DATA_HOME", "relative"),
                ("HOME", "/home/synthetic")
            ])
            .unwrap()
            .root(),
            Path::new("/home/synthetic/.local/share/larpcc")
        );
        assert_eq!(
            resolve(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/synthetic")])
                .unwrap()
                .root(),
            Path::new("/data/larpcc")
        );
    }
}
#[cfg(target_os = "linux")]
#[test]
fn r17_non_utf8() {
    use std::os::unix::ffi::OsStringExt;
    let (t, _) = fixture();
    let r = Registry::at(
        t.path()
            .join(std::ffi::OsString::from_vec(b"root\xff".to_vec())),
    )
    .unwrap();
    add(&r, "a");
    assert_eq!(r.list().unwrap().len(), 1);
    assert!(!bytes(&r).contains(&0xff));
}
#[test]
fn r18_counters() {
    for (next, revision, orphan) in [
        (u64::MAX, 1, false),
        (2, u64::MAX, false),
        (u64::MAX - 1, 1, true),
    ] {
        let (_t, r) = fixture();
        initialize(&r);
        let mut d = store::load(r.root()).unwrap();
        d.next_id = next;
        d.revision = revision;
        write(&r, store::serialize(&d).unwrap());
        if orphan {
            paths::mkdir(&r.root().join("profiles").join(next.to_string())).unwrap();
        }
        let before = snapshot(r.root());
        assert!(matches!(
            r.add(name("b"), Provider::Claude),
            Err(Error::Exhausted)
        ));
        if revision == u64::MAX {
            assert!(matches!(
                r.rename(&name("initial"), name("b")),
                Err(Error::Exhausted)
            ));
            assert!(matches!(r.forget(&name("initial")), Err(Error::Exhausted)));
        }
        assert_eq!(before, snapshot(r.root()));
    }
}
#[cfg(unix)]
#[test]
fn r19_rollback_permissions() {
    if paths::euid() == 0 {
        return;
    }
    let (_t, r) = fixture();
    initialize(&r);
    let before = bytes(&r);
    fs::set_permissions(r.root(), fs::Permissions::from_mode(0o500)).unwrap();
    assert!(matches!(
        r.add(name("b"), Provider::Claude),
        Err(Error::Io { .. })
    ));
    assert!(!r.root().join("profiles/2").exists());
    assert_eq!(before, bytes(&r));
    fs::set_permissions(r.root(), fs::Permissions::from_mode(0o700)).unwrap();
}
#[test]
fn r20_errors() {
    let p = PathBuf::from("/root/registry.json");
    let n = name("work");
    let table = vec![
        (
            Error::NoDataDir,
            "cannot determine the data directory",
            Some("set LARPCC_HOME to an absolute path"),
        ),
        (
            Error::InvalidRoot {
                path: p.clone(),
                reason: "must be an absolute path".into(),
            },
            "invalid data directory /root/registry.json: must be an absolute path",
            Some("set LARPCC_HOME to an absolute path"),
        ),
        (
            Error::ManagedMode {
                path: p.clone(),
                mode: 0o644,
                directory: false,
            },
            "unsafe data path /root/registry.json: has mode 0644; group and other access must be removed",
            Some("run: chmod 600 /root/registry.json"),
        ),
        (
            Error::Corrupt {
                path: p.clone(),
                reason: "bad".into(),
            },
            "registry /root/registry.json is invalid: bad",
            Some(
                "Larp CC did not modify it; move it aside to start an empty registry (state directories under /root/profiles are not touched)",
            ),
        ),
        (
            Error::NewerSchema {
                path: p.clone(),
                found: 3,
            },
            "registry /root/registry.json uses schema version 3; this Larp CC supports version 2",
            Some("upgrade Larp CC; the file was not modified"),
        ),
        (
            Error::NotFound { name: n.clone() },
            "no profile named work",
            Some("run 'larp cc profile list'"),
        ),
        (
            Error::NameTaken { name: n },
            "a profile named work already exists",
            None,
        ),
        (
            Error::Busy { path: p.clone() },
            "registry is locked by another Larp CC process (/root/registry.json)",
            Some("retry when the other command finishes"),
        ),
        (
            Error::TooLarge,
            "prospective registry exceeds 1 MiB",
            Some("forget unused profiles or use a separate data root"),
        ),
        (Error::Exhausted, "registry counters are exhausted", None),
        (
            Error::Io {
                action: "open",
                path: p,
                source: io::Error::other("bad"),
            },
            "cannot open /root/registry.json: bad",
            None,
        ),
    ];
    for (e, message, hint) in table {
        assert_eq!(e.to_string(), message);
        if matches!(e, Error::Corrupt { .. }) {
            assert_eq!(
                e.hint(),
                Some(format!(
                    "Larp CC did not modify it; move it aside to start an empty registry (state directories under {} are not touched)",
                    Path::new("/root").join("profiles").display()
                ))
            );
        } else {
            assert_eq!(e.hint().as_deref(), hint);
        }
    }
    assert_eq!(
        NameError::Invalid {
            input: "a\n".into()
        }
        .to_string(),
        "invalid profile name \"a\\n\": use 1-32 characters from a-z, 0-9, '-' and '_', starting with a letter"
    );
    assert_eq!(
        NameError::Reserved {
            input: "con".into()
        }
        .to_string(),
        "invalid profile name \"con\": reserved on Windows"
    );
}
#[test]
fn r21_limits() {
    assert!(store::capacity(vec![b' '; LIMIT]).is_ok());
    assert!(matches!(
        store::capacity(vec![b' '; LIMIT + 1]),
        Err(Error::TooLarge)
    ));
    assert_eq!(store::bounded(io::repeat(b' ')).unwrap().len(), LIMIT + 1);
    let (_t, r) = fixture();
    initialize(&r);
    let mut d = Data::default();
    // Binary search keeps this large synthetic fixture cheap to construct.
    let mut low = 1;
    let mut high = 20_000;
    while low + 1 < high {
        let count = (low + high) / 2;
        d.profiles = (1..=count)
            .map(|id| Profile {
                id: ProfileId::new(id),
                name: name(&format!("p{id}")),
                provider: Provider::Claude,
            })
            .collect();
        d.next_id = count + 1;
        if store::serialize(&d).is_ok() {
            low = count;
        } else {
            high = count;
        }
    }
    d.profiles = (1..=low)
        .map(|id| Profile {
            id: ProfileId::new(id),
            name: name(&format!("p{id}")),
            provider: Provider::Claude,
        })
        .collect();
    d.next_id = low + 1;
    // Tune whitespace-free data by extending names until longer rename and add cannot fit.
    let mut output = store::serialize(&d).unwrap();
    for i in 0..d.profiles.len() {
        let old = d.profiles[i].name.clone();
        d.profiles[i].name = name(&format!("p{:031}", i));
        match store::serialize(&d) {
            Ok(b) => output = b,
            Err(_) => {
                d.profiles[i].name = old;
                break;
            }
        }
    }
    let remaining = LIMIT - output.len();
    if remaining != 0 {
        let profile = d
            .profiles
            .iter_mut()
            .find(|p| p.name.as_str().len() + remaining <= 32)
            .unwrap();
        profile.name = name(&format!("{}{}", profile.name, "x".repeat(remaining)));
        output = store::serialize(&d).unwrap();
    }
    assert_eq!(output.len(), LIMIT);
    write(&r, &output);
    assert_eq!(r.list().unwrap().len(), d.profiles.len());
    let before = snapshot(r.root());
    assert!(matches!(
        r.add(name("longnewprofile"), Provider::Codex),
        Err(Error::TooLarge)
    ));
    assert_eq!(before, snapshot(r.root()));
    let short = d
        .profiles
        .iter()
        .find(|p| p.name.as_str().len() < 32)
        .unwrap()
        .name
        .clone();
    assert!(matches!(
        r.rename(&short, name("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")),
        Err(Error::TooLarge)
    ));
    assert_eq!(before, snapshot(r.root()));
    // Compact input is readable, even when its pretty representation exceeds the cap.
    let id = d.next_id;
    d.next_id += 1;
    d.profiles.push(Profile {
        id: ProfileId::new(id),
        name: name("extra"),
        provider: Provider::Claude,
    });
    let compact = serde_json::to_vec(&d).unwrap();
    assert!(compact.len() < LIMIT);
    write(&r, compact);
    let before = snapshot(r.root());
    assert!(matches!(
        r.add(name("more"), Provider::Codex),
        Err(Error::TooLarge)
    ));
    assert_eq!(before, snapshot(r.root()));
    assert!(matches!(
        r.rename(&name("extra"), name("longer")),
        Err(Error::TooLarge)
    ));
    assert_eq!(before, snapshot(r.root()));
}
#[test]
fn r22_no_parent_creation() {
    let (t, _r) = fixture();
    let r = Registry::at(t.path().join("missing-parent/root")).unwrap();
    let before = snapshot(t.path());
    assert!(matches!(
        r.add(name("a"), Provider::Claude),
        Err(Error::Io {
            action: "create directory",
            ..
        })
    ));
    assert_eq!(before, snapshot(t.path()));
}
#[cfg(unix)]
#[test]
fn r23_phases_and_order() {
    let (t, r) = fixture();
    PHASES.with(|s| *s.borrow_mut() = (Vec::new(), None));
    add(&r, "a");
    let recorded = PHASES.with(|s| s.borrow().0.clone());
    assert_eq!(
        recorded,
        vec![
            ("sync".into(), t.path().into()),
            ("sync".into(), r.root().into()),
            ("sync".into(), r.root().join("profiles")),
            ("write".into(), r.root().join("registry.json.tmp")),
            ("file sync".into(), r.root().join("registry.json.tmp")),
            ("replace".into(), r.root().join("registry.json")),
            ("sync".into(), r.root().into())
        ]
    );
    for (action, suffix) in [
        ("sync", "profiles"),
        ("write", "registry.json.tmp"),
        ("file sync", "registry.json.tmp"),
        ("replace", "registry.json"),
    ] {
        let (_t, r) = fixture();
        initialize(&r);
        paths::mkdir(&r.root().join("profiles/2")).unwrap();
        let orphan = snapshot(&r.root().join("profiles/2"));
        let before = bytes(&r);
        PHASES.with(|s| s.borrow_mut().1 = Some((action.into(), r.root().join(suffix))));
        assert!(r.add(name("b"), Provider::Claude).is_err());
        PHASES.with(|s| s.borrow_mut().1 = None);
        assert_eq!(before, bytes(&r));
        assert!(!r.root().join("profiles/3").exists());
        assert_eq!(orphan, snapshot(&r.root().join("profiles/2")));
        assert!(!r.root().join("registry.json.tmp").exists());
    }
    let (_t, r) = fixture();
    paths::mkdir(r.root()).unwrap();
    let lock = paths::options()
        .create_new(true)
        .open(r.root().join("registry.lock"))
        .unwrap();
    drop(lock);
    PHASES.with(|s| s.borrow_mut().1 = Some(("sync".into(), r.root().into())));
    assert!(r.add(name("a"), Provider::Claude).is_err());
    PHASES.with(|s| s.borrow_mut().1 = None);
    assert!(!r.root().join("profiles/1").exists());
    assert!(!r.root().join("registry.json").exists());
}

#[cfg(unix)]
#[test]
fn root_symlink_suffixes_and_mode_hint() {
    let (t, r) = fixture();
    initialize(&r);
    let link = t.path().join("link");
    symlink(r.root(), &link).unwrap();
    for suffix in ["/", "/."] {
        let candidate = Registry::at(PathBuf::from(format!("{}{suffix}", link.display()))).unwrap();
        assert!(matches!(
            candidate.list(),
            Err(Error::Unsafe { .. } | Error::ManagedMode { .. })
        ));
    }
    let root = t.path().join("registry.json");
    paths::mkdir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    let error = Registry::at(root.clone()).unwrap().list().unwrap_err();
    assert_eq!(
        error.hint(),
        Some(format!("run: chmod 700 {}", root.display()))
    );
}
#[cfg(unix)]
#[test]
fn r23_post_replace_sync_and_root_parent_failure() {
    let (_t, r) = fixture();
    initialize(&r);
    PHASES.with(|s| s.borrow_mut().1 = Some(("sync".into(), r.root().into())));
    let error = r.add(name("applied"), Provider::Codex).unwrap_err();
    PHASES.with(|s| s.borrow_mut().1 = None);
    assert_eq!(
        error.hint().as_deref(),
        Some("the change was applied but may not be durable; run 'larp cc profile list'")
    );
    assert!(r.get(&name("applied")).unwrap().state_dir.exists());
    let (t, r) = fixture();
    PHASES.with(|s| s.borrow_mut().1 = Some(("sync".into(), t.path().into())));
    assert!(r.add(name("a"), Provider::Claude).is_err());
    PHASES.with(|s| s.borrow_mut().1 = None);
    assert!(r.root().exists());
    assert_eq!(fs::read_dir(r.root()).unwrap().count(), 0);
}
#[test]
fn extra_error_hints_and_missing_parent_guidance() {
    let (t, r) = fixture();
    let path = t.path().join("missing/root");
    let e = Registry::at(path)
        .unwrap()
        .add(name("a"), Provider::Claude)
        .unwrap_err();
    assert_eq!(
        e.hint().as_deref(),
        Some("choose a data root under an existing parent directory")
    );
    let e = Error::Unsafe {
        path: r.root().into(),
        reason: "is not a directory".into(),
    };
    assert_eq!(
        e.hint().as_deref(),
        Some(if cfg!(windows) {
            "use a private data root under trusted parents; review existing permissions manually; Larp CC will not repair or copy provider state"
        } else {
            "fix or move it, or set LARPCC_HOME to another directory"
        })
    );
}

#[cfg(unix)]
#[test]
fn captured_mode_hints_survive_removal() {
    let (t, _) = fixture();
    for directory in [true, false] {
        let path = t.path().join(if directory { "directory" } else { "file" });
        if directory {
            fs::create_dir(&path).unwrap();
        } else {
            fs::write(&path, b"").unwrap();
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o754)).unwrap();
        let error = paths::inspect(&path, directory).unwrap_err();
        if directory {
            fs::remove_dir(&path).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        assert_eq!(
            error.to_string(),
            format!(
                "unsafe data path {}: has mode 0754; group and other access must be removed",
                path.display()
            )
        );
        assert_eq!(
            error.hint(),
            Some(format!(
                "run: chmod {} {}",
                if directory { "700" } else { "600" },
                path.display()
            ))
        );
    }
}

#[cfg(unix)]
fn native_fixture(t: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = t.path().join(name);
    #[cfg(target_os = "macos")]
    let magic = [0xcf, 0xfa, 0xed, 0xfe];
    #[cfg(not(target_os = "macos"))]
    let magic = *b"\x7fELF";
    fs::write(&path, magic).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
#[cfg(unix)]
#[test]
fn bindings_upgrade_noop_precedence_and_counters() {
    let (t, r) = fixture();
    initialize(&r);
    let v1 = br#"{"version":1,"revision":0,"next_id":1,"profiles":[]}"#;
    write(&r, v1);
    assert!(r.list().unwrap().is_empty());
    assert_eq!(bytes(&r), v1);
    let first = native_fixture(&t, "native-a");
    let second = native_fixture(&t, "native-b");
    assert_eq!(
        r.bind(Provider::Codex, first.clone(), false).unwrap(),
        BindOutcome::Bound
    );
    let data = store::load(r.root()).unwrap();
    assert_eq!(data.version, 2);
    assert_eq!(data.revision, 1);
    assert_eq!(data.next_id, 1);
    let mut max: serde_json::Value = serde_json::from_slice(&bytes(&r)).unwrap();
    max["revision"] = serde_json::json!(u64::MAX);
    write(&r, serde_json::to_vec(&max).unwrap());
    fs::remove_file(&first).unwrap();
    let unchanged = bytes(&r);
    assert_eq!(
        r.bind(Provider::Codex, first.clone(), false).unwrap(),
        BindOutcome::Unchanged
    );
    assert_eq!(bytes(&r), unchanged);
    assert!(matches!(
        r.bind(Provider::Codex, second.clone(), false),
        Err(Error::AlreadyBound { .. })
    ));
    assert!(matches!(
        r.bind(Provider::Codex, second, true),
        Err(Error::Exhausted)
    ));
    assert_eq!(bytes(&r), unchanged);
    assert!(matches!(
        &r.bindings().unwrap()[1].1,
        BindingState::Invalid(..)
    ));
}
#[cfg(unix)]
#[test]
fn bindings_strict_v2_and_sorted_serialization() {
    let (t, r) = fixture();
    let executable = native_fixture(&t, "native");
    r.bind(Provider::Codex, executable.clone(), false).unwrap();
    r.bind(Provider::Claude, executable, false).unwrap();
    let data = store::load(r.root()).unwrap();
    assert_eq!(
        data.bindings.iter().map(|b| b.provider).collect::<Vec<_>>(),
        Provider::ALL
    );
    let good = String::from_utf8(bytes(&r)).unwrap();
    let mut invalid = vec![
        good.replace("\"bindings\"", "\"unknown\""),
        good.replace("\"bindings\": [", "\"bindings\": [], \"bindings\": ["),
    ];
    let mut value: serde_json::Value = serde_json::from_str(&good).unwrap();
    value["bindings"].as_array_mut().unwrap().reverse();
    invalid.push(value.to_string());
    for path in [
        "relative",
        "/bad/../x",
        "/bad//x",
        "/bad/./x",
        "/bad/",
        "/bad\0x",
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&good).unwrap();
        value["bindings"][0]["executable"] = path.into();
        invalid.push(value.to_string());
    }
    for input in invalid {
        write(&r, &input);
        assert!(matches!(r.bindings(), Err(Error::Corrupt { .. })));
        assert_eq!(bytes(&r), input.as_bytes());
    }
}
#[cfg(unix)]
#[test]
fn all_fixed_metadata_refuses_unsafe_temp_and_lock_on_reads() {
    let (t, r) = fixture();
    initialize(&r);
    for name in ["registry.lock", "registry.json.tmp"] {
        let path = r.root().join(name);
        let outside = t.path().join(format!("outside-{name}"));
        fs::write(&outside, b"invented sentinel").unwrap();
        let original = fs::read(&path).ok();
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        symlink(&outside, &path).unwrap();
        let before = bytes(&r);
        assert!(r.list().is_err());
        assert!(r.rename(&name_parse("initial"), name_parse("new")).is_err());
        assert_eq!(bytes(&r), before);
        assert_eq!(fs::read(&outside).unwrap(), b"invented sentinel");
        assert!(path.is_symlink());
        fs::remove_file(&path).unwrap();
        if let Some(bytes) = original {
            fs::write(&path, bytes).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    fn name_parse(s: &str) -> ProfileName {
        s.parse().unwrap()
    }
}
#[cfg(unix)]
#[test]
fn binding_executable_native_type_links_containment_and_self_checks() {
    let (t, r) = fixture();
    let native = native_fixture(&t, "native");
    r.bind(Provider::Claude, native.clone(), false).unwrap();
    let link = t.path().join("link");
    symlink(&native, &link).unwrap();
    assert!(r.bind(Provider::Claude, link.clone(), true).is_ok());
    let unsafe_parent = t.path().join("unsafe-parent");
    fs::create_dir(&unsafe_parent).unwrap();
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777)).unwrap();
    let unsafe_target = unsafe_parent.join("native");
    fs::copy(&native, &unsafe_target).unwrap();
    assert!(r.bind(Provider::Claude, unsafe_target, true).is_err());
    assert!(
        r.bind(Provider::Claude, std::env::current_exe().unwrap(), true)
            .is_err()
    );
    let inside = r.root().join("native");
    fs::copy(&native, &inside).unwrap();
    assert!(r.bind(Provider::Claude, inside, true).is_err());
    fs::write(&native, b"#!/bin/sh\n").unwrap();
    assert!(
        r.bindings()
            .unwrap()
            .iter()
            .any(|(_, s)| matches!(s, BindingState::Invalid(..)))
    );
}

#[test]
fn exact_v2_first_mutation_bytes() {
    let (_temporary, r) = fixture();
    add(&r, "work");
    let expected =
        include_str!("../../tests/fixtures/registry/valid/v2.json").replace("\r\n", "\n");
    assert_eq!(bytes(&r), expected.as_bytes());
    assert_eq!(
        store::serialize(&store::parse(Path::new("registry.json"), expected.as_bytes()).unwrap())
            .unwrap(),
        expected.as_bytes()
    );
}

#[cfg(unix)]
#[test]
fn provided_handle_is_verified_without_reopening_diagnostic_path() {
    let (t, _) = fixture();
    let path = t.path().join("original");
    fs::write(&path, b"synthetic metadata").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let file = fs::File::open(&path).unwrap();
    let original = file.metadata().unwrap();
    fs::rename(&path, t.path().join("moved")).unwrap();
    fs::write(&path, b"different unsafe file").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let checked =
        crate::platform::verify_opened(file, &path, crate::platform::ObjectKind::File).unwrap();
    assert_eq!(
        checked.identity,
        crate::platform::FileIdentity::Unix {
            dev: original.dev(),
            ino: original.ino()
        }
    );
    let unsafe_file = fs::File::open(&path).unwrap();
    assert!(matches!(
        crate::platform::verify_opened(
            unsafe_file,
            &t.path().join("unused-diagnostic"),
            crate::platform::ObjectKind::File
        ),
        Err(Error::ManagedMode {
            mode: 0o644,
            directory: false,
            ..
        })
    ));
}

#[cfg(windows)]
#[test]
fn fixed_guards_pin_real_root_and_profiles_without_querying_sentinel_leaf() {
    let (temporary, registry) = fixture();
    initialize(&registry);
    let home = registry.root().join("profiles/1");
    fs::remove_dir(&home).unwrap();
    fixture_file(&home, b"synthetic non-directory sentinel leaf");
    let root = registry.root();
    let profiles = root.join("profiles");
    let moved_root = temporary.path().join("moved-root");
    let moved_profiles = root.join("moved-profiles");
    let held = store::fixed(root).unwrap();
    assert!(matches!(
        fs::rename(root, &moved_root).unwrap_err().raw_os_error(),
        Some(5 | 32)
    ));
    assert!(matches!(
        fs::rename(&profiles, &moved_profiles)
            .unwrap_err()
            .raw_os_error(),
        Some(5 | 32)
    ));
    // The same operations must succeed after dropping only these guards, excluding static ACL denial.
    drop(held);
    fs::rename(&profiles, &moved_profiles).unwrap();
    fs::rename(&moved_profiles, &profiles).unwrap();
    fs::rename(root, &moved_root).unwrap();
    fs::rename(&moved_root, root).unwrap();
}

#[test]
fn atomic_replacement_reads_one_complete_validated_snapshot() {
    let (_temp, registry) = fixture();
    initialize(&registry);
    let replacement = registry.root().join("replacement");
    let mut data = store::load(registry.root()).unwrap();
    data.revision += 1;
    let expected = data.revision;
    fixture_file(&replacement, &store::serialize(&data).unwrap());
    let mut calls = 0;
    let result = store::load_with(registry.root(), |path| {
        calls += 1;
        if calls == 1 {
            fs::rename(&replacement, path).unwrap();
        }
    })
    .unwrap();
    assert_eq!(result.revision, expected);
    assert_eq!(calls, 2);
}
#[test]
fn atomic_replacement_churn_is_bounded_without_consuming_invalid_bytes() {
    let (_temp, registry) = fixture();
    initialize(&registry);
    let replacement = registry.root().join("replacement");
    let mut calls = 0;
    let error = store::load_with(registry.root(), |path| {
        calls += 1;
        fixture_file(&replacement, b"invalid encoding must never be read");
        fs::rename(&replacement, path).unwrap();
    })
    .err()
    .unwrap();
    assert_eq!(calls, 32);
    assert!(
        matches!(error, Error::Unsafe {reason,..} if reason == "filesystem identity cannot be verified")
    );
}
#[cfg(unix)]
#[test]
fn atomic_unsafe_replacement_refuses_without_retry() {
    for symlink_replacement in [false, true] {
        let (_temp, registry) = fixture();
        initialize(&registry);
        let mut calls = 0;
        let replacement = registry.root().join("replacement");
        let error = store::load_with(registry.root(), |path| {
            calls += 1;
            if symlink_replacement {
                symlink("missing", &replacement).unwrap();
            } else {
                fixture_file(&replacement, b"invalid encoding");
                fs::set_permissions(&replacement, fs::Permissions::from_mode(0o644)).unwrap();
            }
            fs::rename(&replacement, path).unwrap();
        })
        .err()
        .unwrap();
        assert_eq!(calls, 1);
        if symlink_replacement {
            assert!(matches!(error, Error::Io { .. }));
        } else {
            assert!(matches!(error, Error::ManagedMode { mode: 0o644, .. }));
        }
    }
}
