use crate::registry::ProfileName;
use serde::{Deserialize, Deserializer, Serialize};
use std::net::Ipv4Addr;

pub(super) const WORD_LIMIT: usize = 4096;
const SCRIPT_LIMIT: usize = 64 * 1024;
const OS_CHECK: &str =
    "case \"$(uname -s </dev/null 2>/dev/null)\" in Linux|Darwin) ;; *) exit 45 ;; esac\n";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Host {
    pub name: ProfileName,
    pub ssh: Address,
    #[serde(deserialize_with = "required_nullable")]
    pub larp: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub larpcc_home: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub identity_file: Option<String>,
}

fn required_nullable<'de, D: Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    Option::deserialize(input)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Address {
    Config {
        alias: String,
    },
    Direct {
        destination: String,
        resolved: String,
    },
}

struct Direct<'a> {
    user: &'a str,
    host: &'a str,
    port: u16,
}

fn identifier(value: &str, extras: &[u8]) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || extras.contains(&b))
}

fn direct(value: &str) -> Option<Direct<'_>> {
    let (user, address) = value.split_once('@')?;
    if !identifier(user, b"_-") {
        return None;
    }
    let (host, port) = match address.split_once(':') {
        Some((host, port)) => {
            let n: u16 = port.parse().ok()?;
            if n == 0 || n.to_string() != port {
                return None;
            }
            (host, n)
        }
        None => (address, 22),
    };
    let dns = host.len() <= 253
        && host
            .rsplit('.')
            .next()
            .is_some_and(|last| last.as_bytes().first().is_some_and(u8::is_ascii_alphabetic))
        && host.split('.').all(|label| {
            let bytes = label.as_bytes();
            bytes.len() <= 63
                && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
                && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        });
    if !dns
        && !host
            .parse::<Ipv4Addr>()
            .is_ok_and(|ip| ip.to_string() == host)
    {
        return None;
    }
    Some(Direct { user, host, port })
}

impl Address {
    pub(crate) fn config(alias: String) -> Option<Self> {
        identifier(&alias, b"_-.").then_some(Self::Config { alias })
    }

    pub(crate) fn direct(destination: String) -> Option<Self> {
        let Direct { user, host, port } = direct(&destination)?;
        let resolved = format!("{user}@{host}:{port}");
        Some(Self::Direct {
            destination,
            resolved,
        })
    }
}

pub(super) fn word(value: &str) -> bool {
    value.len() <= WORD_LIMIT && !value.chars().any(char::is_control)
}

fn remote_path(value: &str) -> bool {
    value.starts_with('/') && word(value)
}

