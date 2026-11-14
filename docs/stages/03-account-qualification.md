# Stage 3 — Account qualification contract

Planning contract; no native operation or account support is enabled by this document.

## 1. Baseline and authority

The prepared checkout was clean at merged M3 `022fb05278c633e849a5d7f44a5c67855ebe1cfc`, tree `5967b864c4219039872fccacc74870cb35152e4c`, equal to accepted candidate `433a6dd5c31e79f52e8d89b8863c9366d9d0850d`. PRs 5/6 are merged. Retained Stage 2 reports record final QA ACCEPT (47 IDs), independent OCR delegate review ACCEPT (54/54: four fresh, 50 inherited), actual M1 migration and native L07 PASS. These are inherited evidence, not tests rerun for this planning PR. Non-elevated Windows, real Close/Logoff/Shutdown, unsupported volume/ACL classes and macOS x86 ABI remain unqualified. The testability review supplied complete durable findings but its final provider turn failed quota; this contract claims no terminal QA ACCEPT from that task.

This contract prospectively supersedes only the Stage 2 ordinary launch gate, four-byte executable-read limit, synthetic-only launch wiring and provisional argument policy where explicitly stated below. [Stage 2](02-native-launch.md) otherwise remains in force. Its historical evidence is unchanged. Registry v2, managed-tree checks, native binding validation, no permission repair and metadata diagnostics remain intact.

Routine credential-free engineering is already authorized. Independent technical planning review and lead planning merge precede implementation. Separate reviewed artifact-operation rows and concrete human approvals govern installed-provider discovery/execution and accounts. No installed-provider command, even version/help/status, is authorized here. No installs, production deployment, spending, credential/config/keyring reads, screenshots or account creation. Main lead alone communicates approvals and merges; workers own visible PR worktree threads and never merge/watch.

## 2. Product and shipped text

Keep one existing binary crate, explicit profile selection, one bound unmodified official native installation per provider and stable `<root>/profiles/<immutable-id>` homes. Set only `CLAUDE_CONFIG_DIR` or `CODEX_HOME`; preserve cwd, terminal and unrelated environment. Normal repositories and normal native configuration remain required. No config-absence/nonrepository MVP, HOME/USERPROFILE/XDG/APPDATA rewrite, endpoint sandbox, vendor parser, SDK auth, credential transport, daemon manager, background monitor, fallback account/backend/path, rotation or retry.

Both providers and native Windows remain MVP goals. Select the first eligible official native release on the chosen OS; exact release/path/backend/flags are TBD row inputs, not presumed installed/latest 0.167.0 facts. Claude keyless Console, desktop/ACP and integration credentials are outside subscription qualification. Source research alone proves neither distribution identity nor runtime account separation.

Static operation help must contain: `Profiles select native state, not a verified account, model or endpoint. Native project, configuration and administrator policy still apply. Provider restrictions remain possible.` Auth help additionally contains: `Quiesce relevant sessions before authentication changes. Refresh and logout may affect grants and integrations.` Emit that same routing notice once to stderr before each admitted child; add the auth notice for login/status/logout. No account-derived health/verified-account label. No repeated run confirmation. Provider output for **all** operations passes directly to inherited human terminal/stdio: no capture, log, tee, archive, parsing or redaction promise. Human observation starts before sensitive official flows; secrets/codes stay in the official human-operated flow and out of agent transcripts. No zero-ban promise.

## 3. CLI, precedence and errors

```text
larp cc run NAME [-- ARGS...]
larp cc login NAME [--yes]
larp cc status NAME
larp cc logout NAME [--yes]
```

Existing profile/provider bind/list commands retain semantics. No qualify/enroll command, sidecar, schema, enrollment API, status persistence or qualification column. Binding `ok` still means path validation only. No default profile, logout-all, refresh command or arbitrary provider command.

Only the first embedded interactive run row may admit zero forwarded tokens: `run NAME` and `run NAME --` are equivalent. Every nonempty forwarded vector refuses without echo, including prompts, another delimiter and all flags. Later grammar requires a reviewed contract/row amendment. Login/status/logout accept only the Larp grammar above; exact native command vectors are row data, never guessed from another provider/version/operation. `--yes` skips this invocation's confirmation only; it grants no account-operation authorization.

Precedence (first failure wins):

