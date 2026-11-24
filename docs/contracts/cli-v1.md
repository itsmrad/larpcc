# CLI JSON v1 (prospective freeze)

Normative schema: [cli-v1.schema.json](cli-v1.schema.json), JSON Schema 2020-12.
This is a planning contract, not shipped behavior. Import is **OUT OF SCOPE**;
fresh profiles only. No account/auth output is captured, parsed or inferred. Only explicit bounded
version text may be parsed by doctor --detect-versions (stage 08). There are two bounded
exceptions with distinct scopes:
1. **Stage 11 usage (D14):** usage windows, reset times and reset-credit counts from the
   allowlisted LOCAL records and the opt-in LIVE usage response in
   [stage 11](../stages/11-usage.md). A plan appears only as the user-entered label, the
   client-recorded LOCAL plan (D29) or the Codex LIVE response plan (D32), never from
   credentials. Provider-derived plans are never stored; only the user-entered
   `plan_label` persists (in `usage.json`). No native client output is read.
2. **Stage 14 account summary (D25/D26):** `status.account` parses an admitted official
   status run through the exact row's parser ([§Account summary](#account-summary)) and
   extracts the sign-in state plus any explicitly reported email and plan (either may
   be absent), shown and never stored.
Outside these two, account identity and auth inference stay forbidden.
SwiftUI later executes the installed CLI; no library extraction.

## Framing and compatibility

`--json` is a global option, accepted before or after `cc` and its subcommands
(before `--`; everything after that delimiter is child argv). Success and errors
both emit exactly one UTF-8 JSON object plus LF to stdout; stderr is empty.
No progress, notices, prompt, native output, ANSI or debug logging in this mode.
Warnings are safe `code,message` objects in mutation data. Human mode retains
stage-3 pass-through and notices. Help/version with `--json` emit `help`/`version`
envelopes, exit 0, no root IO; ordinary help/version remain human text.
Missing commands, invalid arguments and missing `--yes` emit error envelopes,
exit 2. An early argv scan recognizes literal `--json` only before the child
separator; parser failures use `command:"parse"`, never echo raw argv. Valid
parsed command errors use its dotted schema discriminator. Unknown options,
invalid Unicode and values are described without echoing the supplied value.
All local operational errors exit 1; metadata success exits 0. Rebind without
`--replace` is `conflict`, exit 2. No error embeds a raw OS/provider/config error.
Codes are stable; messages are safe explanatory text, not a parsing API.

`run|login|logout --json` always fail `json_native_io`, exit 2, before any
root IO or child. `status --json` is explicitly **local metadata status**, not
native auth status: it never invokes native `status`. Human `status` retains
stage-3 native semantics and early qualification refusal. `profile delete
--logout --json` likewise fails `json_native_io` before effects. No implementation
may redirect/capture native stdout to manufacture a JSON account result.
Human child exit statuses pass through as before (including full Windows u32);
they are outside the JSON 0/1/2 contract. Output-write failure exits 1, with no
second envelope; a broken pipe may therefore yield incomplete/absent JSON.

All schema properties are required unless explicitly nullable. Unknown fields
are rejected by v1 producers' validation; consumers must select `schema` first
and refuse unsupported major schemas. No arbitrary account objects or secret
values can be added. `revision` is a decimal string snapshot revision, null
only for help/version or no registry. Profile IDs are canonical nonzero u64
strings in JSON (registry persistence keeps numeric u64). Validate numeric range
in addition to the schema regex. Names also exclude Windows device names and
`larp`. Paths are display strings only; non-Unicode paths use the existing
escaped display representation and must never be round-tripped as command
arguments.

### Request precondition

`--expected-revision REV` is a global option before the child `--` separator.
REV is a canonical decimal u64. GUI mutations and app-open requests MUST supply
it from the displayed snapshot; human callers may omit it. It is supported by
all registry-backed commands, including read-only list/show/status/doctor.
Under the same registry lock used to resolve names and prepare the operation,
compare REV to the current revision **before name resolution or any mutation,
probe, prompt or dispatch**. A missing registry has revision 0. Mismatch returns
`conflict`, exit 1, without effects; the GUI refreshes and requires renewed user
intent, never automatically retries a destructive action. Retain existing
snapshot revalidation through apply/dispatch. Help/version reject this option
with `usage`, exit 2; closed native-operation and JSON-native-IO gates retain
precedence and must not gain root IO to check a precondition.

