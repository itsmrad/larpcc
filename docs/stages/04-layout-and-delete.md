# Stage 04 — v3 seed, surfaces and deletion

Prospective contract; stages 01–03 remain historical. Import/adoption is
**OUT OF SCOPE** everywhere in this sequence; fresh profiles only. No Rust
changes in Phase 0. Preserve existing fail-closed launch, ancestry, ACL,
permission, hash admission, privacy and confirmation gates.

## Ordered seed and ownership

CORE first merges a validated v3 seed before dependent aliases/sharing/MAC work.
CORE exclusively serializes cli/main/registry/platform/Cargo/shared tests and
CI registration; MAC requests shared integration rather than editing it.
Stages 04→05→06→07→08 deliver CLI first; 09 desktop then 10 native GUI.
JSON [contract](../contracts/cli-v1.md) applies to each slice, implemented with
the seed and expanded only through its frozen variants. No concurrent writers
in shared files. Lead arranges independent QA/OCR and alone merges.

## Delivery split (lead decision)

The first implementation PR ships only the v3 model/layout, surface toggles,
JSON framing for implemented commands and locked revision preconditions. Legacy
v1/v2 reads remain available; all legacy mutations refuse `recovery_required`.
It does not expose migrate/delete/recover until their durable transactions,
confinement and acceptance tests are implemented in the follow-up PR. Dependent
modules consume the reviewed seed; this split does not mark full Stage04 complete.

## Grammar and persisted v3

Existing profile/provider commands retain grammar. Add:

```text
larp cc profile migrate
larp cc profile surface PROFILE cli|desktop on|off
larp cc profile delete PROFILE --yes [--logout]
larp cc profile recover PROFILE --yes
```

`forget --yes` remains metadata-only: folder retained, ID retired, no reattach.
It refuses profiles with aliases, sharing references, launcher ownership or
pending transactions (`conflict`); remove references explicitly first. It never
silently leaves active aliases pointing at a reused name. `delete` removes the
entire profile folder, including native-created files; this is its explicit
confirmed destructive boundary. No `--trash`, force or recursive arbitrary-path
option in this slice. `--logout` is optional, runs official qualified logout
before tombstoning, and is refused while rows are empty. Without it show warning
`keychain_retained`: OS keychain, native remote grants and Electron Safe Storage
may remain; removing a folder is not revocation. Larp never reads/removes those
stores. JSON warning is in the receipt; no confirmation prompt in JSON mode.

Registry `version:3`, numeric revision/next_id/profiles/bindings retained.
Profiles add `surfaces:{cli:bool,desktop:bool}`, `state:active|deleting` and
nullable deletion transaction ID. Defaults cli=true, desktop=false. Aliases
are an ordered array `{name,profile_id,surface}`; desktop bindings an array
`{provider,path,recorded_version}`. Paths for profile surfaces are always derived:
`profiles/<id>/cli` and `profiles/<id>/desktop`. No persisted user-selected home.
Desktop binding path is absolute, label is nullable; it is never qualification.
`provider bind` remains CLI binding; desktop bind grammar belongs to stage 09.
Registry remains identity-only, <=1 MiB; MCP catalog and owned manifests live
separately under the same registry lock. IDs and revisions overflow by refusal,
never wrap or reuse. Unknown/duplicate fields, IDs, names and references refuse.

## v2 migration transaction

Read-only v1/v2 list/show remains unchanged and reports legacy layout; mutations
requiring v3 refuse `recovery_required` until explicit `profile migrate`.
v1 can first use the existing v2 upgrade; migrate accepts v1 by the same
validated v2 logical representation. No automatic content migration on reads.
Migrate preflights the entire managed set under existing safety rules, all
profiles satisfying the bounded liveness checks below; occupied homes refuse.
Only empty or documented synthetic fixture homes are eligible in this delivery. If nonempty unknown/native data
exists, refuse without content reads or moves; no login data adoption/copy.

