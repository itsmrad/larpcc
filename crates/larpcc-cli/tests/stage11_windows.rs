#![cfg(windows)]

mod common;
use common::*;

#[test]
fn stage11_windows_refuses_usage_before_registry_access() {
    let fixture = Fixture::new();
    let binary = fixture.install_larp();
    create_file(&fixture.root, b"this is not a registry directory");
    let run = |args: &[&str], code| {
        envelope(&exec(
            fixture.command_for(&binary, &[&["cc", "--json"][..], args].concat()),
            code,
        ))
    };
    for args in [
        &["usage", "codex-45"][..],
        &["usage", "--live"],
        &[
            "settings",
            "usage",
            "interval",
            "300",
            "--expected-revision",
            "4",
        ],
        &["settings", "usage", "live", "on", "--acknowledge-risk"],
        &["settings", "usage", "mode", "local"],
        &[
            "settings",
            "usage",
            "claude-statusline",
            "on",
            "--profile",
            "claude-work",
        ],
        &["profile", "plan-label", "codex-45", "Plus"],
    ] {
        let reply = run(args, 2);
        assert_eq!(reply["error"]["code"], "unsupported");
        assert_eq!(
            reply["error"]["message"],
            "Usage reads and settings changes are unavailable on Windows."
        );
    }
    let reply = run(&["settings", "usage"], 0);
    assert_eq!(reply["revision"], "0");
    assert_eq!(reply["data"]["mode"], "local");
    assert_eq!(reply["data"]["profiles"], serde_json::json!([]));
    assert_eq!(
        std::fs::read(&fixture.root).unwrap(),
        b"this is not a registry directory"
    );
}
