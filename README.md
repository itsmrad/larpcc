# Larp CC

A lightweight, Rust-based account manager for **Claude Code and Codex**, built for the terminal.

The goal: give each authorized personal or work account a recognizable name, log in through the official provider, and launch the right account with one command. Future desktop integration and optional platform UIs will build on the same account model.

> **Status: project bootstrap.** This repository contains a Cargo workspace and a CLI placeholder. Account creation, authentication, switching, aliases, sync, desktop support, and ACP integration are not implemented. The installation and isolation approach remains a proposal pending approval.

## Intended experience

These commands illustrate the proposed interface; they do not work yet:

```sh
larp cc add claude-work --provider claude
larp cc login claude-work
larp cc add codex-personal --provider codex
larp cc login codex-personal

larp cc list
larp cc use claude-work
larp cc use codex-personal -- --model MODEL_ID

# Optional named launcher, explicitly installed by the user
larp cc alias claude-work --name claude-work
claude-work

# Future desktop capability, only for verified app versions
larp cc use claude-work --desktop
```

Profile names are user-defined. Each launch selects one profile explicitly; selecting another profile affects new launches, while existing sessions continue with their original account. Provider arguments follow `--`.

The normal `claude` and `codex` commands should keep working. Creating a profile should not replace an existing installation, change the global login, or overwrite shell configuration. A launcher name must not silently shadow another command.

## Account isolation: proposed approach

Prefer **one official CLI binary per provider, with a distinct state directory per profile**. Separate binaries may be useful for version pinning, but installing a second copy does not itself isolate credentials.

