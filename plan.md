# Larp CC plan

Baseline: `14e299f` (Cargo workspace, placeholder `larp` binary). Plan adopted 2026-11-11 from the master draft. The section "Changes from the master draft" lists every material edit. Detailed stage contracts live in `docs/stages/`. Stage 1: [`docs/stages/01-profiles.md`](docs/stages/01-profiles.md).

## Product boundary

The MVP is a local Rust launcher. It selects an explicitly named, authorized Claude Code or Codex profile and gives each profile a stable, distinct, absolute state directory. Each provider uses one unmodified official installation. Login, status and logout use the provider's native commands. Optional `claude1`, `claude2`, `codex1`, `codex2` launchers are included. Ordinary `claude` and `codex` commands keep working unchanged.

Out of scope for the MVP:
- default-account switching
- reading, inspecting or copying credentials
- subscription pooling, request proxies, quota rotation
- a required daemon or background polling
- migrating existing accounts
- installing or updating providers

Users authenticate only through official clients. Profile directories are organizational boundaries. They are not sandboxes against other programs that run as the same OS user. Nothing here guarantees that a provider will not restrict an account.

## Completion

The MVP is complete when all of the following hold:
1. Stages 1–5 are merged.
2. Final-head synthetic and platform gates are green.
3. Release archives are reproducible from the accepted tag.
4. The authorized native two-account matrix has passed for every advertised provider/OS/backend combination.

Until that real-account authorization exists, the result is an engineering-complete candidate. It is not validated account-isolation support. Unsupported combinations are listed explicitly. Dropping Windows entirely, or dropping either provider, fails the cross-platform MVP goal. WSL is a separate Linux environment and is not Windows evidence. Production distribution, skills/MCP sharing, desktop, menu bar and ACP are separately gated tracks. They are not hidden MVP requirements, and they cannot enlarge the MVP boundary.

## Evidence baseline (2026-11-11)

Each claim below carries its evidence level. Source inspection is not runtime proof. Record exact versions with every support claim.

- **Claude Code native auth**: official docs scope `.credentials.json` and the macOS Keychain entry by `CLAUDE_CONFIG_DIR`. The exception is keyless Console sign-ins, whose state lives outside that directory. Larp rejects that mode for multi-account support. Installed Claude version: unknown (its version probe exited 1). Latest release seen: 2.1.322.
- **Codex native auth**: in inspected releases (0.167.0 at runtime, 0.167.1 at source), the keyring account is derived from a SHA-256 hash of the canonical home. Distinct real homes are separate. Symlinks to one canonical home are not.
- **MCP OAuth**: native auth isolation does **not** imply MCP OAuth isolation. Codex direct-keyring MCP slots are keyed by server name and URL, not by home. File-mode logout can still delete keyring entries. Claude's MCP OAuth scope is unknown.
- **Skills**: Codex `~/.agents/skills` and project/admin scopes survive a changed `CODEX_HOME`. Claude personal skills move with `CLAUDE_CONFIG_DIR`. Claude project and managed scopes do not move.
- **Desktop/ACP**: no runtime evidence. Desktop isolation and ACP subscription eligibility are unresolved. See the gated tracks.

## Delivery rules

