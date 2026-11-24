mod common;
use common::Fixture;

#[cfg(unix)]
#[test]
fn stage19_detect_checks_revision_before_search() {
    let fixture = Fixture::new();
    let envelope = fixture.larp_json(
        &[
            "cc",
            "provider",
            "detect",
            "--provider",
            "claude",
            "--expected-revision",
            "1",
            "--json",
        ],
        1,
    );
    assert_eq!(envelope["command"], "provider.detect");
    assert_eq!(envelope["error"]["code"], "conflict");
    assert!(!fixture.root.exists());
}

#[cfg(windows)]
#[test]
fn stage19_detect_is_unsupported_before_registry_lookup() {
    let fixture = Fixture::new();
    let output = fixture
        .command(&["cc", "provider", "detect", "--json"])
        .env_remove("LARPCC_HOME")
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["command"], "provider.detect");
    assert_eq!(envelope["error"]["code"], "unsupported");
}
