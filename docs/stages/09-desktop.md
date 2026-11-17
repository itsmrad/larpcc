# Stage 09 — macOS desktop interface

Import is **OUT OF SCOPE**; fresh profiles only. MAC owns
`crates/larpcc-cli/src/desktop/**`, `tests/desktop*.rs`, `apps/macos/**` and new
`.github/workflows/macos-app.yml`. CORE serializes CLI/main/registry/platform/Cargo
registration and resolves one validated v3 profile/binding/surface snapshot.
MAC waits merged v3 seed; UI scaffold can proceed independently without live app.

```text
larp cc app bind claude|codex ABSOLUTE_APP [--version LABEL] [--replace]
larp cc app open PROFILE [--json]
```

`app bind` validates unmodified stock Claude.app/ChatGPT.app path, retained
ancestry/ACL/identity, records nullable version label in v3 desktop bindings;
it does not open app or qualify it. JSON `app.bind` uses common mutation receipt.
CORE passes provider plus derived cli/desktop dirs and checked binding to MAC;
MAC revalidates app/path/snapshot immediately before dispatch and retains handles
as required. Exact Rust signature is deliberately unfrozen. Claude app gets
CLAUDE_CONFIG_DIR=cli, ChatGPT app CODEX_HOME=cli; both proposed --user-data-dir=
desktop require exact app-version proof. Claude desktop chat auth is separate;
ChatGPT sharing with CLI is an unqualified hypothesis until native proof.
No modified app, copy/re-sign, executable swap or token meter.

Open/focus returns [JSON local dispatch receipt](../contracts/cli-v1.md), never
account/auth success. Current ordinary implementation must qualification-refuse;
unknown versions may produce safe warnings/diagnostics, **never launch** despite
handoff D4's earlier warn-and-launch suggestion. This is explicit prospective
resolution retaining stage-3 fail-closed safety. Focus only a verified matching
profile process under reviewed singleton rules; process-name/PID string matching
alone cannot prove it. No process argv harvesting in doctor. Refuse uncertain
liveness, callbacks or ownership. No background manager or automated retry.

MAC implements native synthetic `desktop_contract` tests (execute
`cargo test --workspace --locked --test desktop_contract`, zero cases fails):
validated fixture binding, both directory inputs, closed admission, synthetic
launched/focused receipt, stale binding/profile and wrong PID refusal, no auth
strings/output, safe cleanup. Native app qualification is a distinct later
finite authorization sheet: both launch orders, simultaneous windows, identity/
history, callbacks, refresh/logout, restart/update, keychain denial, singleton
focus, CLI↔desktop auth effects and no global writes. All unfilled combinations
stay closed. Signed-out test app data only as RULES authorizes, fresh throwaway
homes, quit and delete all fixtures. Real-account work remains unauthorized.
Windows desktop is later proof, Linux has no claimed official desktop support.
