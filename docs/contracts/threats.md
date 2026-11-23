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
| Metadata request turned into an account request | Under `--remote-request`, unmarked `status` is metadata-only on every receiver release. Account mode needs the receiver-checked `--remote-account` opt-in, checked before any effect; old receivers reject it as unknown. Expected revision and `--expected-profile-id` are checked before the guard and spawn. Prove zero spawns when the remote version changes between probe and dispatch, and on rename + name reuse. |
| Signing in before a safe sign-out exists | In qualification, login runs only after G0 proves status, login and logout (or an approved cleanup path) for that provider. A Keychain item survives deleting the temporary root, so it needs proven logout or an approved user-run removal of that exact item. |
| Remote persistence | The remote receiver applies D26 exactly as a local call: request memory only, nothing logged or written. The local side and the app cache nothing and drop the result on route change. Prove with sentinel emails on both fakes. |