For example, after reading revision 7 and `codex-45` ID 2, the GUI sends
`larp cc profile delete codex-45 --yes --expected-revision 7 --json`.
If another client renames ID 2 and reuses its name for ID 3, the revision has
changed and deletion refuses before selecting ID 3. Matrix toggles and alias
mutations use the same precondition, including operations spanning all profiles.

List order: profiles by ASCII name, aliases by ASCII name, MCP/skills by ASCII
name, switches by numeric ID, detections by provider (`claude`,`codex`) then
surface (`cli`,`desktop`), diagnostics by code then message. Matrix columns
match profile name ordering; every row has exactly one boolean per column.
Rows include disabled entries. No timestamps, PIDs or nondeterministic keys, except
the stage 11 `usage` envelope's `generated_at`/`captured_at`/`resets_at`/`runs_out_at`
and the row fields `captured_at`/`resets_at`/`runs_out_at` inside `status.account.usage`
(`generated_at` belongs only to standalone `usage`).
Secret references sort by kind then name. Schema constraints are structural;
unique names/IDs, valid cross references, u64 range and matrix dimensions are
semantic validation requirements. Serialized envelope maximum is 1 MiB;
overflow fails `too_large` before any mutation commits.

## Local status and desktop coordination

Bindings expose `path`, `recorded_version`, `detected_version`, `state`,
`qualified`. A detected version comes only from an explicitly requested bounded version-only
probe or reviewed reliable static metadata; otherwise null. Recorded version is a user-supplied label, not qualification.
An explicit `doctor --detect-versions` uses the bounded version-only preparation
path in [stage 08](../stages/08-json-launch-doctor.md); no artifact descriptor is
approved by this planning PR. Default doctor remains read-only. `native_rows:[]`,
`native_auth:"not_observed"`, and `qualified:false` remain fixed until a separately
reviewed qualification/schema amendment. Default doctor is metadata-only and never
reads auth/config/session contents, keychain, process argv or endpoints.
`doctor [PROFILE] [--detect-versions]` and JSON `status PROFILE` expose the same status shape;
doctor without a profile returns null profile and all binding diagnostics.
Missing/unsafe homes are diagnostic states, not evidence of being signed out.

