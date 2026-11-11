# Larp CC

Larp CC provides a command-line interface for named Claude Code and Codex profiles.
This build implements command parsing, validation and help. Profile storage is unavailable in this build.

Requires Rust 1.89 or newer. Build and inspect the command help:

```sh
cargo build --workspace --locked
cargo run --locked --bin larp -- --help
cargo run --locked --bin larp -- cc --help
cargo run --locked --bin larp -- cc profile --help
```

Install the binary from this checkout:

```sh
cargo install --path crates/larpcc-cli --locked
larp --version
larp cc profile --help
```

Checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.89 test --workspace --locked
```

Storage acceptance tests require the registry implementation. Parser, help and renderer tests run independently:

```sh
cargo test --locked --bin larp
cargo test --locked --test parser
```

Larp CC is an independent project. It is not affiliated with Anthropic or OpenAI.