1. Clap parse/help/version, including valid profile-name syntax. Help/version exit 0 locally; usage exit 2. No managed IO.
2. Empty compiled table for the requested operation, or no matching build OS/arch operation candidate: QualificationPending immediately, before root lookup, environment enumeration, metadata or provider access. A table with unrelated rows cannot open this gate.
3. Locate/validate root and fixed managed metadata; load one strict bounded registry snapshot, select profile, then check whether its provider has a candidate operation row. No provider candidate refuses before provider-home inspection/hash. Preserve existing registry/path error order within each stage.
4. Whole registered-home set safety, selected-home presence/distinct identity, binding and executable validation; bounded executable digest and exact artifact/operation match. Missing match refuses before native-specific metadata IO.
5. Row environment-name conflicts, then forwarded-argument grammar, then exact row native metadata/backend/bootstrap guards. Never read provider contents.
6. For login/status/logout acquire the per-ID cooperative guard; login/logout confirmation follows. No registry transaction during either human input or child lifetime.
7. Revalidate snapshot revision/profile ID/home/binding, checked executable identity/size/digest and row policy; reject any change, then dispatch the exact plan. No retry/mixed snapshot. Release registry guards before process creation; retain path safety handles as applicable. Path-reopen/same-user races after checks remain explicit.

Local errors leave stdout empty and exit 1, except missing non-TTY confirmation (2). Each message is prefixed `error: ` and ends newline. Freeze these new fixtures; existing Stage 2 error escaping, fixed argument/environment labels and launch/wait errors remain:

| Error | Literal message | Hint (omit when empty) |
|---|---|---|
| QualificationPending | `provider operation is disabled for this combination` | `review the documented qualification matrix` |
| ArtifactInvalid | `provider installation qualification is invalid` | `use an already reviewed official artifact` |
| NamespaceUnverified | `profile subscription namespace cannot be qualified` | `review the supported backend and native policy conditions` |
| AuthBusy | `profile authentication operation is busy` | `quiesce the selected profile before retrying` |
| AdmissionChanged | `launch context changed during admission` | `retry after metadata changes have finished` |
| ConfirmationRequired | `authentication operation requires confirmation` | `use a terminal or explicitly pass --yes` |
| ConfirmationDeclined | `authentication operation cancelled` | |

Login/logout prompt on stderr shows escaped operation, label, immutable ID, provider and selected home, followed by `Native state and grants may change. Continue? Type yes: `. Read one line only, maximum 16 bytes; only `yes` followed by LF or CRLF accepts. No/EOF/overlong/input error cancels before child. Require terminal stdin and stderr unless `--yes`; no raw supplied text in errors. Status has no confirmation but remains native/account access and uses the same guard. Native status exit 0 is not remote validity. Preserve provider exit/signal behavior, including full Windows u32; do not truncate through Rust `ExitCode::from(u8)`.

## 4. Private implementation surface and executable bytes

Reuse `Registry::launch_target`, provider validation, platform checks and LaunchPlan transport; do not duplicate the security walk. M3 `LaunchTarget`/method/export are feature-gated, `main.rs` does not compile launch, and `launch::execute` only rechecks executable identity. The current returned target retains neither snapshot guards nor an executable handle. Explicitly ungate safe production primitives and wire ordinary dispatch; helpers/fakes remain nonpackaged. Do not accidentally expose the test driver through ordinary all-features CLI.

Freeze these crate-private responsibilities (Rust field representation can follow existing types; no public library):

```text
Operation = Run | Login | Status | Logout
ArtifactRow = provider, exact release/build, SHA256, install kind, OS/arch,
              operation, interface, backend/fallback, policy revision,
              fixed argv, env-name policy, native path rules, evidence references
AdmissionSnapshot = registry revision, selected profile ID/name/provider,
                    stored/resolved executable + identity, home identities
CheckedArtifact = owned verified File + identity + size + SHA256
Admitted = snapshot + exact row reference + checked artifact + LaunchPlan
admit(registry, request, rows, env names) -> Admitted or Error
revalidate(admitted) -> unchanged or Error
confirm(operation, profile, input) -> accepted or Error
acquire_auth_guard(root, immutable ID) -> owned noninheritable guard or Error
dispatch(admitted, optional auth guard) -> native status or Error
```

`rows` is a private constant empty slice in ordinary builds. A separate synthetic entrypoint supplies invented rows to these **same** admission/revalidation/dispatch functions. No ordinary CLI, environment, filename, feature-marker or permissive verifier unlock. A test seam may select deterministic failure/race hooks only in the nonpackaged harness, never return successful safety validation without checking fixtures. Existing Stage 2 `prompt` grammar remains confined to legacy transport fixtures or is deliberately migrated; it is not Stage 3 grammar.

