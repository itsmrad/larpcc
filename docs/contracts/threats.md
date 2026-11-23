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
