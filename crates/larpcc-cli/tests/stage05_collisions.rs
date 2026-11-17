mod common;
use common::*;
use std::fs;

fn setup() -> Fixture {
    let f = Fixture::new();
    f.larp_json(
        &[
            "cc",
            "profile",
            "add",
            "claude-work",
            "--provider",
            "claude",
            "--json",
        ],
        0,
    );
    f
}
fn code(value: &serde_json::Value) -> &str {
    value["error"]["code"].as_str().unwrap()
}

#[test]
fn stage05_collisions_shadow_flag_and_reserved_names_precede_root_io() {
    let f = Fixture::new();
    for name in ["claude", "codex"] {
        let refused = f.larp_json(&["cc", "alias", "add", name, "claude-work", "--json"], 2);
        assert_eq!(refused["command"], "alias.add");
        assert_eq!(code(&refused), "usage");
        let human = exec(
            f.command_for(
                &f.install_larp(),
                &["cc", "alias", "add", name, "claude-work"],
            ),
            2,
        );
        assert!(human.stdout.is_empty());
        assert!(
            stderr(&human).starts_with("error: Aliases named claude or codex require --shadow.")
        );
    }
    for name in ["larp", "con", "lpt1", "Codex", "a.exe", "a b", "../x", ""] {
        let refused = f.larp_json(&["cc", "alias", "add", name, "claude-work", "--json"], 2);
        assert_eq!(refused["command"], "parse");
    }
    assert!(!f.root.exists());
    let f = setup();
    for name in ["claude", "codex"] {
        let added = f.larp_json(
            &[
                "cc",
                "alias",
                "add",
                name,
                "claude-work",
                "--shadow",
                "--json",
            ],
            0,
        );
        assert_eq!(added["data"]["name"], name);
        assert!(f.launcher(name).exists());
    }
}

#[test]
fn stage05_collisions_never_overwrite_existing_files_or_path_commands() {
    let f = setup();
    create_directory(&f.root.join("bin"));
    // An existing file at the fixed launcher or receipt path refuses, even with --shadow.
    for (name, path) in [
        ("claude-work", f.launcher("claude-work")),
        ("codex-45", f.alias_receipt("codex-45")),
    ] {
        create_file(&path, b"unrelated sentinel");
        for shadow in [false, true] {
            let mut args = vec!["cc", "alias", "add", name, "claude-work", "--json"];
            if shadow {
                args.push("--shadow");
            }
            assert_eq!(code(&f.larp_json(&args, 1)), "conflict");
        }
        assert_eq!(fs::read(&path).unwrap(), b"unrelated sentinel");
        fs::remove_file(&path).unwrap();
    }
    // A same-named command on PATH refuses unless shadowing is explicit; never touched.
    let commands = f.temp.path().join("commands ü space");
    create_directory(&commands);
    let command = commands.join(if cfg!(windows) {
        "codex-45.cmd"
    } else {
        "codex-45"
    });
    create_file(&command, b"unrelated command sentinel");
    let path = std::env::join_paths([&commands]).unwrap();
    let larp = f.install_larp();
    let mut add = f.command_for(
        &larp,
        &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
    );
    add.env("PATH", &path);
    let refused = envelope(&exec(add, 1));
    assert_eq!(code(&refused), "conflict");
    assert!(!f.launcher("codex-45").exists());
    let mut add = f.command_for(
        &larp,
        &["cc", "alias", "add", "codex-45", "claude-work", "--shadow"],
    );
    add.env("PATH", &path);
    let added = exec(add, 0);
    assert!(stdout(&added).contains("does not edit PATH or shell files"));
    assert_eq!(fs::read(&command).unwrap(), b"unrelated command sentinel");
    // Registered names, missing profiles and unknown aliases.
    assert_eq!(
        code(&f.larp_json(
            &["cc", "alias", "add", "codex-45", "claude-work", "--json"],
            1
        )),
        "conflict"
    );
    assert_eq!(
        code(&f.larp_json(&["cc", "alias", "add", "other", "missing", "--json"], 1)),
        "not_found"
    );
    let missing = f.larp_json(&["cc", "alias", "remove", "missing", "--json"], 1);
    assert_eq!(missing["command"], "alias.remove");
    assert_eq!(code(&missing), "not_found");
    // Forget refuses while aliases reference the profile.
    assert_eq!(
        code(&f.larp_json(
            &["cc", "profile", "forget", "claude-work", "--yes", "--json"],
            1
        )),
        "conflict"
    );
    f.larp_json(&["cc", "alias", "remove", "codex-45", "--json"], 0);
    f.larp_json(
        &["cc", "profile", "forget", "claude-work", "--yes", "--json"],
        0,
    );
}
