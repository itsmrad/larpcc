# Phase 0 threat review — DELETE and MCP SECRETS

Import is **OUT OF SCOPE**, not an unreviewed migration/adoption escape hatch.
Fresh profiles only; v2 migration moves eligible empty/synthetic directories.
Local filesystem only; no same-user/privileged sandbox guarantee. This review
specifies controls and tests, not implementation certification.

| Threat | Required control and discriminating acceptance |
|---|---|
| Traversal / malicious deletion manifest or journal | derive profile folder from immutable validated ID, fixed install roots, relative allowlist; never accept arbitrary cleanup path. Forge ../, absolute, device/UNC and alternate-stream names; outside sentinel remains |
| Symlink/reparse/mount race | retained trusted ancestry, handle-relative no-follow recursive removal, every tag refused/traversed never; leaf unlink only where safe; rename/swap rendezvous preserves outside sentinel. Path check then remove_dir_all is forbidden |
| Concurrent edits / forged ownership | registry/lifecycle lock order, snapshot revision, independent identity+hash validation, durable journal; drift fails before tombstone or leaves recoverable tombstone afterwards. No marker-only deletion |
| Crash before tombstone | no destructive cleanup has begun, snapshot unchanged or safe pending journal blocks consumers |
| Crash after tombstone / reference removal / folder removal / final commit | deterministic idempotent per-phase replay; alias/MCP/skill reference cleanup transaction; reads cannot treat partial state as active; every fsync/rename boundary fault-injected |
| Child survives crashed launcher | cooperative locks do not prove liveness after SIGKILL/OS crash or constrain direct native clients. Human quiescence plus reviewed native checks required, uncertainty refuses; never kill guessed PID or reuse ID |
| A live macOS process has lost its executable path | an initial `ENOENT` triggers a bounded argument check bracketed by process start-time probes. A matching desktop directory or inconclusive probe blocks deletion. Resolve a nonmatching absolute directory argument before clearing it: symlinks, `/var` aliases and trailing slashes cannot hide a match; failed resolution refuses. This check depends on the process retaining the profile argument and cannot establish quiescence for arbitrary same-user clients |
| Permission/access/query failure | preserve existing Unix 0700/0600 + ancestry and Windows protected ACL/no-reparse gates, no automatic repair, no unsafe recursive fallback |
| Secrets in args / URL / env / headers | reference-only supported fields; reject arbitrary literal values and ambiguous command args; reviewed adapter allowlist, no regex-only safety claim. Invented secrets in each input must not reach catalog/render/stdout/stderr/errors |
| Reference leakage / keychain reads | identifiers only, no keychain lookup or env-value interpolation, unsupported native reference syntax refuses. Keychain item names are not evidence of auth; no listing credentials |
| Error disclosure | fixed safe code/message, no raw argv/provider/config/OS text; unexpected version probe output discarded, not logged |
| Render tampering / drift / resource exhaustion | pure bounded plans, per-profile namespace, hashes+identity, exact revision, journals and explicit recovery; never overwrite changed bytes. 1 MiB catalog/render, 16 MiB total, bounded counts; overflow no partial commit |
| Shared OAuth / config scopes | no OAuth isolation claim, no MCP server execution. Per-ID names reduce known Codex collision but need native lifecycle proof. Global/project skills and admin config remain independent scopes |

Test IDs and commands are stage04_delete_escape/recovery, stage07_secrets/drift/
recovery and stage08_doctor. Native Linux/macOS/Windows required for native
confinement claims; unavailable handle-relative safety stays refused. Abnormal
exit and same-user interference limits must appear in future shipped docs, not
be hidden by a passing synthetic test. Reviewers independently check actual
implementation and per-file coverage before merge; no review pass claimed here.

Stale GUI intent: require `--expected-revision` on GUI mutation/open requests,
checked under the resolution lock before effects. Name reuse after rename must
fail with conflict; internal post-resolution revalidation alone is insufficient.

