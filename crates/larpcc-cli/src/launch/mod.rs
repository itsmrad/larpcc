mod types;
use crate::registry::{Error, LaunchTarget, Provider, Registry};
use std::ffi::OsString;
pub use types::LaunchPlan;
pub fn plan(
    t: &LaunchTarget,
    args: Vec<OsString>,
    env: &[(OsString, OsString)],
) -> Result<LaunchPlan, Error> {
    let variable = match t.profile.provider {
        Provider::Claude => "CLAUDE_CONFIG_DIR",
        Provider::Codex => "CODEX_HOME",
    };
    let names = environment_conflicts(variable, env);
    if !names.is_empty() {
        return Err(Error::EnvConflict { names });
    }
    arguments(&args)?;
    Ok(LaunchPlan {
        executable: t.resolved_executable.clone(),
        argv0: t.stored_executable.as_os_str().to_owned(),
        argv: args,
        cwd: None,
        env_delta: vec![(variable.into(), t.state_dir.as_os_str().to_owned())],
        identity: t.identity.clone(),
    })
}
fn environment_conflicts(variable: &str, env: &[(OsString, OsString)]) -> Vec<String> {
    const PREFIX: &[u8] = b"LARPCC_SYNTHETIC_DENY_";
    let mut names = Vec::new();
    for (name, _) in env {
        // OsStr's encoded bytes preserve ASCII even with unrelated non-Unicode suffixes.
        let bytes = name.as_encoded_bytes();
        if ascii_equal(bytes, variable.as_bytes()) {
            names.push(variable.to_owned());
        }
        if bytes
            .get(..PREFIX.len())
            .is_some_and(|prefix| ascii_equal(prefix, PREFIX))
        {
            names.push("LARPCC_SYNTHETIC_DENY_".into());
        }
    }
    names.sort();
    names.dedup();
    names
}
fn ascii_equal(value: &[u8], label: &[u8]) -> bool {
    #[cfg(windows)]
    {
        value.eq_ignore_ascii_case(label)
    }
    #[cfg(unix)]
    {
        value == label
    }
}
fn arguments(args: &[OsString]) -> Result<(), Error> {
    let mut label = false;
    let mut i = 0;
    while i < args.len() {
        let Some(token) = args[i].to_str() else {
            return Err(Error::ArgConflict {
                label: "argument syntax",
            });
        };
        if token == "prompt" {
            return Ok(());
        }
        if token == "--label" && !label && i + 1 < args.len() {
            label = true;
            i += 2;
            continue;
        }
        let label = if token.starts_with("--config") || token.starts_with("-c") {
            "config"
        } else if token.starts_with("--profile") || token.starts_with("-p") {
            "profile"
        } else if token.starts_with("--settings") {
            "settings"
        } else {
            match token {
                "auth" => "auth",
                "login" => "login",
                "logout" => "logout",
                "server" => "server",
                _ => "argument syntax",
            }
        };
        return Err(Error::ArgConflict { label });
    }
    Err(Error::ArgConflict {
        label: "argument syntax",
    })
}
pub fn execute(plan: &LaunchPlan) -> Result<u32, Error> {
    let root = Registry::locate()?;
    validate_identity(plan, root.root())?;
    #[cfg(windows)]
    {
        crate::platform::spawn_wait(plan)
    }
    #[cfg(unix)]
    {
        crate::platform::execve(&plan.executable, &plan.argv0, &plan.argv, &plan.env_delta)
    }
}