Narrow read-rule amendment: read executable bytes only, from a verified native executable handle, streaming SHA-256 with a 512 MiB maximum **actual bytes read**, bounded buffer, no subprocess/version probe. Reject oversize initial stat; detect excess without consuming an unbounded stream. Check handle identity/type/security/size before and after, compare total bytes with stable size, and check current path identity and a fresh bounded digest immediately before dispatch. Unreadable/changing/oversized/mismatched bytes refuse. No hash cache/persistence. Same bytes at a new identity may qualify after all fresh path checks; changed/revoked bytes do not. Use one maintained MSRV-1.89-compatible SHA-256 dependency if necessary (core justifies exact pin/lock against existing dependencies); no custom crypto or unnecessary dependency.

Digest equality discriminates reviewed bytes; it does not prove official provenance, loaded assets/dependencies, account eligibility or identity-pinned execution. Each row must link official distribution bytes/checksum/signature where available to exact source/release/install format, document unavailable provenance and load/update limits, and pass native qualification. Shell/JS shims, auto-resolved package targets and copied providers remain rejected. Atomic updater/rebind/self-update must requalify bytes on next invocation; old offline Larp cannot learn future row revocations. Rollback is reviewed table removal/closed build, never credential downgrade/copy or native cleanup by implication.

## 5. Finite native row inventory — empty until reviewed

There are **zero enabled rows and zero native path/env/argv inventories** at freeze, for either provider or any OS. This is a concrete closed data contract, not permission for developers to fill gaps by inference. Parallel Codex/Claude evidence artifacts, when routed by the lead, are inputs to later reviewed row amendments; their absence does not delay closed-table engineering or establish facts.

For each operation, the row amendment must fill this exact finite inventory (no wildcard settings scan):

| Row sheet field | Required content |
|---|---|
| Artifact/operation | Exact official release/build/digest/install assets/OS/arch; subscription interface/backend/fallback; operation; source pin/file/symbol and distribution/native evidence |
| argv | Literal ordered tokens, including root/subcommand placement; bootstrap, status, refresh/logout coverage; no borrowing run-only daemon flags for auth |
| env | Exact conflicting names and explicitly justified prefixes/exceptions; selected-home name; presence including empty refuses; Unix byte-exact/Windows ASCII case-insensitive; unrelated non-Unicode values preserved |
| stores and policy | Every subscription read/write/bootstrap/refresh/logout store and controlling setting, precedence/admin requirements and fallback; how safe conditions are established without content reads |
| each native path | Exact derivation from verified home or literal OS path; operations; object kind (regular file/directory/socket/etc.); absent/present/error disposition; no-follow component policy; owner/privacy/link-count requirements; upstream provenance and purpose (alias/backend/attachment risk) |
| daemon branch | Attach/spawn/discovery behavior and exclusion proof; omit sentinel only with exact source **and authorized native** evidence of embedded bypass; observations-only paths belong to B/C sheets |
| lifecycle | Native guard/store interactions, terminal and abnormal-exit limits; evidence and closed dependencies |

Only explicitly reviewed fixed-path **metadata** checks may be added by such an amendment. Check all ancestry without following symlink/reparse/alias paths; access denied/unknown is never absence. Present credential/commit regular files require trusted private ownership and Unix link-count one; directories/sockets need their own correct object-kind rule. No auth/config/session content, credential hash, readlink target read, directory enumeration, pid parsing, socket connection, keyring query, cleanup or permission repair. Retain restrictive managed-storage exclusions unless exact per-operation proof justifies narrowing; do not reject ordinary config merely because it exists. If controls are unknown or cannot be verified under these rules, affected rows stay closed. No safety waiver.

## 6. Cooperative guard and native lifecycle

Larp-owned coordination only: `<root>/auth-locks/` and `<root>/auth-locks/<decimal immutable ID>.lock`, outside provider homes. Create the directory/file exclusively under verified private parents using existing secure platform creators; existing objects are verify-only, regular single-link files, trusted owner/privacy and no-follow ancestry. Lock files contain no data, are never truncated/unlinked on release/forget, and are not a qualification store. Stable files avoid split-inode locking. Missing directory/file may be created only after successful admission for login/status/logout; this is the sole new coordination write, never enrollment/registry/provider repair. Root/profile must already exist. Fixed-metadata validation must recognize this directory without enumerating provider state. Unsafe paths/error refuse.

