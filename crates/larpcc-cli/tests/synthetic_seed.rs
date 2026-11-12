#![cfg(feature = "synthetic-launch")]
mod common;
use common::*;
use std::process::Command;
#[test]
fn synthetic_driver_refuses_missing_profile() {
    let f = Fixture::new();
    let out = Command::new(env!("CARGO_BIN_EXE_larpcc-synthetic"))
        .env_clear()
        .env("LARPCC_HOME", &f.root)
        .args(["cc", "run", "work", "--", "prompt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    #[cfg(unix)]
    assert!(stderr(&out).contains("no profile named work"));
    #[cfg(windows)]
    assert!(stderr(&out).contains("platform safety validation is unavailable"));
    assert!(!f.root.exists());
}
