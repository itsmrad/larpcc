#![cfg(feature = "synthetic-launch")]

#[path = "../common/mod.rs"]
mod common;

use common::{Fixture, copy_executable, envelope, exec, stdout};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

struct StatusFixture {
    fixture: Fixture,
    marker: PathBuf,
    digest: String,
    installed: PathBuf,
}

impl StatusFixture {
    fn new() -> Self {
        let fixture = Fixture::new();
        let fake = PathBuf::from(env!("CARGO_BIN_EXE_larpcc-stage14-fake"));
        let installed = fixture.temp.path().join(fake.file_name().unwrap());
        copy_executable(&fake, &installed);
        fixture.profile(&["add", "codex-45", "--provider", "codex"], 0);
        fixture.run(
            &[
                "cc",
                "provider",
                "bind",
                "codex",
                installed.to_str().unwrap(),
            ],
            0,
        );
        let digest = format!("{:x}", Sha256::digest(fs::read(&installed).unwrap()));
        let marker = fixture.temp.path().join("executed");
        Self {
            fixture,
            marker,
            digest,
            installed,
        }
    }

    fn command(&self, synthetic: bool, args: &[&str]) -> Command {
        let mut cmd = if synthetic {
            self.fixture.command_for(
                std::path::Path::new(env!("CARGO_BIN_EXE_larpcc-synthetic")),
                args,
            )
        } else {
            self.fixture.command(args)
        };
        cmd.env_remove("CODEX_HOME")
            .env("LARPCC_STAGE14_FAKE_DIGEST", &self.digest)
            .env("LARPCC_STAGE14_EXECUTED", &self.marker)
            .env(
                "LARPCC_STAGE14_SELECTED_HOME",
                self.fixture.root.join("profiles").join("1").join("cli"),
            );
        cmd
    }

    fn json(&self, options: &[&str], exit: i32) -> Value {
        let args = [&["cc", "status", "codex-45", "--json"][..], options].concat();
        envelope(&exec(self.command(true, &args), exit))
    }
}

#[test]
fn stage14_status_modes() {
    let f = StatusFixture::new();
    let metadata = f.json(&["--no-account", "--expected-profile-id", "1"], 0);
    assert_eq!(metadata["command"], "status");
    assert_eq!(metadata, f.json(&["--no-account"], 0));
    assert_eq!(metadata["data"]["native_auth"], "not_observed");

    for mode in [&[][..], &["--no-account"][..]] {
        let args = [mode, &["--expected-profile-id", "2"]].concat();
        assert_eq!(f.json(&args, 1)["error"]["code"], "conflict");
        let args = [mode, &["--expected-profile-id", "01"]].concat();
        assert_eq!(f.json(&args, 2)["error"]["code"], "usage");
        let args = [mode, &["--expected-host-revision", "0"]].concat();
        assert_eq!(f.json(&args, 1)["error"]["code"], "unsupported");
    }
    for option in [&["--no-account"][..], &["--expected-profile-id", "1"][..]] {
        let args = [&["cc", "profile", "list", "--json"][..], option].concat();
        assert_eq!(
            envelope(&exec(f.command(true, &args), 2))["error"]["code"],
            "usage"
        );
    }
    assert!(!f.marker.exists());

    let account = f.json(&["--expected-profile-id", "1"], 0);
    assert_eq!(account["command"], "status.account");
    assert_eq!(account["data"]["profile"], metadata["data"]["profile"]);
    assert_eq!(account["data"]["account"]["state"], "signed_in");
    assert!(f.marker.exists());
}