Lock order: initial short registry snapshot, release; acquire one exclusive per-ID OS advisory lock nonblocking (busy immediately AuthBusy); confirm if needed; short registry lock for strict snapshot comparison/revalidation, release before child. No code holding registry lock may wait for auth lock; no multi-profile lock acquisition. Renames retain ID/home but a changed snapshot during admission refuses; rebind/forget/revision changes refuse. No registry transaction spans prompt/native lifetime. Run has no auth guard. Different IDs do not serialize. Direct native clients, running run sessions, descendants and in-session auth are outside this cooperative exclusion; humans quiesce them before auth changes.

Auth/status guard owns a noninheritable handle until the direct child is reaped, including spawn failure/cancellation/unwind. Never forcibly unlock/delete stale files: OS lock ownership ends at process exit. Windows uses the existing direct-child console/wait model, noninheritance, Ctrl-C/Break behavior and full-u32 propagation; no tree-kill promise.

Unix run stays exec with existing signal/TTY transport. Unix auth/status needs a narrowly reviewed spawn/wait path: same foreground process group and inherited terminal/stdio/cwd, no shell, setsid, new terminal or background service. Child receives direct terminal input/resize/INT/QUIT/TSTP; parent must not exit/unlock on terminal INT/QUIT before reap, must suspend/resume with foreground job control, and must not double-forward group-delivered signals. Parent-directed TERM/HUP are forwarded once to the known direct child then reaped; interruption retries wait. Native normal exit returns its code; signal death restores disposition and terminates wrapper by that signal after reap/release. Spawn/setup failure unwinds without child/lock leak; post-spawn wait failure cancels the known direct child and reaps before returning a local error. No unknown-tree kill. Pre-spawn cancellation produces no child. After spawn, cancellation follows the signal/reap contract, never unlock-and-abandon.

This specifies required behavior, not approval of an improvised unsafe signal handler/pre-exec wrapper. All signal setup/restore, fork-safety and wait/error paths require code review and native fake/PTY proof before acceptance. If bounded cleanup cannot be established, retain guard until reap and mark the case failed; do not claim success. Uncatchable parent SIGKILL/OS crash can drop the cooperative lock with a surviving child; no stale-file or direct-client guarantee is possible. Qualification must require human quiescence after abnormal exit; affected native rows remain closed unless their guard/store controls and recovery limits are concretely reviewed. No safety waiver or claim the OS lock excludes such survivors.

## 7. Acceptance and meaningful test interfaces

Engineering tests use invented profiles, fake native binaries, private project-disk fixtures and deterministic rendezvous only. Extend existing harness rather than build a framework. No installed providers/network/accounts in CI. Stage 1/2 migration/security/ABI/argv/exit/console regressions remain. Fake IO is evidence of machinery, never native subscription qualification.

| ID | Discriminating engineering evidence |
|---|---|
| A01 | Ordinary debug/release/all-features run/login/status/logout refuse with zero/unrelated operation rows before managed IO/child; corrupt root and forged markers do not unlock. Actual ordinary install/release inventory only contains larp; synthetic helpers cannot ship. |
| A02 | Common admission hashes checked bytes: same-path replacement, same bytes/new identity, changed bytes/same version label, unreadable/oversize/changing stream, actual read cap and final recheck. No version subprocess; path-reopen limit explicit. |
| A03 | Replaces removed sidecar tests: exact artifact/OS/operation/interface/backend/policy match, unrelated rows, strict single registry snapshot, rename/rebind/forget races, no mixed pair or metadata repair; unchanged registry bytes on refusal; no new persistence. |
| A04 | Invented enabled rows exercise empty argv/empty outer delimiter and every nonempty-vector refusal, exact generated vectors and home delta across four operations; env presence/empty/case/non-Unicode tests; cwd/unrelated env/parent untouched. |
| A05 | Invented row paths cover no-follow/hardlink/reparse/ancestry/privacy/access errors and correct per-kind absence rules; sentinel endpoint receives no connection. Pre-provider traces exclude auth/config reads/enumeration/pid reads; distinguish loader/fake IO. |
| A06 | Exact yes/no/EOF/overlong/non-TTY/--yes, busy across every auth/status pairing, different IDs independent, rename/rebind/forget during prompt, noninheritance, spawn failure/unwind/reacquisition; no registry lock during input/wait. |
| A07 | Same ordinary admission/dispatch via nonpackaged invented rows actually enters fake for all operations. Linux/macOS foreground PTY input/resize/INT/QUIT/TERM/HUP/TSTP/continue, native status, reap/cancel/setup/wait failures and no resident wrapper after exit; Windows full-u32/Ctrl-C/Break/direct-child regressions. Record abnormal parent death limits. |
| A08 | Ordinary repository/native-config sentinels untouched by Larp while synthetic namespace conflict refuses; compiled row removal/policy change/update/rebind invalidate with zero fallback or repair. No config-absence-only acceptance. |