- **Stage contract first.** Before any developer starts a stage, the stage specialist writes `docs/stages/NN-<name>.md`. It covers commands, schema, ownership, error cases and an executable acceptance matrix. A planning PR carries it.
- **Branches.** Use `stage/<n>-<slice>` from accepted `main`. A dependent sub-branch declares its exact parent SHA. Each concurrent writer gets its own lead-allocated worktree. If isolation is unavailable, serialize the work.
- **Pipeline.** Stage planner → developers (separate visible worktree threads) → independent testers on the integrated head → open-code-review (OCR) delegate reviewers → fix cycles → fresh final-head evidence → the **main lead merges**. OCR selects files and rules; the reviewers do the review and report per-file coverage. OCR's model pipeline is never used, and nobody reports "OCR review passed". Reviews prioritize simplification, correctness, efficient data structures, performance and security.
- **Invalidation.** A fix invalidates the affected checks and reviews. Integration and conflict-resolution commits need their own final-head checks and review. Workers never merge and never enable auto-merge.
- **Git identity.** Commits use the existing human Git identity. Human co-authors are kept. Preserve human co-authors; omit AI co-author trailers and generated-by signatures.
- **Handoffs.** Every handoff is at most 10 lines: artifact paths, branch/base/head SHA, checks, review coverage, blockers. Artifacts never contain secrets, auth transcripts or account identifiers.
- **Separate authorization** is required for: real login/refresh/logout, live MCP OAuth, desktop app launches, destructive operations, new spending, production deployment.
- **Code shape.** Everything starts in the existing `larpcc-cli` crate, with modules `cli`, `registry`, `provider`, `launch`, `aliases` added only by the stage that needs them. A shared library is extracted only when a second shipped consumer exists. No async runtime, empty crates, plugin framework or ACP dependency. Each new dependency is justified against `std` in its stage contract. `Cargo.lock` is committed and the MSRV is declared.
- **README.** Every commit's README describes only behavior present at that commit. Plans live here and in `docs/stages/`.
- **Later workers** (subject to admission/quota policy): Codex/Codex2 GPT 6.1 Sol high, or Claude Opus 5.5 high.

## Cross-stage conventions (frozen in stage 1)

- **Command namespace.** `larp cc …`. There is no implicit or default profile. Each stage freezes only the commands it implements. The proposed later grammar is `larp cc run NAME -- ARGS`, `larp cc login|status|logout NAME`, `larp cc alias install|list|remove`. It is a proposal until its stage contract freezes it.
- **Exit codes.** 0 = success or explicitly requested help. Missing-subcommand help goes to stderr and exits 2. 2 = usage error, including a missing confirmation flag. 1 = local operational failure. A launched provider's exit code is passed through (stage 2).
- **Error format.** Errors go to stderr as `error: …`, optionally followed by `hint: …` lines; clap missing-subcommand help is the exit-2 exception, with no `error:` prefix.
- **Profiles.**
  - The immutable ID is a number and is never reused.
  - The name is a mutable unique label (`[a-z][a-z0-9_-]{0,31}`, Windows device names reserved).
  - The provider is an enum.
  - The state directory is `<data root>/profiles/<id>`. Renaming never moves it.
- **Data root.** `LARPCC_HOME`, otherwise the OS application-data directory. It is independent of provider homes.
- **No reads of provider state.** Larp never reads provider auth, config or session content. Provider output stays on the terminal and is never logged by Larp.

## Stage sequence

| Stage | Depends on | Delivers | Gate summary |
|---|---|---|---|
| 1 Profiles and contracts | planning PR | registry v1, `profile add/list/show/rename/forget`, help, baseline 3-OS CI | synthetic/platform tests, review, README fidelity |
| 2 Native launch | 1 | provider binding (registry v2), `run`, LaunchPlan, Unix exec, Windows wait, Windows ACL hardening | OS-native process tests on Linux/macOS/Windows |
| 3 Auth qualification | 2 | `login/status/logout`, confirmation and conflict policy | synthetic gate for code; real-account gate for release qualification |
| 4 Aliases | 2, 3 | alias install/list/remove, manifest (registry v3) | per-OS alias lifecycle plus account-safety regression |
| 5 Cross-platform release | 1–4 | packaging, target matrix, reproducible archives | complete advertised account matrix, reproducible archives |

## Stage 1 — Profiles registry and contracts

**Prerequisite:** this planning PR is merged.

**Deliverable:** a metadata-only profile registry plus `larp cc profile add|list|show|rename|forget` with help. It creates empty state directories (Unix owner/mode checked; Windows privacy unverified until stage 2). It reads no provider files and runs no provider. Baseline CI runs fmt, clippy and tests on native Linux, macOS and Windows runners, plus an MSRV build. Registry schema v1 holds profiles only. Provider bindings and alias records arrive with stages 2 and 4 as schema bumps with read-upgrade tests.

**Tests:** CRUD and restart stability; rename stability; malformed and newer-schema files left untouched; permissions; symlink/junction rejection; concurrent multi-process mutations; kill-during-write recovery; path traversal; Unicode and spaces; no writes outside the data root; no provider execution.

