mod common;
use common::*;
#[test]
fn ordinary_run_refuses_before_root_or_environment_inspection() {
    let f = Fixture::new();
    for args in [
        vec!["cc", "run", "missing"],
        vec!["cc", "run", "work", "--", "--config", "synthetic"],
    ] {
        let output = f
            .command(&args)
            .env("LARPCC_HOME", "relative")
            .env("LARPCC_SYNTHETIC_DENY_", "invented-secret")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            stderr(&output),
            include_str!("fixtures/qualification-pending.txt").replace("\r\n", "\n")
        );
    }
    assert!(!f.root.exists());
}

#[test]
fn ordinary_run_ignores_corrupt_registry_and_runtime_markers() {
    let f = Fixture::new();
    f.install_registry(b"corrupt synthetic registry");
    let before = std::fs::read(f.root.join("registry.json")).unwrap();
    for name in ["claude", "codex"] {
        let mut command = f.command(&["cc", "run", name, "--", "prompt"]);
        command
            .env("LARPCC_SYNTHETIC", "1")
            .env("LARPCC_FAKE_MODE", "exit");
        let out = capture(command).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            include_str!("fixtures/qualification-pending.txt").replace("\r\n", "\n")
        );
    }
    assert_eq!(std::fs::read(f.root.join("registry.json")).unwrap(), before);
}