#[cfg(unix)]
#[test]
fn stage14_human_interrupt_reaps_and_publishes_no_account() {
    use std::{
        os::unix::process::ExitStatusExt,
        process::Stdio,
        time::{Duration, Instant},
    };

    let f = StatusFixture::new();
    let release = f.fixture.temp.path().join("release");
    let mut cmd = f.command(true, &["cc", "status", "codex-45"]);
    cmd.env("LARPCC_STAGE14_FAKE_MODE", "hold")
        .env("LARPCC_STAGE14_RELEASE", &release)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let wrapper = common::spawn(&mut cmd).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    let pid = loop {
        if let Some(pid) = fs::read_to_string(&f.marker)
            .ok()
            .and_then(|pid| pid.parse::<libc::pid_t>().ok())
        {
            break pid;
        }
        assert!(Instant::now() < end, "synthetic child did not start");
        std::thread::sleep(Duration::from_millis(5));
    };
    // SAFETY: wrapper is the direct test child and pid came from its isolated fake.
    assert_eq!(
        unsafe { libc::kill(wrapper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let output = common::wait_output(wrapper).unwrap();
    assert_eq!(output.status.signal(), Some(libc::SIGINT));
    assert!(output.stdout.is_empty());
    // SAFETY: signal zero queries only the synthetic child PID.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    assert_eq!(f.json(&[], 0)["data"]["account"]["state"], "signed_in");
}

#[test]
fn stage14_admission_closed_and_digest_mismatch_do_not_execute() {
    let f = StatusFixture::new();
    let args = ["cc", "status", "codex-45", "--json"];
    let ordinary = envelope(&exec(f.command(false, &args), 0));
    let mut unknown_build = f.command(true, &args);
    unknown_build.env("LARPCC_STAGE14_FAKE_DIGEST", "00".repeat(32));
    assert_eq!(envelope(&exec(unknown_build, 0)), ordinary);
    assert_eq!(ordinary["data"]["account"]["state"], "not_qualified");
    assert!(ordinary["data"]["provider_version"].is_null());
    assert!(!f.marker.exists());
}

#[test]
fn stage14_admission_requires_a_bound_enabled_safe_home() {
    let f = StatusFixture::new();
    f.fixture
        .profile(&["add", "claude-work", "--provider", "claude"], 0);
    let unbound = f.command(true, &["cc", "status", "claude-work", "--json"]);
    let result = envelope(&exec(unbound, 0));
    assert_eq!(result["data"]["account"]["state"], "unavailable");
    assert!(result["data"]["provider_version"].is_null());

    f.fixture.profile(&["surface", "codex-45", "cli", "off"], 0);
    assert_eq!(f.json(&[], 0)["data"]["account"]["state"], "unavailable");
    f.fixture.profile(&["surface", "codex-45", "cli", "on"], 0);

    let mut ambient = f.command(true, &["cc", "status", "codex-45", "--json"]);
    ambient.env("CODEX_HOME", f.fixture.temp.path().join("codex"));
    assert_eq!(envelope(&exec(ambient, 1))["command"], "status.account");

    let home = f.fixture.root.join("profiles/1/cli");
    fs::rename(home, f.fixture.temp.path().join("displaced")).unwrap();
    assert_eq!(f.json(&[], 0)["data"]["account"]["state"], "unavailable");
    assert!(!f.marker.exists());
}

#[test]
fn stage14_admission_cannot_publish_a_stale_snapshot() {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };

    let f = StatusFixture::new();
    let release = f.fixture.temp.path().join("release");
    let mut cmd = f.command(true, &["cc", "status", "codex-45", "--json"]);
    cmd.env("LARPCC_STAGE14_FAKE_MODE", "hold")
        .env("LARPCC_STAGE14_RELEASE", &release)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = common::spawn(&mut cmd).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while !f.marker.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(f.marker.exists());
    assert_eq!(f.json(&[], 1)["error"]["code"], "busy");
    assert_eq!(f.json(&["--no-account"], 0)["command"], "status");
    f.fixture.profile(&["rename", "codex-45", "claude-work"], 0);
    fs::write(release, b"").unwrap();
    let output = common::wait_output(child).unwrap();
    assert_eq!(output.status.code(), Some(1));
    let result = envelope(&output);
    assert_eq!(result["command"], "status.account");
    assert_eq!(result["error"]["code"], "conflict");
    assert!(result.get("data").is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn stage14_admission_cannot_spawn_after_a_blocked_notice() {
    use std::{
        io::{self, Read, Write},
        os::fd::AsRawFd,
        process::Stdio,
        time::{Duration, Instant},
    };

    let f = StatusFixture::new();
    let (mut reader, mut writer) = io::pipe().unwrap();
    let fd = writer.as_raw_fd();
    // SAFETY: Both fcntl calls operate on the live test pipe writer.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    let filler = [0; 4096];
    loop {
        match writer.write(&filler) {
            Ok(n) => assert_eq!(n, filler.len()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("failed to fill test pipe: {error}"),
        }
    }
    // SAFETY: Restore blocking writes before giving the write end to the child.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFL, flags) }, 0);
    let mut cmd = f.command(true, &["cc", "status", "codex-45"]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(writer);
    let child = common::spawn(&mut cmd).unwrap();
    drop(cmd);
    let wait_channel = format!("/proc/{}/wchan", child.id());
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let blocked = fs::read_to_string(&wait_channel).unwrap();
        if blocked.contains("pipe_write") {
            break;
        }
        assert!(Instant::now() < end, "status did not block writing its notice");
        std::thread::sleep(Duration::from_millis(5));
    }
    f.fixture.profile(&["rename", "codex-45", "claude-work"], 0);
    let drain = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let output = common::wait_output(child).unwrap();
    let diagnostics = drain.join().unwrap();
    assert!(
        diagnostics
            .windows(b"conflict".len())
            .any(|part| part == b"conflict")
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!f.marker.exists(), "stale status executed its provider");
}

#[test]
fn stage14_parser_capture_timeout_and_overflow() {
    let f = StatusFixture::new();
    for mode in ["stdout-overflow", "stderr-overflow", "timeout"] {
        let mut cmd = f.command(true, &["cc", "status", "codex-45", "--json"]);
        cmd.env("LARPCC_STAGE14_FAKE_MODE", mode);
        let result = envelope(&exec(cmd, 0));
        assert_eq!(result["data"]["account"]["state"], "unknown");
        assert!(result["data"]["account"]["email"].is_null());
        assert!(result["data"]["account"]["plan"].is_null());
        assert_eq!(f.json(&[], 0)["data"]["account"]["state"], "signed_in");
    }
}

#[test]
fn stage14_json_contract() {
    let f = StatusFixture::new();
    let actual = f.json(&[], 0);
    let examples: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../docs/contracts/cli-v1.examples.json"
    ))
    .unwrap();
    let mut example = examples
        .into_iter()
        .find(|value| {
            value["command"] == "status.account" && value["data"]["account"]["email"].is_string()
        })
        .unwrap();
    assert_eq!(actual["data"]["provider_version"], "0.0.14-fixture");
    example["revision"] = actual["revision"].clone();
    example["data"]["provider_version"] = actual["data"]["provider_version"].clone();
    example["data"]["profile"]["id"] = "1".into();
    example["data"]["profile"]["aliases"] = serde_json::json!([]);
    for surface in ["cli", "desktop"] {
        example["data"]["profile"]["surfaces"][surface]["path"] =
            actual["data"]["profile"]["surfaces"][surface]["path"].clone();
    }
    assert_eq!(actual, example);
    for operation in ["run", "login", "logout"] {
        let cmd = f.command(false, &["cc", operation, "codex-45", "--json"]);
        assert_eq!(envelope(&exec(cmd, 2))["error"]["code"], "json_native_io");
    }
}

#[test]
fn stage14_human() {
    let f = StatusFixture::new();
    let email = ["user", "@", "example.com"].concat();
    let plan = ["Pl", "us"].concat();
    for (mode, line) in [
        ("", format!("Signed in as {email} · {plan}")),
        ("email-only", format!("Signed in as {email}")),
        ("plan-only", format!("Signed in · {plan}")),
        ("neither", "Signed in".into()),
        ("signed-out", "Signed out".into()),
        ("credential", "account: unknown".into()),
    ] {
        let mut cmd = f.command(true, &["cc", "status", "codex-45"]);
        cmd.env("LARPCC_STAGE14_FAKE_MODE", mode);
        assert_eq!(
            stdout(&exec(cmd, 0)),
            format!("codex-45 (codex 0.0.14-fixture)\n{line}\nusage: not available\n"),
        );
    }
}

#[test]
fn stage14_never_store() {
    fn verify(path: &std::path::Path, fake: &std::path::Path, forbidden: &[String]) {
        for item in fs::read_dir(path).unwrap() {
            let item = item.unwrap();
            let kind = item.file_type().unwrap();
            if kind.is_dir() {
                verify(&item.path(), fake, forbidden);
            } else if kind.is_file() && item.path() != fake {
                let bytes = fs::read(item.path()).unwrap();
                for value in forbidden {
                    assert!(
                        !bytes
                            .windows(value.len())
                            .any(|window| window == value.as_bytes())
                    );
                }
            }
        }
    }

    let f = StatusFixture::new();
    let email = ["user", "@", "example.com"].concat();
    // Assemble credential-shaped test data at runtime to avoid scanner matches.
    let credential = ["sk-", "inventedfixture01234567890123456789"].concat();
    let forbidden = [email, ["Pl", "us"].concat(), credential];
    assert_eq!(f.json(&[], 0)["data"]["account"]["state"], "signed_in");
    let mut cmd = f.command(true, &["cc", "status", "codex-45", "--json"]);
    cmd.env("LARPCC_STAGE14_FAKE_MODE", "credential");
    let result = exec(cmd, 0);
    assert_eq!(envelope(&result)["data"]["account"]["state"], "unknown");
    let listed = exec(f.command(false, &["cc", "profile", "list", "--json"]), 0);
    for value in &forbidden {
        assert!(!stdout(&result).contains(value));
        assert!(!stdout(&listed).contains(value));
    }
    verify(f.fixture.temp.path(), &f.installed, &forbidden);
}
