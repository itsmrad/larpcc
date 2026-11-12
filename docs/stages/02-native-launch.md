# Stage 2 — Native launch engine, qualification closed (draft v2)

2026-11-11. Proposed complete contract for `docs/stages/02-native-launch.md`; not frozen and no
implementation authorized by this artifact. Lead-approved D1–D6 are incorporated below. No real
provider, auth, credential, model, or network operation is part of this stage's acceptance.

## 0. Baseline, reconciliation and freeze

Inspected fixed stage-1 H2 `bd2b838ef4cc890511bc744260707d593eb2cb12`, including complete `plan.md`
and `docs/stages/01-profiles.md`. Final M1 is `2cbe7affca3461e988cd90824540e5a0374c4893`, after
first merge `32fde5956f7c9f7e53bb118a2ddeb133ce1a3685`. Local Git-object reconciliation confirms
**both trees are `02e70b8e3f46296cc58f86569c15e82f295619b5`**, and `git diff H2 M1` is empty,
including parent docs. Thus this draft's H2 interface/doc assumptions also hold at M1. This replaces
the original draft's obsolete `08505e6`/`4c5e017` anchors. Archived M1 parent docs accompany this
artifact.

Lead-supplied final-stage-1 evidence: QA ACCEPT all 49 IDs and T01–T09; exact-H2 native Linux 42,
macOS 40, Windows 34 and Ubuntu MSRV 42 tests passed, zero ignored; combined independent OCR
delegate coverage 11/11 + 29/29 covers all 37 diff files, no blockers. This planner verified tree
equivalence, not those runtime results. The earlier pending CLI review is now reported complete by
the lead.

Before code: lead creates a visible planning PR from M1, applying the exact parent edits in the
changes artifact and this stage document together. Fresh independent plan review must accept it;
merged planning head is M2. If any M1/base/doc assumption changes, repeat the relevant
reconciliation and update the contract before freeze. No stage-2 source work before that gate.
Parent Windows whole-managed-tree entry criterion remains mandatory; runtime qualification remains
closed.

