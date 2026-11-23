# Stage 07 — MCP catalog, switches and renders

Import/adoption is **OUT OF SCOPE**; fresh profiles only. Definition/rendering
only: no server execution, network or OAuth. Depends on stage 04 owned manifests;
CORE owns share renderers/catalog, shared registration and tests.

```text
larp cc mcp add NAME --command ABSOLUTE [--arg SAFE_ARG]... [--env-ref VAR]...
larp cc mcp add NAME --url SAFE_URL [--header-ref HEADER=ENV_VAR]...
larp cc mcp remove NAME
larp cc mcp enable|disable NAME PROFILE|--all
larp cc mcp list
larp cc mcp matrix
larp cc mcp sync-all on|off
larp cc mcp sync
```

Exactly one transport, duplicate names/keys rejected, no shell interpolation.
`mcp.json` separate strict version=1 bounded catalog with revision, sync_all bool,
server definitions and explicit profile-ID switch matrix under registry lock.
Default sync_all=false, new definitions disabled. Turning on explicitly enables
all current definitions on all active CLI profiles and makes future definitions
and future profiles enabled; off preserves current switches but stops inheritance.
Stage 13 catalog apply follows the destination's `sync_all` value for a new
entry. An identical existing entry keeps its switches.
Per-profile switches always may override; --all is one bounded transaction.
Removal deletes switches and renders transactionally. Unsupported surface is
refused, never silently mapped. List/matrix JSON exposes reference names only,
never the full definition, and follows CLI v1 ordering/dimensions.

## Secret boundary and rendering

No literal secrets anywhere: args, command paths, URLs (userinfo/query/fragment
forbidden), env or headers. No arbitrary --env/--header literals. Env references
are identifier names, keychain references identifiers only: catalog serialization
may support `{kind:keychain,name}` but CLI add does not resolve/read them and
render refuses unsupported references. No `security`/keyring invocation.
Allow command paths and arguments only through a reviewed non-secret adapter
field allowlist; initial --arg values are exactly `--stdio` or `--transport=stdio`; all others
refuse until a reviewed adapter amendment. Commands are validated absolute local
file paths, checked component by component on add, catalog load and journal
replay. The [credential shape list](../contracts/threats.md) covers known API
token prefixes, Bearer markers and JWTs at component or punctuation boundaries,
including backslashes in Unix paths. Fixed-width AWS and Google key shapes
are checked even inside a word. Mixed-case runs of 20 or more ASCII
alphanumeric characters are refused when they contain a digit or end in
base64 padding. Hyphens, underscores and dots separate runs, preserving
package names, GUIDs and hash paths. On add, ambiguous values refuse
`secret_literal` without echo. Invalid persisted definitions require recovery.
Initial URL permits https origin and exactly `/mcp` or `/sse` path only, no
percent-encoded credentials or other components; loopback http requires a later
reviewed adapter. Names/hosts are not arbitrary secret carriers: ambiguous values
refuse, and no secret-detection completeness claim is made. Env-ref identifiers
alone are safe to show. References must be valid, unique and supported by exact
renderer; absent reference/support fails `secret_reference` before writes/child.
Never expand env values into argv, catalog, render, diagnostics or logs.

Claude render owns a separate JSON file below profile-owned render directory;
exact qualified CLI will pass `--mcp-config` plus reviewed `--strict-mcp-config`.
Codex render owns a separate overrides description, translated to exact `-c`
argv only by a reviewed qualified adapter. Do not read/merge config.toml or
.claude.json; no marked native config block in this slice. If a required reference
cannot be represented by the native format without secret materialization,
refuse that adapter, not a plaintext fallback. Generated config contains only
native-supported reference syntax; qualification must prove it. Desktop renders
remain unsupported until MAC proves app config location and reference handling.
No inference from CLI flags to desktop behavior.

Names in provider render become `<catalog-name>__p<ID>` to avoid known Codex
MCP keyring namespace collisions, with checked provider length limits; collision
refuses. This is not OAuth isolation proof; live MCP OAuth stays disabled pending
independent native lifecycle qualification. Claude OAuth scope remains unknown.

Render pure plan from one snapshot, max 1 MiB/catalog, 256 servers, 256 profiles,
1 MiB per render, 16 MiB total transaction. Preflight all files and manifest hashes
and privacy, journal then atomic per-file replace+fsync; commit snapshot and clear
journal only after the final manifest matches the render identities and hashes
on disk. Recovery verifies completed and unchanged renders before replay writes.
A combined profile update stores MCP intent in the skills journal and atomically
hands off to the MCP journal after skills commits. If interrupted, finish the
current journal owner with `skills sync` or `mcp sync` as appropriate. Stale
revision or drift refuses and leaves any existing journal pending.
Revalidate at launch, no stale renders, no implicit repair on list/doctor/launch.
See [threats](../contracts/threats.md); preserve stage-3 config/admission refusals
until exact renderer+args+secret-reference amendment independently reviewed.

Implement and execute native `cargo test --workspace --locked stage07_`:
`stage07_matrix` defaults/sync-all/overrides/new profiles/rename/delete dimensions;
`stage07_secrets` invented literals in each args/url/env/header position, malformed
and unsupported/missing references, no keychain reads, zero secret bytes in output;
`stage07_render` golden deterministic Claude/Codex reference-only files and limits;
`stage07_drift` manifests/tampering/path traversal/reparse/partial write/rename race;
`stage07_recovery` concurrent toggle/delete, fault each journal/fsync/replace.
Zero matches fails; synthetic only, temporary root/names claude-work/codex-45,
finally remove all renders/catalog/profile/fixtures, no MCP child or account.
