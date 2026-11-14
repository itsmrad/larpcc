#![cfg(windows)]
mod common;
use common::*;
#[test]
fn ordinary_execution_remains_closed_even_with_synthetic_markers() {
    let f = Fixture::new();
    for provider in ["claude", "codex"] {
        let mut command = f.command(&["cc", "run", provider, "--", "prompt"]);
        command
            .env("LARPCC_FAKE_MODE", "exit")
            .env("LARPCC_FAKE_EXIT", "0");
        let output = capture(command).unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            stderr(&output),
            include_str!("fixtures/qualification-pending.txt")
        );
    }
    assert!(!f.root.exists());
}