**Stage-2 supersession (effective only after the stage-2 planning merge):** this document records
stage-1 behavior and evidence. Stage 2 extends registry/schema and strengthens managed
metadata/ancestor checks per [this contract](#5-managed-tree-ancestry-and-transactions): fixed
lock/temp safety on reads, unsafe stale-temp refusal, safe opened-handle validation, protected new
Windows objects and verification-only existing objects. In particular historical R10 stale-temp
symlink cleanup becomes refusal, and new Windows safety fixtures replace inherited-ACL privacy
deferral. Per-home diagnostic status and metadata-only rename/forget remain. Before any provider
launch/auth the entire required managed set is verified; no existing-tree ACL repair or
provider-content access is authorized. Ordinary stage-2 run stays disabled pending stage-3
qualification. These are prospective stage-2 changes, not retroactive stage-1 claims.

## 1. Delivered behavior and boundary

Stage 2 delivers v2 bindings, `provider bind|list`, ordinary `run` refusal, a pure launch plan,
native Unix exec and Windows spawn/wait **exercised only in a dedicated synthetic build**, and
filesystem/ACL validation. Engineering completion means these synthetic/platform gates pass. It does
not mean provider launch is enabled or account isolation is qualified.

Ordinary builds, including release and installed binaries, always reject a syntactically valid `run`
with `QualificationPending` **immediately after clap parsing and before root lookup,
environment-policy inspection, registry IO, version probes, or process creation**. There is no
runtime unlock: not an environment variable, flag, basename, executable magic, fake marker, binding
field, or path. Unknown/missing profile in an ordinary `run` therefore also reports qualification
pending; a missing/invalid NAME is still a clap usage error. Provider binding and metadata commands
remain available.

Use one Cargo feature `synthetic-launch`, default off, and a separate test driver binary
`larpcc-synthetic` plus native fake binary `larpcc-fake-provider`, both `required-features =
["synthetic-launch"]`, `test = false`, sources under `tests/support/`. The normal `larp` dispatch
**never** enables launch, even in an all-features build. The synthetic driver includes the existing
bin crate modules by path (no new library/workspace crate), invokes the same
registry/planning/execute functions, and accepts the same run grammar for testing. Feature-gated
engine modules are compiled into the synthetic driver; the ordinary binary keeps only the refusal
path and metadata functionality. Test-only driver dispatch cannot be selected through `larp` argv.
Synthetic builds use a separate target directory. Synthetic integration-test targets also declare
required-features or are wholly cfg-gated so default tests never reference absent CARGO_BIN_EXE
helper paths; default gate tests use only CARGO_BIN_EXE_larp. Ordinary build/install/package
commands explicitly use `--bin larp` without the feature; release archives contain neither helper.
Stage 5 must test archive absence and ordinary refusal. `test = false` alone is insufficient
packaging protection.

Stage 3 must freeze documented provider/version/interface/backend/OS and installation compatibility
before enabling any real launch. Changed/unknown combinations fail closed; qualification cannot be a
permanent Boolean attached to a path that self-updates. It must settle supported
argument/environment grammar, auth/backend scope and daemon behavior. Codex shared-server
uncertainty blocks enablement even for sequential profiles; inject no unknown `--no-daemon`. No real
qualification or auth is brought into stage 2.

No PATH search, wrapper discovery/execution, shell shims, downloads, aliases, job objects, daemon
management, config codec, async runtime, new framework or credential inspection. Native official
executable targets are the intended later supported installations; stage-2 format checks prove
neither official provenance nor architecture validity nor installation compatibility. No in-session
login workaround. Auth/server CLI filtering cannot guarantee that native interactive auth, settings,
MCP OAuth or external policy stays inside a home.

## 2. CLI and rendering

```
larp cc provider bind <PROVIDER> <EXECUTABLE> [--replace]
larp cc provider list
larp cc run <NAME> [-- <ARGS>...]
```

`PROVIDER` is the existing exact lowercase enum. EXECUTABLE parses as `PathBuf` from `OsString`, not
a lossy String; validation then requires Unicode for serialization. NAME uses `ProfileName`.
Forwarded args are `Vec<OsString>` with clap `last = true`; any forwarded token requires the outer
`--`. Preserve empties and OS-native representation. No implicit profile, environment default, root
flag or additional globals. Bare `provider` prints help to stderr, exit 2. New about texts: provider
`Manage native provider bindings`; bind `Bind an absolute native executable`; provider list `List
provider bindings`; run `Launch a profile (qualification pending; disabled in this build)`.
Synthetic driver labels its help as test-only. Regenerate exact help fixtures; existing profile help
remains metadata-only.

Bound output, with newline and exit 0:
- `Bound <provider> to <p>.`
- `<provider> is already bound to <p>.`
- `Rebound <provider> from <old> to <p>.`

Provider list always prints header `PROVIDER  STATE  EXECUTABLE`, column widths computed as in stage
1, two spaces between padded columns, final column unpadded; rows in `Provider::ALL` order. STATE is
`ok`, `unbound`, or `invalid: <reason>`; executable is stored path or `-`. No root yields both
unbound rows, without creation. `ok` means current path validation only, never qualification. Exact
fixtures cover every state and long escaped paths.

All printable paths, including existing profile outputs, errors and hints, go through one rendering
helper: start with `to_string_lossy`, replace each control character and each Unicode format/bidi
control (U+061C, U+200B–200F, U+202A–202E, U+2060–206F, U+FEFF) with Rust `escape_default` notation,
preserve other characters, no color. Escaping affects display only; execution/storage retains
original values. Do not interpolate raw argument/env values or arbitrary parsed keys into errors.
Clap errors for OsString EXECUTABLE must also pass the no-raw-control fixtures; sanitize rendered
diagnostics at the CLI output boundary if required. No filesystem calls in formatting/hint
selection.

Exit dispatch: 0 success/help; 2 clap usage and `AlreadyBound`; 1 every other Error/stdout failure.
Existing BrokenPipe behavior remains. Synthetic success adds no launcher output. Unix replacement
gives native signal status; Windows preserves full u32 exit codes (process exit via
`std::process::exit(code as i32)`, verified natively).

## 3. Registry v2 and mutation rules

Retain stage-1 strict schema/counter/ordering/serialization rules, adding `bindings` after
`profiles` in the top-level pretty serialization order. Example logical object:
```
{"version":2,"revision":4,"next_id":2,"profiles":[{"id":1,"name":"work","provider":"claude"}],"bindings":[{"provider":"claude","executable":"/opt/claude"}]}
```

Binding field order is provider then executable; one binding per provider, sorted by
`Provider::ALL`; executable serialized as Unicode string. v1 reads into in-memory empty bindings,
without writing. v2 requires bindings and rejects unknown/duplicate fields, duplicate/unsorted
providers and lexical-invalid executable strings as `Corrupt`. Valid lexical path with later
filesystem failure stays a valid registry, yielding `BindingState::Invalid` or `InvalidExecutable`,
not `Corrupt`. Probe unsigned version first: >2 is `NewerSchema` even with new fields,
0/missing/noninteger remains Corrupt. Bounded actual reads retain 1 MiB+1 cap.

Successful **actual** add/rename/forget/bind mutations write v2, pretty two-space JSON plus newline,
revision checked +1 exactly once. First mutation of missing registry creates v2 directly. Binding
does not change next_id. Stage-1 name/ID semantics and orphan handling remain. Every prospective
complete serialized output is checked <=1,048,576 bytes before any profile/temp changes. Counter
exhaustion uses existing `Exhausted`; bind only checks revision. Failed preflight leaves initialized
tree, stale temp and registry unchanged; first-write root/lock scaffolding may remain as in stage 1.

Bind lexical validation occurs before filesystem IO. Under the safe transaction lock, load once,
check whether already bound: identical stored path returns Unchanged with **no target revalidation
or write** (provider list diagnoses stale targets); different path without replace returns
AlreadyBound before filesystem validation. A new/replacement path gets full target validation then
overflow/size preflight before persistence. Thus no same-binding no-op exists in v1 (it has no
bindings); reading v1 alone never migrates; failed/no-op profile operations never migrate. No
ACL-only mutation or repair exists; no revision bump for verification or no-op. Revision overflow
does not prevent an identical no-op. No executable file read is needed for no-op.

The M1 binary rejects v2 as NewerSchema, unchanged. No downgrade, backup/migrate command or
credential migration. Document this only after implemented.

## 4. Path and executable contract

Lexical validation order is Unicode, absolute, `..`, normalized spelling, Windows local-disk prefix,
length, containment. Length is Unicode UTF-8 byte length <=4096 on both platforms, excluding any
terminator. Embedded NUL rejects as `contains a NUL character`. Unix spelling: exactly one initial
`/`, single `/` between nonempty components, no `.` or `..`, no trailing separator. Windows: ASCII
drive letter, colon, backslash, nonempty components separated only by single backslash; no `.`/`..`,
forward slashes, trailing separator, trailing space/dot in components, colon in a component, or
reserved DOS device component (case insensitive, including extension). No UNC/device/verbatim
prefixes. Compare reconstructed spelling by raw OsStr bytes/code units, not Path equality.
Drive-letter case need not be uppercase. Lexical containment is component-boundary based, Windows
ordinal case-insensitive; do not use a string prefix. Root may be non-Unicode on Unix; compare
native components. Serialized invalid paths are Corrupt; CLI invalid paths are InvalidExecutable.

Every new bind, provider-list row, and synthetic run validates again in this order:
1. Validate stored spelling and lexical containment against normalized data root.
2. Validate filesystem ancestry and resolve the target; validate resolved containment against
   canonical data root (if root absent, nearest-existing-ancestor resolution plus suffix).
   Case-insensitive Windows containment uses resolved local volume paths and component boundaries.
   Alias-to-root escape is refused.
3. Verify target is regular, trusted-owned and not foreign-writable; Unix requires at least one
   execute bit. Unix trusted owners are euid or uid 0 for the binary and its ancestry; foreign
   group/other write bits are rejected, including sticky foreign-writable parents. macOS
   conservatively rejects every extended allow ACE on executable ancestry/target, avoiding a second
   principal/permission parser; unknown ACL semantics/query errors refuse. Root ownership for
   managed objects remains euid only. Windows executable/ancestor owners may be user, SYSTEM,
   Administrators or well-known TrustedInstaller service SID
   (`S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464`); this is a declared OS trust
   model, not protection from those principals. For the executable file itself, allow ACEs granting
   write/append/delete/owner/DACL/generic-write/generic-all rights to other principals refuse;
   unknown ACEs/masks fail closed. Existing ancestor directories use the distinct component-control
   rule in §5.1, not the executable-file write mask. Execute/read permission failures are operational
   errors.
4. Read at most the first four executable bytes from the validated file handle. Linux `7f454c46`;
   macOS `feedface`, `cefaedfe`, `feedfacf`, `cffaedfe`, `cafebabe`, `bebafeca`, `cafebabf`,
   `bfbafeca`; Windows first two bytes `4d5a` and stored **and resolved** extension `.exe`
   case-insensitive. Short/text/script files refuse; PE/architecture/load validity is left to OS
   error. No shell fallback.
5. Compare file identities against a simultaneously opened current_exe: Unix `(dev, ino)`; Windows
   `FILE_ID_INFO` volume serial + all 128 file-ID bits. Hard-link aliases to Larp refuse. Query
   failure refuses. Stage-4 managed-copy exclusion remains future work; no aliases are created now.

Stored path is kept for future self-update lookup and printed in bindings. `LaunchTarget` carries
both stored and resolved path plus validated identity. Execute the **resolved** path, never the
stored symlink spelling. Unix explicitly supplies stored path as argv0 to `execve`; Windows
argv0 follows std's resolved executable command-line construction, no stored-argv0 promise. Unix
permits symlinked installation paths only after validating every link's containing ancestry, chain
and final ancestry (bounded 40 hops; loop/error refuses); symlink owner euid/root and parent
non-foreign-writable. Windows rejects **all** reparse points on every installation-path component,
including final file symlinks and junctions. This conservative Windows limitation is explicit; no
shim workaround.

Immediately before execution revalidate resolved metadata/identity against the plan; changed
identity refuses. No registry lock held at exec/spawn. Handles are closed/non-inheritable.
Path-based process creation still reopens the file: a same-user updater or privileged writer can
race after the last check. No identity-pinned/race-proof execution claim. During controlled update
tests, precheck replacement refuses; atomic replacement of stored symlink path can execute the
previously validated resolved target or refuse. The final postcheck interval remains outside
guarantees, and tests must label it rather than promise refusal. Foreign-writable ancestry is
**not** included in that exception: static such ancestry must refuse. No fallback to PATH/another
profile/another binding on any error.

Reasons (frozen): original lexical/type/magic reasons, plus `contains a NUL character`, `is owned by
an untrusted principal`, `has a foreign-writable ancestor`, `grants write access to other users`,
`filesystem identity cannot be verified`, `changed since validation`, `cannot resolve a trusted
executable path`. Preserve original `does not exist`, `is not executable`, `is writable by group or
other users`, `must end in .exe`, `is Larp CC itself`, `is inside the Larp CC data directory`;
normalize Windows spelling errors to `must be normalized`. OS errors do not expose raw path
controls.

## 5. Managed tree, ancestry and transactions

Managed metadata set: root; `profiles/`; registry; lock and fixed temp when present; **every
existing registered home directory**. Inspect directories' metadata/security only, never
enumerate/open/read provider contents. Orphan homes remain untouched. Metadata identity duplicates
and registered-home identities are diagnosed as in M1, extended to Windows file IDs. For launch, any
unsafe existing required object, including another profile's home, blocks. A missing other home is
informational; missing selected home gives HomeUnavailable; missing profiles parent makes selected
home missing. Missing lock/tmp is normal on reads. Missing root/registry means empty registry for
metadata commands. Unexpected type/reparse/access-denied never counts as missing.

List/show are diagnostic: fatal root/metadata/profiles-parent failures stop the command, whereas
per-home checks yield Ok/Missing/Unsafe rows (including verification-access failure as an unsafe
status with fixed reason). Provider list does not require all homes safe. Rename/forget remain
possible with unsafe/missing homes when metadata parents are safe; they change registry only.
Add/bind also need safe metadata, not safe unrelated homes, and never repair those homes. Synthetic
launch checks every registered existing home. Whole-set launch failure order: root, profiles parent,
registry, lock, temp; homes by ascending ID; selected-home absence after unsafe-set checks. Registry
must be safely opened/parsed before it can supply registered IDs; check all fixed existing metadata
before reading its bytes. Unregistered contents are not inspected.

### 5.1 Namespace and ownership before consumption

Validate each ancestor from a trusted filesystem anchor to root and each executable location,
resolving permitted Unix ancestor symlinks with chain checks. Managed root/below never accepts
symlinks/reparse points. Unix ancestors must be euid/root-owned and have no 0022 write bits; on
macOS reject extended allow ACEs too. Reject `/tmp` ancestry even with sticky bit; fixtures must
allocate under a verified user-private parent (e.g. runner workspace), not blindly use system temp.
Do not silently weaken this for tests. macOS `/var` alias may resolve through safe root-owned
ancestors.

Windows requires a local drive/volume, persistent ACLs and the trusted ancestor owners listed in
§4. For **existing ancestor directories**, distinguish control of the selected existing component
from permission to create a different child. Inspect each component's DACL; NULL/empty DACLs,
unsupported ACE types/flags or unknown access-mask bits refuse. Ordinary allow/deny ACEs may carry
`OBJECT_INHERIT_ACE`, `CONTAINER_INHERIT_ACE`, `NO_PROPAGATE_INHERIT_ACE`, `INHERIT_ONLY_ACE` and
`INHERITED_ACE`. An ordinary `INHERIT_ONLY_ACE` does not apply to that ancestor itself; validate its
structure but do not count its SID/mask as effective control of that ancestor. Inspect the next
component's actual ACL separately, including any ACE inherited there as effective. This exemption
is ancestor-only: stricter managed-object confidentiality/protected-DACL checks remain unchanged.

For an effective ordinary allow ACE to any principal outside the §4 trust set, reject
`FILE_DELETE_CHILD`, `FILE_WRITE_EA`, `FILE_WRITE_ATTRIBUTES`, `DELETE`, `WRITE_DAC`, `WRITE_OWNER`,
`GENERIC_WRITE` or `GENERIC_ALL`. The only permitted mask bits for such an allow are
`FILE_LIST_DIRECTORY`, `FILE_READ_EA`, `FILE_READ_ATTRIBUTES`, `FILE_TRAVERSE`, `READ_CONTROL`,
`SYNCHRONIZE`, `GENERIC_READ`, `GENERIC_EXECUTE`, `FILE_ADD_FILE` and `FILE_ADD_SUBDIRECTORY`.
The last two directory-specific rights alone permit sibling creation, not replacement/control of
the already selected component. Do not apply their file-object aliases `FILE_WRITE_DATA`/`FILE_APPEND_DATA` as ancestor-directory write
control. Deny ACEs do not cancel an unsafe allow for this conservative check; denial of required
Larp access remains an operational error. No general effective-access evaluator is introduced.
Reparse flag/type is checked on every component, not merely the final leaf.

Walk components in order using full paths from that verified anchor. Open
each existing directory with `CreateFileW(OPEN_EXISTING, READ_CONTROL | FILE_READ_ATTRIBUTES,
FILE_SHARE_READ | FILE_SHARE_WRITE, ... BACKUP_SEMANTICS | OPEN_REPARSE_POINT)`, non-inheritable,
**no FILE_SHARE_DELETE**, then query actual opened type, tag, owner, ACL and identity; retain those
handles until the relevant transaction/validation ends. A retained preceding ancestor cannot be
renamed/deleted through a compatible Windows open. Sharing conflict, unsupported queries, any
reparse tag, unresolved namespace, or foreign-write control refuses before registry
consumption/creation. Check next component only after anchoring its parent. Do not rely on a name
marker as ownership proof. API access failures remain errors. No claim to defeat
same-user/privileged filesystem manipulation or all exotic filesystems.

For immutable metadata reads, open with READ_CONTROL plus required read access, allow sharing
compatible with atomic replacement, verify the **opened** handle matches inspected
identity/type/security before reading. Read a single handle's complete old/new registry. For
mutation retain ancestor/root/profiles/lock handles without delete-sharing; registry/temp data
handles can be closed before replacement. Never retain a no-delete registry handle across rename.
Namespace control comes from retained safe parents, not the advisory lock. Identity mismatch during
opening aborts. Windows rename sharing retries remain bounded as M1. Tests must verify no-delete
ancestor behavior and registry replacement compatibility; unavailable semantics on a
runner/filesystem fail the corresponding gate, not inferred safe.

Unix uses existing ownership/mode checks and no-follow final opens, comparing opened metadata
identities before data reads. Trusted ancestor checks reject static foreign replacement control; no
cross-platform inode-pinning layer. Same-user/privileged TOCTOU remains the M1 limit.

### 5.2 Unix managed-object checks and macOS ABI

Managed dirs retain euid ownership and mode &0077 ==0; files same owner/mode rule and regular type.
Linux mode mask is the managed-object POSIX access-ACL gate; no chmod repair. macOS additionally
refuses **any extended allow ACE** on managed objects, not only foreign write. Native private FFI
stays in macos.rs: `acl_t`/`acl_entry_t` opaque pointers; `acl_type_t` and `acl_tag_t` represented
as `libc::c_int` for the C-enum ABI, `acl_get_link_np(*const c_char, acl_type_t)->acl_t`,
`acl_get_entry(acl_t,c_int,*mut acl_entry_t)->c_int`, `acl_get_tag_type(acl_entry_t,*mut
acl_tag_t)->c_int`, `acl_free(*mut c_void)->c_int`. Use Darwin constants ACL_TYPE_EXTENDED=0x100,
ACL_EXTENDED_ALLOW=1, ACL_EXTENDED_DENY=2, ACL_FIRST_ENTRY=0, ACL_NEXT_ENTRY=-1 (not Linux values).
Check those enum sizes/constants against the target SDK in seed validation. `acl_get_entry` returns
0 for an entry; exhaustion is -1/EINVAL in Apple's implementation, not Linux's 0 return convention;
validate ACL before iteration, reject unknown tag or unexpected error. Null acquisition accepts only
a documented no-ACL absence result confirmed by native fixture, never generic
EINVAL/access/unsupported errors; if that cannot be distinguished, refuse and correct the contract
before enabling the platform. RAII frees each returned allocation once on every branch. Native SDK
ABI/constant validation and ACL fixtures are required before macOS acceptance. Static ABI/iterator
references: [Apple ACL
header](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/include/sys/acl.h) and
[entry
implementation](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/posix1e/acl_entry.c),
read 2026-11-11; native SDK confirmation remains required. No automatic ACL repair; hint `remove the
extended ACL manually after reviewing it`. No shell command constructed by Larp.

### 5.3 Windows trust, verify-only existing objects, secure new objects

Declared managed-object trust set is current user/SYSTEM/Administrators; owner must be user or
Administrators (not SYSTEM). Verification opens READ_CONTROL, never WRITE_DAC. Query owner+DACL;
require present, non-NULL, **protected** DACL and persistent ACL volume. Empty ACL is `has an empty
access control list` (unusable, not public). Validate descriptor/ACL/ACE/SID lengths before casts.
Accept ordinary allow/deny ACE types only; unknown/object/callback/conditional forms refuse even
inherit-only. For every nonzero allow ACE (conservatively including inherit-only), SID must be
user/SYSTEM/Administrators; no CREATOR OWNER exception needed. Denies grant nothing but can make
operations fail. This scan is a conservative trust policy, not an effective-access or usability
proof. Actual minimum-access opens/operations must succeed; denial yields useful Io, never
bypass/elevation. Reject unprotected existing DACL even if apparently narrow. Administrators
ownership is accepted for verification, but no owner takeover/repair is implemented.

**No existing-object DACL writes in this stage.** No SetSecurityInfo, recursive upgrade, permission
repair, propagation into populated homes, credential copying or automatic conversion of inherited
stage-1 roots. Even safe existing objects are verify-only. Unsafe/unverifiable existing roots refuse
before lock creation/registry read. Remedy: operator independently secures the existing root/parents
after reviewing them, or selects an empty, private root under a trusted parent and creates fresh
profiles; Larp does not copy old state. README later describes only this behavior. A broader repair
would need a separate reviewed contract and authorization; JSON rollback cannot undo ACL writes.

All **new** root, profiles parent, homes, lock and temp are created with explicit protected
SECURITY_ATTRIBUTES, `bInheritHandle=FALSE`, before use: files CREATE_NEW, dirs CreateDirectoryW
then anchored open/reverify. File SDDL `D:P(A;;FA;;;<user SID>)(A;;FA;;;SY)(A;;FA;;;BA)`; new empty
directories use `D:P(A;OICI;FA;;;<user SID>)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)` to let
provider-created descendants inherit private ACLs. Descriptor stays alive through creation; no
setting this descriptor on existing populated objects. Create collision never truncates:
re-open/reverify or skip orphan by M1 rules. All handles non-inheritable. ACL allocation freed with
LocalFree after borrowed SIDs/ACLs are no longer used; HANDLE closes via owned RAII. Existing root
verified before foreign-directory marker evaluation; safe parents precede any creation. For new root
verify immediate parent and all ancestry, create nonrecursively, then open and verify before lock.
Do not recursively create parents.

Transaction order for **all four** mutations: lexical input validation → verify/anchor ancestry and
fixed existing metadata → create/verify missing root if needed → foreign-directory check →
create/verify/open lock securely, acquire M1 bounded lock → recheck fixed metadata and read registry
through validated handle → operation/no-op/name/target/counter/size preflight → create only needed
protected new directories → create protected temp/write/sync/close → atomic replace → Unix directory
sync → release handles/lock. Re-check metadata under lock handles cooperating-writer changes. No ACL
side effects from verification, failed preflight or no-op. First-write scaffolding may remain; later
failures use M1 exact-empty-home/temp rollback and durability rules, never delete provider contents.
Stale unsafe/reparse temp now refuses rather than removing it; safe stale regular temp is ignored by
reads and removed only after successful preflight. This explicitly tightens stage-1 R10 and
read-path lock/temp checks; fixtures and parent reconciliation must reflect it.

## 6. Policy/plan and error precedence

No vendor CLI admission table ships as a universal safety policy. Stage 2 synthetic engine preserves
raw OsStrings and tests policy plumbing with a **provisional fixture grammar**, not a partial vendor
parser. Provisional grammar is frozen solely for tests: zero or one `--label <OsString>` followed by
`prompt [OsString...]`; `prompt` required, args after it are opaque positional text, including an
inner `--` and words such as login/config. Before prompt, unknown/duplicate options, missing label
value, delimiter, or non-Unicode grammar token refuse. Label value can be non-Unicode and may equal
a forbidden word; consume exactly one value before finding prompt. Recognized synthetic `--config`
(all equals/attached forms), `-c...`, `--profile`/`-p...`, `--settings`, `auth`, `login`, `logout`,
`server` refuse with fixed recognized labels; unknown syntax uses fixed `argument syntax`. No
TOML/key parsing. Fixtures exercise quoted/dotted keys, equals/attached/duplicate forms and
nested/double delimiters only as provisional rejection/positional behavior. This grammar must never
unlock ordinary run or be advertised as Claude/Codex grammar.

Provisional environment fixture policy rejects exact ASCII names `CLAUDE_CONFIG_DIR` for Claude,
`CODEX_HOME` for Codex, and fixture prefix `LARPCC_SYNTHETIC_DENY_` (empty suffix included), no
exceptions. Presence includes empty value; Windows ASCII case folding, Unix byte-exact. Non-Unicode
unrelated names/values preserved; errors use fixed recognized labels, sorted by canonical ASCII
name, never values or unknown raw names. This deliberately replaces the speculative vendor
prefix/exception list. Stage 3 must freeze selected-version exact names/prefixes/exceptions and
documented CLI parsing, reject all unreviewed config/profile/settings/remote/auth overrides
initially (including Codex --profile/-p and every -c/--config form) rather than parse TOML. Unknown
grammar cannot be accepted. Stage-2 fixtures are not proof against real vendor overrides.

Synthetic run order: clap → locate root → safe fixed metadata/load/strict schema once → find profile
(NotFound) → whole-set safety in §5 order → selected missing home → unbound → executable checks →
provisional env conflict → provisional argument conflict → pure plan → release all registry
locks/handles → final target identity check → exec/spawn. All failures before execute leave
registry/home contents unchanged. Ordinary run only executes its earlier fixed QualificationPending
branch, so prerequisite errors are intentionally not exposed there.

LaunchPlan: resolved executable; stored argv0 (Unix only); args verbatim; cwd None; env_delta
**exactly one** selected home variable set to derived state_dir verbatim. Do not canonicalize
state_dir for setting it. Other variables, including the other provider home, remain unchanged.
`Command.env` applies delta without process-global mutation. CWD/PWD inherited by Larp; provider's
own later cwd behavior is outside claim. A plan is data, not evidence that a provider is qualified
or its configuration sanitized. Loader injection and arbitrary same-user code execution are outside
this account-organizing boundary; no generic environment scrub, token reads or log output.

New error messages/hints (all stderr `error:` then optional `hint:`, final newline; stdout empty):

| Variant | Message | Hint / exit |
|---|---|---|
| ManagedMode {path,mode,directory} | `unsafe data path <p>: has mode <mode:04o>; group and other access must be removed` | `run: chmod <700 for directory, 600 for file> <p>` / 1 |
| QualificationPending | `provider launch is disabled pending stage 3 qualification` | `binding and profile metadata commands remain available` / 1 |
| AlreadyBound {provider, old} | `refusing to rebind <provider> without --replace` | `currently bound to <old>` / 2 |
| Unbound {provider} | `no executable bound for <provider>` | `run 'larp cc provider bind <provider> <ABSOLUTE_PATH>'` / 1 |
| InvalidExecutable {provider,path,reason} | `invalid <provider> executable <p>: <reason>` | `bind a supported absolute native executable path` / 1 |
| HomeUnavailable {name,status} | `state directory for profile <name> is <missing-or-unsafe: reason>` | `inspect it with 'larp cc profile show <name>'; Larp CC will not recreate it` / 1 |
| EnvConflict {names} | `conflicting environment variables: <comma-space canonical labels>` | `remove these overrides before launch` / 1 |
| ArgConflict {label} | `unsupported launch argument: <fixed label>` | `the argument form has not been admitted by the launch policy` / 1 |
| Launch {path,source} | `cannot launch <p>: <escaped IO source>` | `check the native executable and its access permissions` / 1 |
| Console {action,source} | `cannot <fixed console action>: <escaped IO source>` | none / 1 |
| Wait {source,cleanup_failed} | `cannot wait for provider: <escaped IO source>` | `the direct child may require manual cleanup`; when cleanup_failed, append `; cleanup failed; the direct child may still be running` / 1 |

Approved pre-seed API amendment: `Error::ManagedMode { path: PathBuf, mode: u32,
directory: bool }` carries the observed Unix permission/special bits (`metadata.mode() & 0o7777`)
and directory type captured from already validated inspection metadata. Type/owner/mode precedence
is unchanged. Display and hints are pure and escape paths; neither filesystem queries nor filename
or human-reason inference selects the chmod mode. `Unsafe` remains for other reasons. Captured
file/directory hints remain correct after removal, including a root named `registry.json`.

No Unsupported/in-session-sign-in hint. Existing Error strings retain M1 semantics, except
NewerSchema says supports version 2, path escaping and safety additions here. New Unsafe reasons:
`has an extended ACL that grants access`, `owner is not a trusted managed-path owner`, `has no
access control list`, `has an empty access control list`, `has an unprotected access control list`,
`grants access to other users`, `contains an unsupported access control entry`, `access control
cannot be verified on this volume`, `has a foreign-writable ancestor`, `filesystem identity cannot
be verified`, `access control cannot be verified`, and `platform safety validation is unavailable`
(seed only). Windows unsafe hint fixed: `use a private data root under trusted parents; review
existing permissions manually; Larp CC will not repair or copy provider state`. No SIDs, ACL dumps
or hostile values printed. Fixed metadata errors use Unsafe; per-home verification errors use
DirStatus Unsafe; actual failed data reads use Io. Freeze exact error/precedence fixtures before
seed acceptance.

## 7. Native execution and platform ABI

Approved implementation amendment: Unix uses a private `platform/unix.rs` helper calling
`libc::execve` on the validated resolved absolute path, with stored-path argv0 and exact native
OsString bytes. Owned CString argv/environment vectors and live null-terminated pointer arrays
are built and validated before signal changes; embedded NUL is a Launch error. The environment
inherits all unrelated variables and applies only the selected home delta, without process-global
environment mutation. No PATH lookup, execvp, shell fallback or resident Larp supervisor is used.
This replaces the prescribed CommandExt API because pinned Rust 1.89 uses execvp (including
ENOEXEC shell fallback) and temporarily assigns environ. The supported std direct-launch SIGPIPE
default is matched explicitly; mask and other dispositions remain inherited. Capture errno
immediately on exec failure and restore the captured SIGPIPE disposition where it changed.
Native signal, PID, PTY/job-control, failed-exec environment/signal preservation and harmless
ENOEXEC marker regressions are required. CWD/session/group and stdin/out/err remain inherited;
all registry locks/inspection descriptors are dropped before replacement. No identity-pinned
execution claim is added.

Windows builds `Command` with resolved executable and OsString args, inherited stdio/console, no
shell or new process group in production. Arg fidelity is promised only for tested CRT-compatible
native children. Embedded NUL fails pre-spawn; native WTF-16/surrogate behavior is recorded exactly,
never silently Unicode-normalized. Register `SetConsoleCtrlHandler(Some(handler), TRUE)` **before
spawn**, never NULL. Handler is minimal, no IO/allocation: returns TRUE for CTRL_C_EVENT and
CTRL_BREAK_EVENT, FALSE for all others. It does not resend events; shared console delivers events to
children. Handler registration failure aborts without child. Spawn failure unregisters handler,
returns Launch. On successful spawn wait for direct child; on normal completion unregister then exit
with full u32 status. Unregister failure yields Console error rather than success (child already
exited). On wait failure attempt `Child::kill`, then bounded polling/reap for 2 seconds, unregister,
return the original Wait source in `Error::Wait { source, cleanup_failed }`; set cleanup_failed when bounded cleanup cannot confirm termination/reap (live or unknown), even if kill/reap/unregister also fails. Never wrap or parse source text for cleanup state. The false hint is unchanged; true appends the fixed cleanup-failed suffix. Do not silently report 0
or discard Child on that path. No job/tree kill: only the known direct child is cleanup target.
No-console operation registers handler if supported; API failure refuses rather than ignoring it.
Close/Logoff/Shutdown are delegated to Windows default handler behavior; actual window closure
remains unverified without its separate native test. Killing Larp may leave a child; no general
tree-cleanup promise. A provider daemon is not a Larp supervisor.

Dependencies: existing std/clap/serde/json/libc/tempfile retained; exact target-only `windows-sys
=0.61.2` with `Win32_Foundation`, **`Win32_Globalization`** (approved native ordinal containment), `Win32_Security`, `Win32_Security_Authorization`,
`Win32_Storage_FileSystem`, `Win32_System_Console`, `Win32_System_Threading`,
**`Win32_System_SystemServices`**; test ConPTY uses `Win32_System_Pipes`. No additional framework.
Static ABI inventory and provenance are in `windows-security-api-evidence.md` (archive SHA256
`ae137229bcbd6cdf0f7b80a31df61766145077ddf49416a728b02cb3921ff3fc`). This is supplied static
evidence, not native validation.

Use binding types verbatim: CreateFileW takes `*const SECURITY_ATTRIBUTES`, returns HANDLE checked
against INVALID_HANDLE_VALUE; CreateDirectoryW returns BOOL. GetSecurityInfo returns WIN32_ERROR
directly with output `PSECURITY_DESCRIPTOR`, owner `PSID`, DACL `*mut ACL`; borrowed SID/ACL
pointers die before LocalFree of descriptor. GetSecurityDescriptorControl uses `*mut u16`, `*mut
u32`; GetSecurityDescriptorDacl distinguishes absent/null/empty; GetAclInformation/GetAce use
checked lengths and raw output pointers. GetFileInformationByHandleEx FileAttributeTagInfo ->
FILE_ATTRIBUTE_TAG_INFO, FileIdInfo -> FILE_ID_INFO. GetVolumeInformationByHandleW supplies
FILE_PERSISTENT_ACLS. GetTokenInformation(TokenUser) uses checked two-call buffer sizing; current
SID copied/kept alive independently. CreateWellKnownSid supplies system/admin SIDs;
ConvertStringSecurityDescriptorToSecurityDescriptorW with SDDL_REVISION_1 creates the descriptor,
LocalFree frees once. SetSecurityInfo is intentionally not used. Never cast an unchecked ACE layout
or let descriptor-borrowed pointers escape RAII owner. `unsafe` limited to
platform/{windows,macos,unix}.rs plus existing geteuid, each with SAFETY comments; test platform FFI
similarly localized. Command only launch/platform production modules, plus synthetic/test drivers.
No source credential paths or provider file reads.

## 8. Concrete shared surface and call sites

Paths below are relative to `crates/larpcc-cli/`; retain the bin-only crate. Concrete semantics
frozen, private helper factoring is free. `Error` remains `registry::Error`; provider enum stays
registry-owned and reexported if needed, no duplicate domain enum.

```rust
// registry exports; owned values, no borrowed OS security descriptors
pub struct Binding { pub provider: Provider, pub executable: PathBuf }
pub enum BindingState { Unbound, Ok(Binding), Invalid(Binding, String) }
pub enum BindOutcome { Bound, Unchanged, Replaced { old: PathBuf } }
pub struct LaunchTarget {
    pub profile: Profile, pub state_dir: PathBuf,
    pub stored_executable: PathBuf, pub resolved_executable: PathBuf,
    pub identity: crate::platform::FileIdentity,
}
impl Registry {
    pub fn bindings(&self) -> Result<Vec<(Provider, BindingState)>, Error>;
    pub fn bind(&self, p: Provider, exe: PathBuf, replace: bool) -> Result<BindOutcome, Error>;
    // compiled for synthetic driver only; metadata methods remain ordinary-build APIs
    pub fn launch_target(&self, name: &ProfileName) -> Result<LaunchTarget, Error>;
}
// launch (synthetic-only module in stage 2)
pub struct LaunchPlan {
    pub executable: PathBuf, pub argv0: OsString, pub argv: Vec<OsString>,
    pub cwd: Option<PathBuf>, pub env_delta: Vec<(OsString, OsString)>,
    pub identity: crate::platform::FileIdentity,
}
pub fn plan(t: &LaunchTarget, args: Vec<OsString>, env: &[(OsString, OsString)])
    -> Result<LaunchPlan, Error>; // pure; provisional policy before building delta
pub fn execute(plan: &LaunchPlan) -> Result<u32, Error>; // Unix success never returns
// platform/mod.rs, all crate-private
pub enum ObjectKind { Directory, File }
pub enum FileIdentity {
    #[cfg(unix)] Unix { dev: u64, ino: u64 },
    #[cfg(windows)] Windows { volume: u64, id: [u8; 16] },
}
pub struct CheckedObject { pub file: std::fs::File, pub identity: FileIdentity }
pub struct AnchoredPath { pub ancestors: Vec<CheckedObject> }
pub fn anchor(path: &Path) -> Result<AnchoredPath, Error>; // existing parent chain
pub fn inspect(path: &Path, kind: ObjectKind) -> Result<Option<CheckedObject>, Error>;
pub fn contains_path(path: &Path, root: &Path) -> Result<bool, Error>; // pure native component containment
pub fn verify_opened(file: std::fs::File, path: &Path, kind: ObjectKind)
    -> Result<CheckedObject, Error>; // validates provided handle, never reopens diagnostic path
pub fn inspect_home(path: &Path) -> Result<DirStatus, Error>;
pub fn create_file(path: &Path) -> Result<CheckedObject, Error>; // create-new protected
pub fn create_directory(path: &Path) -> Result<CheckedObject, Error>; // nonrecursive
pub fn executable(path: &Path, root: &Path) -> Result<(PathBuf, FileIdentity), Error>;
#[cfg(windows)] pub fn spawn_wait(plan: &LaunchPlan) -> Result<u32, Error>;
```

Derive Clone/Debug/PartialEq/Eq on identity and suitable data structs, not file handles.
Approved containment amendment: `contains_path` is crate-private and pure. Unix uses native
Path::starts_with. Windows compares exact encode_wide components with maintained windows-sys
CompareStringOrdinal, ignore-case true, without lossy strings, normalization or Unicode casing.
Lengths are checked before i32 conversion; zero API results are errors. Component boundaries,
prefixes and unpaired surrogates are preserved. The existing before-IO lexical-containment
precedence and resolved containment are unchanged; no clap validator is added.

Approved opened-handle amendment: `verify_opened` derives type/security/owner/identity from the
provided File without reopening the diagnostic path or reading contents. macOS uses the supplemental
`acl_get_fd_np(fd: c_int, kind: c_int) -> acl_t` for provided-handle ACL acquisition, checked against
the native SDK alongside the existing link-path ABI. Callers retain verified
parents and compare its returned identity against the inspected object before data reads/use.
Metadata inspection remains READ_CONTROL-only on Windows; actual data/lock opens are nontruncating,
use required access and are verified separately. Registry handles allow atomic replacement and are
closed before rename; no-follow and no-delete parent retention are unchanged.

AnchoredPath owns handles and is held by caller for the entire scoped operation; no raw HANDLE/SID
escape. `anchor(path)` anchors parents, not leaf; `inspect` checks leaf on opened handle. On Unix
same shape uses File handles but does not imply openat/identity-pinned execution. `inspect_home`
maps missing to Missing and security inability to Unsafe; it never inspects contents. Whole-tree
sequencing uses these primitives, existing registry list of profiles, and deterministic iteration;
do not create a generic tree/ACL framework. `platform/mod.rs` selects unix.rs under cfg(unix),
windows.rs under cfg(windows), with macos.rs supplemental only on macOS. Reexports are cfg-gated;
Windows symbols never leak to Unix builds. launch module is synthetic-feature gated; spawn_wait is
additionally Windows gated.

Call sites requiring coordinated edits: `Registry::{list,get,bindings,launch_target}` and store load
validate fixed metadata before read; `paths::entries` uses inspect_home and identity duplicate
logic; every `store::mutate` variant (Add/Rename/Forget/Bind) uses anchored secure transaction,
new-object creators, preflight and rollback; CLI no-confirm forget keeps get semantics; `bind`
returns no-op without a second load; CLI render/error/hints use escaped path helper; main exports no
library. Profile commands do not route through launch prerequisites. Tests must distinguish
read-only safe-registry status collection from launch whole-tree rejection.

## 9. Seed, ownership and integration

Two branches suffice. Core owns all shared code under
`crates/larpcc-cli/src/{registry,provider,launch,cli}/**`, main.rs, platform/{mod,unix,macos}.rs,
manifests/lock, non-Windows tests/support drivers/shared fixtures, CI, README. Windows owns only
`crates/larpcc-cli/src/platform/windows.rs`, `crates/larpcc-cli/tests/windows_*.rs`,
`crates/larpcc-cli/tests/fixtures/windows/**`. Lead owns plan changes. No concurrent edits to shared
files; Windows requests a shared change from core and waits/rebases.

Seed S, first core commit from M2, includes manifests/features, concrete platform surface, fixed
errors/fixtures, driver/fake support declarations, ordinary refusal wired **for real**, and
cfg-correct platform stubs returning errors. No no-op hardener, no successful fake verifier.
Existing Unix metadata behavior retained until secure additions land; Windows seed new
safety-dependent metadata operations fail `platform safety validation is unavailable`, never claim
safe and never silently retain the old unsafe writer. Seed Windows tests explicitly expect this
transitional refusal; restored final native metadata tests are mandatory. Seed-only targeted
dead-code/unused-import allowances are permissible with removal before integrated acceptance; no
panic/todo stubs reachable by user commands. Windows worker receives needed shared test helpers from
S, not permission to edit core files.

Actual S validation before Windows branch starts: fmt; clippy workspace/all-targets -D warnings;
ordinary build/tests; synthetic build/tests; release `--bin larp`; MSRV 1.89 build for default and
synthetic. Use locked/offline locally after prepared dependency cache. Native Windows draft-PR seed
CI must compile/test both surfaces and expected stub refusal; native macOS must compile supplemental
ABI. Report actual S SHA and commands; scratch builds do not count. Lead verifies before branch
creation. Windows is `stage/2-windows` from exact S, core `stage/2-core` from M2, each in own
visible worktree/thread arranged by lead. This artifact launches neither.

Core opens draft PR-core to main; Windows rebases onto core's completed green head then PR-win to
stage/2-core. Integrated head is J2 (avoid confusion with stage-1 H2). File owners fix findings,
refresh dependent branch/evidence. Contract changes stop affected work, go to lead with exact
proposed diff/reason, and are committed by core with matching plan update after approval; Windows
rebases. Lead merges core with merge commit, retargets Windows PR to main, then merges it with merge
commit; final main tree must equal accepted J2. Existing human identity/coauthors preserved, no AI
signatures; workers never merge. Any integration change needs final-head tests/review. README
updated last, only implemented behavior and measured limits; no research prose or future support
claims.

## 10. Acceptance cases and evidence

Every process test uses only native synthetic helpers built for that runner. No `/bin/sh` provider,
installed provider, network, auth, real account or provider-state read. Harness creates a fully
explicit parent environment with `env_clear`, synthetic HOME/USERPROFILE/XDG/APPDATA/LOCALAPPDATA
and test roots under a verified private parent. Windows minimum SystemRoot and OS essentials are
explicitly supplied from an allowlisted OS location, never ambient auth/config variables. Values
recorded only for fixed synthetic allowlist; do not dump ambient names/values. Fake record contains
args as Unix byte hex/Windows u16 hex, selected synthetic env values, cwd, pid, ppid where
supported, is-terminal, Unix pgrp and signal state, Windows received events. Modes: exit u32, sleep,
echo IO, wait-ctrl, bounded grandchild. Driver/test policy and fake controls never reach normal
dispatch.

Bounded waits: each child assertion 10 seconds, console/PTY tests 20 seconds; cleanup direct
children via kill then poll/reap at most 2 seconds. Grandchild exits itself after <=5 seconds and
has an explicit completion record; on harness failure bounded cleanup by known PID/handle. Never
indefinitely wait or kill an unknown tree. Sentinel secrets are invented test values only and absent
from Larp stdout/stderr. Child output itself is forwarded rather than captured/logged by Larp.

Acceptance groups: [G01–G03](#g01-g03), [B01–B03](#b01-b03), [B04–B06](#b04-b06), [P01–P04](#p01-p04), [M01–M04](#m01-m04), [L01–L08](#l01-l08), [U01–U04](#u01-u04), [U05](#u05), [W01–W03](#w01-w03), [W04](#w04), [W05–W08](#w05-w08).

| IDs | Required cases |
|---|---|
| <a id="g01-g03"></a>G01–G03 | Ordinary debug/release/all-features `larp` refuses after valid parse for both providers, absent/corrupt roots, bindings to native fake/real-looking basename, user flags/env/magic markers; zero process/probe/registry IO. Help/parse precedence and exit fixtures. Separate target dirs and release/install artifacts contain only larp; no synthetic helper in ordinary packaging. |
| <a id="b01-b03"></a>B01–B03 | v1 read-only unchanged; each actual mutation writes byte-exact v2; failed/no-op operations unchanged. v2 strict malformed table, newer schema precedence, Unicode/NUL/lexical path failures. Bound/unchanged/rebind/no-replace exits and precedence; eight binders serialize; max revision no-op succeeds but real mutation Exhausted. |
| <a id="b04-b06"></a>B04–B06 | Each executable reason, missing/type/execute/mode/owner, all native magics, bad architecture reaches OS error, PE alias and target extension, Unix links/chains/retarget, Windows all reparse tags, hard-link self, lexical/resolved containment, case/normalization, foreign-writable stored/resolved parents/ACLs. Static and controlled precheck replacements, trusted atomic updater and explicit residual race limit. At/over 1 MiB output and compact-input expansion preflight, unchanged tree and overflow. |
| <a id="p01-p04"></a>P01–P04 | OsString exact spaces/quotes/backslashes/trailing slash/metacharacters/empty/Unicode; Linux invalid UTF-8; Windows unpaired surrogates and NUL failure. Pure plan env delta and parent unchanged; provisional exact env presence/empty/prefix/no-exception/case tests; provisional grammar labels/option values-before-command/attached/equal/duplicates/quoted-dotted-config/non-Unicode/two delimiters/positional forbidden words; fixed errors no supplied values. Clearly labeled provisional, not vendor qualification. |
| <a id="m01-m04"></a>M01–M04 | Required managed path classes root/profiles/registry/lock/temp/selected+other home/alternate root: unsafe existing blocks launch before execute; missing other allowed, missing selected refused; metadata failures fatal vs row status; diagnostic list/show/rename/forget remain usable for unsafe home only. Missing lock/tmp read normal; safe stale temp ignored/read and removed/write; unsafe stale temp refuses. Ancestry trust all OS, no symlink/reparse traversal, access denial != missing. |
| <a id="l01-l08"></a>L01–L08 | Two profiles/provider -> four exact distinct homes; cwd and PWD inherited; exit 0/1/2/42/255, Windows 256/0xC000013A/0xFFFFFFFF; no PATH/profile fallback on all prerequisites; no record on policy refusal; piped input/redirection/8 MiB output exact; run never creates root or changes registry/provider sentinels; direct child exits without lingering Larp supervisor; bounded grandchild gets same env delta (no claim providers cannot daemonize). |
| <a id="u01-u04"></a>U01–U04 | Unix native fake PID equals exec'd driver PID; INT/TERM/HUP/QUIT native signal status; fake reports mask/PIPE/ignored dispositions versus direct-launch baseline; real PTY stdin/out/err, foreground pgrp, Ctrl-C, Ctrl-Z/fg, resize. No shell-bound fake. |
| <a id="u05"></a>U05 | macOS SDK ABI/constants and absence/iterator convention validation, all managed path classes extended allow refusal, query failure and allocation cleanup; executable/ancestor foreign-write ACL refusal. Reject-only, no automatic ACL mutation. |
| <a id="w01-w03"></a>W01–W03 | Every managed class broad/unprotected/NULL/empty/deny/unknown ACE/owner/access/query/volume state; all foreign allow principals, READ_CONTROL-only verification; native non-elevated plus separately labeled elevated results. New objects private before use, collision/retry/rollback, descriptor/handle lifetime and noninheritance. Existing unsafe trees refuse with byte+ACL snapshots proving no provider-content changes; no transparent upgrade test. |
| <a id="w04"></a>W04 | Separate isolated console harnesses: Ctrl-C with harness/driver/fake sharing console and no CREATE_NEW_PROCESS_GROUP for driver; harness own handler absorbs broadcast, GenerateConsoleCtrlEvent(CTRL_C_EVENT,0), fake records exactly once, wrapper remains waiting until child exits. Break harness creates driver group with CREATE_NEW_PROCESS_GROUP and sends CTRL_BREAK_EVENT to its group ID; fake records exactly once. Explicit ready handshakes, bounded waits, no event forwarding. ConPTY closure may be recorded separately; never substitute it for real conhost Close/Logoff/Shutdown. Registration/unregistration failure, spawn failure and wait failure injected via private test seam, cleanup verified. |
| <a id="w05-w08"></a>W05–W08 | GetExitCodeProcess full-u32 status; CREATE_NO_WINDOW/piped IO; all-tag final/ancestor reparse and identity/self hardlink; no-delete ancestor handles prevent compatible rename, sharing failure refuses, registry atomic replace still works, child does not inherit inspection/lock handles. Existing-name race refuses or revalidates under trusted parent; no unsafe collision truncation. Non-elevated positive ancestry fixture: a trusted-owned ancestor grants an untrusted SID only FILE_ADD_FILE/FILE_ADD_SUBDIRECTORY, with a safe existing selected branch and ordinary inherit-only ACEs; both data-root and executable ancestry must pass without system-volume ACL edits. Negative fixtures separately cover effective delete-child/write-attributes/write-EA/owner/DACL rights, effective unsafe grants inherited onto the next component, reparse control and hostile missing-component collisions. Missing components still require exclusive protected creation and collision verification/refusal; sibling-creation permission never authorizes adopting a foreign-created object. |

Applicable matrix: common G/B/P/M/L on Linux, macOS, Windows native runners; U01–04 Unix
(Linux-specific raw-byte assertions Linux only); U05 macOS; W01–08 Windows. Ubuntu MSRV 1.89 runs
applicable common/Linux cases; current stable runs all native targets. Linux local evidence
required, not local macOS/Windows evidence from AWS. Permission tests requiring
non-root/non-elevated execution must record identity class; a skip does not qualify that class.
Non-elevated Windows is mandatory for advertising it; if standard runner cannot provide it, record
exact unsupported gate, obtain lead-arranged authorized runner or retain support unverified. No
creation of accounts or installs implicitly authorized. Optional ACL-owner/unsupported-volume
fixtures requiring unavailable privileges use safe API failure injection plus explicitly unverified
native combination, never fake native proof.

Independent integrated-J2 tester checklist: [T01](#t01), [T02](#t02), [T03](#t03), [T04](#t04), [T05](#t05), [T06](#t06), [T07](#t07), [T08](#t08), [T09](#t09).
- <a id="t01"></a>T01 clean clone fmt/clippy -D warnings, default+synthetic tests/build/release, Ubuntu MSRV,
  exact-J2 native matrix. Retain stage-1 regressions with explicitly changed v2/help/security
  fixtures.
- <a id="t02"></a>T02 Linux strace pre-exec phase: only allowed directory metadata/ACL inspection (no provider-home
  content reads); executable probe <=4 bytes; distinguish loader/post-exec opens from Larp probe.
  Ordinary refusal trace has no provider process/version probe. Unix driver PID becomes fake.
- <a id="t03"></a>T03 real PTY interactions above; record tool availability (`script` syntax differs) and
  fail/pending if absent until authorized manual equivalent, no install here. Actual Windows
  window-close/logoff/shutdown remains explicitly unverified unless separately executed on
  authorized native host.
- <a id="t04"></a>T04 hostile paths/env/args never raw-control/secret echoed; exact precedence/error/AlreadyBound
  exit fixtures.
- <a id="t05"></a>T05 M1-built binary creates v1; stage2 metadata mutation writes v2; M1 rejects unchanged. No
  provider runs.
- <a id="t06"></a>T06 static Command/unsafe/cfg/dependency/packaging placement; no provider contents/auth reads,
  global env writes, shell fallback, existing-object ACL writes, icacls or hidden launch unlock.
- <a id="t07"></a>T07 README every claimed shipped command verified; ordinary qualification gate and
  filesystem/TOCTOU limits accurate, no vendor grammar/account-isolation claim.
- <a id="t08"></a>T08 native Windows ABI/ACL/console/non-elevated matrix and remaining unverified cases, macOS FFI
  evidence; source docs do not substitute runtime.
- <a id="t09"></a>T09 cooperating rebind/forget/rename versus synthetic run: one loaded snapshot, stable profile
  ID/home/binding pair or refusal, never mixed/fallback; updater/race residual limit explicitly
  bounded to same-user/privileged writers.

Definition of done: same J2, applicable cases passed with explicit per-case pass/fail/unsupported
evidence, no unexplained ignore; native three-OS+MSRV green, default gate and nonpackaging verified,
no seed stubs/allowances/todo/debug residue; independent T01–T09 and OCR **delegate** diff review
with per-file coverage/skips/reasons, all findings fixed or lead-accepted. Never say OCR passed.
Lead verifies merged tree equals J2. Handoffs <=10 lines; full evidence artifacts have sufficient
detail outside checkout. No arbitrary 10-line cap on this contract or test evidence. Engineering
completion does not open real run; unverified platform claims remain unadvertised. Actual
auth/account matrix, hardware beyond runners, spending, account creation, production/tag/publication
remain separate authorization gates.

## 11. D5 static evidence and pending stage-3 work

Supplied research `codex-daemon-boundary.md` pins openai/codex
`e58bf8f5233b51e7cd1679d947e3028c1ed91b06` and installed-version source tag 0.167.0. Static source
indicates canonical CODEX_HOME-scoped socket `app-server-control/app-server-control.sock` and daemon
state/lock/pid under `app-server-daemon/`; interactive TUI probes its own home's socket. Thus do
**not** describe implicit distinct-canonical-home cross-attachment as established or wholly unknown.
Home aliases, symlinked subdirectories (inferred/untested), explicit remote endpoints and same-home
daemon reuse remain risks; the reused daemon retains its initial launch environment, so a later
client's env_delta cannot retroactively sanitize it. The inspected interactive `--no-daemon` path
bypasses lookup and auto-start, uses embedded server and conflicts with remote;
`features.daemon_auto_start=false` does not prevent attachment to an existing server. This is
version/entrypoint-specific static evidence, no exec-path/Windows/runtime qualification.

Preserve candidate future gates G1 distinct canonical homes/no symlinked server paths; G2 separately
reviewed refusal policy for existing live socket/pid; G3 exact-qualified-version
foreground/no-daemon interface only; G4 authorized post-run leak check; G5 accurately documented
scope. These are **stage-3 candidates**, not stage-2 filesystem scans or flags; state/socket/auth
contents must not be read under this contract. Freeze exact safe observations/entrypoints in that
later plan. No unknown-version injection, no blanket all-backend/OS assertion. The facts narrow a
knowledge gap, not the qualification or permission gate.

## 12. Historical draft-preparation evidence

The following records the earlier draft-preparation run, not publication of this planning PR. No
runtime verification is claimed. Only nonsecret planning artifacts outside checkout were written.
Project and H2 worktree were clean before reads; final status recorded in the resource/handoff
artifact. Actual worker-cgroup resource sample is `stage2-correction-resources.json`: correct
project-disk workspace, 101,897,863,168 bytes free; effective RAM headroom 6,018,347,008; admit. The
earlier `/tmp` disk denial was a tmpfs/workspace mismatch, not project-disk exhaustion. Codex2 fresh
eligibility supplied and refreshed through policy cache; no auth/provider commands. New lease
`larpcc-stage2-plan-correction-disk` is released at finish. Fresh independent review of this draft
and planning-PR merge remain pending; no source/branch/PR changes here.