MCP command admission checks each path component on add, catalog load and
journal replay. Case-insensitive markers are `sk-` (including `sk-ant-`), `sk_`,
`ghp_`, `gho_`, `ghs_`, `ghu_`, `ghr_`, `github_pat_`, `glpat-`,
`xoxa-`, `xoxb-`, `xoxp-`, `xoxr-`, `xoxs-`, `-----begin`, and the
standalone word `Bearer`. These markers and JWTs are checked at the start
of a component and after any character outside Unicode alphanumerics, including a
backslash in a Unix filename. JWTs start with `eyJ` and have three
dot-separated base64url segments of at least eight characters each.
Fixed-width key shapes are also refused within any path component, including when
embedded in a word. These are uppercase `AKIA` or `ASIA` followed by 16
uppercase letters or digits, and `AIza` followed by 35 base64url characters.
The mixed-case check considers runs of ASCII letters, digits and `+`, split
at all other characters, including hyphens, underscores and dots. It refuses a run of at
least 20 characters that contains both letter cases and a digit. A mixed-case
run of the same length followed by base64 padding (`=`) is also refused.
Standard 8-4-4-4-12 hex GUIDs and lowercase hex and Nix base32 hashes can be used
in command paths. The finite shape list leaves other secret formats possible,
so callers must provide only known non-secret paths.

## Usage (stage 11, D14)

Scope: [stage 11](../stages/11-usage.md). The user accepted LIVE's ban risk
explicitly and informed (2026-11-17); LOCAL stays the default. Controls below
are requirements with synthetic acceptance, not implementation certification.

| Threat | Required control and discriminating acceptance |
|---|---|
| Token in memory, swap, core dumps | zeroizing buffers and token type without Debug/Display, dropped right after the one request; core dumps disabled for the process before a LIVE read on Unix (`RLIMIT_CORE` 0, Linux `PR_SET_DUMPABLE` 0). No `mlock` promise: swap and hibernation images remain a residual risk |
| Token in logs, panics, error strings | no logging in LIVE paths; fixed codes only; parser/TLS/IO errors never formatted into output; JSON-mode panic hook emits only a fixed envelope. `stage11_secrets` injects failures at every step and finds zero fake-token bytes in stdout/stderr/files/panic output |
| Token in child argv/env | LIVE starts no child or PTY and never refreshes; tokens never enter argv/env. Asserted by the fake transport and process-spawn seam |
| Endpoint confusion, MITM, exfiltration | two fixed host/path pairs, TLS verification, proxy env ignored, redirects never followed, no cookies, 64 KiB response cap, 5/10 s timeouts, one attempt. Test-only base-URL/trust-root override is compiled out of release |
| Hostile or changed response | allowlisted deserializer; unknown shape, out-of-range values or 401/403 fail closed to `live_unavailable`/`sign_in_again`; nothing echoed; account IDs, user IDs, credits and dollar fields discarded unread into output. Codex `plan_type` is the one plan field kept (D32): verified enum value only, in memory for the invocation, never stored, cached or logged |
| Codex account ID in LIVE | inside D14 (D29): read from auth.json only when LIVE is enabled, zeroized, sent only as `chatgpt-account-id` to the pinned host, never stored or output; covered by `stage11_secrets` with fake account IDs |
| Session-log privacy (LOCAL Codex) | confined no-follow reader in the profile's own CODEX_HOME, grammar-checked paths, caps on dirs/files/bytes/lines/time; streaming skip of all non-allowlisted values; `session_meta` yields only `cli_version`. Conversation sentinels in fixtures never appear in output or files |
| Provider names in output | bucket names/IDs (`limit_name`, `limit_id`, `metered_feature`) can name an organization or private model: labels come only from a fixed Larp vocabulary (`Other limit N` for unreviewed buckets), enforced by the schema pattern; grammar-valid private sentinels never appear in local or forwarded output |
| Wrong-bucket or stale data | filter by `limit_id`, map by `window_minutes` not position, per-window observation time for pace (no row-level mixing), per-file version gate, `reset_since_capture` after reset; resumed sessions written by another Codex version than `session_meta` names are a known limitation (shape check still applies) |
| Hook injection: hostile statusLine input | fixed argv and `CLAUDE_CONFIG_DIR` match, 256 KiB/2 s stdin cap, allowlist parse, range checks, no stderr, no echo, exit 0; deep nesting, huge strings, NaN and wrong types tested |
| Overwriting the user's statusLine | capture is opt-in per profile (D29); a key-presence-only check refuses `statusline_exists` on enable and skips injection at launch when the profile or the launch directory's project/local settings (any parent) define a statusLine; an indeterminate check (malformed/unsupported syntax, oversize, unsafe file, IO error, walk cap) refuses `statusline_unverifiable` at enable and launches without injection; never wraps another command; the hook prints nothing. Settings values are never read or output |
| Torn settings commit | `usage.json` changes use a journaled two-phase commit under the registry lock and the pending-journal gate; recovery runs before any revision comparison or read, so LIVE consent/mode is never visible at the wrong revision; kill-between-writes tests |
| Hook tamper: file/settings drift | hook is a root-pinned Larp copy validated by receipt and hash; the `--settings` render is an owned, hash-checked file outside native homes and refuses on drift; no native `settings.json` writes. Managed policy, `disableAllHooks` or untrusted workspaces silently stop capture (shows stale/`not_captured`, never guessed) |
| Capture file forgery by same-user programs | capture is untrusted input to `usage`: no-follow, owner-only, size/field/range checks; a same-user process can still forge plausible numbers. Display is "as reported", never used for automation |
| macOS Keychain access | a plain read query; macOS prompts the user, and the prompt is explained at enable time (D29). Larp never requests, sets or edits "Always Allow" or the item's ACL. If the user chooses Always Allow themselves, a later compromised Larp binary could read that item without asking |
| ToS / ban risk | LIVE is off by default, needs `--acknowledge-risk` on every enable, labelled unverified; still a request from a non-official client with the user's token, which the provider may detect and restrict. No imitation of official clients' User-Agent |

