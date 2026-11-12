#![cfg(feature = "synthetic-launch")]
mod common;
use common::*;
use std::path::Path;
#[test]
fn synthetic_driver_refuses_missing_profile() {
    let f = Fixture::new();
    let out = capture(f.command_for(
        Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
        &["cc", "run", "work", "--", "prompt"],
    ))
    .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("no profile named work"));
    assert!(!f.root.exists());
}
