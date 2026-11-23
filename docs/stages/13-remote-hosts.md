# Stage 13 — Remote hosts over SSH

D17, D20–D22 planning contract. The user installs Larp and the unmodified official clients
on each machine. Larp checks the remote CLI contract and uses that machine's own
registry and profiles. The macOS app presents both local and remote metadata.
Linux and macOS are remote targets with a POSIX `sh` on PATH. macOS, Linux and
Windows 10+ are client platforms with supported system OpenSSH. VMs, sandboxes
and containers qualify when they meet the SSH and remote OS requirements. Native
Windows remotes, exec-based transports, installers and general-purpose file
transfer are outside v1. Only the explicit catalog protocol transfers skill
packages and MCP definitions. Larp never transfers its binary. WSL qualifies only through its own Linux sshd. See [CLI JSON v1](../contracts/cli-v1.md#remote) and
[the threat review](../contracts/threats.md#remote).

## Commands and storage

```text
larp cc host add NAME --ssh-config-host ALIAS [--larp REMOTE_PATH] [--larpcc-home REMOTE_PATH]
larp cc host add NAME --ssh USER@HOST[:PORT] [--larp REMOTE_PATH] [--larpcc-home REMOTE_PATH] [--identity-file LOCAL_PATH]
larp cc host list
larp cc host show NAME
larp cc host remove NAME --yes
larp cc host check NAME

larp cc [--host NAME] host catalog list KIND
larp cc [--host NAME] host catalog share KIND ENTRY on|off
larp cc [--host NAME] host catalog apply mcp ENTRY [stage 07 mcp add options]
larp cc host catalog copy KIND ENTRY --from ROUTE --to ROUTE --expected-source-revision REV --expected-revision REV --sha256 DIGEST

# Internal receiver operations, with --json --remote-request
larp cc host catalog export KIND ENTRY --expected-revision REV
larp cc host catalog import skills ENTRY --sha256 DIGEST --expected-revision REV
```

`KIND` is `mcp` or `skills`. `ROUTE` is a configured host NAME or `@local`.
Reject identical source and destination routes. GUI copy also passes
`--expected-host-revision` for all remote routing in the request. Standard
JSON, confirmation and revision flags apply. Host add/list/show/remove are local operations. Duplicate add fails
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
JSON status, and catalog list, share and MCP apply. Forward only implemented
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

Choose exactly one addressing mode. NAME uses the profile grammar and
reserved names.

- `--ssh-config-host ALIAS` accepts an exact SSH config host argument.
  ALIAS starts with an ASCII letter, digit or underscore and may contain
  ASCII letters, digits, underscores, hyphens and dots, maximum 64 bytes.
  No whitespace, controls, wildcard, expansion, URI or option syntax.
  Persist `ssh:{mode:"config",alias}`. An identity path cannot be supplied
  in this mode.
- `--ssh USER@HOST[:PORT]` targets a direct address. USER matches
  `[A-Za-z0-9_][A-Za-z0-9_-]{0,63}`. HOST is canonical four-octet IPv4 or
  ASCII DNS labels of 1–63 characters, each ending and starting with a
  letter or digit, with hyphen also allowed inside. The last DNS label
  must start with a letter. Maximum 253 characters. IPv6 is deferred.
  PORT is canonical decimal 1–65535, default 22. Validate the entire
  value and reject whitespace, controls and option-like characters.
  Persist `ssh:{mode:"direct",destination,resolved}`, with an explicit
  port in `resolved`. Pass user, host and port as separate argv elements.

Resolve and validate a system OpenSSH executable and launch it directly
with an argv array. On Windows resolve an absolute path to a regular PE executable named
`ssh.exe`, using trusted executable lookup that excludes the current
directory. Reject scripts, `.bat`/`.cmd` wrappers and App Execution Aliases.
Require an `OpenSSH_for_Windows` version line there, so MSYS and Cygwin
clients using the generic OpenSSH prefix refuse. OpenSSH 6.8 is
the compatibility floor because it introduced `UpdateHostKeys`, the newest
option in the frozen argv. That release also supports `-F none`, `-S none`,
`ClearAllForwardings`, `LogLevel` and the remaining options.
[6.8 release notes](https://www.openssh.org/txt/release-6.8),
[6.8 client source](https://github.com/openssh/openssh-portable/blob/V_6_8_P1/ssh.c),
[6.8 client reference](https://github.com/openssh/openssh-portable/blob/V_6_8_P1/ssh.1),
[6.8 option reference](https://github.com/openssh/openssh-portable/blob/V_6_8_P1/ssh_config.5).

Require exit 0 from local `ssh -V` with closed stdin, a two-second deadline
and a 4 KiB combined output cap. Accept one OpenSSH or OpenSSH_for_Windows
version line on either stream, permit platform/patch suffixes, and compare
major/minor numerically. Never display the captured line. Unsupported or
unverifiable clients fail as `ssh_unavailable`. Only the synthetic harness
may use a fixture binary on private PATH.

```text
ssh -T -a -x -o BatchMode=yes -o ClearAllForwardings=yes
    -o LogLevel=ERROR -o ConnectTimeout=4 -- ALIAS 'sh -s'

ssh -F none -T -a -x -S none -e none
    -o BatchMode=yes -o StrictHostKeyChecking=yes -o UpdateHostKeys=no
    -o PermitLocalCommand=no -o ClearAllForwardings=yes -o LogLevel=ERROR
    -o ConnectTimeout=4 -o ConnectionAttempts=1
    [identity options] -l USER -p PORT -- HOST 'sh -s'
```

The final argv element is the literal string `sh -s`. Config mode uses the
user's own SSH config, trust policy and authentication as OpenSSH normally
does, subject only to the fixed options shown. It may use ProxyJump,
ProxyCommand, LocalCommand, or other configuration the user already trusts.
Larp never parses or imports that file. A configured hook or transport can
cause local side effects or contact other machines. The fixed `-T` and
`ClearAllForwardings=yes` neutralize `RequestTTY`, `LocalForward` and
`RemoteForward` for Larp's calls. ProxyJump requires OpenSSH 7.3 or later.
[OpenSSH 7.3 release notes](https://www.openssh.org/txt/release-7.3). Direct mode
skips ambient configuration, disables connection sharing and forwarding,
and requires pre-established host-key trust using default host-key files.
Use config mode for ProxyJump. Neither mode accepts raw SSH options.
[OpenSSH command options](https://man.openbsd.org/ssh) and
[configuration semantics](https://man.openbsd.org/ssh_config).

Pass `SSH_AUTH_SOCK` through unchanged if present. OpenSSH may also use the
platform's default local agent when applicable. Larp never opens or reads
the socket and `-a` tells OpenSSH to disable agent forwarding. The Stage 10 bridge passes the
app process's `SSH_AUTH_SOCK` through only for host-routed calls. Config
mode launched from the app uses the bridge's minimal `PATH=/usr/bin:/bin`,
so hooks that require a different PATH can fail. Direct mode cannot
use agents selected only by `ssh_config IdentityAgent`. Config mode uses
the available process environment and the user's SSH config. Direct mode clears askpass
and provider credential variables and passes only the environment needed
by OpenSSH, including the local agent socket identifier.

Direct mode may take `--identity-file LOCAL_PATH`. Validate syntax only:
a Unicode absolute local filesystem path without control characters,
option-like syntax or expansion characters (`%` and `$`). On Windows
require a drive-rooted path and refuse UNC and device paths. OpenSSH alone
opens the identity file. Add `-o IdentitiesOnly=yes -i PATH` as separate
arguments when specified, so a matching agent key may also be used.
Omit these flags without a path. Never send the path to the remote shell
or include it in diagnostics.

## Remote command and compatibility

Send one generated POSIX script over SSH stdin. Close stdin for ordinary
requests. The login shell
only receives the fixed command `sh -s`, so customizable values never reach a
non-POSIX login-shell parser. Generate these lines, each ending in one LF.
`{larp}` is the quoted executable word and `{args}` is the space-joined list
of quoted argument words after it. With an absolute `--larp` path, use the
two POSIX file tests on the second line. With the default PATH lookup replace
that line with `command -v 'larp' </dev/null >/dev/null 2>&1 || exit 44`.
Include the export line only when `--larpcc-home` is supplied, with
`{home}` the quoted path.

```sh
case "$(uname -s </dev/null 2>/dev/null)" in Linux|Darwin) ;; *) exit 45 ;; esac
[ -f {larp} ] </dev/null && [ -x {larp} ] </dev/null || exit 44
export LARPCC_HOME={home} </dev/null
exec {larp} {args} </dev/null
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
The fixed `</dev/null` on the `exec` line is required except for the
skills import handshake below, which uses stdin for the framed package.

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

Drain both streams concurrently, with a 1 MiB cap per stream for ordinary
envelopes and empty stderr required. Private export limits are specified below. Probe/read deadline is 4.5 seconds per SSH
child, including the 4-second OpenSSH connection timeout. The outer
deadline also covers hooks and jumps in config mode. Stop at the first
failure and allow at most 2 seconds for cleanup. The app bridge has a
15-second end-to-end deadline per read: up to 2 seconds for the local client
version check, 4.5 for the remote version probe, 4.5 for the command and
2 for failure cleanup, leaving 2 seconds of slack for bridge overhead. Cache the local client check by
verified executable identity and invalidate it on replacement.
A dispatched mutation or binary export has a 60-second per-child deadline. On timeout, cancel or overflow, terminate and reap the
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
| Explicit `host.check` classified probe failure | Exit 0, `ok:true`, with the status below. No deployment or host change occurs. |
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
An absent registry reports revision `"0"` and an empty entry list. The GUI
retains `"0"` and sends it as the expected revision for that snapshot.
Share never updates the other machine catalogs or starts a sync.

The app's **Copy to machine…** selects one source entry and destination.
**Sync machines** previews opted-in entries and the destination list,
including the local machine when the source is remote. The user confirms
the source, entry, content digest, targets and resulting destination
enablement. Use `host catalog list` for the digest of the validated
definition or the stored Stage 06 package digest at the listed source revision.
Recheck the actual package against that digest during export. Unavailable entries
have null digests and are reported per destination. Neither raw MCP
definitions nor skill file bytes pass through the GUI bridge.

Each approved entry/destination triggers one `host catalog copy` with the
approved digest, source revision and destination revision. The CLI checks
the source revision and validates and hashes the entire source payload
before dispatching to the destination. The payload must match the approved
digest. Never auto-redirect a destination or retry. For successive copies
to one target, use each confirmed receipt's revision for the next approved
entry. Stop changes on that target on conflict or unknown outcome. Continue
independent targets and report each result, including unattempted entries.
Cancel stops remaining dispatches and makes an interrupted mutation's
outcome unknown. No cross-host rollback or background sync.

New entries have sharing off and follow destination-local Stage 06 skill
defaults or Stage 07 MCP `sync_all`. Identical entries are no-ops that
preserve switches and flags. An existing entry with the same name but different content fails
`conflict`. No transfer starts a server, skill or provider, or copies
profile IDs or switches.

### Skills transfer

Export only a single catalog-owned package. Apply the Stage 06 / PR #16
source identity, no-follow and package checks before reading the files.
Require root `SKILL.md`, portable file paths and supporting regular files.
Reject dot components (including `.credentials.json`, `.env*` and `.git/`),
case-insensitive `auth.json`, `*.pem`, `id_*`, path collisions, traversal,
symlinks, hardlinks, reparse points, devices and other special entries.
Refuse the entire package before reading any denied file's contents. A
package may still contain sensitive text in an allowed file. Preview the
name, digest and destinations and obtain explicit user approval to send
the package.

The export frame and import payload have this exact format:

1. ASCII `LARPCC-SKILLS/1 LF`, then a canonical nonzero ASCII decimal
   manifest byte count followed by LF (at most 1 MiB).
2. Exactly that many UTF-8 bytes of one JSON object with fields
   `{name,sha256,files:[{path,size}]}`, without duplicate or unknown fields.
   Paths are unique, slash-separated relative paths with ASCII components,
   sorted in bytewise UTF-8 order. Require the exact Stage 06 component
   rules on both ends. Each component is 1–255 bytes. Refuse controls,
   `<>:"\|?*`, trailing dot or space, Windows device names (including
   with an extension), and the credential and dot names above. Require
   exact root `SKILL.md` and reject ASCII case-fold collisions among
   sibling components and file/directory prefix conflicts. Each size is
   a canonical decimal string. Cap at 1,000 files, 1,000 implied
   directories, 16 path levels and 16 MiB total file bytes. Empty directories
   are dropped. There are no link or device record types.
3. Exactly each file's declared bytes, in manifest order, then EOF. The
   Stage 06 digest is SHA-256 over each entry in order: u64 big-endian path
   byte length, UTF-8 path bytes, u64 big-endian file byte length, file
   bytes. Require the same digest in the manifest, the approved request
   and the receiver's computed value.

After the version probe, the sender launches
`larp --json --remote-request cc host catalog import skills ENTRY --sha256 DIGEST --expected-revision REV`
with the fixed script prologue and `exec` **without** stdin redirection.
Send only the script first, leaving stdin open. The remote Larp refuses
TTY streams and checks arguments and the destination revision before
emitting the exact readiness line `LARPCC-IMPORT/1 READY` plus LF. Send
no package bytes until that marker is received, so the shell cannot
consume them as script input. Bound the readiness wait to 5 seconds
within the 60-second transfer deadline. The readiness marker has a fixed
length and does not count toward the 1 MiB JSON receipt cap. Then send the frame and close
stdin. The marker is stripped locally. An early valid JSON error can
replace readiness. Only the ordinary JSON receipt or error follows a
readiness line, with the regular stdout/stderr caps. If the receiver refuses
early and stdin breaks, stop writing and continue draining within the
same deadline. Honor a valid JSON error with empty stderr and matching exit
status. An unverified outcome after dispatch remains unknown.

The receiver independently validates the manifest before accepting file
data, stages under a private managed temporary root, writes only confined
new regular files and verifies the digest before the destination catalog
transaction. Recheck the destination registry revision under lock before
commit. No sender-supplied destination path or metadata permissions.
Refuse and clean up staged data on all failures. After a crash, a subsequent
explicit import or recovery may remove abandoned stages using checked
managed-root paths and the same ownership, journal and lifecycle checks.
Leave active or unverifiable stages alone. Never resume a partial upload. Success is exit 0,
`host.catalog.import`. Malformed/truncated/trailing frames or a digest
mismatch are exit 1 `corrupt`, forbidden paths or entry types are exit 1
`unsafe_path`, limit violations are exit 1 `too_large`, stale revision
or name collision is exit 1 `conflict`, and invalid CLI syntax is exit 2
`usage` or `parse`. Transport loss after mutation dispatch surfaces as
unknown outcome. No scp, sftp, arbitrary remote paths or Larp binary
installation.

### MCP transfer

Export the Stage 07 MCP entry as one bounded UTF-8 JSON object plus LF
after a fixed `LARPCC-MCP/1` line plus LF. The object contains exactly one
transport: `{name,command,args,env_refs}` or `{name,url,header_refs}`.
Args and environment reference names are string arrays. Header references
are a sorted array of `{header,env_ref}`. Use Stage 07 validation and
canonical compact JSON in this field order, sorted references, original
argument order, literal UTF-8 with only JSON-required escapes and an LF.
The content digest is SHA-256 over the JSON bytes including LF, excluding
the prefix. Bound the JSON to 1 MiB and refuse definitions whose typed
apply argv exceed the per-word or script limits. Only Stage 07 supported
fields and safe args are transferable. Keychain references make an entry
unexportable and its catalog-list digest null. No keychain lookups, literal
values in secret fields, arbitrary shell snippets or provider credentials.

For a remote source, `larp --json --remote-request cc host catalog export
KIND ENTRY --expected-revision REV` writes the applicable prefixed frame
on exit 0 or an ordinary unprefixed JSON error on failure. Cap its stdout at
the applicable documented frame limit, including prefix and length line. The frame is a
private transport exception to the JSON stdout rule. The local CLI
validates it with the same bounded rules and matches the user-approved
digest before using any field. A local source uses the same canonical
serialization and validation in-process. Never display or log raw export
data or allow a source to choose a new destination.

The CLI reconstructs the typed `host catalog apply mcp` argv from verified
fields and sends the actual command, args or URL and environment/header
reference names through the ordinary quoted command path. The receiver
revalidates Stage 07 fields and checks the command file without executing
the server or contacting an endpoint. Reference validation checks supported
kinds and syntax only. It never checks whether an environment variable is
present, reads its value or looks up a keychain item. The receiver reports
an unavailable command as `not_found` and unsupported references as
`secret_reference`. Copy maps either destination error to `catalog_unavailable`. A copied URL does not cause a request during copy
or sync. Apply success is `host.catalog.apply`. The outer
`host.catalog.copy` returns its own common mutation receipt with the
destination revision for either kind.

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
| `stage13_hosts` | Strict configurable alias and direct DEST parsing, exactly one mode, no SSH on CRUD or stale routing, atomic changes, concurrent name reuse refuses, no remote deletion on remove |
| `stage13_argv` | Exact flags and one fixed SSH command across platforms, config mode keeps ambient config with fixed options only, direct mode disables config and passes agent selection unchanged, identity_file rejects relative/leading-hyphen paths, percent/dollar expansion, control characters and Windows UNC/device paths before spawn, exact spaces/quotes/dollars in paths and argv, attempted injected options/commands remain inert, no local SSH config or credentials inspected by Larp |
| `stage13_transport` | Missing client, host-key refusal and BatchMode prompt refusal with no fallback, alias mode overrides configured forwardings and PTY, Windows PE/version admission rejects scripts, aliases, current-directory lookup and MSYS/Cygwin clients, banner and post-quantum warning on stderr rejected and redacted, caps on both streams, deadlines/cancel/reap, no raw output or secret sentinels in errors, unknown mutation outcome |
| `stage13_protocol` | Missing/old/incompatible Larp, dash fixture with a non-executable absolute file and a directory both exit 44, unsupported OS including MSYS/Cygwin exit 45, escaped paths, reserved router error codes, malformed/framed/trailing/unknown schema and wrong command or exit, compatible version, receiver allowlist, closed native admission and remote LIVE refusal before effects |
| `stage13_revision` | Same profile IDs/names on different machines, re-add race and stale target conflict before effects, one-shot mutation on disconnect, no fallback or retry |
| `stage13_catalog` | Destination inheritance including MCP sync_all=true and skill future-profile defaults, isolated share revision/no-op and older writer invalidation, opt-in explicit sync, real skill bytes and MCP definition under explicit approval, reference names only, unavailable target command/reference and no credential values, same-name conflict and same-definition no-op, independent success amid partial failure |
| `stage13_skill_transfer` | Frame round-trip through fixed script/readiness, no payload before READY, TTY refused, truncated/trailing stream, wrong digest, invalid/duplicate manifest fields, oversize, credential-shaped filename, missing root SKILL.md, non-ASCII path, case-fold collision, symlink entry and traversal all refuse before commit. Early refusal after a broken pipe retains the valid error. Crash cleanup preserves active stages and outside sentinels. |
| App fixture suite | Decode new and old envelopes, add/remove/check states, revision/routing isolation, previewed copy and sync outcomes, no remote LIVE action, host-only SSH_AUTH_SOCK propagation, menu uses loaded data and does not poll, launch with N hosts and restore/wake/window/menu open spawn zero SSH processes |

Zero matching tests fails implementation acceptance. This planning PR runs
contract fixture validation only.
