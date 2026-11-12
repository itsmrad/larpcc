#![cfg(feature = "synthetic-launch")]
mod common;
use common::*;
use std::process::Command;
#[test]
fn synthetic_seed_cannot_launch() {
    let f = Fixture::new();
    let out = Command::new(env!("CARGO_BIN_EXE_larpcc-synthetic"))
        .env_clear()
        .args(["cc", "run", "work", "--", "prompt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        include_str!("fixtures/qualification-pending.txt")
    );
    assert!(!f.root.exists());
}
