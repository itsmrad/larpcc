# Larp CC

Larp CC manages named Claude Code and Codex profile records and empty state directories. Names are labels; each profile has an immutable numeric ID and a state directory derived from that ID. Renaming preserves both. Forgetting removes only the registry entry and keeps its directory; it cannot be attached to another profile.

Requires Rust 1.89 or newer. Build or install from this checkout:

```sh
cargo build --workspace --release --locked
cargo install --path crates/larpcc-cli --locked
larp --version
larp cc profile --help
```

Run directly from the checkout with `cargo run --locked --bin larp --` followed by the command arguments, for example:

```sh
cargo run --locked --bin larp -- cc profile list
```

Profile commands:

```sh
larp cc profile add work --provider claude
larp cc profile add personal --provider codex
larp cc profile list
larp cc profile show work
larp cc profile rename work job
larp cc profile forget personal --yes
```

Names contain 1–32 lowercase ASCII letters, digits, hyphens or underscores, starting with a letter. Windows device names are reserved on all platforms. Providers are exactly `claude` and `codex`. `list` sorts by name and reports each state directory as `ok`, `missing` or `unsafe`; `show` includes its path and any unsafe reason. `forget` requires `--yes` and never deletes the state directory.

Help and version exit 0. Usage errors exit 2; local operational failures exit 1. Errors go to stderr. Reading a missing registry does not create it.

The data root is non-empty `LARPCC_HOME`, otherwise:

| OS | Default root |
|---|---|
| Linux | `$XDG_DATA_HOME/larpcc` when XDG_DATA_HOME is absolute; otherwise `$HOME/.local/share/larpcc` |
| macOS | `$HOME/Library/Application Support/larpcc` |
| Windows | `%LOCALAPPDATA%\larpcc` |

The root must be absolute with no `..` component, and its immediate parent must already exist. The root contains `registry.json`, `registry.lock`, and `profiles/<id>` directories; writes may temporarily create `registry.json.tmp`. Profile names never become paths.

On Unix, Larp checks ownership and rejects group/other access; it creates directories with mode 0700 and metadata files with mode 0600. Windows checks file types and rejects symbolic links and junctions, but uses inherited ACLs: directory privacy and ownership are **not verified** on Windows.

Use a local filesystem. Advisory locks coordinate Larp writers, but do not constrain other programs; NFS/SMB locking is unsupported. Larp detects static unsafe paths, but cannot defend against another process running as the same user swapping files between checks and use. On WSL, use a Linux path; roots under `/mnt/c` usually fail Unix mode checks. Atomic replacement and Unix directory sync provide durability only on filesystems that honor them. Windows has process-crash recovery but no power-loss durability guarantee.

Checks (all tests use synthetic temporary data):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.89 test --workspace --locked
```

CI runs tests and clippy on native Linux, macOS and Windows runners, plus Rust 1.89 tests on Linux. After dependencies have been fetched, these Cargo build/check commands also accept `--offline`.

Larp CC is an independent project. It is not affiliated with Anthropic or OpenAI.