Bounds: retain Stage 2 child assertions 10s, PTY/console 20s, known direct-child cleanup/reap 2s and self-bounded grandchildren <=5s in tests. A timeout is failure/pending, never pass. Native Linux/macOS/Windows run applicable cases; Linux MSRV 1.89 runs common/Linux checks. Actual test names/counts/entry markers required; zero filtered tests and elevated-only Windows do not fill missing classes. Unsupported native classes remain explicitly unadvertised. No WSL substitution for Windows.

## 8. Finite B/C qualification sheets and enablement

No B/C sheet is filled/authorized here. Before approval fill **every** actual path, argv, env condition (safe names only), host/OS/arch, account logical label, artifact digest/version/install kind, exact Larp candidate head, backend/fallback/interface/policy, cwd/repository/native-admin context, evidence destination, authorization reference/date, maximum invocations, per-invocation timeout, total observation deadline, permitted network/model action, state effects, success/stop criteria and cleanup. Human nominates the exact official executable; no implicit PATH scan/discovery, version/help or cleanup. No provider install/spending/deployment. Secrets are human-only official flow; no authfiles, identifiers, screenshots or raw transcripts in reports.

| Sheet/case | Finite action and separate evidence |
|---|---|
| B1 identity | Explicitly approved exact-path byte/metadata/provenance inspection; connect official bytes/source/install assets. No execution implied. |
| B2 no-account | Separately listed literal version/help/foreground invocations in named fresh no-account homes, allowed network and bounded duration; prove chosen branch attach/spawn behavior and ordinary repository/config compatibility. No login/status/logout/model action implied. |
| C1 login/persistence | Existing authorized A/B aliases, A then B and B then A, exact official human login flow and required re-logins; local alias match and persisted restart independently recorded. |
| C2 status/restart/concurrency | Exact human-observed status and restart, bounded simultaneous foreground sessions; correct local alias and independent remote grant validity are distinct results. Status zero is insufficient. |
| C3 natural refresh | Each direction, approved validity observation while other alias remains valid; fixed deadline, no forced expiry/token edit/polling/model request by implication. Pending at deadline closes dependent rows. |
| C4 logout | Quiesced A logout with B validity, reverse direction, each specifically necessary re-login/reset listed. Local removal and remote/grant/integration effects recorded separately; cleanup only as approved. |
| C5 global baseline | Optional separately authorized exact global/default operations before/after; absent authorization means not-authorized, never a claim of observed global invariance. |

Each result cell uses pass/fail/pending/not-authorized independently for local identity, persistence, remote validity, refresh, concurrency and global baseline. Missing validity/refresh evidence remains pending; request only the concrete additional action needed later. Stop on wrong alias, unexpected backend/fallback, attach/spawn, namespace/global impact or sensitive-output capture; close the dependent rows, record safe facts, do not copy credentials or improvise recovery. Refresh/logout may revoke grants; account restrictions remain possible.

Static/source and approved native direct-command evidence can be gathered while production table stays closed. There is no public unlock. Reviewed row-enable candidate must then receive authorized affected native acceptance through Larp on its exact head before lead merge. Enabled row requires exact per-operation source/argv/env/path/store/control sheet, official byte linkage, B/C passes for all its dependencies, engineering QA/review and native security/lifecycle evidence. Unknown/pending dependencies keep only the affected rows closed. One Linux/provider success is not the two-provider/native-Windows MVP. Publication remains Stage 5 and separately authorized.

## 9. Seed, exclusive ownership and gates

```text
planning review/lead merge -> S validated closed-table seed
S -> C core + optional W Windows -> I integrated candidate -> independent QA and OCR -> lead merge/equality
planning freeze -> E static evidence/runbook (may overlap engineering)
E + approved B/C + engineering acceptance -> reviewed row amendment/enablement -> affected QA/review -> lead merge/equality
```

