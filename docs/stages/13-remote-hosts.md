# Stage 13 — Remote hosts over SSH

D17 planning contract. The user installs Larp and the unmodified official clients
on each machine. Larp checks the remote CLI contract and uses that machine's own
registry and profiles. The macOS app presents both local and remote metadata.
Linux and macOS are remote targets with a POSIX `sh` on PATH. macOS, Linux and
Windows 10+ are client platforms with supported system OpenSSH. VMs, sandboxes
and containers qualify when they meet the SSH and remote OS requirements. Native
Windows remotes, exec-based transports, installers and file transfer are outside
v1 scope. WSL qualifies only through its own Linux sshd. See [CLI JSON v1](../contracts/cli-v1.md#remote) and
[the threat review](../contracts/threats.md#remote).

## Commands and storage

```text
larp cc host add NAME --ssh DEST [--larp REMOTE_PATH] [--larpcc-home REMOTE_PATH] [--identity-file LOCAL_PATH]
larp cc host list
larp cc host show NAME
larp cc host remove NAME --yes
larp cc host check NAME

larp cc [--host NAME] host catalog list KIND
larp cc [--host NAME] host catalog share KIND ENTRY on|off
larp cc [--host NAME] host catalog apply mcp ENTRY [stage 07 mcp add options]
larp cc [--host NAME] host catalog apply skills ENTRY --package REMOTE_PATH --sha256 DIGEST
```

`KIND` is `mcp` or `skills`. Standard JSON, confirmation and revision flags
apply. Host add/list/show/remove are local operations. Duplicate add fails
`conflict`. Remove retires the local record only and cancels no active work on the
host. Check is an explicit compatibility probe.

Store routing records in a separate private `hosts.json` under LARPCC_HOME, using
the registry lock and its checked ancestry, file identity and atomic durable
replacement rules. Strict version 1 has a canonical u64 revision and an array
of records `{name,ssh,larp,larpcc_home,identity_file}`, sorted by ASCII name. Omitted paths
are null. Bound the file to 1 MiB and 32 hosts, reject malformed files, unknown
fields, duplicate names and revision overflow. Missing file is empty at revision
0. Add/remove advance the host revision only. Each host owns its account and
catalog state. Status is session-local UI state derived from the last explicit
check of the same record at the same host revision.

`--host NAME` before `--` routes profile, alias, MCP, skills, default doctor,
JSON status and the three catalog commands above. Forward only implemented
metadata subcommands, through the receiver allowlist, with stage 04–10
preconditions intact. Target-local paths must be supplied explicitly. Refuse
`doctor --detect-versions`, remote desktop and provider binding, login/logout,
human native status, and nested routing. `run --host NAME` is intended for an
interactive `ssh -t` with inherited terminal streams after a separate review
of native admission and terminal transport. Today it refuses
`qualification_pending` before registry IO or SSH. JSON native IO and remote
`profile delete --logout` refuse before effects.

A forwarded `--expected-revision` refers solely to the target machine's
registry. GUI forwarded reads and mutations also carry
`--expected-host-revision` from the displayed routing snapshot. Check the
routing precondition under the local lock before resolving the host, then
revalidate the record and revision under that lock through each child spawn.
Release the lock before waiting. An in-flight request uses that route. Discard a
stale read or check in the app. A mutation in flight retains the outcome for its
original route, even if the UI removed the host. On the destination, check the
target revision under its registry lock before resolving names or applying
changes. Conflicts name the machine in the app and require refresh and renewed
intent.

## Destination and OpenSSH

**Pending user confirmation.** NAME is the user's host label. Accept only
`USER@HOST[:PORT]` as DEST. Larp neither imports nor interprets SSH config
aliases, including those in ambient SSH config. ProxyCommand/ProxyJump-only
hosts are unsupported in v1.

- NAME uses the profile grammar and reserved names. USER matches
  `[A-Za-z0-9_][A-Za-z0-9_-]{0,63}`.
- HOST is either a canonical four-octet IPv4 address or ASCII DNS labels
  separated by dots. Each label has 1–63 characters with letters/digits at
  both ends and only letters/digits/hyphen inside. For DNS, the last label must
  start with a letter, including for single-label names. Maximum 253
  characters. IPv6 literals are deferred.
- PORT is canonical decimal 1–65535, default 22. Validate the entire value.
  Leading hyphens, whitespace, control characters, URI syntax, percent
  expansion, wildcards and extra delimiters refuse before spawning. Persist
  the input and its explicit-port form as `ssh:{destination,resolved}`. Pass
  user, port and host as separate argv elements.

Invoke a validated system OpenSSH executable directly with an argv array.
On Windows require a native `ssh.exe` and never use batch or command-script
wrappers. Require OpenSSH 9.2 or later. Before using the selected binary, run
only its local `-V` invocation with a two-second deadline, closed stdin and a
4 KiB combined output cap. Accept exit 0 and a single version line with an
OpenSSH or OpenSSH_for_Windows version prefix and recognized numeric major
and minor components. Permit platform/patch suffixes, compare major/minor
numerically and never display the captured line. Unsupported or unverifiable
clients fail as `ssh_unavailable`. The fixture binary on private PATH is
allowed only in the synthetic harness. The noninteractive shape is:

```text
ssh -F none -T -a -x -S none -e none
    -o BatchMode=yes -o StrictHostKeyChecking=yes -o UpdateHostKeys=no
    -o PermitLocalCommand=no -o ClearAllForwardings=yes -o LogLevel=ERROR
    -o ConnectTimeout=4 -o ConnectionAttempts=1
    [identity options] -l USER -p PORT -- HOST 'sh -s'
```

The final argv element is the literal string `sh -s`. Ignore ambient SSH
configuration. The user establishes host-key trust outside Larp. OpenSSH
verifies trust using its default trusted host-key files. Forwarding,
multiplexing and prompts stay disabled. Strip askpass and provider credential
variables, and pass the minimum platform environment plus the agent allowance
below if approved. Reject incompatible clients rather than retry with weaker
settings. [OpenSSH command options](https://man.openbsd.org/ssh) and
[configuration semantics](https://man.openbsd.org/ssh_config) define the flags.

### SSH authentication (pending user decision)

The recommended contract allows OpenSSH to use the user's existing local
agent. Pass `SSH_AUTH_SOCK` through unchanged if present and omit
`IdentityAgent=none`. OpenSSH may use the platform's default local agent
when applicable. Larp never opens the socket, reads agent contents or
forwards it, and `-a` still disables forwarding. The agent must be usable
under BatchMode without a Larp prompt.

The recommended optional `--identity-file LOCAL_PATH` stores a nullable
`identity_file` path in the host record. Validate syntax only. Require a
Unicode absolute local filesystem path without control characters, option-like
syntax or SSH expansion tokens (`%` and `$` are refused). On Windows
require a drive-rooted path and refuse UNC and device paths. OpenSSH alone
opens the identity file. Add `-o IdentitiesOnly=yes -i PATH` as separate
arguments when specified, so an already-loaded matching agent key can also be
used. Omit these flags without a path so the agent can offer its identities.
Do not send the path to the remote shell or include it in diagnostics.

The strict alternative omits `--identity-file`, keeps the field null,
clears `SSH_AUTH_SOCK` and uses
`-o IdentityAgent=none -o IdentitiesOnly=yes`. It limits users to default
identities that work without an agent or passphrase prompt. This planning
contract does not authorize either implementation until the decision lands.

## Remote command and compatibility

Send one generated POSIX script over SSH stdin and close it. The login shell
only receives the fixed command `sh -s`, so customizable values never reach a
non-POSIX login-shell parser. Generate the following lines, with exactly one
LF after each. `{larp}` is the quoted executable word. `{home}` is empty or
`LARPCC_HOME=` plus the quoted path and one trailing space. `{args}` is the
space-joined quoted argument list excluding the executable.

```sh
case "$(uname -s </dev/null 2>/dev/null)" in Linux|Darwin) ;; *) exit 45 ;; esac
command -v {larp} </dev/null >/dev/null 2>&1 || exit 44
{home}exec {larp} {args} </dev/null
```

The executable is `larp` from remote PATH or the explicit absolute POSIX
`--larp` path. An omitted `--larpcc-home` uses normal remote root resolution.
SSH does not send the local LARPCC_HOME.

Every dynamic shell word is valid UTF-8 without control characters. Quote each
word independently by surrounding it with single quotes and replacing every
embedded single quote with the four-character sequence `'\''`. Quote empty
words as `''`. Join argv words with exactly one ASCII space. The optional
assignment is the fixed prefix `LARPCC_HOME=` immediately followed by the
quoted value. For example, `/opt/Larp's bin/larp` is
`'/opt/Larp'\''s bin/larp'`. No tilde, globs, variable interpolation or
unquoted remote arguments. Validate both paths and the forwarded argv before
spawning. Limit each word to 4 KiB and the encoded script to 64 KiB. Build argv
from the parsed allowlisted command and its typed arguments. Use the parser's
unambiguous value form for values that could otherwise be parsed as flags.
The script is the only input. The fixed redirection `</dev/null` on the
`exec` line prevents Larp from reading the script stream.

Check and every forwarded operation first run `larp --json --version` through
this path. Require exit 0, one exact schema-1 `version` success envelope, null
revision and a canonical version from the client build's explicit allowlist of
released versions implementing this stage's receiver contract. The initial
implementation ships a nonempty allowlist containing its first compatible
release. Schema 1 alone does not establish feature support. Do not infer
compatibility from version ordering or cache it as admission. Probe and command
are two separate SSH logins for each forwarded operation. Then invoke the
requested command with `--json --remote-request` and fixed parsed argv, including
any target revision. The internal `--remote-request` flag requires JSON and
enforces the receiver allowlist and LOCAL-only usage before IO. Old receivers
must reject the unknown flag before effects. The probe and receiver gate
neither qualify a native launch nor establish remote account identity.

Drain both streams concurrently, with a 1 MiB cap per stream and empty stderr
required for an accepted envelope. Probe/read deadline is 5 seconds per SSH
child, plus at most 2 seconds for failure cleanup. Within the app the bounded
local SSH version check also counts toward the read deadline. Cache the local
client check by verified executable identity and invalidate it on replacement.
A dispatched mutation has a 60-second deadline. On timeout, cancel or overflow, terminate and reap the
direct SSH child. Losing that process does not establish whether a remote
mutation committed. Enforce one UTF-8 JSON object plus LF, exact discriminator
(or `parse` for a parser rejection), schema and semantic invariants, and matching
CLI exit status. Re-encode validated data for the local caller. Remote error,
warning and diagnostic messages are replaced with fixed local text by code.
Treat remote display fields as inert text, never as command arguments, file
paths, URLs to open, markup, authority to select a different target, or evidence
of authentication. Human output escapes remote paths with the stage 02 display
helper. No raw stderr, failed payloads or commands in results or logs.
`LogLevel=ERROR` reduces client-side chatter. Larp cannot distinguish client
stderr from remote stderr and never parses either. After deadline/limit checks,
exit 255 is an SSH failure. On any other exit, stderr makes the probe or read
response invalid (`remote_output`, and `incompatible` for an explicit check).
Any invalid response after mutation dispatch leaves the outcome unknown.
Reserve `ssh_*`, `remote_*` and `catalog_unavailable` codes for the local
router. Reject them in forwarded envelopes as invalid responses.

| Observation | Forwarded error / explicit check state |
|---|---|
| Unsafe syntax, forbidden route or command | `usage` or `unsupported`; no child |
| Host record absent or routing revision mismatch | `not_found` or `conflict`; no child |
| No supported SSH client | `ssh_unavailable` / `unreachable` |
| SSH exit 255, including transport, host-key or authentication refusal | `ssh_failed` / `unreachable`; fixed hint to verify connectivity, host key and authentication outside Larp |
| Probe exit 44 or 45 with both streams empty | `remote_larp_missing` / `larp_missing`, or `remote_incompatible` / `incompatible`, respectively |
| Old/unsupported version, other probe exit or invalid version framing with empty stderr | `remote_incompatible` / `incompatible` |
| Deadline or stream cap exceeded | `remote_timeout` or `too_large` / `unreachable` or `incompatible`, respectively |
| Invalid response on a read, or unexpected stderr on a non-255 probe/read exit | `remote_output` / `incompatible`; no trusted snapshot |
| Confirmed remote CLI error, including stale target revision | Preserve the allowlisted code and applicable exit class, with local text |
| Transport failure or unverified response after mutation dispatch | `remote_outcome_unknown`; refresh explicitly before deciding whether to retry |

OpenSSH exit 255 does not distinguish network, host-key or authentication
failures. Diagnostics are never parsed for those distinctions.

## Catalog copy and explicit sync

Store opt-ins on the owning machine in a separate private
`catalog-sharing.json` file, never in the strict v1 MCP catalog or skills
index. Its strict shape is `{version:1,mcp,skills}`, with each kind represented
as `{catalog_revision,names}`. Revisions are canonical decimal u64 strings
(`"0"` for an absent catalog). Opted-in names are sorted and unique. Limit
to 256 names per kind and 1 MiB total.
Absent file means all flags are off. Unknown fields, malformed records or
unsupported versions refuse sharing operations. Use the same registry lock
and checked durable transaction rules as the catalogs. Update flags and their
catalog revision binding with catalog mutations. Remove the flag with its
entry. If an older writer changes a catalog without updating the binding,
treat all flags for that kind as off until the user opts in again. Never
reattach a stale flag to a reused name.

`host catalog share` checks the selected machine's expected registry revision.
A changed flag advances that registry revision once and its receipt reports
the new revision. A no-op retains the revision. Commit the flag file and
registry revision with the catalog journal and recovery rules. List derives
one flag row per current catalog entry at the reported registry revision.
Share never updates the other machine catalogs or starts a sync.

The app's **Copy to machine…** action uses one selected source entry and one
explicit destination. **Sync machines** on a source machine previews opted-in
entries and all destination machines in the current snapshot, including the
local machine when the source is remote. The user confirms the destination list
and target inputs. No implied destinations, background retries or transitive
sync. Read the flag and definition metadata at one source revision and
revalidate that revision before dispatching any target changes. Freeze the
approved inputs for the batch. The app composes the existing reads and one
`host catalog apply` call per approved entry/destination, with the displayed
target revisions. For successive applies on one target, use each validated
success receipt's revision for the next preapproved apply. Stop changes on that target on conflict or unknown
outcome and report pending entries as unattempted. Continue independent targets
and show a result per target/entry, including unavailable dependencies and
unattempted entries. Target apply returns `not_found` for unavailable
dependencies, which the local router maps to `catalog_unavailable`. Cancel stops
remaining dispatches and marks an interrupted in-flight mutation's outcome unknown.
No cross-host transaction or rollback.

- MCP copies the entry name, transport and secret reference identifiers. The
  app uses source list metadata and asks for the target-specific executable
  or endpoint, flags and any needed header-to-reference
  mappings under stage 07. It previews the exact target and recipe for user
  approval. Never auto-forward source paths, endpoints or untrusted host text.
  Validate reference identifiers against the approved source metadata, and the
  command/endpoint and references against stage 07 on the destination. A missing
  destination executable or unsupported reference is unavailable, with zero
  server execution, credential lookup or endpoint requests. No secret literals.
- Skills copy catalog enrollment by name and digest. The user stages the
  package on the destination independently. Apply validates it with stage 06
  confinement and limits, computes its digest before commit, and adds it from
  the destination filesystem only when it matches. Missing or different
  package is unavailable. No code or file bytes cross SSH.
- Apply adds a destination-local entry with sharing off and inherits the
  destination's stage 06 skill defaults or stage 07 MCP `sync_all` defaults.
  Preview the resulting enablement from the checked destination snapshot.
  Identical definitions are successful no-ops preserving existing switches
  and flags. A differing same-name definition
  fails `conflict`. Existing drift and preflight rules still apply. No apply
  starts a server, skill, or provider, or copies profile IDs or switches.

## App behavior and account limits

Add a **Machines** sidebar with **This Mac** and host rows. Add/remove use a
sheet. Host status dot reflects the last explicit check, with unknown initially.
Group profiles by machine and key selections by route and profile ID. Route
changes invalidate stale selections, cached metadata and status. Use
per-machine MCP and Skills pages for editing, copy and sync. Display last loaded
data with a stale/error state if a refresh fails. Never redirect a remote action
to a local profile of the same name. The menu bar lists profiles for all machines
from data already loaded in the app, without starting a refresh. Only an explicit
machine selection, Refresh, Check, edit, copy or sync can contact a host.
App launch, restored selection, opening a window or the menu bar, and wake
never contact a host. Remote native
actions are shown unavailable while admission is closed.

For Stage 11 usage, remote profiles support LOCAL mode only, including when
LIVE is enabled at the destination for its own local callers. Forward only the
reviewed local usage reads once that contract ships. The receiver refuses any
LIVE request or switch to LIVE before token access or a provider request.
Remote Larp follows the same user intent, credential and account-safety rules as
local Larp. Each host signs in only on that machine through its official client.

## Synthetic implementation acceptance

Run `cargo test --workspace --locked stage13_` natively on Linux, macOS and
Windows (also Linux MSRV). Use an executable fake `ssh` on a private PATH that
records argv/stdin, returns replayed envelopes and simulates failures without a
network. No fallthrough to real SSH. Exercise the exact quoting through a
synthetic POSIX decoder and, on POSIX CI, a local POSIX shell with inert fixture
executables and outside sentinels. Use temporary roots and clean all artifacts.

| Test | Required evidence |
|---|---|
| `stage13_hosts` | Strict direct DEST parsing, SSH config aliases refused, no SSH on CRUD or stale routing, atomic changes, concurrent name reuse refuses, no remote deletion on remove |
| `stage13_argv` | Exact flags and one fixed SSH command across platforms, pending authentication policy passed correctly, exact spaces/quotes/dollars in paths and argv, attempted injected options/commands remain inert, no local SSH config or credentials inspected by Larp |
| `stage13_transport` | Missing client, host-key refusal and BatchMode prompt refusal with no fallback, banner and post-quantum warning on stderr rejected and redacted, caps on both streams, deadlines/cancel/reap, no raw output or secret sentinels in errors, unknown mutation outcome |
| `stage13_protocol` | Missing/old/incompatible Larp, unsupported OS including MSYS/Cygwin exit 45, escaped paths, reserved router error codes, malformed/framed/trailing/unknown schema and wrong command or exit, compatible version, receiver allowlist, closed native admission and remote LIVE refusal before effects |
| `stage13_revision` | Same profile IDs/names on different machines, re-add race and stale target conflict before effects, one-shot mutation on disconnect, no fallback or retry |
| `stage13_catalog` | Destination inheritance including MCP sync_all=true and skill future-profile defaults, isolated share revision/no-op and older writer invalidation, opt-in explicit sync, no file or secret bytes across SSH, missing executable/package and digest mismatch, same-name conflict and same-definition no-op, independent success amid partial failure |
| App fixture suite | Decode new and old envelopes, add/remove/check states, revision/routing isolation, previewed copy and sync outcomes, no remote LIVE action, menu uses loaded data and does not poll, launch with N hosts and restore/wake/window/menu open spawn zero SSH processes |

Zero matching tests fails implementation acceptance. This planning PR runs
contract fixture validation only.
