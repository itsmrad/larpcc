#![allow(dead_code, unused_imports)]
#![cfg_attr(
    windows,
    allow(
        clippy::duplicate_mod,
        reason = "Existing platform unit tests and integration fixtures each own the Windows private ACL fixture"
    )
)]
mod common;
#[path = "../src/desktop/mod.rs"]
mod desktop;
#[path = "../src/launch/mod.rs"]
mod launch;
#[path = "../src/platform/mod.rs"]
mod platform;
#[path = "../src/provider/mod.rs"]
mod provider;
#[path = "../src/registry/mod.rs"]
mod registry;
#[path = "../src/share/mod.rs"]
mod share;
use desktop::{synthetic::*, *};
use std::{ffi::OsString, path::PathBuf};

struct Fixture {
    // Retained Windows handles must close before TempDir removes the fixture.
    target: ValidatedDesktopTarget,
    base: common::Fixture,
}
impl Fixture {
    fn new(provider: registry::Provider) -> Self {
        Self::with_root(provider, "LARPCC_HOME".into()).unwrap()
    }
    fn with_root(
        provider: registry::Provider,
        root_name: OsString,
    ) -> Result<Self, registry::Error> {
        let base = common::Fixture::new();
        let root = base.temp.path().join(root_name);
        common::create_directory(&root);
        common::create_directory(&root.join("profiles"));
        let container = root.join("profiles/1");
        common::create_directory(&container);
        let cli = container.join("cli");
        let desktop = container.join("desktop");
        common::create_directory(&cli);
        common::create_directory(&desktop);
        let app = base.temp.path().join("Invented App.app");
        for path in [&app, &app.join("Contents"), &app.join("Contents/MacOS")] {
            common::create_directory(path);
        }
        common::create_file(
            &app.join("Contents/MacOS/ChatGPT"),
            b"invented executable metadata fixture",
        );
        common::create_file(
            &app.join("Contents/Info.plist"),
            b"invented bundle metadata",
        );
        let snapshot = CheckedSnapshot {
            profile_id: serde_json::from_str("1").unwrap(),
            provider,
            revision: 7,
            recorded_version: Some("invented-version".into()),
            root: fixture_directory(&root).unwrap(),
            container: fixture_directory(&container).unwrap(),
            cli: fixture_directory(&cli).unwrap(),
            desktop: fixture_directory(&desktop).unwrap(),
        };
        let target = ValidatedDesktopTarget::from_checked(snapshot, fixture_app(app).unwrap())?;
        Ok(Self { base, target })
    }
    fn process(&self) -> Process {
        // Invented identities only; never query or address an operating-system PID.
        Process {
            pid: 424242,
            birth: 17,
            executable: fixture_app(self.base.temp.path().join("Invented App.app"))
                .unwrap()
                .executable_identity(),
            argv: vec![
                "invented-app".into(),
                format!(
                    "--user-data-dir={}",
                    self.target.snapshot().desktop.path.display()
                )
                .into(),
            ],
        }
    }
}
#[derive(Default)]
struct Fake {
    processes: Vec<Process>,
    incomplete: bool,
    failure: Option<InspectionCode>,
    replacement: Option<Process>,
    launches: usize,
    focuses: usize,
}
impl Runtime for Fake {
    fn inspect(&mut self) -> Result<Inspection, InspectionCode> {
        if let Some(e) = self.failure {
            return Err(e);
        }
        Ok(Inspection {
            processes: self.processes.clone(),
            complete: !self.incomplete,
        })
    }
    fn recheck(&mut self, p: &Process) -> Result<Process, InspectionCode> {
        Ok(self.replacement.clone().unwrap_or_else(|| p.clone()))
    }
    fn focus(&mut self, _: &Process) -> Result<(), InspectionCode> {
        self.focuses += 1;
        Ok(())
    }
    fn launch(&mut self, c: &Command) -> Result<(), InspectionCode> {
        assert_eq!(c.executable, "/usr/bin/open");
        self.launches += 1;
        Ok(())
    }
}
fn dispatch_ok(
    t: &ValidatedDesktopTarget,
    runtime: &mut Fake,
) -> Result<DesktopReceipt, registry::Error> {
    dispatch(
        t,
        &[],
        |id, _, rev, version| {
            assert_eq!(id.get(), 1);
            assert_eq!(rev, 7);
            assert_eq!(version, Some("invented-version"));
            Ok(())
        },
        runtime,
    )
}
#[test]
fn desktop_unsupported_admission_and_binding_before_effects() {
    assert_eq!(preflight().is_ok(), cfg!(target_os = "macos"));
    for p in registry::Provider::ALL {
        for path in [
            "relative.app",
            "/tmp/Claude.app",
            "/Applications/../Applications/ChatGPT.app",
        ] {
            assert!(desktop::validate_binding(p, std::path::Path::new(path)).is_err());
        }
    }
}
#[test]
fn desktop_synthetic_launch_focus_receipt_and_environment_names() {
    for provider in registry::Provider::ALL {
        let f = Fixture::new(provider);
        let mut rt = Fake::default();
        let r = dispatch_ok(&f.target, &mut rt).unwrap();
        assert_eq!(
            (r.surface, r.outcome, r.native_auth),
            ("desktop", "launched", "not_observed")
        );
        let env: Vec<OsString> = [
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_EXTRA",
            "ANTHROPIC_API_KEY",
            "CODEX_HOME",
            "CODEX_EXTRA",
            "OPENAI_API_KEY",
            "HOME",
            "PATH",
            "UNRELATED",
        ]
        .map(Into::into)
        .to_vec();
        let cmd = command(&f.target, &env).unwrap();
        assert_eq!(cmd.remove_env, env[..6]);
        let variable = if provider == registry::Provider::Codex {
            "CODEX_HOME="
        } else {
            "CLAUDE_CONFIG_DIR="
        };
        let mut assignment = OsString::from(variable);
        assignment.push(&f.target.snapshot().cli.path);
        let mut data = OsString::from("--user-data-dir=");
        data.push(&f.target.snapshot().desktop.path);
        assert_eq!(
            cmd.argv,
            vec![
                OsString::from("-n"),
                "-a".into(),
                f.base.temp.path().join("Invented App.app").into_os_string(),
                "--env".into(),
                assignment,
                "--args".into(),
                data
            ]
        );
        if provider == registry::Provider::Codex {
            assert!(
                !cmd.argv
                    .iter()
                    .any(|arg| arg.to_string_lossy().contains("CLAUDE"))
            );
        }
        rt.processes = vec![f.process()];
        assert_eq!(dispatch_ok(&f.target, &mut rt).unwrap().outcome, "focused");
        assert_eq!((rt.launches, rt.focuses), (1, 1));
        let text = format!("{r:?}");
        for forbidden in [
            "invented-version",
            "LARPCC_HOME",
            "ANTHROPIC_API_KEY",
            "argv",
            "token",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
}
#[test]
fn desktop_stale_coherent_snapshot_refuses_before_effects() {
    let f = Fixture::new(registry::Provider::Codex);
    let mut rt = Fake::default();
    assert!(
        dispatch(
            &f.target,
            &[],
            |_, _, _, _| Err(registry::Error::AdmissionChanged),
            &mut rt
        )
        .is_err()
    );
    assert_eq!((rt.launches, rt.focuses), (0, 0));
}
#[cfg(unix)]
#[test]
fn desktop_retained_directory_and_app_replacement_refuse() {
    for path in [
        "LARPCC_HOME/profiles/1/desktop",
        "LARPCC_HOME/profiles/1/cli",
        "Invented App.app/Contents/MacOS/ChatGPT",
    ] {
        let f = Fixture::new(registry::Provider::Codex);
        let old = f.base.temp.path().join(path);
        std::fs::rename(&old, old.with_extension("retained")).unwrap();
        if path.ends_with("ChatGPT") {
            common::create_file(&old, b"replacement");
        } else {
            common::create_directory(&old);
        }
        let mut rt = Fake::default();
        assert!(dispatch_ok(&f.target, &mut rt).is_err());
        assert_eq!((rt.launches, rt.focuses), (0, 0));
    }
}
#[test]
fn desktop_process_reuse_wrong_pid_and_identity_refuse() {
    let f = Fixture::new(registry::Provider::Codex);
    for change in 0..4 {
        let original = f.process();
        let mut replacement = original.clone();
        match change {
            0 => replacement.pid += 1,
            1 => replacement.birth += 1,
            2 => {
                replacement.executable = fixture_directory(&f.base.temp.path().join("LARPCC_HOME"))
                    .unwrap()
                    .object
                    .identity
            }
            _ => replacement.argv = vec!["default-app".into()],
        }
        let mut rt = Fake {
            processes: vec![original],
            replacement: Some(replacement),
            ..Fake::default()
        };
        assert!(dispatch_ok(&f.target, &mut rt).is_err());
        assert_eq!((rt.launches, rt.focuses), (0, 0));
    }
}
#[test]
fn desktop_exact_data_dir_arguments_and_unknowns() {
    let f = Fixture::new(registry::Provider::Claude);
    let p = f.process();
    let exact = &f.target.snapshot().desktop.path;
    let exact = exact.to_str().unwrap();
    for (args, expected) in [
        (
            vec![format!("--user-data-dir={exact}")],
            Activity::KnownActive,
        ),
        (
            vec!["--user-data-dir".into(), exact.into()],
            Activity::KnownActive,
        ),
        (
            vec![format!("--user-data-dir={exact}-other")],
            Activity::Inactive,
        ),
        (
            vec![format!("--user-data-dir=\"{exact}\"")],
            Activity::Inactive,
        ),
        (
            vec![format!("--user-data-dir={exact}/../desktop")],
            Activity::Inactive,
        ),
        (
            vec!["--user-data-dir".into()],
            Activity::Unknown(InspectionCode::Malformed),
        ),
        (
            vec![
                format!("--user-data-dir={exact}"),
                format!("--user-data-dir={exact}"),
            ],
            Activity::Unknown(InspectionCode::Malformed),
        ),
    ] {
        let mut process = p.clone();
        process.argv = args.into_iter().map(Into::into).collect();
        let mut rt = Fake {
            processes: vec![process],
            ..Fake::default()
        };
        assert_eq!(activity(&mut rt, &f.target), expected);
    }
    for code in [
        InspectionCode::Denied,
        InspectionCode::Timeout,
        InspectionCode::Malformed,
        InspectionCode::Truncated,
        InspectionCode::Limit,
    ] {
        let mut rt = Fake {
            failure: Some(code),
            ..Fake::default()
        };
        assert_eq!(activity(&mut rt, &f.target), Activity::Unknown(code));
        assert!(dispatch_ok(&f.target, &mut rt).is_err());
        assert_eq!((rt.launches, rt.focuses), (0, 0));
    }
    let mut rt = Fake {
        incomplete: true,
        ..Fake::default()
    };
    assert_eq!(
        activity(&mut rt, &f.target),
        Activity::Unknown(InspectionCode::Truncated)
    );
    rt = Fake {
        processes: vec![p; 129],
        ..Fake::default()
    };
    assert_eq!(
        activity(&mut rt, &f.target),
        Activity::Unknown(InspectionCode::Limit)
    );
}

#[test]
fn desktop_bounds_large_arguments_names_and_cleanup() {
    let f = Fixture::new(registry::Provider::Codex);
    let mut p = f.process();
    p.argv.push("x".repeat(32769).into());
    let mut rt = Fake {
        processes: vec![p],
        ..Fake::default()
    };
    assert_eq!(
        activity(&mut rt, &f.target),
        Activity::Unknown(InspectionCode::Limit)
    );
    assert!(command(&f.target, &vec!["CLAUDE_TEST".into(); 4097]).is_err());
    let path = f.base.temp.path().to_owned();
    drop(f);
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn desktop_singleton_metadata_is_bounded_and_never_followed() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new(registry::Provider::Codex);
    let dir = f.base.temp.path().join("LARPCC_HOME/profiles/1/desktop");
    let lock = dir.join("SingletonLock");
    assert_eq!(
        singleton_pid(&dir, b"invented-host"),
        Err(InspectionCode::Missing)
    );
    symlink("invented-host-424242", &lock).unwrap();
    assert_eq!(singleton_pid(&dir, b"invented-host"), Ok(424242));
    assert_eq!(
        singleton_pid(&dir, b"other-host"),
        Err(InspectionCode::ForeignHost)
    );
    std::fs::remove_file(&lock).unwrap();
    let outside = f.base.temp.path().join("outside-never-read");
    common::create_file(&outside, b"invented authentication sentinel, never consume");
    symlink(&outside, &lock).unwrap();
    assert!(singleton_pid(&dir, b"invented-host").is_err());
    assert!(outside.is_file());
    std::fs::remove_file(&lock).unwrap();
    common::create_file(&lock, b"not a symlink");
    assert_eq!(
        singleton_pid(&dir, b"invented-host"),
        Err(InspectionCode::Malformed)
    );
}
#[cfg(unix)]
#[test]
fn desktop_singleton_rejects_malformed_or_oversized_names_without_pid_output() {
    for target in [
        b"host-0".as_slice(),
        b"host-01",
        b"host--1",
        b"host-2147483648",
        b"host-",
        b"host-1\0auth",
        b"host-+1",
    ] {
        assert!(singleton_parse(target, b"host").is_err());
    }
    assert!(singleton_parse(&vec![b'x'; 512], b"host").is_err());
    assert_eq!(
        singleton_parse(b"host-with-dashes-123", b"host-with-dashes"),
        Ok(123)
    );
}

#[test]
fn desktop_native_entry_never_executes_invented_targets_or_closed_claude() {
    for provider in registry::Provider::ALL {
        let f = Fixture::new(provider);
        let mut checks = 0;
        assert!(
            open_target(
                &f.target,
                || {
                    checks += 1;
                    Ok(())
                },
                |_| panic!("closed target must not emit a notice")
            )
            .is_err()
        );
        assert_eq!(checks, 0);
    }
}

#[test]
fn desktop_claude_binding_is_qualification_pending() {
    assert!(matches!(
        validate_binding(
            registry::Provider::Claude,
            std::path::Path::new("/Applications/Claude.app")
        ),
        Err(registry::Error::QualificationPending)
    ));
}
#[test]
fn desktop_open_env_version_gate_refuses_unknown_and_old_os() {
    for version in [
        b"12.6.9".as_slice(),
        b"",
        b"unknown",
        b"13.bad",
        b"13.",
        b"999999999999999",
        b"13.0\0",
    ] {
        assert!(!desktop::synthetic::supports_environment(version));
    }
    for version in [b"13.0".as_slice(), b"14.7.1", b"26.1"] {
        assert!(desktop::synthetic::supports_environment(version));
    }
}
#[cfg(unix)]
#[test]
fn desktop_vendor_hook_accepts_group_writable_ancestor_but_managed_anchor_refuses() {
    use std::os::unix::fs::PermissionsExt;
    let base = common::Fixture::new();
    let applications = base.temp.path().join("Applications");
    common::create_directory(&applications);
    let app = applications.join("Invented App.app");
    for path in [&app, &app.join("Contents"), &app.join("Contents/MacOS")] {
        common::create_directory(path);
    }
    common::create_file(&app.join("Contents/MacOS/ChatGPT"), b"invented executable");
    common::create_file(&app.join("Contents/Info.plist"), b"invented metadata");
    std::fs::set_permissions(&applications, std::fs::Permissions::from_mode(0o775)).unwrap();
    let checked = fixture_app(app.clone()).unwrap();
    assert!(platform::anchor(&app.join("Contents/MacOS/ChatGPT")).is_err());
    checked.recheck().unwrap();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(app.join("Contents/Info.plist"))
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(42))
        .unwrap();
    assert!(checked.recheck().is_err());
}
#[test]
fn desktop_cheap_recheck_detects_same_inode_executable_size_change() {
    let f = Fixture::new(registry::Provider::Codex);
    let path = f
        .base
        .temp
        .path()
        .join("Invented App.app/Contents/MacOS/ChatGPT");
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(2)
        .unwrap();
    let mut rt = Fake::default();
    assert!(dispatch_ok(&f.target, &mut rt).is_err());
    assert_eq!((rt.launches, rt.focuses), (0, 0));
}

#[test]
fn desktop_ambiguous_root_paths_refuse_before_target_admission() {
    #[cfg(unix)]
    let roots = ["root=other", "root\nother", "root\tother", "root\x7fother"];
    #[cfg(windows)]
    let roots = ["root=other"];
    for root in roots {
        assert!(matches!(
            Fixture::with_root(registry::Provider::Codex, root.into()),
            Err(registry::Error::QualificationPending)
        ));
    }
}
#[cfg(unix)]
#[test]
fn desktop_non_utf8_root_refuses_before_target_admission() {
    use std::os::unix::ffi::OsStringExt;
    assert!(matches!(
        Fixture::with_root(
            registry::Provider::Codex,
            OsString::from_vec(b"root-\xff".to_vec())
        ),
        Err(registry::Error::QualificationPending)
    ));
}
#[cfg(unix)]
#[test]
fn desktop_same_inode_rewrite_with_restored_mtime_refuses() {
    use std::{io::Write, os::unix::fs::MetadataExt};
    for relative in ["Contents/MacOS/ChatGPT", "Contents/Info.plist"] {
        let f = Fixture::new(registry::Provider::Codex);
        let path = f.base.temp.path().join("Invented App.app").join(relative);
        let before = std::fs::metadata(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.write_all(&vec![b'X'; before.len() as usize]).unwrap();
        file.set_modified(before.modified().unwrap()).unwrap();
        let after = file.metadata().unwrap();
        assert_eq!(
            (
                after.dev(),
                after.ino(),
                after.len(),
                after.modified().unwrap()
            ),
            (
                before.dev(),
                before.ino(),
                before.len(),
                before.modified().unwrap()
            )
        );
        assert_ne!(
            (after.ctime(), after.ctime_nsec()),
            (before.ctime(), before.ctime_nsec())
        );
        let mut rt = Fake::default();
        assert!(dispatch_ok(&f.target, &mut rt).is_err());
        assert_eq!((rt.launches, rt.focuses), (0, 0));
    }
}

#[test]
fn desktop_forwarded_paths_refuse_open_delimiters_and_controls() {
    for path in [
        "/root/cli=other",
        "/root/\0cli",
        "/root/\x01cli",
        "/root/\x1fcli",
        "/root/\x7fcli",
    ] {
        assert!(matches!(
            desktop::synthetic::validate_launch_path(std::path::Path::new(path)),
            Err(registry::Error::QualificationPending)
        ));
    }
    assert!(
        desktop::synthetic::validate_launch_path(std::path::Path::new("/root/with space/é"))
            .is_ok()
    );
}
