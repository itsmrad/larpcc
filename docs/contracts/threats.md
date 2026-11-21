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
| Implicit ssh_config commands or forwarding | Disable ambient SSH config and multiplexing. Accept explicit destinations only, pending user confirmation. Use no PTY for metadata, no agent/X11 forwarding, no askpass and no raw user-supplied SSH options. Assert no fallback on BatchMode refusal. |
| SSH authentication material | The proposed local agent and typed identity path are pending user decision. Only OpenSSH accesses the agent or identity file. Larp never opens either, logs identity paths or sends them to the remote shell. Assert agent forwarding stays disabled under either authentication policy. |
| MITM, first contact or changed host key | Require StrictHostKeyChecking=yes. Trust is established out of band by the user. Simulated unknown/changed keys fail without prompts, retries, accepts or changed trust state. Never expose raw SSH diagnostics. |
| Data exfiltration via MCP copy or sync | Require an explicit source, target and preview of approved data. Never use remote-provided paths, URLs or raw command/argument strings as target inputs. Target command/endpoint validation remains Stage 07; no server execution or endpoint check. Test a malicious source trying to redirect a copy or cause a second request. |
| Secrets or arbitrary files in copied catalog entries | Copy reference identifiers only for secrets, with no literal env/header values, OAuth state, keychain lookups or credential interpolation. A skill transfer enrolls a matching target-local package by digest. Assert invented secret values and package bytes never enter SSH input, stdout, stderr or logs. |
| Stale routing, stale registry or lost receipt | Check host and target revisions independently. Never fall back or retry on conflict. After uncertain mutation outcome, refresh explicitly to reconcile. Mixed sync results preserve confirmed successes and expose unattempted destinations/entries. |
| Remote usage or native launch bypass | Gate forwarded requests on both sides. Refuse LIVE before token access and closed native operations before SSH; prove zero provider requests and zero native child creation with fakes. |
