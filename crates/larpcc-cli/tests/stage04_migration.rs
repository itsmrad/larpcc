mod common;
use common::*;

#[test]
fn stage04_migration() {
    let f = Fixture::new();
    let registry = serde_json::json!({
        "version": 2,
        "revision": 6,
        "next_id": 9,
        "profiles": [{"id": 2, "name": "codex-45", "provider": "codex"}],
        "bindings": [],
    });
    f.install_registry(&serde_json::to_vec(&registry).unwrap());
    create_directory(&f.root.join("profiles"));
    let home = f.root.join("profiles/2");
    create_directory(&home);
    let cli = |args: &[&str], code| {
        envelope(&exec(
            f.command(&[&["cc", "profile", "--json"][..], args].concat()),
            code,
        ))
    };
    assert_eq!(cli(&["show", "codex-45"], 0)["revision"], "6");
    let refused = cli(&["add", "claude-work", "--provider", "claude"], 1);
    assert_eq!(refused["error"]["code"], "recovery_required");
    #[cfg(windows)]
    {
        assert_eq!(cli(&["migrate"], 1)["error"]["code"], "recovery_required");
        assert!(!f.root.join("share-journal.json").exists());
        assert!(!home.join("cli").exists());
    }
    #[cfg(unix)]
    {
        let result = cli(&["migrate", "--expected-revision", "6"], 0);
        let examples: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../docs/contracts/cli-v1.examples.json"))
                .unwrap();
        assert_eq!(
            result,
            *examples
                .iter()
                .find(|v| v["command"] == "profile.migrate")
                .unwrap()
        );
        assert!(home.join("cli").is_dir());
        assert!(home.join("desktop").is_dir());
        let after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(f.root.join("registry.json")).unwrap()).unwrap();
        assert_eq!(after["next_id"], 9);
        assert_eq!(after["bindings"], registry["bindings"]);
        assert_eq!(after["profiles"][0]["id"], 2);
        assert_eq!(cli(&["migrate"], 0)["data"]["changed"], false);
    }
}