`app open PROFILE --json` success is a **local dispatch receipt** only:
`profile_id,provider,surface:"desktop",outcome:"launched"|"focused",
native_auth:"not_observed"`. It says nothing about account identity, successful
authentication, UI readiness or isolation. The [stage09 lead desktop amendment](../stages/09-desktop.md#lead-desktop-admission-amendment--2026-11-17)
authorizes local dispatch of strictly verified ChatGPT.app on macOS following
the D34 attribution re-test, with a fixed warning for untested/unknown versions.
Claude and unsupported platforms remain
`qualification_pending`. CLI native rows remain empty and `qualified:false`;
local desktop dispatch does not qualify accounts or storage isolation.
CORE resolves one coherent v3 profile/provider/cli/desktop/binding snapshot,
validates ancestry/ACLs and paths, and revalidates snapshot before dispatch.
MAC retains checked paths through open/focus, revalidates app identity and
launch target, refuses unknown focus matches, and returns safe local outcomes.
Do not freeze an exact Rust signature here. MAC consumes the merged v3 seed;
CORE owns shared CLI/main/registry/platform/Cargo registration. Binding path
and recorded version are included in profile list/show through desktop_binding.

## Command map and examples

Space-separated command paths map to dotted `command` values. Existing profile
and provider mutations and all stage 04–09 metadata mutations use the common
`changed,profile_id,name,warnings` result (null where no profile/name applies).
Profile add/rename/forget/delete/recover/surface receipts always carry the
non-null affected ID and profile name (post-rename name for rename). Deletion
and recovery retain these from the tombstone even after record removal.
Alias add/remove and per-profile skills enable/disable receipts carry the
non-null target profile ID; their `name` is the alias/skill name. MCP
single-profile enable/disable carries its target ID and server name; `--all`
uses null ID and server name. Global catalog mutations use null ID and the
catalog entry name; all-profile sync/mode/provider/app-bind operations use null
ID/name. `changed:false` keeps the same command-specific identity requirements.
`profile migrate` is explicit and has null ID/name. CLI surface toggles use
`profile surface PROFILE cli|desktop on|off`. No toggling qualifies a launch.
`mcp sync` and `skills sync` render/apply all profiles without starting anything.
MCP/skills list exposes safe metadata and references, never raw command/env/URL
values. Matrix row `enabled` is the effective explicit switch vector.
See [examples](cli-v1.examples.json) for SwiftUI decoding fixtures including
profile/provider/surfaces/desktop, alias, MCP, skills, matrices, status/doctor,
mutation, parser failure and closed desktop dispatch.

## Usage limits (stage 11, additive)

Additive amendment, schema stays 1. Read envelopes `usage` and `settings.usage`;
mutation receipts `settings.usage.live|mode|interval|claude-statusline` and
`profile.plan-label`; new error codes `live_disabled`, `statusline_exists` and
`statusline_unverifiable` (all exit 1). Each usage window carries its own
`captured_at`; extra-window labels use a fixed Larp vocabulary and never carry
provider bucket names; `settings.usage` reports `live.state`
(`off|on|needs_acknowledgement`). Grammar, sources,
reason codes, ordering, normalization and the pace formula are normative in
[stage 11](../stages/11-usage.md); pace test vectors are in
[usage-pace-vectors.json](usage-pace-vectors.json). `usage --live --json` is Larp's
own bounded network request, not native IO, so it is not `json_native_io`;
per-profile LIVE failures are row reasons in an `ok:true` envelope. Usage rows carry
no tokens, account IDs, emails, spend, cost or token counts; `plan` is the user label
(`user`), a client-recorded LOCAL plan (`client_local`) or the Codex LIVE plan
(`live_endpoint`). `settings.usage.mode` carries the profile ID and
name for a per-profile override and null/null installation-wide;
`settings.usage.live|interval` carry null/null; `settings.usage.claude-statusline` and
`profile.plan-label` carry the profile ID and name.

## Remote

[Stage 13](../stages/13-remote-hosts.md) adds host commands and a global
`--host NAME` selector before the child `--`. Existing local invocations and
envelope shapes are preserved. A forwarded invocation returns the same dotted
command and data shape as the target's metadata command. The consumer binds
the response to the route used in its request. There is no host field in the
forwarded envelope, and names, IDs and revisions are machine-scoped. An app
that supports this amendment sends a remote command only through a compatible
local CLI and validates new envelopes with the amended schema. An older CLI
can reject the new syntax as `parse`.

| Command | Success data | Revision domain |
|---|---|---|
| `host.add`, `host.remove` | Common mutation receipt, non-null host `name` and null `profile_id` | Local host store |
| `host.list` | `{hosts:[Host]}`, ASCII name order, unique names | Local host store |
| `host.show` | `{host:Host}` | Local host store |
| `host.check` | `{name,status}` for the explicitly checked host. Status is `ok`, `unreachable`, `larp_missing` or `incompatible`. A classified probe failure is a completed check with exit 0. | Local host store |
| `host.catalog.list` | `{kind,entries:[{name,all_machines,content_sha256}]}`, ASCII name order, unique names. The digest is null when the entry cannot be exported. | Selected machine's registry. Report `"0"` with an empty list when absent. |
| `host.catalog.share`, `host.catalog.apply`, `host.catalog.copy`, `host.catalog.import` | Common mutation receipt, non-null entry `name` and null `profile_id` | Selected machine's non-null registry revision. For copy, this is the destination. |

`Host` is `{name,ssh,larp,larpcc_home,identity_file}` with nullable paths.
The two host-add options are mutually exclusive. `--ssh-config-host ALIAS`
uses `ssh:{mode:"config",alias}` and requires a null `identity_file`.
`--ssh USER@HOST[:PORT]` uses `ssh:{mode:"direct",destination,resolved}`
with the explicit port in `resolved`. Direct mode allows an optional
`--identity-file LOCAL_PATH`. Paths and destinations are display data in
the app. Host-store success revisions are canonical decimal u64 strings,
including `"0"` for a missing host store. For an absent catalog registry, the
GUI uses the returned `"0"` as its expected revision.

`host.catalog.share` advances the selected registry revision once when the
flag changes and preserves it on a no-op. Copy uses an approved content
digest and independent source and destination revision checks. Catalog
apply and skills import use the destination's inheritance rules, previewed
against its registry revision.

Stage 13 defines a private framed SSH stream for catalog exports and for
`host.catalog.import skills`. Export success is a private frame rather than
a CLI JSON success variant. Export errors and import results use schema 1.
The transport consumes the import readiness marker before reading its JSON
receipt. App callers use `host catalog copy`, which emits one ordinary JSON
envelope and never returns payload bytes. See the
[transfer protocol](../stages/13-remote-hosts.md#skills-transfer) for exact
framing, limits and import exit codes.

For host-store commands, `--expected-revision` is a host-store precondition.
For forwarded commands it guards the remote registry. Forwarded app reads and
mutations also require `--expected-host-revision` (a canonical decimal u64),
which guards local routing. A direct CLI caller may omit either precondition.
The selector and host revision never pass through SSH. A routing or target
conflict returns exit 1 under the requested command and requires renewed user
intent. Host remove requires `--yes`. Closed native admission and JSON native
IO refusal keep their early precedence.

New operational error codes are `ssh_unavailable`, `ssh_failed`,
`remote_larp_missing`, `remote_incompatible`, `remote_timeout`,
`remote_output`, `remote_outcome_unknown` and `catalog_unavailable`, all
with exit 1. Error messages contain fixed safe text. An unknown mutation
outcome requires an explicit read before deciding whether to retry. The
ordinary JSON `too_large`, `conflict` and other applicable codes remain valid.
An unsupported route/operation exits 2 with `usage` or `unsupported`.
The local router reserves `ssh_*`, `remote_*` and `catalog_unavailable`.
Those codes in a forwarded reply make it invalid. After mutation dispatch,
an invalid reply leaves the outcome unknown.

Remote profiles use LOCAL usage mode only. If the destination has LIVE enabled,
forwarded reads still use LOCAL and the receiver refuses requests to read or
enable LIVE before accessing tokens or contacting a provider. This applies
independently of the app's controls.

## Validation without installations

Run `python3 docs/contracts/validate.py` from the repo. This stdlib-only fixture
check resolves local refs and checks the schema vocabulary used here; it is
not a general JSON Schema implementation. It also checks negative fixtures,
semantic dimensions and numeric bounds. If an already available 2020-12
validator exists, validate the schema and examples with it too; install nothing.
Implementers add Rust serialization/CLI e2e fixtures against these exact examples,
including stderr/stdout/exit assertions. Planning validation does not prove CLI
implementation or native launch behavior.

Skills source confinement uses `forbidden_source` with a fixed message and no
source path. Recorded desktop version labels contain 1–128 ASCII letters, digits,
`.`, `_`, `+` or `-`; labels remain unverified metadata.

## Account summary

[Stage 14](../stages/14-account-summary.md) adds one success variant,
`status.account`, under schema 1. It amends three statements above for this
variant only: "No account/auth output is captured, parsed or inferred",
"`status --json` ... never invokes native `status`" and "No implementation may
redirect/capture native stdout to manufacture a JSON account result". Every
other command keeps them.

| Invocation | `command` | Data | Runs official client |
|---|---|---|---|
| `status NAME [--json]` | `status.account` | `{profile,provider_version,account,usage,diagnostics}` | Only when an exact Status row admits |
| `status NAME --no-account [--json]` | `status` | Frozen `status` data, unchanged | Never |

`--no-account` is accepted only by `status`. Elsewhere it is `usage`, exit 2.
An older CLI rejects it as `parse`, exit 2. `doctor` is unchanged.

`--expected-profile-id ID` is a new public global option before the child
`--`, accepted only by `status` in this amendment (elsewhere `usage`, exit 2;
an older CLI rejects it as `parse`). ID is a canonical nonzero decimal u64,
and an invalid value is `usage` without echo. Under the registry lock, after
the `--expected-revision` check and before any guard, probe or spawn, NAME
must resolve to ID, or the command fails `conflict`, exit 1, with no effects.
It applies to public `status` in both modes, local and `--host`. GUI
account-status requests MUST send both preconditions from the displayed
snapshot, plus stage 13's `--expected-host-revision` when routed. The router
checks the host revision locally before SSH and never forwards it. Human
callers may omit any of them. It adds no response field. Remote metadata
status forwards `--expected-profile-id` without the account marker to a
stage 14 receiver, which enforces it. A stage 13-only receiver fails closed
with `remote_incompatible`; the ID is never dropped. For a request carrying
`--remote-account` or `--expected-profile-id`, and after stage 13's transport,
deadline and stream-cap checks (exit 255 stays `ssh_failed`), a fully
validated reply with exit 2, empty stderr, `error.code` `usage` and an allowed
discriminator (the requested `status` or `status.account`, or `parse` for a
parser rejection) maps to local `remote_incompatible`, exit 1. Any other
invalid reply stays `remote_output`. The expected success
discriminator follows the request mode: `status` for metadata,
`status.account` for account. Under a
stage 13 `--host` route (implemented with stage 13), the remote Larp runs its
own account status and the forwarded envelope is the same `status.account`
shape with no host field. Under `--remote-request`, an unmarked `status` is
always metadata-only. Account mode requires the internal `--remote-account`
flag, valid only with `--remote-request` and checked by the receiver before
any effect. The local router forwards exactly the `--expected-revision` and
`--expected-profile-id` its caller gave, with no extra SSH read. Before
display, the router validates the reply like any forwarded read and also
requires the requested name, the sent profile ID (if sent) and a success
`revision` equal to the sent `--expected-revision` (if sent). Any mismatch
is `remote_output` with no account data. A remote without stage 14 support rejects the unknown
flag, which yields `remote_incompatible`. Deadlines are 16 s per SSH child
and 33 s for the app bridge, as in stage 13. No separate example is
needed because the shape is identical. See
[stage 14](../stages/14-account-summary.md#remote-account-status).

`account` is `{state,email,plan}`. `state` is `signed_in`, `signed_out`,
`unknown`, `not_qualified` or `unavailable`. Email and plan may be non-null
only for `signed_in`, and each is null when the row's exact shape lacks it.
Email is printable ASCII `local@domain` with no further `@`, at most 64 and
253 bytes. Plan is 1–64 printable ASCII with no leading or trailing space.
A value outside those forms makes the parser return `unknown`. These values
are display data, never stored by producer or consumer, and never a parsing
API. `provider_version` is the admitted row's exact version, or the approved
version-only probe's, or null. It is always null for `unavailable`.
`usage` is the selected profile's stage 11 LOCAL usage row (`usage_profile`,
from the same reader as `usage PROFILE --local`, `mode:"local"`, never LIVE
data or a `live_endpoint` plan), or null when that reader is unavailable. The
stage 11 timestamp exception covers its `captured_at`/`resets_at`/`runs_out_at`. `revision` is a non-null canonical decimal u64.

Exit codes: every completed observation, including `signed_out`, `unknown`,
`not_qualified` and `unavailable`, exits 0 with one envelope and empty stderr.
`busy` and `conflict` (the snapshot changed before dispatch or before
publishing) exit 1 and carry no account data. Errors use the `status.account`
discriminator. `run|login|logout --json` keep failing `json_native_io`.

Compatibility: a strict v1 consumer that does not know `status.account`
rejects it. Consumers that must not cause provider activity send
`--no-account`. The Mac app's frozen schema copy carries this variant, and its
test asserts that decoding still fails closed until the app step adds support.
Profile extras fields (`display_name`, `launch_args`) are not part of this
amendment. They land with the registry v4 bump as a separate amendment.