- **Claude Code:** official documentation describes multiple accounts through `CLAUDE_CONFIG_DIR`, including a separate login for each directory. It also documents an exception: separate directories do not isolate two Console sign-ins without an API key. That mode needs a separate decision. [Claude authentication](https://code.claude.com/docs/en/authentication#log-in-with-multiple-accounts).
- **Codex:** `CODEX_HOME` selects local state, including configuration, authentication, and sessions. Credential backends and database overrides must also be checked; a different directory is not proof that every integration is isolated. [Codex environment variables](https://learn.chatgpt.com/docs/config-file/environment-variables), [authentication](https://learn.chatgpt.com/docs/auth?surface=cli).
- **Desktop apps:** CLI home overrides do not establish a supported desktop switching API. Claude Desktop and ChatGPT desktop need separate, app-version-specific investigation and tests. No desktop isolation is promised by this scaffold.

A profile will hold a stable internal ID, display name, provider, absolute state directory, and selected executable. Provider-managed credentials stay with the provider. A display label is not proof of the authenticated identity.

The launch layer should use a native argument vector, preserve the working directory and terminal, and set environment variables only for the child. It must detect conflicting credential or routing variables without logging their values, respect managed settings, and reject profile paths that resolve to the same state directory. Profile directories are organizational boundaries, not sandboxes against other programs running as the same OS user.

Login, status, refresh, and logout should remain official-client operations within the selected profile. Routine profile selection should not require logging another account out. Destructive profile removal, existing-install migration, and authentication changes during live sessions need explicit, scoped behavior before implementation.

## Account safety

Use accounts the user is authorized to access and retain the provider's authentication flow and organization restrictions. Do not extract browser cookies, copy live tokens between accounts, impersonate provider clients, pool subscriptions, or automatically rotate accounts to bypass limits.

No tool can guarantee that a provider will never restrict an account. Separate installations and repeated logout/login are not guarantees either. Provider policies and supported authentication mechanisms must be checked before releases. This project makes no provider endorsement or zero-ban claim.

## Skills and MCP sharing

Sharing will be **optional and explicit**, with a reviewed plan before any write.

- Allowlist individual skill bundles and instruction files; review scripts and resources as well as Markdown. Skills can execute code and are not automatically portable between hosts.
- Translate only supported, nonsecret MCP definition fields into each provider's schema. Preserve unrelated configuration and reject unsupported conversions.
- Keep MCP authentication, account tokens, cookies, histories, memories, trust decisions, and approvals separate. Re-authorize each intended identity.
- Show a redacted diff, detect concurrent edits, and support atomic updates and rollback of managed files. Do not start servers or execute skill scripts during sync.
- Prefer reviewed snapshots over live cross-account symlinks. Existing global skill discovery and project configuration must be accounted for.

Claude and Codex use different discovery paths and configuration scopes. Changing a CLI state directory does not necessarily isolate globally discovered skills. [Claude skills](https://code.claude.com/docs/en/skills), [Codex skills](https://learn.chatgpt.com/docs/build-skills), [Claude MCP](https://code.claude.com/docs/en/mcp), [Codex MCP](https://developers.openai.com/codex/mcp).

A specific compatibility risk: inspected Codex source describes direct-keyring MCP OAuth entries shared across Codex homes. Unique per-profile MCP identifiers and backend-specific tests must be evaluated before claiming MCP identity isolation. This is source evidence, not a test of a released binary. [Pinned credential-store source](https://github.com/openai/codex/blob/fe2f18520864ddbee86cdd4b103a9fefffe9c6a2/codex-rs/rmcp-client/src/oauth/credential_store.rs).

## Where Agent Client Protocol fits

[Agent Client Protocol (ACP)](https://github.com/agentclientprotocol/agent-client-protocol) standardizes communication between a client UI and a coding agent. It covers session creation, streamed updates, tool permissions, and negotiated authentication. Rust client/runtime libraries and Claude/Codex adapters exist. [Protocol overview](https://agentclientprotocol.com/protocol/v1/overview), [authentication](https://agentclientprotocol.com/protocol/v1/authentication), [agent integrations](https://agentclientprotocol.com/get-started/agents).

**Proposal:** keep ordinary CLI launch independent of ACP. Evaluate ACP when Larp CC needs its own interactive UI or editor integration. Launch an adapter within the selected profile's environment, negotiate capabilities, and verify the adapter's authentication method and policy eligibility before use.

ACP does not define Larp CC's account registry or provide a desktop account-switching guarantee. Multiple conversation sessions must not be assumed to represent different authenticated accounts. An ACP client also remains active while communicating with its agent, so its resource cost belongs to that optional UI feature.

No ACP dependency or adapter is installed by this bootstrap. Adapter compatibility, subscription eligibility, and supported protocol versions remain release gates.

## Architecture and repository

Use a Cargo workspace as a small monorepo. Start with one CLI crate; add a shared core crate when profile and launch logic is implemented, then let future platform UIs call that core. Keep provider-specific authentication and launch behavior behind narrow modules. Avoid empty crates and an extensibility framework before there is working behavior to share.

```text
larpcc/
├── Cargo.toml                  # Workspace and release settings
├── Cargo.lock
├── crates/
│   └── larpcc-cli/
│       ├── Cargo.toml          # Builds the larp binary
│       └── src/main.rs         # Bootstrap status message only
└── README.md
```

Planned boundaries:

- **Shared core:** profile metadata, validation, provider launch plans, and reviewed sync plans; no UI dependency.
- **CLI:** commands, prompts, aliases, terminal lifecycle, and exit status.
- **Platform integrations:** desktop launch compatibility and a future optional macOS menu bar; Windows/Linux UIs only as their requirements become concrete.
- **Optional ACP integration:** communication for a Larp CC-owned agent UI; separate from basic profile selection.

There will be no required daemon, background polling service, hosted account store, or embedded web runtime in the CLI. An optional menu bar app will naturally consume resources while open.

For CLI launches, Unix process replacement is the intended way to eliminate a resident launcher. Windows requires a separately tested console/process strategy and may retain a small foreground wrapper until the provider exits. “No daemon” does not mean zero wrapper memory on every OS. Measure startup time, binary size, and launcher memory before publishing performance claims.

## Build the scaffold

Install a Rust toolchain supporting edition 2024. No Claude/Codex account or provider installation is needed to build this placeholder.

```sh
cargo build --workspace --locked --offline
cargo run --locked --offline --bin larp
```

Current output:

```text
Larp CC is in development. Account management is not implemented yet. See README.md.
```

The placeholder accepts no functional commands and never launches a provider or accesses account data. Its dependencies are limited to Rust's standard library.

Developer checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
cargo build --workspace --release --locked --offline
```

There is no account-management logic or behavioral test suite yet. A successful scaffold build is not evidence of working account isolation.

## Delivery plan

1. **Bootstrap:** publish this README, dependency-free CLI placeholder, and Cargo workspace on `main`.
2. **Approve the approach:** review CLI directory isolation versus separate installations, credential-store behavior, desktop feasibility, and ACP's role with the project owner.
3. **CLI MVP:** implement named profiles, official login/status/logout, explicit launches, and optional aliases on feature branches.
4. **Sharing:** add reviewed skill and MCP sync after identity boundaries are tested.
5. **Desktop:** enable only verified app/OS/version combinations; document unsupported combinations.
6. **Optional UIs:** macOS menu bar first as requirements are approved; evaluate ACP where an interactive agent interface is needed.

Before advertising a supported platform, test two authorized accounts for login isolation, refresh, restart, concurrent launches, logout scope, inherited credential conflicts, keychain denial, and profile path collisions. Add synthetic-fixture tests for parsing, sync conflicts, secret exclusion, path traversal, rollback, exit codes, and signal handling as those features are implemented. Windows native and WSL need distinct coverage. Desktop gates also include singleton behavior, OAuth callbacks, updates, and credential-store identity.

macOS, Linux, and Windows are product targets; account switching has not been validated on any platform yet. Future feature work goes through separate branches and review. Approval of this bootstrap does not approve an account-management implementation.

## Research references

Initial research: 2026-11-11. Official documentation and source behavior can change; record exact tested versions with each supported capability.

- [AI Profiles for macOS](https://github.com/bartekczyz/ai-profiles): reference for CLI/profile launch UX and desktop experiments; community implementation is not vendor support.
- [AI Profiles for Windows](https://github.com/JacquesGariepy/ai-profiles): reference for named CLI launchers; reuse requires checking licensing.
- [ACP Rust runtime](https://github.com/agentclientprotocol/rust-sdk): candidate for a future optional client, not a required launcher dependency.

Larp CC is an independent project and is not affiliated with Anthropic or OpenAI.
