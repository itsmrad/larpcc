#![cfg(windows)]
mod common;
use common::*;
#[test]
fn seed_metadata_explicitly_refuses_before_creation() {
    let f = Fixture::new();
    for args in [vec!["list"], vec!["add", "work", "--provider", "claude"]] {
        assert!(stderr(&f.profile(&args, 1)).contains("platform safety validation is unavailable"));
    }
    assert!(!f.root.exists());
}