**Ownership:** two developer branches, `stage/1-registry` (core) and `stage/1-cli` (CLI, help, fixtures, e2e, CI). They share one contract seed commit and follow a contract-change rule. Independent testers and reviewers are arranged by the lead.

**Merge gate:** the definition of done in the stage doc, evidenced on the final integrated head.

## Stage 2 — Explicit native launch and terminal lifecycle

**Prerequisite:** stage 1 types and paths.

**Deliverable:** `run`, provider binding and resolution, and a pure `LaunchPlan { executable, argv, cwd, env_delta }`.
- **Binding.** One absolute official executable is bound per provider; users install it themselves. Larp never searches aliases recursively and never auto-downloads. A changed path needs explicit rebinding. Larp rejects its own managed launchers, missing or non-executable paths, and unsupported script shims. On Windows the MVP targets native official `.exe` files; `.cmd`/`.bat` support needs a separately reviewed quoting contract.
- **Child environment.** Only the selected `CLAUDE_CONFIG_DIR` or `CODEX_HOME` is set in the child. CWD, terminal and unrelated environment are preserved.
- **Conflict rejection.** Larp rejects, before launch, inherited profile overrides, provider API/token/routing overrides, and known home/backend flags. Errors name the variables, never their values. Version-specific allow/deny rules cover alternate auth and routing modes. Unsupported override paths fail closed and tell the user to run the official client directly. Enterprise policy is not stripped, and arbitrary provider configuration is not claimed to be sandboxed.
- **Support scope.** Only fresh managed profiles are supported. No credential-store inspection. Version diagnostics use documented version interfaces and never make model requests.
- **Unix.** `exec` with no resident launcher; exec errors keep useful diagnostics.
- **Windows.** Inherited console and std handles; waits for the child without building shell strings. Propagates exit codes, including unsigned native values. Handles Ctrl-C/Break once. Defines parent-close behavior. No process-tree killing without a tested policy.
- **Entry criterion carried from stage 1:** before any Windows provider launch or auth operation, verify ownership and safe ACLs (harden where safe, otherwise refuse) across the entire managed metadata tree: root, all parents within root, registry, lock, transient metadata and every profile home. Refuse unsafe or unverifiable roots, including alternate `LARPCC_HOME` roots; securing only the leaf homes is insufficient. Stage 1 makes no verified-private-directory claim on Windows. Synthetic broad-ACL fixtures for every path class must prove refusal before provider spawn.

**Tests:** a fake provider asserts:
- exact argv (spaces, quotes, metacharacters, empty arguments, Unicode)
- CWD and environment, four distinct homes, no mutation of the parent
- conflicts rejected before spawn
- no fallback binary or profile
- Unix: PID, exit status, signals, TTY
- Windows: Ctrl-C/Break, console close, redirected IO, nonzero exits, no orphan on failed spawn

Fixtures never invoke installed providers and contain no credentials.

**Ownership:** B-plan (resolver/policy), B-unix and B-windows (execution against one LaunchPlan), an independent terminal tester.

**Merge gate:** OS-native process tests on all three OSes. No shell-shim support is claimed without tests.

## Stage 3 — Native authentication and account-safety qualification

**Prerequisite:** stage 2. Freeze the exact CLI versions, backend modes and native command table first.

**Deliverable:**
- **Command mapping.** `login/status/logout` use the same LaunchPlan as `run`. Expected mapping: Claude `auth login|status|logout`; Codex `login`, `login status`, `logout`. Verify the mapping per supported version. Unknown versions get an explicit "unverified" diagnostic; native flags are never guessed. Refresh happens only through the client's supported lifecycle.
- **Confirmation.** Auth-changing commands need explicit selection and interactive confirmation. They fail closed in non-interactive use unless a deliberate confirmation flag is given. No logout-all.
- **What Larp does not record.** No email, no tokens, no background status polling.
- **Live sessions.** Cooperative auth-operation locks. Unix `exec` cannot exclude independently running official clients, so users must quiesce sessions first. Larp does not claim otherwise.

**Tests:**
- **Fake clients:** command mapping, home and binary equivalence, cancellation and denial, native errors, no credential paths opened, conflict precedence, no automatic login.
- **Human-run matrix (separately authorized):** two disposable identities; both login orders; restart; concurrent sessions; refresh A while B stays valid; logout A while B stays valid; keychain denial and fallback; ordinary global commands unchanged. Repeat per advertised OS/backend/version. No model turn without separate authorization. Store only redacted pass/fail/version evidence. Identity is checked via native status, never by reading credentials. Keyless Console is excluded. An unknown result blocks the support claim.