Use a durable root journal with original revision, immutable IDs, checked folder
identities, fixed phase and generated temporary sibling names. Validate the
journal as strictly as registry; never trust absolute/relative deletion paths
from it. For each v2 `profiles/<id>`, atomically rename whole directory to a
reserved sibling, create private container, rename sibling into `cli`, create
private empty `desktop`. Names and operation order are fixed by ID, not input.
No copying, recursion through old contents or changing existing permissions.
Journal+parent fsync precedes each destructive rename; persist progress after
each rename. While journal exists, block other mutations/launch and expose safe
recovery diagnostic. `profile migrate` resumes only if checked identities and
phase match; otherwise refuse for human recovery. Publish v3 only after all
moves durable, then retire journal durably. No ambiguous two-source layout,
rollback by guessing, or v3 downgrade. Crash tests inject every boundary. Older
binaries cannot safely participate during migration: quiesce them explicitly;
cooperative lock does not constrain unknown old clients.

This delivery migrates only empty legacy homes on Linux and macOS. Windows
refuses legacy profile-tree moves until a durable confined rename is available.
An interruption after creating a staging container and before recording its
identity requires manual recovery. Keep Larp and native clients stopped,
preserve the journal and both staged directories, and inspect the staged
container and the legacy sibling before making any repair. If you can establish
that the unrecorded container is the empty staging container from the interrupted
attempt, move it aside without touching the staged legacy home or the journal,
then rerun `profile migrate`. Leave ambiguous or changed directories in place.

## Delete transaction and confined removal

Acquire per-ID lifecycle guard first, then registry lock; shared order applies
to launch/auth/delete/share. Never hold registry lock across logout, prompts or
child waits. Revalidate snapshot after any unlocked interval. Refuse visible
Larp-managed activity (profile guard held), guard contention,
or safety failure. On macOS enumerate current-user processes and select
executables inside Claude.app or ChatGPT.app bundle contents before reading any
arguments. Keep the official main-app signature requirements and verify nested
helpers against the corresponding vendor team signature. An exact profile
`--user-data-dir` argument match refuses deletion. Skip a process that exits or
whose PID is reused during the scan, and skip denied path lookups that have yet
to identify a candidate. Once identified, a live candidate with unverifiable
identity or denied, malformed, or truncated arguments still refuses deletion.
Failed or oversized process enumeration also refuses. If the scan reaches its
two-second deadline, retry the whole scan once with a fresh two-second budget.
Never inspect process environment, tokens or account data.

The user must close sessions started outside Larp (including manual terminal
`CLAUDE_CONFIG_DIR` / `CODEX_HOME` sessions) before confirming `--yes`. This is
bounded local data hygiene, not proof against arbitrary same-user native clients
that close handles or change executable identity. Empty folders, an empty
qualification table, marker files and absence of a cooperative guard are not
independent proof of native inactivity. No broad `/proc` heuristic is required.
OS lock loss after parent crash does not establish child death: recover requires
explicit human quiescence confirmation (`--yes`) plus the same bounded surface
checks. Never kill unknown PIDs or automatically resume a transaction.

Under lock preflight reference cleanup and ownership drift **before** tombstone.
Record a bounded fixed transaction describing ID, expected revision, owned
artifact identities/hashes, catalog/skill references and phase. Atomically
publish `state:deleting` before removing anything. All reads may show tombstone;
launch/toggles/rename/forget refuse it. Delete/recover resumes idempotently.
First remove verified owned aliases/launchers and sharing references, then
remove the folder, then remove identity metadata and journal; next_id never
moves backward. The delete journal records the MCP cleanup alongside aliases
and skills. Recovery verifies the recorded render ownership and metadata,
removes the profile's switches and render, and advances the MCP catalog
revision once. A profile without MCP entries leaves the MCP catalog untouched.
Journal is the recovery source for multi-file catalog/manifest
cleanup; readers refuse affected state until committed. Never advertise an
atomic multi-file rename. Every journal step has fsync+idempotent resume semantics.
Drift before tombstone fails unchanged; drift afterwards leaves tombstone and
requires explicit manual resolution, never overwrites user edits.

