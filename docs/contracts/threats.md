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

MCP command admission checks each path component on add, catalog load and
journal replay. Case-insensitive markers are `sk-` (including `sk-ant-`), `sk_`,
`ghp_`, `gho_`, `ghs_`, `ghu_`, `ghr_`, `github_pat_`, `glpat-`,
`xoxa-`, `xoxb-`, `xoxp-`, `xoxr-`, `xoxs-`, `-----begin`, and `Bearer`
as a component or followed by a space, hyphen, underscore or dot. It also
refuses uppercase `AKIA` or `ASIA` followed by 16 uppercase letters or digits,
`AIza` followed by 35 base64url characters, and JWT-shaped names beginning
`eyJ` with three dot-separated base64url segments of at least eight characters.
A dot-delimited base64 or base64url segment is refused when it has 20 or more
characters, both letter cases, and a digit, `+`, `-`, `_`, or trailing
padding. The base64 shape allows up to two trailing padding characters.
Standard 8-4-4-4-12 hex GUIDs and lowercase hex and Nix base32 hashes can be used
in command paths. The finite shape list leaves other secret formats possible,
so callers must provide only known non-secret paths.

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
