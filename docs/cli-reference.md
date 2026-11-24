# CLI reference

`cargo install` and `cargo build --release --bin larp` do not enable `synthetic-launch`. That feature builds test-only driver and fake binaries. It is not a release artifact.

#### Profiles and providers

Each block is its own example. `forget` removes the registry record and keeps the directory, so `delete` of that same name then returns `NotFound`. `recover` resumes an interrupted `delete`. It is not a follow-up to a finished `delete`, and it is not valid after `forget`. `rename` refuses unless the registry revision is exactly 3.

```sh
larp cc profile add work --provider claude
larp cc profile add personal --provider codex
larp cc profile list
larp cc profile show work
larp cc profile surface work desktop on
larp cc profile list --json
larp cc profile rename work job --expected-revision 3
```

```sh
larp cc profile forget personal --yes
```

```sh
larp cc profile delete personal --yes
larp cc profile recover personal --yes
```

```sh
larp cc profile migrate
larp cc provider bind claude /absolute/path/to/native-executable
larp cc provider list
larp cc provider evidence claude
```

#### Skills, MCP, aliases, desktop, usage, doctor

The profiles in this block are `claude-work` and `codex-45`. `codex-45` is a profile name here, not an alias. `skills remove` refuses while an active profile still references the package, so it is not the next step after `enable`.

```sh
larp cc profile add claude-work --provider claude
larp cc profile add codex-45 --provider codex

larp cc skills add /absolute/path/to/package --name example
larp cc skills list
larp cc skills matrix
larp cc skills disable example claude-work
larp cc skills enable example claude-work
larp cc skills sync
larp cc skills remove example

larp cc mcp add example --command /absolute/path/to/server
larp cc mcp list
larp cc mcp matrix
larp cc mcp enable example claude-work
larp cc mcp disable example claude-work
larp cc mcp sync
larp cc mcp remove example

larp cc alias add shortcut claude-work
larp cc alias add work-app codex-45 --desktop
larp cc alias add claude claude-work --shadow
larp cc alias list
larp cc alias remove shortcut

larp cc profile surface codex-45 desktop on
larp cc app bind codex /Applications/ChatGPT.app --version LABEL
larp cc app open codex-45
larp cc usage
larp cc settings usage
larp cc doctor
larp cc doctor --detect-versions
larp cc status claude-work
larp cc status claude-work --no-account
```

`run`, `login`, and `logout` are in the grammar and refuse before root lookup while the qualification table is empty, whether or not the profile exists:

```sh
larp cc run work
larp cc login work --yes
larp cc logout work --yes
```

### Command behavior

Names contain 1–32 lowercase ASCII letters, digits, hyphens, or underscores, and start with a letter. `larp` and Windows device names are reserved on all platforms. New profile names and rename destinations reserve `larp`. Existing legacy profiles with that name stay readable. Providers are exactly `claude` and `codex`.

Help and version exit 0. Usage errors and a rebind without `--replace` exit 2. Local operational failures exit 1. Human errors go to stderr. Reading a missing registry does not create it.

`--json` is global, before a child `--` separator. It writes one schema-1 envelope to stdout, with empty stderr, and exits 0, 1, or 2. Errors use fixed messages and omit the values you typed and raw OS errors. Help and version do no root IO.

Registry commands accept `--expected-revision REV`. The registry lock compares that revision before name resolution. A stale revision returns `conflict`. Receipts contain the committed revision and the immutable ID. Repeating a surface setting is a no-op: JSON reports `changed: false` and the revision stays the same. A binding that is already recorded is a no-op and does not increment the revision. A refusal before the registry is published leaves the revision unchanged. `profile delete` is different: publishing the `deleting` tombstone increments the revision, and a failure during later cleanup or directory removal leaves that revision and the `deleting` state in place. Resume that profile with `profile recover`. Registry and JSON envelopes are limited to 1 MiB.

The data root is non-empty `LARPCC_HOME`, otherwise:

| OS | Default root |
| --- | --- |
| Linux | `$XDG_DATA_HOME/larpcc` when that path is absolute; otherwise `$HOME/.local/share/larpcc` |
| macOS | `$HOME/Library/Application Support/larpcc` |
| Windows | `%LOCALAPPDATA%\larpcc` |

The root must be absolute, with no `..` component, and its immediate parent must exist. Metadata is `registry.json`, `registry.lock`, a transient `registry.json.tmp`, and `profiles/<id>/{cli,desktop}`. New roots use schema v3. CLI defaults on and desktop off. Toggling a surface does not qualify native launch. JSON profile list and show include both surfaces, the provider, lifecycle state, aliases, and desktop bindings.

