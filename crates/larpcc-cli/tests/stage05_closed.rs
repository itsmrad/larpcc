#![cfg(unix)]
mod common;
use common::*;
use std::{fs, path::Path};

const PENDING: &str = "error: provider operation is disabled for this combination\nhint: review the documented qualification matrix\n";

fn tree(path: &Path) -> Vec<String> {
    let mut out = vec![];
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        out.push(path.to_string_lossy().into_owned());
        if path.is_dir() {
            out.extend(tree(&path));
        }
    }
    out.sort();
    out
}

#[test]
fn stage05_closed_ordinary_alias_still_qualification_refuses() {
    let f = Fixture::new();
    let before = f.outside_snapshot();
    for (name, provider) in [("claude-work", "claude"), ("codex-45", "codex")] {
        f.larp_json(
            &[
                "cc",
                "profile",
                "add",
                name,
                "--provider",
                provider,
                "--json",
            ],
            0,
        );
    }
    f.larp_json(
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
        0,
    );
    f.larp_json(
        &["cc", "alias", "add", "claude-work", "codex-45", "--json"],
        0,
    );
    let state = tree(&f.root);
    for alias in ["codex-45", "claude-work"] {
        for args in [
            vec![],
            vec!["--json"],
            vec!["--version"],
            vec!["login", "--yes"],
        ] {
            let output = exec(f.command_for(&f.launcher(alias), &args), 1);
            assert!(output.stdout.is_empty());
            assert_eq!(stderr(&output), PENDING);
        }
    }
    // Same refusal as the standard run path; no lifecycle lock, child or native state.
    let run = exec(
        f.command_for(&f.install_larp(), &["cc", "run", "claude-work"]),
        1,
    );
    assert_eq!(stderr(&run), PENDING);
    assert_eq!(tree(&f.root), state);
    assert!(!f.root.join("auth-locks").exists());
    for name in ["codex-45", "claude-work"] {
        f.larp_json(&["cc", "alias", "remove", name, "--json"], 0);
    }
    // Outside sentinels (fake vendor homes, HOME, XDG...) are untouched; the fixture
    // temp parent, including the root and launchers, is deleted on drop.
    let mut after = f.outside_snapshot();
    after.retain(|path, _| !path.starts_with(f.temp.path().join("install")));
    let mut before = before;
    before.retain(|path, _| !path.starts_with(f.temp.path().join("install")));
    assert_eq!(after, before);
}