**Ownership:** C-auth, C-policy, an independent C-qualification tester.

**Merge gate:** the synthetic gate covers code. The real-account gate separately controls release qualification. Without authorization the status is "pending", never an inferred pass.

## Stage 4 — Recognizable aliases and owned cleanup

**Prerequisite:** stage 2 and stage 3 launch contracts.

**Deliverable:**
- **Install.** Explicit installation of `claude1`, `claude2`, `codex1`, `codex2` mapped to profile IDs. Custom names pass the same validation.
- **Mechanism.** The smallest cross-platform option is a copy of the Larp launcher named for the alias. It dispatches by invocation name through the registry. Provider binaries are never copied.
- **Compatibility.** The alias ABI and version are pinned; an incompatible alias is reinstalled.
- **Registry root.** Alternate registry roots are frozen so an alias can never silently select another registry.
- **No shell edits.** No shell rc or PATH edits. Users get PATH placement guidance and collision checks.
- **Manifest.** The manifest (registry v3) records exact path, file identity, hash and profile ID.
- **Install safety.** Refuses overwrites, shadowing, traversal and reserved names. Install is transactional, and crash and retry are safe.
- **Removal.** Removes only explicitly selected, unmodified, owned artifacts. Modified or moved files need manual handling.
- **Never deleted.** Provider directories, official binaries and user shell files.

**Tests:** all four aliases launch the exact profile and argv. Also covered:
- PATH collisions, spaces and Unicode, duplicate names
- stale or moved launcher, renamed profile, missing profile
- interrupted install, concurrent installs
- changed-file uninstall refusal, forged manifest, path escape
- provider state untouched by uninstall

**Ownership:** D-alias, D-platform, independent D-tests.

**Merge gate:** per-OS alias lifecycle plus an account-safety regression. The README documents installed behavior only.

## Stage 5 — Candidate packaging and finite MVP release gate

**Prerequisite:** stages 1–4 integrated.

**Deliverable:**
- **CI.** Stage 1's CI is extended with release builds and a frozen x86_64/aarch64 target matrix. Platform behavior is tested on native runners; cross-compilation alone is insufficient.
- **Archives.** They contain the launcher, license notices, usage docs and checksums. Installation is manual; providers are not bundled and auth is not provisioned.
- **Records.** Provenance, dependency licenses and a security reporting route are recorded. Signing or notarization that needs credentials or spending is gated and never faked.

**Tests:**
- clean offline build after cache preparation
- fresh-user archive install, help, profile, fake launch, alias, uninstall
- no absolute build paths or private data in archives
- locked-source checksum verification
- registry upgrade roundtrip and refusal to roll back an incompatible schema
- cold startup, binary size and Windows wrapper RSS measured before any numeric claim

A traceability table maps every MVP requirement to its evidence and lists every unsupported combination.

**Ownership:** E-ci/platform packaging, E-docs, an independent E-release tester. The lead serializes shared workflow edits.

**Merge/release gate:** fresh final-head review, the complete advertised native-account matrix, reproducible archives, and a README describing shipped behavior. Tagging and publication need scoped authorization. Production deployment and new spending stay separate. Stage 5 closes the MVP.

## Separately gated tracks

Each track needs its own explicit approval and its own stage contract. Nothing in the MVP merge approval implies any of them.

- **Production distribution** (after stage 5; needs distribution scope plus approval for credentials, spending or deployment).
  - Scope: signing/notarization/provenance, update channels, schema migration/recovery, a versioned support matrix, vulnerability/dependency response, rollback runbooks. No automatic provider updates. No telemetry.
  - Tests: tampered-archive rejection, clean-machine install, interrupted upgrade, old/new registry compatibility, retained permissions, aliases upgraded without overwriting user edits, a provider-version canary matrix.
  - Unsigned candidates are never called production-qualified.
