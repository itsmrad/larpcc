# Larp CC

Larp CC manages named Claude Code and Codex profile records, distinct state directories, and explicit native executable bindings. Names are labels; immutable numeric IDs determine the directories. Renaming preserves both. Forgetting removes metadata and keeps the directory.

**Provider execution is disabled pending stage 3 qualification.** Every ordinary `larp cc run NAME -- ARGS` refuses immediately after argument parsing, before root lookup, registry access, provider probes or process creation. Debug, release, installed and all-features binaries use the same gate. Binding a path does not qualify a provider, version, installation or account-isolation claim.

Requires Rust 1.89 or newer:

```sh
cargo build --release --locked --bin larp
cargo install --path crates/larpcc-cli --locked --bin larp
larp --version
larp cc profile --help
```

Metadata commands:

```sh
larp cc profile add work --provider claude
larp cc profile add personal --provider codex
larp cc profile list
larp cc profile show work
larp cc profile rename work job
larp cc profile forget personal --yes
larp cc provider bind claude /absolute/path/to/native-executable
larp cc provider list
```

Use `provider bind … --replace` to change a binding. An identical binding is a no-op, even if the executable has subsequently disappeared. Provider list revalidates paths and reports `ok`, `unbound` or `invalid: …`; `ok` means path validation only. Binding never runs an executable or probes its version. Paths and diagnostics escape terminal controls and the specified Unicode format controls.

Names contain 1–32 lowercase ASCII letters, digits, hyphens or underscores, starting with a letter. Windows device names are reserved on all platforms. Providers are exactly `claude` and `codex`. Profile list sorts by name and reports each home as `ok`, `missing` or `unsafe`. Unsafe or missing homes remain diagnosable and can be renamed or forgotten without reading their contents. `forget` requires `--yes` and never deletes a home.

Help and version exit 0. Usage errors and rebinds missing `--replace` exit 2; local operational failures exit 1. Errors go to stderr. Reading a missing registry does not create it.

The data root is non-empty `LARPCC_HOME`, otherwise:

| OS | Default root |
|---|---|
| Linux | `$XDG_DATA_HOME/larpcc` when absolute; otherwise `$HOME/.local/share/larpcc` |
| macOS | `$HOME/Library/Application Support/larpcc` |
| Windows | `%LOCALAPPDATA%\larpcc` |

The root must be absolute with no `..` component, and its immediate parent must exist. Metadata comprises `registry.json`, `registry.lock`, transient `registry.json.tmp`, and `profiles/<id>`. Reads accept schema v1 unchanged. Every actual mutation writes schema v2, including bindings; older v1-only Larp binaries refuse v2. Failed operations and no-ops do not migrate or increment the revision. Complete serialized output is limited to 1 MiB.

On Unix, managed objects require the current owner and no group/other access. Ancestors must be root/current-user owned without group/other write access, even for sticky directories such as `/tmp`. Files are created 0600 and directories 0700. macOS requires native extended-security support and a successful descriptor security query; an explicitly absent ACL property is accepted, while extended allow ACLs and query errors are refused without repair. The x86_64 ACL ABI branch is not native-qualified. Unsafe lock/temp objects block reads and writes; safe stale temp files are ignored by reads and removed only after successful write preflight. Provider directory contents are never inspected.

Native binding checks reject scripts, unsafe ownership/permissions/ancestry, data-root containment and Larp's own filesystem identity. Unix installation symlinks are resolved through a bounded trusted chain; the resolved target is used. Native file headers prove neither architecture nor provenance. Windows metadata and binding safety is currently unavailable and explicitly refuses; Windows support is not advertised.

Use a local filesystem. Advisory locks coordinate Larp writers; NFS/SMB locking is unsupported. Opened metadata handles are validated before data reads. Unix atomic replacement and directory sync depend on filesystem support. Same-user or privileged writers can still race path-based checks and execution; there is no identity-pinned execution or sandbox claim. On WSL, use a Linux path rather than `/mnt/c`.

Checks use synthetic data and native test helpers. Set `LARPCC_TEST_PARENT` to an existing private parent with trusted ancestry; the setting affects test fixtures only. CI provisions a private per-run parent and runs native Linux/macOS/Windows and Ubuntu Rust 1.89 gates.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --features synthetic-launch --target-dir /absolute/private/build-parent/synthetic-target --locked
cargo test --bin larpcc-synthetic --features synthetic-launch --target-dir /absolute/private/build-parent/synthetic-target --locked
cargo +1.89 test --workspace --locked
```

The `synthetic-launch` feature builds separate test-only driver/fake binaries. Their provisional grammar and environment fixtures do not describe admitted vendor interfaces. Unix synthetic execution uses direct `execve`, preserves native argument bytes and inherited terminal/process-group behavior, and has no shell fallback or resident supervisor. Synthetic helpers are excluded by the explicit ordinary build/install commands above and are not release artifacts. No real-provider, authentication, credential-store or model operation is part of these checks.

Larp CC is independent and is not affiliated with Anthropic or OpenAI.