fn validate_identity(plan: &LaunchPlan, root: &std::path::Path) -> Result<(), Error> {
    let (_, identity) = crate::platform::executable(&plan.executable, root)?;
    if identity != plan.identity {
        return Err(Error::Unsafe {
            path: plan.executable.clone(),
            reason: "changed since validation".into(),
        });
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn target(provider: Provider) -> LaunchTarget {
        LaunchTarget {
            profile: crate::registry::Profile {
                id: serde_json::from_str("7").unwrap(),
                name: "work".parse().unwrap(),
                provider,
            },
            state_dir: "/synthetic/profiles/7".into(),
            stored_executable: "/synthetic/provider-link".into(),
            resolved_executable: "/synthetic/provider-native".into(),
            identity: crate::platform::FileIdentity::Unix { dev: 1, ino: 2 },
        }
    }
    #[test]
    fn provisional_grammar_and_exact_delta() {
        for provider in Provider::ALL {
            let t = target(provider);
            for args in [
                vec!["prompt"],
                vec!["--label", "login", "prompt", "--config", "--", "logout"],
                vec!["prompt", "", "a b", "a\"b", "\\", "é"],
            ] {
                let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
                let env = vec![(
                    OsString::from("LARPCC_TEST_VALUE"),
                    OsString::from("invented-secret"),
                )];
                let original = env.clone();
                let p = plan(&t, args.clone(), &env).unwrap();
                assert_eq!(p.argv, args);
                assert_eq!(p.cwd, None);
                assert_eq!(p.argv0, t.stored_executable.as_os_str());
                assert_eq!(p.executable, t.resolved_executable);
                assert_eq!(env, original);
                assert_eq!(
                    p.env_delta,
                    vec![(
                        OsString::from(match provider {
                            Provider::Claude => "CLAUDE_CONFIG_DIR",
                            Provider::Codex => "CODEX_HOME",
                        }),
                        t.state_dir.as_os_str().to_owned()
                    )]
                );
            }
        }
    }
    #[test]
    fn provisional_refusals_use_fixed_labels() {
        for args in [
            vec![],
            vec!["--"],
            vec!["--label"],
            vec!["--label", "x", "--label", "y", "prompt"],
            vec!["--config=a.b=secret", "prompt"],
            vec!["-csecret", "prompt"],
            vec!["--profile=x", "prompt"],
            vec!["-pfoo", "prompt"],
            vec!["--settings", "secret", "prompt"],
            vec!["auth"],
            vec!["login"],
            vec!["server"],
            vec!["logout"],
            vec!["unknown-secret"],
        ] {
            let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
            let e = plan(&target(Provider::Claude), args, &[]).unwrap_err();
            assert!(matches!(e, Error::ArgConflict { .. }));
            assert!(!e.to_string().contains("secret"));
        }
        for variable in [
            "CLAUDE_CONFIG_DIR",
            "LARPCC_SYNTHETIC_DENY_",
            "LARPCC_SYNTHETIC_DENY_SECRET",
        ] {
            for value in ["", "invented-secret"] {
                let env = vec![(variable.into(), value.into())];
                let e = plan(&target(Provider::Claude), vec!["prompt".into()], &env).unwrap_err();
                assert!(matches!(e, Error::EnvConflict { .. }));
                assert!(!e.to_string().contains("invented-secret"));
            }
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn non_unicode_payloads_preserved_but_grammar_refuses() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![0xff, 0xfe]);
        let args = vec!["--label".into(), raw.clone(), "prompt".into(), raw.clone()];
        let env = vec![(raw.clone(), raw.clone())];
        assert_eq!(
            plan(&target(Provider::Claude), args.clone(), &env)
                .unwrap()
                .argv,
            args
        );
        assert!(plan(&target(Provider::Claude), vec![raw], &[]).is_err());
    }
}

#[cfg(all(test, unix))]
mod replacement_tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    #[test]
    fn controlled_replacement_refuses_and_stored_alias_update_preserves_resolved_target() {
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let temporary = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let root = temporary.path().join("root");
        let a = temporary.path().join("a");
        let b = temporary.path().join("b");
        let alias = temporary.path().join("alias");
        #[cfg(target_os = "macos")]
        let magic = [0xcf, 0xfa, 0xed, 0xfe];
        #[cfg(not(target_os = "macos"))]
        let magic = *b"\x7fELF";
        for path in [&a, &b] {
            fs::write(path, magic).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        symlink(&a, &alias).unwrap();
        let registry = Registry::at(root.clone()).unwrap();
        registry
            .add("work".parse().unwrap(), Provider::Claude)
            .unwrap();
        registry
            .bind(Provider::Claude, alias.clone(), false)
            .unwrap();
        let first = registry.launch_target(&"work".parse().unwrap()).unwrap();
        let original = plan(&first, vec!["prompt".into()], &[]).unwrap();
        fs::remove_file(&alias).unwrap();
        symlink(&b, &alias).unwrap();
        assert_eq!(original.executable, a);
        assert!(validate_identity(&original, &root).is_ok());
        let updated = registry.launch_target(&"work".parse().unwrap()).unwrap();
        assert_eq!(updated.resolved_executable, b);
        assert_ne!(updated.identity, first.identity);
        fs::rename(&b, &a).unwrap();
        assert!(
            matches!(validate_identity(&original,&root),Err(Error::Unsafe {reason,..}) if reason=="changed since validation")
        );
    }
}

#[cfg(test)]
mod environment_key_tests {
    use super::*;
    #[test]
    fn known_ascii_prefix_has_no_non_unicode_suffix_exception() {
        #[cfg(unix)]
        let (denied, unrelated) = {
            use std::os::unix::ffi::OsStringExt;
            (
                OsString::from_vec(b"LARPCC_SYNTHETIC_DENY_\xff".to_vec()),
                OsString::from_vec(b"OTHER_\xff".to_vec()),
            )
        };
        #[cfg(windows)]
        let (denied, unrelated) = {
            use std::os::windows::ffi::OsStringExt;
            let mut denied = "larpcc_synthetic_deny_".encode_utf16().collect::<Vec<_>>();
            denied.push(0xd800);
            let mut unrelated = "OTHER_".encode_utf16().collect::<Vec<_>>();
            unrelated.push(0xd800);
            (
                OsString::from_wide(&denied),
                OsString::from_wide(&unrelated),
            )
        };
        let env = vec![
            (denied, OsString::from("invented-secret")),
            (unrelated, OsString::new()),
        ];
        assert_eq!(
            environment_conflicts("CODEX_HOME", &env),
            vec!["LARPCC_SYNTHETIC_DENY_"]
        );
        let lower = vec![(OsString::from("codex_home"), OsString::new())];
        #[cfg(windows)]
        assert_eq!(
            environment_conflicts("CODEX_HOME", &lower),
            vec!["CODEX_HOME"]
        );
        #[cfg(unix)]
        assert!(environment_conflicts("CODEX_HOME", &lower).is_empty());
    }
}