What LarpCC still never does: refresh, store or copy tokens; drive a client in a
PTY; scrape or capture native client output for usage; read native config
values (only `statusLine` key presence); read plan or account identity from
tokens or credential files; poll in the background or at login; switch, rotate or
pool profiles based on usage; send usage data anywhere; read real credentials
or call real endpoints in tests (fakes only). Residual risks stay: same-user
processes can read the same credentials and forge captures, memory can be
swapped, and a provider can restrict an account for LIVE use regardless of
these controls.

## Desktop dispatch

| Threat | Control and remaining exposure |
|---|---|
| Writes to the default Codex home | D34 repeated the exact ChatGPT.app launch after stopping all processes using `~/.codex` and establishing a quiet baseline. The metadata diff for `~/.codex` was empty and `lsof` found no open files there. ChatGPT.app started its own CodexCLI app-server with `CODEX_HOME=<T>/cli`. Profile state went to `<T>/cli` (343 entries) and the Chromium profile at `<T>/desktop`. |
| Shared runtime files | ChatGPT.app extracts Node runtime binaries into `~/.cache/codex-runtimes`, a shared directory outside `CODEX_HOME` isolation. The cache holds only runtime binaries. `profile delete` leaves the shared cache in place. |
| Stale nested signature verdict | Bind validates nested code and resources. On open, the cached verdict is keyed by executable and Info.plist metadata and the code directory hash. Changes limited to nested code or resources can leave the key unchanged and go undetected on a cache hit. |

## Version probe and provider evidence (stage 08)

| Threat | Control and remaining exposure |
|---|---|
| Probe runs replaced bytes | The descriptor pins the exact artifact SHA-256. As stage 3 dispatch does, the last step before process creation re-resolves the stored binding, requires the same resolved path and identity, and re-hashes the current bytes (bounded) against the retained verified handle. Any change refuses with a null version. The /dev/null and pipe descriptors and the held probe-home directory are prepared before that check, and the child enters the home with `fchdir`, so the only path lookup after the check is the exec. The `Command` has no environment builder call; the child gets only the prepared list passed to `execve`. Rust std's unavoidable pre-fork setup (its CLOEXEC error-report pipe and the environment read lock) still runs between the check and fork, as part of the stage 3 accepted residual. The child is then started by exact pathname. Path-reopen/same-user races after checks remain explicit: a same-user process that replaces the path between that final check and exec is outside the guarantee. |
| Probe output, hang or leakage | Clean explicit env with synthetic homes, null stdin, a 4 KiB combined stdout+stderr cap enforced per read before appending, a 5 s deadline, SIGKILL and a reap proved within 2 s, and the exact grammar with exit 0 and empty stderr. Rejections give null plus a fixed diagnostic; output is never returned or logged. A cleanup or reap failure is an operational error. |
| Evidence touches more than the requested executable | Only the requested provider's binding is selected from the revision-checked snapshot. Only that executable is validated and read. No profile home, account data or other binding is opened. |
| Evidence pairs the digest and code hash of different bytes | The macOS code-directory hash API reads by pathname. After that read, evidence re-hashes through the checked path and requires the same identity, size, SHA-256, mtime and ctime as the hashed handle; any change refuses the export. A same-user race that restores identical bytes and timestamps in between remains the stage 3 residual. |

