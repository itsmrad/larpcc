# CLI JSON v1 (prospective freeze)

Normative schema: [cli-v1.schema.json](cli-v1.schema.json), JSON Schema 2020-12.
This is a planning contract, not shipped behavior. Import is **OUT OF SCOPE**;
fresh profiles only. No account/auth output is captured, parsed or inferred. Only explicit bounded
version text may be parsed by doctor --detect-versions (stage 08). The only exception is
the stage 11 usage exception (D14): usage windows, reset times and reset-credit counts
from the allowlisted sources in [stage 11](../stages/11-usage.md). Account identity and
auth inference stay forbidden; a plan appears only as the user-entered label or
the client-recorded LOCAL plan (D29), never from credentials.
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
the stage 11 `usage` envelope's `generated_at`/`captured_at`/`resets_at`/`runs_out_at`.
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
authorizes local dispatch of strictly verified ChatGPT.app on macOS, with a fixed
warning for untested/unknown versions. Claude and unsupported platforms remain
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
`profile.plan-label`; new error codes `live_disabled` and `statusline_exists`
(both exit 1). Grammar, sources,
reason codes, ordering, normalization and the pace formula are normative in
[stage 11](../stages/11-usage.md); pace test vectors are in
[usage-pace-vectors.json](usage-pace-vectors.json). `usage --live --json` is Larp's
own bounded network request, not native IO, so it is not `json_native_io`;
per-profile LIVE failures are row reasons in an `ok:true` envelope. Usage rows carry
no tokens, account IDs, emails, spend, cost or token counts; `plan` is the user label
or a client-recorded LOCAL plan value. `settings.usage.mode` carries the profile ID and
name for a per-profile override and null/null installation-wide;
`settings.usage.live|interval` carry null/null; `settings.usage.claude-statusline` and
`profile.plan-label` carry the profile ID and name.

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
