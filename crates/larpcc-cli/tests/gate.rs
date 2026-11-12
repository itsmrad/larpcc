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
            include_str!("fixtures/qualification-pending.txt")
        );
    }
    assert!(!f.root.exists());
}