Removal is relative to retained verified root/profiles/ID handles, no pathname
`remove_dir_all` after a check. Unix openat/unlinkat with no-follow, identity
rechecks and same-device restriction; Windows retained handles, no-delete-share
ancestors and handle-relative enumeration/deletion without following any reparse
tag. Symlink/reparse leaves are unlinked as leaves only, never traversed;
unsafe/unverifiable leaf handling refuses. Never read regular-file contents;
hardlinks unlink only the in-folder name, never mutate target bytes. Replaced
identities, mount points, junctions, permission failures, oversized enumeration
or activity detected by the bounded checks leave tombstone, not a fallback.
Bound 100,000 entries,
64 levels and 30 seconds per attempt; resume explicitly, no background job.
Manifest cannot enlarge this subtree. Forged path/hash metadata grants no
arbitrary deletion authority. Same-user privileged racing remains a limitation;
if native handle confinement cannot be proved, deletion on that OS stays refused.
Windows deletion and recovery refuse before publishing a tombstone or removing
an artifact. Linux and macOS use the retained-handle Unix implementation.
Unix rechecks the full retained ancestry after syncing and immediately before
unlinking a directory. The kernel does not make that check and the unlink a
single operation. Same-user or privileged namespace changes can still race that
interval. If a check cannot establish confinement, the attempt stops.

Lifecycle journal staging uses exclusive creation. Recovery only promotes an
occupied staging file when its bytes match the next journal record and the
published journal identifies the same transaction. Preserve unproven staging
files for inspection. An interruption before the first journal publication can
require moving the orphaned staging file aside manually.

## Owned-file amendment

Supersedes only the blanket future ban on writing provider config: Larp may
create **its own** render files, skills links, launcher artifacts and reviewed
config blocks. It still never reads auth/history/session/native config contents.
One bounded manifest per profile records version=1, profile ID, catalog revision,
relative allowlisted destination, kind, generation, SHA-256 for owned bytes,
link target or checked identity. External aliases/apps use separate fixed-root
ownership manifests, never profile-supplied absolute cleanup paths. Manifest is
not authority: destination and ownership independently validated each operation.
No clobbering existing paths, no broad config-file parsing/rewriting; changed
owned bytes are drift and refuse. Whole-folder delete alone includes unowned
native files after confirmation. See [threat review](../contracts/threats.md).

## Executable synthetic acceptance (implementation obligations)

Extend existing native harness, not a new framework. Tests named below must be
implemented and actually executed with `cargo test --workspace --locked stage04_`
(on native Linux/macOS/Windows and Linux MSRV); zero matches is failure.

| Test | Required discriminating assertions |
|---|---|
| stage04_migration | v1/v2 read unchanged; empty/synthetic migration, IDs/bindings/next_id preserved; cli+desktop layout; old schema refusal; nonempty unknown home untouched |
| stage04_migration_crash | crash each rename/journal/registry boundary; deterministic resume or safe refusal; two migrators serialize; no lost/outside bytes |
| stage04_delete | Delete removes the folder and cleans aliases and sharing references. Forget retains the folder and refuses while references exist. Verify ID retirement, --yes/--logout/JSON precedence and the keychain warning. |
| stage04_delete_escape | traversal/forged manifest/journal, symlink/junction swap, mount/hardlink/outside sentinel; no outside deletion, no token-content reads |
| stage04_delete_recovery | crash every cleanup boundary; tombstone blocks all consumers; busy CLI/desktop, surviving managed child and uncertain inspected app identity refuse; concurrent edits/drift retain tombstone |

Every fixture uses private temporary LARPCC_HOME and names claude-work/codex-45,
synthetic children only, bounded waits and finally cleanup of all test artifacts.
No existing homes/wrappers/skills touched. No cargo is needed for this planning PR.

All registry-backed commands accept global `--expected-revision REV` as frozen in
[CLI JSON v1](../contracts/cli-v1.md#request-precondition). Check atomically under
the registry lock before resolving profile names or preparing effects; mismatch
is exit-1 `conflict`. GUI mutations supply the displayed revision. Acceptance
includes rename ID 2, reuse old name for ID 3, then stale-revision delete/recover
refusal with neither profile changed. Recovery receipts retain tombstoned ID/name.
