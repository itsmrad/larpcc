mod common;
use common::*;

#[test]
fn help_fixtures_and_missing_subcommands() {
    let f = Fixture::new();
    for (args, fixture) in [
        (vec![], "larp"),
        (vec!["cc"], "cc"),
        (vec!["cc", "profile"], "profile"),
        (vec!["cc", "profile", "add"], "add"),
        (vec!["cc", "profile", "list"], "list"),
        (vec!["cc", "profile", "show"], "show"),
        (vec!["cc", "profile", "rename"], "rename"),
        (vec!["cc", "profile", "forget"], "forget"),
    ] {
        let expected = std::fs::read_to_string(format!(
            "{}/tests/fixtures/help/{fixture}.txt",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
        .replace("\r\n", "\n");
        let help = f.run(&[args.as_slice(), &["--help"]].concat(), 0);
        assert_eq!(stdout(&help), expected);
        if args.len() < 3 {
            assert_eq!(stderr(&f.run(&args, 2)), expected);
        }
    }
    assert!(!f.root.exists());
}
#[test]
fn version_and_usage_errors_do_not_create_root() {
    let f = Fixture::new();
    let before = f.outside_snapshot();
    for flag in ["--version", "-V"] {
        assert_eq!(stdout(&f.run(&[flag], 0)), "larp 0.1.0\n");
    }
    for args in [
        vec!["cc", "run", "x"],
        vec!["cc", "login", "x"],
        vec!["cc", "status", "x"],
        vec!["cc", "logout", "x"],
        vec!["cc", "alias"],
        vec!["--foo"],
        vec!["cc", "--version"],
        vec!["help"],
    ] {
        assert!(stderr(&f.run(&args, 2)).starts_with("error:"));
    }
    for args in [
        vec!["add"],
        vec!["add", "x"],
        vec!["show"],
        vec!["rename", "a"],
        vec!["list", "extra"],
    ] {
        assert!(stderr(&f.profile(&args, 2)).starts_with("error:"));
    }
    for name in [
        "Bad",
        "a b",
        "é",
        "con",
        "../x",
        "a\n",
        "a\x1b",
        "a\u{202e}",
    ] {
        let err = stderr(&f.profile(&["add", name, "--provider", "claude"], 2));
        assert!(
            err.starts_with("error:") && err.contains("invalid profile name"),
            "{err}"
        );
        assert!(!err.contains('\x1b'));
    }
    let err = stderr(&f.profile(&["add", "x", "--provider", "gpt"], 2));
    assert!(err.contains("--provider <PROVIDER>") && err.contains("claude, codex"));
    assert!(!err.contains("invalid profile name"));
    let err = stderr(&f.profile(&["rename", "a", "Bad"], 2));
    assert!(err.contains("<NEW_NAME>") && err.contains("invalid profile name"));
    assert!(!f.root.exists());
    assert_eq!(before, f.outside_snapshot());
}
