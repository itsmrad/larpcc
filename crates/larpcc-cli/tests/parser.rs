mod common;
use common::*;

#[test]
fn help_fixtures_and_missing_subcommands() {
    let f = Fixture::new();
    for (args, fixture) in [
        (vec![], "larp"),
        (vec!["cc"], "cc"),
        (vec!["cc", "app"], "app"),
        (vec!["cc", "app", "bind"], "app-bind"),
        (vec!["cc", "app", "open"], "app-open"),
        (vec!["cc", "skills"], "skills"),
        (vec!["cc", "skills", "add"], "skills-add"),
        (vec!["cc", "skills", "remove"], "skills-remove"),
        (vec!["cc", "skills", "enable"], "skills-enable"),
        (vec!["cc", "skills", "disable"], "skills-disable"),
        (vec!["cc", "skills", "list"], "skills-list"),
        (vec!["cc", "skills", "matrix"], "skills-matrix"),
        (vec!["cc", "skills", "sync"], "skills-sync"),
        (vec!["cc", "mcp"], "mcp"),
        (vec!["cc", "mcp", "add"], "mcp-add"),
        (vec!["cc", "mcp", "remove"], "mcp-remove"),
        (vec!["cc", "mcp", "enable"], "mcp-enable"),
        (vec!["cc", "mcp", "disable"], "mcp-disable"),
        (vec!["cc", "mcp", "list"], "mcp-list"),
        (vec!["cc", "mcp", "matrix"], "mcp-matrix"),
        (vec!["cc", "mcp", "sync-all"], "mcp-sync-all"),
        (vec!["cc", "mcp", "sync"], "mcp-sync"),
        (vec!["cc", "alias"], "alias"),
        (vec!["cc", "alias", "add"], "alias-add"),
        (vec!["cc", "alias", "list"], "alias-list"),
        (vec!["cc", "alias", "remove"], "alias-remove"),
        (vec!["cc", "profile"], "profile"),
        (vec!["cc", "provider"], "provider"),
        (vec!["cc", "provider", "detect"], "detect"),
        (vec!["cc", "provider", "bind"], "bind"),
        (vec!["cc", "provider", "list"], "providers"),
        (vec!["cc", "run"], "run"),
        (vec!["cc", "login"], "login"),
        (vec!["cc", "status"], "status"),
        (vec!["cc", "logout"], "logout"),
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
        if args.len() < 3 && !matches!(args.last(), Some(&"run" | &"login" | &"status" | &"logout"))
        {
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
        vec!["cc", "run"],
        vec!["cc", "login"],
        vec!["cc", "status"],
        vec!["cc", "logout"],
        vec!["cc", "aliases"],
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

#[cfg(not(target_os = "macos"))]
#[test]
fn stage09_app_closed_os_json_gate_precedes_root_io() {
    let f = Fixture::new();
    for (args, command) in [
        (vec!["cc", "app", "open", "codex-45", "--json"], "app.open"),
        (
            vec![
                "cc",
                "app",
                "bind",
                "codex",
                "/invented/ChatGPT.app",
                "--json",
            ],
            "app.bind",
        ),
    ] {
        let out = capture(f.command(&args)).unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["command"], command);
        assert_eq!(value["error"]["code"], "qualification_pending");
        assert!(!f.root.exists());
    }
}
