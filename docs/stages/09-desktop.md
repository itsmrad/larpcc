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
CLAUDE_CONFIG_DIR=cli for a future qualified Claude path; current ChatGPT dispatch
sets CODEX_HOME=cli and passes --user-data-dir=desktop. These switches do not
establish account or storage isolation. No modified app, copy/re-sign, executable
swap or token meter.

## Lead desktop admission amendment — 2026-11-17

**Dispatch paused:** ChatGPT.app open and focus return `qualification_pending`
before effects until the observed `~/.codex` changes are attributed. Binding
and its identity checks remain available. The named dispatch gate may be
reopened only after that attribution re-test.

Bind performs strict, all-architecture, nested signature validation and may
take approximately six seconds; human mode prints one progress line and JSON
prints none. A bounded private verdict in the registry is keyed by the bundle
executable and Info.plist device, inode, size, nanosecond mtime and ctime, plus
the static code directory hash. It is replaced on bind under the registry lock.
Open rechecks that identity and uses the cached verdict; a mismatch triggers a
fresh full validation. Normal cached admission targets the two-second budget;
signature revalidation after an identity change may take longer.

This amendment superseded the earlier desktop-only closed-table requirement and
previously restored D4 warn-and-launch for local macOS dispatch. CLI native qualification
ROWS remain empty: no CLI provider execution or account operation is authorized.
Before the dispatch pause above, ordinary `app open` was enabled only for unmodified `/Applications/ChatGPT.app`
with strict, all-architecture signature validation against identifier
`com.openai.codex`, Apple generic anchor, Developer ID certificate constraints,
and team `2DC432GLL2`. Native Claude binding/open remains closed because its
signing identity is unverified. The observed ChatGPT version is `26.1111.21308`
(build `27194`); other or unavailable versions produce a fixed warning and may
launch after the same identity/snapshot checks. Version labels never grant
identity, account, or isolation qualification. Security.framework reads static
signed app metadata only, never user auth/config/session/keychain contents.

Open/focus returns the [JSON local dispatch receipt](../contracts/cli-v1.md),
never account/auth success. Users interact with the official app themselves;
Larp never determines whether a profile is signed in. Engineering tests remain
signed-out, throwaway profiles only; real-account agent testing is unauthorized.

The accepted association boundary is Electron's per-directory `SingletonLock`
symlink, read only with bounded `readlinkat` against the retained checked desktop
directory. Never follow its target, read a cookie/socket target or connect to
vendor IPC. Require matching hostname and positive PID, unchanged lock inode and
timestamps, live same-user process, exact official executable path, strict app
and running-code identity, process birth no later than lock timestamps, and
process-instance identity retained/rechecked through activation. Process names or
PID strings alone are insufficient. This replaces the earlier exact-argv check;
no process argv or environment is read, and doctor does not perform this check.
A missing lock or confirmed dead PID is Inactive; active verified association is
KnownActive; malformed/foreign-host/denied/truncated/changed/unverifiable data is
Unknown and refuses open/delete. A stale symlink is not removed by Larp. Delete
still requires stage04 lifecycle guard and explicit `--yes` human attestation
that externally started sessions are quiescent. Inactivity is bounded hygiene,
not proof against arbitrary clients. Same-user lock forgery and privileged races
are outside this boundary; Larp is not a same-user sandbox.

CORE resolves one coherent v3 snapshot under the registry lock, with expected
revision checked before name resolution. Acquire lifecycle guard before registry
lock and retain it through dispatch/focus. Retain checked app/root/container/cli/
desktop handles; revalidate immutable ID, provider, active state, desktop surface,
binding/version, revision and pending journal immediately before each effect.
Never hold the registry lock across native waits. Never focus a default or
unrelated process; never kill vendor processes. No background manager, automatic
switching/retry, vendor HTTP calls, or HOME/default-data rewrites.

MAC implements native synthetic `desktop_contract` tests (execute
`cargo test --workspace --locked --test desktop_contract`, zero cases fails):
validated fixture binding, both directory inputs, unsupported/Claude admission, synthetic
launched/focused receipt, stale binding/profile and wrong PID refusal, no auth
strings/output, safe cleanup. Native app qualification is a distinct later
finite authorization sheet: both launch orders, simultaneous windows, identity/
history, callbacks, refresh/logout, restart/update, keychain denial, singleton
focus, CLI↔desktop auth effects and no global writes. Unfilled account/isolation combinations remain unqualified; local desktop
dispatch was the explicitly authorized exception above, now paused pending attribution. Signed-out test app data only as RULES authorizes, fresh throwaway
homes, quit and delete all fixtures. Real-account work remains unauthorized.
Windows desktop is later proof, Linux has no claimed official desktop support.