fn identity_path(value: &str, windows: bool) -> bool {
    let absolute = if windows {
        matches!(value.as_bytes(), [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic())
    } else {
        value.starts_with('/') && !value.starts_with("//")
    };
    absolute && word(value) && !value.contains(['%', '$'])
}

impl Host {
    pub(crate) fn valid(&self) -> bool {
        ProfileName::parse_new(self.name.as_str()).is_ok()
            && self.larp.as_deref().is_none_or(remote_path)
            && self.larpcc_home.as_deref().is_none_or(remote_path)
            && match &self.ssh {
                Address::Config { alias } => {
                    identifier(alias, b"_-.") && self.identity_file.is_none()
                }
                Address::Direct { destination, .. } => {
                    Address::direct(destination.clone()).as_ref() == Some(&self.ssh)
                        && self
                            .identity_file
                            .as_deref()
                            .is_none_or(|path| identity_path(path, cfg!(windows)))
                }
            }
    }

    pub(super) fn ssh_args(&self) -> Option<Vec<String>> {
        if !self.valid() {
            return None;
        }
        let mut args = match &self.ssh {
            Address::Config { alias } => {
                let mut args = strings(&[
                    "-T",
                    "-a",
                    "-x",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ClearAllForwardings=yes",
                    "-o",
                    "LogLevel=ERROR",
                    "-o",
                    "ConnectTimeout=4",
                    "--",
                ]);
                args.push(alias.clone());
                args
            }
            Address::Direct { destination, .. } => {
                let Direct { user, host, port } = direct(destination)?;
                let mut args = strings(&[
                    "-F",
                    "none",
                    "-T",
                    "-a",
                    "-x",
                    "-S",
                    "none",
                    "-e",
                    "none",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "StrictHostKeyChecking=yes",
                    "-o",
                    "UpdateHostKeys=no",
                    "-o",
                    "PermitLocalCommand=no",
                    "-o",
                    "ClearAllForwardings=yes",
                    "-o",
                    "LogLevel=ERROR",
                    "-o",
                    "ConnectTimeout=4",
                    "-o",
                    "ConnectionAttempts=1",
                ]);
                if let Some(identity) = &self.identity_file {
                    args.extend(strings(&["-o", "IdentitiesOnly=yes", "-i", identity]));
                }
                args.extend(strings(&["-l", user, "-p", &port.to_string(), "--", host]));
                args
            }
        };
        args.push("sh -s".into());
        Some(args)
    }

    pub(super) fn script(&self, args: &[String], import: bool) -> Option<Vec<u8>> {
        if !self.valid() || !args.iter().all(|arg| word(arg)) {
            return None;
        }
        let larp = quote(self.larp.as_deref().unwrap_or("larp"));
        let mut script = String::from(OS_CHECK);
        if self.larp.is_some() {
            script.push_str(&format!(
                "[ -f {larp} ] </dev/null && [ -x {larp} ] </dev/null || exit 44\n"
            ));
        } else {
            script.push_str("command -v 'larp' </dev/null >/dev/null 2>&1 || exit 44\n");
        }
        if let Some(home) = &self.larpcc_home {
            script.push_str(&format!("export LARPCC_HOME={} </dev/null\n", quote(home)));
        }
        script.push_str("exec ");
        script.push_str(&larp);
        if script.len() > SCRIPT_LIMIT {
            return None;
        }
        for arg in args {
            script.push(' ');
            script.push_str(&quote(arg));
            if script.len() > SCRIPT_LIMIT {
                return None;
            }
        }
        if !import {
            script.push_str(" </dev/null");
        }
        script.push('\n');
        (script.len() <= SCRIPT_LIMIT).then(|| script.into_bytes())
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage13_argv_address_and_identity_syntax() {
        for destination in ["user@example", "_user@a-1.Example:65535", "u@192.0.2.10"] {
            assert!(
                Address::direct(destination.into()).is_some(),
                "{destination}"
            );
        }
        for destination in [
            "-u@example",
            "u@example:022",
            "u@example:0",
            "u@example:+22",
            "u@host:65536",
            "u@192.00.2.10",
            "u@host.2name",
            "u@-host",
            "u@host.",
            "u@host\n",
            "u@[::1]",
            "u@host -o ProxyCommand",
        ] {
            assert!(
                Address::direct(destination.into()).is_none(),
                "{destination:?}"
            );
        }
        for alias in ["-host", "foo*", "foo%h", "foo/bar", "foo bar", "a\n"] {
            assert!(Address::config(alias.into()).is_none(), "{alias:?}");
        }
        for path in [
            "relative",
            "-option",
            "/%h",
            "/$HOME/key",
            "/x\u{85}",
            "//host/key",
        ] {
            assert!(!identity_path(path, false), "{path:?}");
        }
        for path in [
            r"\key",
            r"C:key",
            r"\\server\share\key",
            r"\\?\C:\key",
            r"C:\%d\key",
        ] {
            assert!(!identity_path(path, true), "{path:?}");
        }
        assert!(identity_path(r"C:\Keys with spaces\profile", true));
        assert!(identity_path("/keys with spaces/profile", false));
    }

    #[test]
    fn stage13_argv_frozen_modes_and_quoted_script() {
        let mut host = Host {
            name: ProfileName::parse_new("claude-work").unwrap(),
            ssh: Address::config("_host.example".into()).unwrap(),
            larp: Some("/opt/Larp's bin/larp".into()),
            larpcc_home: Some("/home/test/${literal}".into()),
            identity_file: None,
        };
        assert_eq!(
            host.ssh_args().unwrap(),
            [
                "-T",
                "-a",
                "-x",
                "-o",
                "BatchMode=yes",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "LogLevel=ERROR",
                "-o",
                "ConnectTimeout=4",
                "--",
                "_host.example",
                "sh -s"
            ]
        );
        host.ssh = Address::direct("user@host.example:2222".into()).unwrap();
        let identity = if cfg!(windows) {
            r"C:\keys with spaces\profile"
        } else {
            "/keys with spaces/profile"
        };
        host.identity_file = Some(identity.into());
        assert_eq!(
            host.ssh_args().unwrap(),
            [
                "-F",
                "none",
                "-T",
                "-a",
                "-x",
                "-S",
                "none",
                "-e",
                "none",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "UpdateHostKeys=no",
                "-o",
                "PermitLocalCommand=no",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "LogLevel=ERROR",
                "-o",
                "ConnectTimeout=4",
                "-o",
                "ConnectionAttempts=1",
                "-o",
                "IdentitiesOnly=yes",
                "-i",
                identity,
                "-l",
                "user",
                "-p",
                "2222",
                "--",
                "host.example",
                "sh -s"
            ]
        );
        let args = strings(&["arg with space", "", "$(false); 'literal'"]);
        let script = String::from_utf8(host.script(&args, false).unwrap()).unwrap();
        assert_eq!(
            script,
            concat!(
                "case \"$(uname -s </dev/null 2>/dev/null)\" in Linux|Darwin) ;; *) exit 45 ;; esac\n",
                "[ -f '/opt/Larp'\\''s bin/larp' ] </dev/null && [ -x '/opt/Larp'\\''s bin/larp' ] </dev/null || exit 44\n",
                "export LARPCC_HOME='/home/test/${literal}' </dev/null\n",
                "exec '/opt/Larp'\\''s bin/larp' 'arg with space' '' '$(false); '\\''literal'\\''' </dev/null\n"
            )
        );
        assert!(!script.contains(identity));
        assert!(host.script(&["invalid\nword".into()], false).is_none());
        assert!(host.script(&["a".repeat(WORD_LIMIT + 1)], false).is_none());
        assert!(
            host.script(&vec!["'".repeat(WORD_LIMIT); 5], false)
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn stage13_protocol_shell_prologue_checks_and_quoting() {
        use std::{
            fs,
            io::Write,
            os::unix::fs::PermissionsExt,
            path::{Path, PathBuf},
            process::{Command, Stdio},
        };
        let parent = std::env::var_os("LARPCC_TEST_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let temp = tempfile::tempdir_in(parent).unwrap();
        let fake_larp = temp.path().join("Larp's ${fixture}");
        let uname = temp.path().join("uname");
        let outside = temp.path().join("outside");
        fs::write(&outside, b"unchanged").unwrap();
        let write_script = |path: &Path, bytes: &[u8]| {
            fs::write(path, bytes).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        };
        write_script(&uname, b"#!/bin/sh\nprintf 'Linux\\n'\n");
        write_script(
            &fake_larp,
            b"#!/bin/sh\nprintf '%s\\n' \"$LARPCC_HOME\" \"$@\"\n",
        );
        let home = "/fixture/home ${literal}' path";
        let host = Host {
            name: "claude-work".parse().unwrap(),
            ssh: Address::config("fixture".into()).unwrap(),
            larp: Some(fake_larp.to_str().unwrap().into()),
            larpcc_home: Some(home.into()),
            identity_file: None,
        };
        let args = strings(&["two words", "${unexpanded}", "$(false); 'text'", ""]);
        let run = || {
            let mut child = Command::new("/bin/sh")
                .arg("-s")
                .env_clear()
                .env("PATH", temp.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&host.script(&args, false).unwrap())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.stderr.is_empty());
            output
        };
        let good = run();
        assert!(good.status.success());
        assert_eq!(
            String::from_utf8(good.stdout).unwrap(),
            format!("{home}\n{}\n", args.join("\n"))
        );
        fs::set_permissions(&fake_larp, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(run().status.code(), Some(44));
        fs::remove_file(&fake_larp).unwrap();
        fs::create_dir(&fake_larp).unwrap();
        assert_eq!(run().status.code(), Some(44));
        for system in ["MSYS_NT-10.0", "CYGWIN_NT-10.0", "FreeBSD"] {
            write_script(
                &uname,
                format!("#!/bin/sh\nprintf '{system}\\n'\n").as_bytes(),
            );
            assert_eq!(run().status.code(), Some(45));
        }
        assert_eq!(fs::read(outside).unwrap(), b"unchanged");
    }
}