- **Skills and MCP definition sharing** (after the MVP plus an approved sharing contract).
  - Scope: a frozen redacted plan/apply transaction API. Explicit allowlisted snapshots, no live symlinks.
  - Never shared: auth files, histories, trust decisions, approvals, secret values. Header and env literals are rejected unless they are reviewed non-secret fields.
  - Safety: revision checks, atomic updates, rollback. Scripts and servers are never run.
  - Tests: traversal, symlink escape, portability rejects, scope collisions, concurrent edits, secret exclusion, crash rollback.
  - Known limits: Codex global skills stay visible across profiles. Claude project/managed scopes persist. Codex MCP direct slots cross homes for the same name and URL, so use distinct per-profile server names. File-mode logout can still delete shared keyring entries. Claude MCP OAuth scope is unknown.
  - Definition-only sharing can merge after synthetic review. Live OAuth support also needs authorized isolated-backend lifecycle tests (refresh, logout, fallback). No blanket MCP isolation claim.
- **Desktop compatibility, then native launch** (needs approved OS/app/version scope and app-launch authorization).
  - First freeze the CLI+GUI data-directory and callback/singleton contract. Only unmodified stock apps; no cloning or re-signing for distinct icons.
  - Tests: both launch orders, simultaneous windows, identity, history, OAuth callback routing, refresh/logout, restart/update, keychain denial, singleton forwarding.
  - Only verified combinations merge; failed or unknown ones stay disabled. Separate data directories do not prove runtime isolation. Native account menus or separate OS users are documented alternatives.
- **Menu bar UI** (needs the accepted profile API and scoped UI requirements; desktop actions also need the desktop track).
  - Scope: explicit account selection, launch and status shortcuts, keyboard and accessibility support. No token access or quota polling.
  - Tests: stale labels, concurrent registry updates, cancellation, logout confirmation, lifecycle and resource use, OS permissions, uninstall.
  - Signing and distribution are gated separately. A shared core is extracted only for real reuse. Linux/Windows UIs only on concrete requirements.
- **ACP integration** (needs distinct approval and resolved distribution/subscription eligibility).
  - Scope: adapters add runtime/SDK processes and are never MVP dependencies. One immutable profile per adapter process. Per-session identity overrides are disallowed.
  - Tests: login/session home equivalence, reconnect, permission/cancellation, orphan cleanup, bounded memory, account/backend separation under separately authorized accounts.
  - Open eligibility questions: Claude's adapter is Agent-SDK based, and SDK subscription login for third-party products needs approval. Codex app-server guidance excludes commercial/hosted use. Commercial, hosted or telemetry use is a new scope decision.

## Changes from the master draft

1. Stages renumbered 1–5 (were A–E). The P/F/G/H/I tracks are now named gated tracks and do not imply an order after stage 5.
2. Stage 1 registry v1 holds profiles only. Provider bindings move to stage 2 (schema v2) and alias records to stage 4 (schema v3), each with read-upgrade tests. This avoids unused fields.
3. The state directory is derived (`<root>/profiles/<id>`) instead of stored as an absolute path. The registry then contains no paths, so non-UTF-8 roots work and two sources of truth cannot disagree. Existing-directory adoption stays deferred.
4. Windows ownership and safe ACL verification for the entire managed metadata tree moves from stage 1 to a stage 2 entry criterion. Stage 1 writes no credentials, and stage 2 is the first stage that lets a provider write into a profile directory. Stage 1 on Windows still rejects reparse points.
5. Baseline 3-OS CI (fmt, clippy, tests, MSRV) moves into stage 1. Stage 2's gate already required native Linux/macOS/Windows tests, which contradicted CI arriving in stage 5. The user approves native three-OS CI on standard public-repository runners with read-only permissions. This is no longer an approval blocker. No paid runners, secrets, artifact uploads, caches or custom images; broader spending remains separately gated.
6. The stage 1 ownership split follows the lead's two-developer requirement: registry/core and CLI/help/fixtures. Independent testing is a lead-arranged role, not a developer branch. The stage 1 README update belongs to the CLI slice and is verified at the final head.
7. `forget` "confirms metadata effect" is made concrete: `--yes` is required, and without it the command exits 2 and prints what would happen.
8. Stage 1 freezes only the commands it implements plus the cross-stage conventions. Later commands are frozen by their own stage contracts.