| Slice | Exclusive files and handover |
|---|---|
| Planning owner | This contract and plan.md only. Independent review resolves F1–F7/B1–B5 against lead decisions before freeze; no README/source/CI edits here. |
| S then C, same core writer | `crates/larpcc-cli/src/{cli,registry,provider,launch}/**` except launch/windows.rs; main.rs; platform/{mod,unix,macos}.rs; manifests/lock; shared tests/support/fixtures; CI. Sequential seed/core work avoids concurrent shared writes. Later README only for done facts. |
| W optional Windows | Only platform/windows.rs, launch/windows.rs, tests/windows_*.rs and fixtures/windows/**. Starts from exact validated S SHA; requests shared changes from core. Serialize if no independent useful platform delta. |
| E evidence | External artifacts only; no installed-provider discovery or account actions. Lead routes provider evidence to later row amendments; no need to wait for it to build empty core. |
| QA / OCR | Independent artifact-only evidence on the same I SHA, may run concurrently. OCR delegate selects files/rules; reviewer reviews with own model and reports per-file coverage/skips. No OCR model pipeline or “OCR passed.” |
| Lead | Integrates snapshots, resolves scope, arranges native human sheets, merges and verifies merged tree equality; no management-only sub-orchestrator. |

S must freeze these private interfaces, exact CLI/error/precedence/guard paths/lifetimes, empty native inventory, read boundary and invented-row seam before C/W parallel work. It must compile and enforce real closure, no successful verifier stubs/panic/todo/blanket allow. Any incomplete safety helper refuses and is explicitly transitional; none remain at integrated acceptance. Validate actual S fmt, clippy all-targets -D warnings, default/synthetic build/tests, ordinary release/all-features gate, MSRV 1.89 and native Windows/macOS shared-API compile before dependent writers consume S. Record exact local parent SHA, not an assumed branch tip.

Core owns CI coverage for dependent PRs: add `stage/3-core` to pull_request target branches in S before W opens against it; retain main/stage2 triggers and all native/MSRV security/lifecycle checks. Core PR targets main; W targets stage/3-core. Ensure CI uses actual head SHA as M3 does; changed target names require matching workflow change before dependence. No credentials/provider/network tests or paid runners. Optional W merges into core only through lead after integrated-head acceptance; required core checks rerun if integration changes the tree. No temporary unsafe stub lands on main.

**Premerge engineering gate:** exact integrated head, applicable A01–A08 and retained native/MSRV regressions, independent QA and OCR per-file review, resolved blockers, no seed residue, ordinary packaging/closure proof and accurate done-only docs. Changed code invalidates affected evidence; retain unrelated evidence only with explicit unchanged-file mapping. This can finish with zero enabled rows and no account support claim. **Postmerge lead check:** accepted candidate tree equals merged main tree; this is subsequent verification, never a prerequisite for premerge QA. A mismatch needs affected retest/review. Row enablement has the additional §8 gates.

Developer handover records exact branch/base/head/tree/seed, file ownership, implemented/refused surfaces, commands/results and limits. Tester handover maps every A ID to named evidence/result and native execution class; reviewer handover maps every diff file to reviewed/skipped/reason and findings. Compact lead summaries <=10 lines (and always <=15) (full evidence separate); retain old artifacts unchanged. PR owners link immediately after create/every push and stay visible/unsettled until lead merge, using existing human Git identity/coauthors with no AI trailers/signatures.

## 10. Review disposition map

| Input finding | Contract resolution |
|---|---|
| Safety F1 / testability B1 | §§3–5 freeze closed engineering interfaces/inventory; exact native inputs are later reviewed row blockers. |
| Safety F2 / testability B3 | §§3–4 remove qualify/enrollment/sidecar/schema entirely; fresh bytes + registry v2/snapshot revalidation; A03 rewritten. |
| Safety F3 | §5 per-path kinds/purpose/source/absence/error/privacy, no scan; daemon omission requires source and native bypass proof. |
| Safety F4 / testability B4 | §§3,6 explicit per-ID coordination writes/order/confirmation/noninheritance/TTY/signals/reap/abnormal limits; A06–A07 proof, unknown native controls stay closed. |
| Safety F5 | §2 exact static text and all-output human pass-through; routing label never verified account. |
| Safety F6 | §8 bounded sheets and independent finite outcome cells, pending closes dependencies. |
| Safety F7 | §1 merged M3/accepted-tree and retained QA/OCR limits; §2 first eligible release TBD. |
| Testability B2 | §§4,7 same real admission/dispatch via nonpackaged invented rows, not vacuous empty-table-only tests. |
| Testability B5 | §9 seed-owned dependent-target CI and premerge gate separated from lead postmerge equality. |