Existing v1 and v2 registries stay readable. Mutations that need v3 refuse `recovery_required` until `profile migrate`. That command upgrades registries whose legacy homes are empty, on Linux and macOS. Keep older Larp binaries and native sessions stopped while migrating. Windows refuses legacy profile-tree moves. An interrupted migration can need the manual steps in [v2 migration](stages/04-layout-and-delete.md#v2-migration-transaction). No old profile directory is copied or adopted on its own. `forget` still keeps the folder and refuses while aliases, sharing references, or a tombstone exist.

Close sessions that use a profile before `delete` or `recover`, including sessions started outside Larp. On Linux and macOS, delete removes the managed profile directory and owned aliases, clears sharing references, and retires the profile ID. If it is interrupted, resume with `profile recover NAME --yes`. A pending journal blocks other registry commands. Windows refuses profile-tree deletion and recovery until confined removal exists. `--logout` requires native qualification. Keychain items, remote grants, and Electron Safe Storage may remain.

On Windows, `larp cc usage` and usage settings changes, including `profile plan-label`, exit 2 with `unsupported` before opening files. `larp cc settings usage` shows LOCAL mode with no profile data. Account status reports `usage: null`. See [stage 11](stages/11-usage.md). On Linux and macOS, a usage read uses LOCAL records when the stored mode is LOCAL, or when `--local` forces LOCAL. If the stored effective mode is LIVE and the read does not force LOCAL, the row is the disabled LIVE row. `--live` checks `--expected-revision` first: a stale revision returns `conflict`, and a matching revision returns `live_disabled`. A settings file that already has LIVE enabled is reported as `needs_acknowledgement` and is left unchanged.

A skill package needs `SKILL.md` and portable ASCII relative filenames. Trusted `0755` and `0644` sources are accepted. Symlinks, reparse points, hardlinks, special files, unsafe ancestry, and case-insensitive collisions refuse. Limits are 1,000 files, 16 MiB, and 16 path levels, with at most 1,000 directories. Nothing executes. Sources that are the managed or vendor home, or that contain or sit inside one, are refused by directory identity before the package is read. Dot entries, `auth.json`, `*.pem`, and `id_*` names at any depth refuse the whole package before those entries are read. The package is copied once into `shared/skills/<name>` and checked with a canonical SHA-256 digest. Existing store entries and occupied profile destinations are never replaced. Unix uses in-store symlinks. Windows uses owned digest copies because confined junction creation is unavailable. New active CLI profiles inherit all skills. Explicit disables stay with the immutable profile ID across renames. Remove requires every active profile reference to be disabled first.

`skills.json` is a bounded version-1 catalog. `owned-files.json` records artifact identities and digests. Both use the registry lock. A bounded `share-journal.json` keeps the package bytes and the mutation plan until the index, manifest, and registry commit finish. `skills sync` resumes a pending journal. Sharing writes take per-profile lifecycle guards before the registry lock. Contention refuses. Cleanup unlinks verified leaves. It does not traverse a link target and it does not use recursive directory removal.

`--json` on skills commands is schema 1. List and matrix are read-only. Mutations and reads accept `--expected-revision` under the same registry lock, before name resolution. JSON contains metadata and switches, never package contents.

An alias is a copy of the running, trusted `larp` at `<data>/bin/NAME` (`NAME.exe` on Windows) plus `NAME.larp-alias.json`. The receipt pins the absolute data root, the profile ID, the surface, and the SHA-256 of the launcher and its source. The registry keeps its own receipt digest. Windows alias creation refuses because the loaded image cannot be verified. Any valid profile name works for either provider. `claude` and `codex` require `--shadow`. An existing file at the alias path, or a same-named command on `PATH` when `--shadow` is absent, refuses. LarpCC prints guidance and does not edit `PATH` or shell startup files.

An alias-named launcher uses its receipt to select the root. A copy that is named and invoked as `larp` uses the ambient root, ignores an adjacent receipt, and parses the `cc` grammar. Alias arguments, including a literal `--json`, are forwarded to `run`, so the qualification refusal still applies. A desktop alias forwards to `app open` and takes no arguments. Renaming the profile keeps the mapping. `profile forget` refuses while an alias references the profile. Moved, copied, or modified launchers, forged receipts, and a newer `larp` (reported `incompatible`) refuse. Add and remove use the registry lock and `share-journal.json` with kind `alias`. If an add is interrupted after the registry commit, recovery keeps the alias when its artifacts still verify. Before that commit, recovery removes the artifacts it can verify. Changed or unrecorded files leave the journal pending for [manual alias recovery](stages/05-aliases.md#manual-alias-recovery). An interrupted remove completes from the committed mapping. Remove deletes only artifacts proven by digest and identity.

`provider bind … --replace` changes a binding. An identical binding is a no-op, even if the executable has since disappeared. `provider list` revalidates paths and reports `ok`, `unbound`, or `invalid: …`. `ok` is path validation only. Binding never runs an executable. Paths and diagnostics escape terminal controls and the specified Unicode format controls. Profile list sorts by name and reports each home as `ok`, `missing`, or `unsafe`. Unsafe or missing homes can still be renamed or forgotten. `forget` requires `--yes` and never deletes a home.

On macOS, bind the verified official ChatGPT.app with `app bind codex /Applications/ChatGPT.app [--version LABEL]`, enable the Codex profile's desktop surface with `profile surface PROFILE desktop on`, then run `app open PROFILE`. An unrecognized app version prints a warning on stderr in human mode and may launch after the same admission checks. JSON mode suppresses that warning. The JSON receipt has no warning field. It reports the local launch or focus outcome. Claude.app, and desktop open on other operating systems, stay closed. CLI qualification rows stay empty.

On Unix, managed objects must be owned by the current user and must not grant group or other access. Ancestors must be owned by root or the current user, without group or other write, including sticky directories such as `/tmp`. New files are `0600` and new directories are `0700`. macOS requires a successful descriptor security query. An absent ACL property is accepted. Extended allow ACLs and query errors are refused, with no repair. The x86_64 ACL ABI branch is not native-qualified. Unsafe lock or temp objects block reads and writes. Safe stale temp files are ignored by reads and removed only after a successful write preflight. Provider directory contents are never inspected for this check.

Native binding checks reject scripts, unsafe ownership, permissions, and ancestry, data-root containment, and Larp's own filesystem identity. Unix installation symlinks are resolved through a bounded trusted chain, and the resolved target is what gets recorded. A file header does not prove architecture or provenance. On Windows, Larp creates managed objects with protected ACLs and checks existing owners, ACLs, and reparse points before use. Unsafe paths are refused without changing permissions or copying provider data. Windows support is not advertised as a finished desktop product.

Use a local filesystem. Advisory locks coordinate Larp writers. NFS and SMB locking is unsupported. Opened metadata handles are validated before data reads. Unix atomic replacement and directory sync depend on the filesystem. A same-user or privileged writer can still race a path check and a later execution. There is no identity-pinned execution sandbox. On WSL, use a Linux path rather than `/mnt/c`.

Checks use synthetic data and native test helpers. `LARPCC_TEST_PARENT` must be an existing private parent with trusted ancestry. It affects test fixtures only. CI provisions a private per-run parent and runs the native Linux, macOS, and Windows tests plus the Ubuntu Rust 1.89 gate.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --features synthetic-launch --target-dir /absolute/private/build-parent/synthetic-target --locked
cargo test --bin larpcc-synthetic --features synthetic-launch --target-dir /absolute/private/build-parent/synthetic-target --locked
cargo +1.89 test --workspace --locked
```

The `synthetic-launch` feature builds separate test-only driver and fake binaries. Invented rows exercise the same bounded executable SHA-256, snapshot revalidation, and metadata admission as ordinary dispatch. They do not describe an admitted vendor interface. Unix `run` uses direct `execve`. Synthetic authentication and status use a short-lived foreground wrapper that holds a per-ID cooperative lock until the direct child is reaped. Registry locks do not span confirmation or the child lifetime. These checks do not exclude a direct native client or a surviving descendant. A parent `SIGKILL` or an OS crash can release the cooperative lock while a child is still running. After an abnormal exit, stop those sessions yourself before delete or recover. There is no shell fallback and no background manager. Only fake operations are exercised. No installed provider, credential store, or account is accessed. Native operation rows remain empty.

Remote host commands from [stage 13](stages/13-remote-hosts.md) are not in this build. On `status`, a well-formed `--expected-host-revision` returns `unsupported` with `Host revision preconditions require remote routing, which is unavailable.` A malformed value, or that flag on another command, is a usage error and exits 2.