## Remote

These controls apply to [Stage 13 SSH requests](../stages/13-remote-hosts.md).
The user must control and trust the SSH server. An authenticated but
compromised server can fabricate valid metadata or mutate its own state.

| Threat | Required control and discriminating acceptance |
|---|---|
| Malicious or compromised server output | Enforce schema, semantics, command/exit matching, stream caps, deadlines and empty stderr. Replace arbitrary messages with fixed local text. Treat display fields as inert, machine-scoped metadata. Feed forged identifiers, ANSI/control strings, wrong commands and schema-valid misleading paths; none selects a local target, starts a process, or reaches a second host automatically. |
| DEST or remote argument injection | Parse a strict destination grammar, split user/host/port into separate argv, and invoke OpenSSH directly. Use one fixed SSH remote command and a POSIX script with every data word quoted. Verify spaces and apostrophes round trip and shell metacharacters stay data. |
| User-owned SSH config and hooks | Config mode delegates trust, routing and authentication to OpenSSH, including ProxyJump, ProxyCommand, LocalCommand and configured multiplexing. Fixed options neutralize the user's LocalForward, RemoteForward and RequestTTY for Larp calls. Treat the user's SSH config and hooks as trusted local code. Direct mode skips the config and disables connection sharing and forwarding. Both use fixed BatchMode, `-a -x` and no raw user-supplied SSH options. |
| SSH authentication material | Only OpenSSH accesses local agents, identity files and host-key files. Direct mode passes the socket identifier unchanged and permits a validated typed identity path. Larp never opens either, logs identity paths or sends them to the remote shell. |
| App subprocess environment | Only host-routed invocations add the app's SSH_AUTH_SOCK unchanged to the minimal bridge environment. No shell fallback, extra inherited variables or PATH expansion. Verify with captured synthetic child environments. |
| MITM, first contact or changed host key | Direct mode requires StrictHostKeyChecking=yes and prior trust. Config mode uses the user's existing SSH trust policy. Verify direct-mode refusal without changed trust state, and preserve config-mode options without adding a weaker fallback. Never expose raw diagnostics. |
| Data exfiltration via MCP copy or sync | Require an explicit source, target, digest and approval. Validate the actual command, arguments or URL and reference names before transfer and again on the destination. No credential values, keychain lookups, endpoint requests or server execution. Untrusted output never chooses a routing destination. |
| Credential-shaped skill files or filesystem escape | Scope transfers to a catalog-owned Stage 06 package and managed receiver staging. Validate path allowlists and limits before reading or accepting file data, reject credential-shaped names, links and special files, and verify the approved digest before commit. Truncated, oversized and malformed frames leave no committed entry. User approval covers the content of accepted files. No arbitrary-path transfer or Larp binary installation. |
| Stale routing, stale registry or lost receipt | Check host and target revisions independently. Never fall back or retry on conflict. After uncertain mutation outcome, refresh explicitly to reconcile. Mixed sync results preserve confirmed successes and expose unattempted destinations/entries. |
| Remote usage or native launch bypass | Gate forwarded requests on both sides. Refuse LIVE before token access and closed native operations before SSH; prove zero provider requests and zero native child creation with fakes. |

## Account summary

These controls apply to [Stage 14 account status](../stages/14-account-summary.md).
The email and plan are personal data the user chose to see (D25). LarpCC
never stores them (D26).

