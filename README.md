# Larp CC

Larp CC is a Rust project for managing named Claude Code and Codex account profiles from the terminal.

**Current state:** this repository contains a Cargo workspace with one crate, `larpcc-cli`, which builds a `larp` binary. The binary is a placeholder. It prints one status line and exits with status 0. It ignores all arguments, including `--help`. It does not create profiles, read or write account data, or launch any provider.

## Repository layout

```text
Cargo.toml                    workspace and release profile
Cargo.lock
crates/larpcc-cli/Cargo.toml  builds the `larp` binary
crates/larpcc-cli/src/main.rs placeholder entry point
plan.md                       project roadmap
docs/stages/                  detailed stage plans
```

## Build and run

Requires a Rust toolchain that supports edition 2024 (Rust 1.85 or newer). The workspace has no third-party dependencies.

```sh
cargo build --workspace --locked --offline
cargo run --locked --offline --bin larp
```

Output:

```text
Larp CC is in development. Account management is not implemented yet. See README.md.
```

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
cargo build --workspace --release --locked --offline
```

There are no tests yet, so `cargo test` runs zero tests.

Larp CC is an independent project. It is not affiliated with Anthropic or OpenAI.
