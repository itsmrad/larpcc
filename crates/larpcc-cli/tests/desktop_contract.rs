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
use desktop::{synthetic::*, *};
use std::{ffi::OsString, path::PathBuf};

struct Fixture {
    // Retained Windows handles must close before TempDir removes the fixture.
    target: ValidatedDesktopTarget,
    base: common::Fixture,
}
impl Fixture {
    fn new(provider: registry::Provider) -> Self {
        let base = common::Fixture::new();
        let root = base.temp.path().join("LARPCC_HOME");
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
        let target =
            ValidatedDesktopTarget::from_checked(snapshot, fixture_app(app).unwrap()).unwrap();
        Self { base, target }
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
                    self.base
                        .temp
                        .path()
                        .join("LARPCC_HOME/profiles/1/desktop")
                        .display()
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
            "HOME",
            "PATH",
            "UNRELATED",
        ]
        .map(Into::into)
        .to_vec();
        let cmd = command(&f.target, &env).unwrap();
        assert_eq!(cmd.remove_env, env[..4]);
        assert_eq!(cmd.argv.len(), 5);
        assert_eq!(cmd.argv[0], "-n");
        assert_eq!(cmd.argv[1], "-a");
        assert_eq!(cmd.argv[3], "--args");
        assert_eq!(
            cmd.set_env.1,
            f.base.temp.path().join("LARPCC_HOME/profiles/1/cli")
        );
        assert_eq!(
            cmd.set_env.0,
            if provider == registry::Provider::Claude {
                "CLAUDE_CONFIG_DIR"
            } else {
                "CODEX_HOME"
            }
        );
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
    let exact = f.base.temp.path().join("LARPCC_HOME/profiles/1/desktop");
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