| Threat | Required control and discriminating acceptance |
|---|---|
| Email in scrollback, screenshots or shared terminals | User choice under D25. `--no-account` gives a view with no email. The app blurs by default, re-blurs on close/relaunch and offers Hide account emails. The menu bar never shows an email. Shipped docs state that LarpCC cannot clear scrollback or screenshots. |
| Output-shape spoofing by a malicious or swapped binary | Admission pins the exact artifact SHA-256 and identity, rechecked before dispatch. The parser is bound to that exact row. Unknown, duplicate, ambiguous or control-bearing output gives `unknown`, never a guess. Feed forged shapes, extra fields, ANSI and oversized output to an invented row; none produces `signed_in` or leaks into output. A same-user attacker who can replace a qualified binary between checks is outside the guarantee. |
| PII in JSON consumers | Email only in `status.account`, only for `signed_in`, schema-bounded. Strict consumers that lack support fail closed. Scripts and app views that must not see PII use `--no-account`. The app requests account status only on explicit open or refresh, never persists it and drops it on close, refresh or ID mismatch. |
| Persistence or log leakage | No registry field, cache, temp file, log, crash output or telemetry holds email or plan. Display names refuse `@`. Sentinel PII and sentinel credentials from a fake client appear nowhere under the root, temp dirs or other commands' output. |
| Credential or token reads to fill fields | None, ever. Absent fields stay null. No credential file, keychain, session or env lookup. Prove zero opens outside the fake's declared outputs. |
| Status spawn hangs, floods or survives | Null stdin, 16 KiB per-pipe caps, 10 s deadline, reviewed cancel-and-reap before guard release. Overflow and timeout give `unknown`, never partial fields. |
| Stale or crossed result | `--expected-revision` and `--expected-profile-id` checked before dispatch, snapshot recheck after reap (`conflict`, no account data). The app always sends both from its displayed snapshot, checks the returned profile ID and revision, and drops in-flight results on selection change. Remotely, the router rejects a reply whose name, ID or revision differs from what was sent (`remote_output`). |
| Qualification on the user's Mac touching real sign-ins | Throwaway profiles in a temporary root, the user does every sign-in, no access to `~/.claude*`, `~/.codex*` or default app data. Each native operation, including the first status call, is blocked unless the exact build's credential backend, key, precedence and fallback are proven per-home (G0). PII-capable observations run only in the user's unrecorded terminal. Evidence records presence only (`email: present`). |
| Email in transit over SSH | Remote account status only on explicit user action, over the user's own encrypted SSH session to their own machine, with stage 13's fixed argv and transport. LarpCC adds no other transport, never logs the SSH stdout, and drops it after validation. |
| Untrusted or forged remote envelope | Treat the remote as able to fabricate any value. Enforce schema, semantics, the email/plan forms, exact discriminator, requested profile ID and name, empty stderr, 1 MiB caps (`too_large`) and the 16 s child / 33 s bridge deadlines (`remote_timeout`); otherwise `remote_output` with no account data. Display the values as inert text labelled with the host. They never select a target or prove a local sign-in. |
| Metadata request turned into an account request | Under `--remote-request`, unmarked `status` is metadata-only on every receiver release. Account mode needs the receiver-checked `--remote-account` opt-in, checked before any effect; old receivers reject it as unknown. Expected revision and `--expected-profile-id` are checked before the guard and spawn. Remote metadata status carries the ID without the account marker; a receiver that cannot enforce it fails closed (`remote_incompatible`), including a downgraded receiver whose fully validated exit-2 `usage` error (under `parse` or the requested discriminator, empty stderr) is mapped to `remote_incompatible` after stage 13's transport, deadline and cap checks. The expected reply discriminator follows the request mode (`status` or `status.account`). App requests also carry `--expected-host-revision`, checked locally before SSH and never forwarded, so a rebound host alias cannot reach a different machine. Prove zero spawns when the remote version changes between probe and dispatch, and on rename + name reuse. |
| Signing in before a safe sign-out exists | In qualification, login runs only after G0 proves status and login for that provider, plus one sign-out path: G0-proven logout or an approved G0-proven cleanup path. Once login has been dispatched, every outcome (success, failure, non-zero exit, cancellation, timeout or stop) runs in order: confirmed reap and quiescence of the login process tree (a surviving child could rewrite the credential), then the selected path, then confirmed sign-out, and only then deletion. A missing confirmation keeps the throwaway state for recovery and blocks the row. A Keychain item survives deleting the temporary root, so its cleanup path is an approved user-run removal of that exact item. |
| Remote persistence | The remote receiver applies D26 exactly as a local call: request memory only, nothing logged or written. The local side and the app cache nothing and drop the result on route change. Prove with sentinel emails on both fakes. |
