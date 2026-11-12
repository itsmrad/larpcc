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
    let mut names = Vec::new();
    for (name, _) in env {
        if let Some(name) = name.to_str() {
            #[cfg(windows)]
            let name = name.to_ascii_uppercase();
            #[cfg(windows)]
            let name = name.as_str();
            if name == variable {
                names.push(variable.to_owned());
            }
            if name.starts_with("LARPCC_SYNTHETIC_DENY_") {
                names.push("LARPCC_SYNTHETIC_DENY_".into());
            }
        }
    }
    names.sort();
    names.dedup();
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
    let (_, identity) = crate::platform::executable(&plan.executable, root.root())?;
    if identity != plan.identity {
        return Err(Error::Unsafe {
            path: plan.executable.clone(),
            reason: "changed since validation".into(),
        });
    }
    #[cfg(windows)]
    {
        crate::platform::spawn_wait(plan)
    }
    #[cfg(unix)]
    {
        crate::platform::execve(&plan.executable, &plan.argv0, &plan.argv, &plan.env_delta)
    }
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
