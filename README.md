# Larp CC

Larp CC manages named Claude Code and Codex profile records, distinct state directories, and explicit native executable bindings. Names are labels; immutable numeric IDs determine the directories. Renaming preserves both. Forgetting removes metadata and keeps the directory.

Use LarpCC only for your own accounts. Separate profiles are a convenience. Do
not use them to share or resell accounts. LarpCC cannot guarantee that a provider
will not restrict or suspend an account. Use it at your own risk and follow each
provider's terms.

**Provider CLI execution is disabled pending stage 3 qualification.** Ordinary `larp cc run NAME [-- ARGS...]`, `login NAME [--yes]` and `logout NAME [--yes]` refuse immediately after argument parsing, before root lookup, registry access, provider probes or process creation. Debug, release, installed and all-features binaries use the same gate. `larp cc status NAME` reads profile metadata and reports the account as `not_qualified` while qualification rows are empty, without running the provider. Binding a path does not qualify a provider, version, installation or account-isolation claim. On macOS, desktop dispatch has a separate admission gate for the official ChatGPT.app.

## Install on macOS (local, unsigned)

Use macOS 14 or newer with Rust 1.89 or newer and Apple's Swift build tools
(Swift 5.9 or newer). From the repository root, preview and run the installer.

```sh
./scripts/install-macos.sh --dry-run
./scripts/install-macos.sh
```

The installer uses `~/.local/bin/larp` and `~/Applications/LarpCC.app`.
Use `--prefix /absolute/path/to/bin` to choose another directory for `larp`. The
installation paths must be free of symlinks. The installer recognizes a previous
`larp` by its matching SHA-256 extended attribute. To replace any other file
named `larp`, move it aside or explicitly pass `--replace`. Add the chosen
directory to `PATH` if needed. In LarpCC, use **General → larp executable →
Choose…** to select the installed binary.

To build and install manually with the default paths, run these commands from
the repository root.

```sh
cargo build --release --locked --bin larp --target-dir "$PWD/target"
./apps/macos/scripts/bundle.sh
mkdir -p -m 0755 "$HOME/.local/bin" "$HOME/Applications"
```

For a manual upgrade, move the existing `larp` aside first. The following
commands require an unused destination and mark the new binary for future
installer upgrades.

```sh
larp_bin="$HOME/.local/bin/larp"
test ! -e "$larp_bin" && test ! -L "$larp_bin" &&
  install -m 0755 target/release/larp "$larp_bin" &&
  larp_hash=$(shasum -a 256 < "$larp_bin") &&
  xattr -w dev.larpcc.installer.sha256 "${larp_hash%% *}" "$larp_bin"
```

Quit any running LarpCC settings app. If you already installed LarpCC, move the
old `~/Applications/LarpCC.app` to the Trash in Finder. Then copy the new bundle.

```sh
ditto apps/macos/dist/LarpCC.app "$HOME/Applications/LarpCC.app"
export PATH="$HOME/.local/bin:$PATH"
```

If macOS blocks your self-built unsigned app due to quarantine, you can clear
the quarantine attribute for that bundle.

```sh
xattr -dr com.apple.quarantine "$HOME/Applications/LarpCC.app"
```

### Uninstall

Quit LarpCC. Remove the `larp` file you installed from your chosen prefix and
move the installed `~/Applications/LarpCC.app` to the Trash. Keep the profile data
directory (`LARPCC_HOME` or `~/Library/Application Support/larpcc`).

## Command-line use

Requires Rust 1.89 or newer:

```sh
cargo build --release --locked --bin larp
cargo install --path crates/larpcc-cli --locked --bin larp
larp --version
larp cc profile --help
```

Metadata commands:

```sh
larp cc profile add work --provider claude
larp cc profile add personal --provider codex
larp cc profile list
larp cc profile show work
larp cc profile surface work desktop on
larp cc profile list --json
larp cc profile rename work job --expected-revision 3
larp cc profile forget personal --yes
larp cc profile delete personal --yes
larp cc profile recover personal --yes
larp cc profile migrate
larp cc provider bind claude /absolute/path/to/native-executable
larp cc provider list
```

Close all sessions using the profile before confirming delete or recover,
including sessions started outside Larp. On Linux and macOS, delete permanently
removes the managed profile directory and owned aliases, clears sharing
references and retires the profile ID. If interrupted, resume with
`profile recover NAME --yes`. Pending cleanup blocks registry consumers.
Windows refuses profile-tree deletion and recovery until confined removal is
available. `--logout` requires native qualification. Keychain items, remote
grants and Electron Safe Storage may remain after deleting a directory.

`profile migrate` upgrades registries with empty legacy homes on Linux and
macOS. Keep older Larp binaries and native sessions stopped while migrating.
Windows refuses legacy profile-tree moves. See the stage 04 contract for
[migration recovery](docs/stages/04-layout-and-delete.md#v2-migration-transaction).

Explicit skill packages can be shared with all active CLI profiles:

```sh
larp cc skills add /absolute/path/to/package --name example
larp cc skills list
larp cc skills matrix
larp cc skills disable example claude-work
larp cc skills enable example claude-work
larp cc skills sync
larp cc skills remove example
```

A package requires `SKILL.md` and portable ASCII relative filenames. Trusted
0755/0644 sources are accepted; symlinks, reparse points, hardlinks, special files,
unsafe ancestry and case-insensitive collisions refuse. Limits are 1,000 files,
16 MiB and 16 path levels, with at most 1,000 directories. Nothing executes.
Sources equal to, containing, or inside managed/vendor homes are refused by
directory identity before package contents are read. Dot entries, `auth.json`,
`*.pem` and `id_*` names at any depth refuse the entire package before those
entries are read. Vendor directory permissions do not authorize content reads.
The package is copied once into `shared/skills/<name>` and verified by a canonical
SHA-256 digest. Existing store entries and occupied profile destinations are never
replaced. Unix uses internal-store symlinks; Windows uses owned digest copies
because confined junction creation is unavailable. New active CLI profiles inherit
all skills; explicit disables stay with the immutable profile ID across renames.
Remove requires disabling all active profile references first. Other global,
project and provider skill scopes remain independently visible.

`skills.json` is a bounded version-1 catalog; `owned-files.json` records exact
artifact identities and digests. Both use the registry lock. A bounded write-ahead
`share-journal.json` keeps package bytes and the concrete mutation plan until the
index, manifest and registry commit completes. Pending work blocks ordinary
registry consumers; `skills sync` explicitly resumes it. Missing owned profile
artifacts are recreated by sync or dropped by disable; changed/occupied entries
still refuse drift. Sharing writes take ordered per-profile lifecycle guards
before the registry lock; contention refuses. Forget retains the directory and
refuses sharing references. Cleanup unlinks verified leaves without traversing a
link target or using recursive directory removal. Local filesystems and the
existing same-user/privileged race limitations still apply.

`--json` uses schema 1 for all skills commands. List/matrix are read-only; mutations
and reads accept `--expected-revision` under the same registry lock before name
resolution. A stale revision refuses without effects. JSON contains metadata and
switches, never package contents.

Aliases are recognizable commands that launch one profile:

```sh
larp cc alias add codex-45 claude-work   # CLI run of profile claude-work
larp cc alias add work-app codex-45 --desktop
larp cc alias add claude claude-work --shadow
larp cc alias list
larp cc alias remove codex-45
```

An alias is a copy of the running, trusted `larp` executable at
`<data directory>/bin/NAME` (`NAME.exe` on Windows) plus an adjacent
`NAME.larp-alias.json` receipt that pins the absolute data root, the profile ID,
the surface and the identities and SHA-256 of the launcher and its source.
Windows alias creation currently refuses because the loaded image cannot be verified.
The registry keeps an independent receipt digest. Any valid profile name works
for either provider; `claude` and `codex` require `--shadow`. Nothing is ever
overwritten: an existing file at the alias path, or (without `--shadow`) a
same-named command on `PATH`, refuses. Larp CC prints guidance but never edits
`PATH` or shell files; add the `bin` folder to `PATH` yourself.

An alias-named launcher uses its receipt to select the root. A copy named and
invoked as `larp` uses the ambient root and ordinary closed admission, ignores
adjacent receipts, and parses the `cc` grammar. Renaming is a user action and
provides no privilege boundary.
Alias arguments (including a literal `--json`) are forwarded to `run`, so the
stage-3 qualification refusal applies unchanged; a desktop alias forwards to
`app open` and takes no arguments. Renaming the profile keeps the mapping;
`profile forget` refuses while aliases reference it. Moved, copied or modified
launchers, forged receipts and a newer `larp` (reported `incompatible`; remove and
re-add) refuse. Add and remove use the registry lock and the shared
`share-journal.json` (kind `alias`): an interrupted add rolls back, an interrupted
remove completes, and an `alias` command without an expected revision can retry
recovery. Drift leaves the journal pending. Remove deletes only artifacts proven
by digest and identity. See [manual alias recovery](docs/stages/05-aliases.md#manual-alias-recovery).

Use `provider bind … --replace` to change a binding. An identical binding is a no-op, even if the executable has subsequently disappeared. Provider list revalidates paths and reports `ok`, `unbound` or `invalid: …`; `ok` means path validation only. Binding never runs an executable or probes its version. Paths and diagnostics escape terminal controls and the specified Unicode format controls.

Names contain 1–32 lowercase ASCII letters, digits, hyphens or underscores, starting with a letter. `larp` and Windows device names are reserved on all platforms. Providers are exactly `claude` and `codex`. Profile list sorts by name and reports each home as `ok`, `missing` or `unsafe`. Unsafe or missing homes remain diagnosable and can be renamed or forgotten without reading their contents. `forget` requires `--yes` and never deletes a home.

Help and version exit 0. Usage errors and rebinds missing `--replace` exit 2; local operational failures exit 1. Human errors go to stderr. Reading a missing registry does not create it.

`--json` is global before the child `--` separator. It emits exactly one schema-1
[JSON envelope](docs/contracts/cli-v1.md) to stdout, with empty stderr and exit
0/1/2. Errors contain fixed safe messages, without supplied values or raw OS
errors. Help/version work without root IO. JSON run/login/logout refuse
`json_native_io` before root IO. JSON `status NAME` returns a `status.account`
summary. Use `status NAME --no-account` for the local metadata-only `status`
response with `native_auth: not_observed`. Unsupported future commands remain
parser errors.

Registry commands accept `--expected-revision REV`. The registry lock protects
comparison before name resolution or mutation effects; a stale revision returns
`conflict`, preventing a stale name from selecting a newly created profile.
Receipts contain the committed revision and affected immutable ID. Native closed
gates take precedence. Help/version reject revision preconditions.

The data root is non-empty `LARPCC_HOME`, otherwise:

| OS | Default root |
|---|---|
| Linux | `$XDG_DATA_HOME/larpcc` when absolute; otherwise `$HOME/.local/share/larpcc` |
| macOS | `$HOME/Library/Application Support/larpcc` |
| Windows | `%LOCALAPPDATA%\larpcc` |

The root must be absolute with no `..` component, and its immediate parent must exist. Metadata comprises `registry.json`, `registry.lock`, transient `registry.json.tmp`,
and `profiles/<id>/{cli,desktop}`. New roots use schema v3. CLI defaults on and
desktop off; toggling a surface never qualifies native launch. JSON profile
list/show include both surfaces, provider, lifecycle state, aliases and desktop
bindings. On macOS, bind the verified official ChatGPT.app with
`app bind codex /Applications/ChatGPT.app [--version LABEL]`, enable the Codex
profile's desktop surface with `profile surface PROFILE desktop on`, then run
`app open PROFILE`. Unrecognized app versions warn and may launch after the
same admission checks. JSON dispatch reports the local launch or focus outcome.
Claude.app and desktop on other operating systems remain closed. CLI qualification rows
remain empty. Alias commands are not shipped.
Existing v1/v2 registries remain readable without migration; mutations refuse
`recovery_required`. Explicit migration, folder deletion and recovery are pending
the next Stage04 slice. No old profile directory is automatically moved, copied
or adopted. `forget` still retains the folder and refuses alias references or
tombstones. Older binaries refuse v3. Failed operations and binding no-ops do not
increment the revision. Registry and JSON envelopes are limited to 1 MiB.

On Unix, managed objects require the current owner and no group/other access. Ancestors must be root/current-user owned without group/other write access, even for sticky directories such as `/tmp`. Files are created 0600 and directories 0700. macOS requires native extended-security support and a successful descriptor security query; an explicitly absent ACL property is accepted, while extended allow ACLs and query errors are refused without repair. The x86_64 ACL ABI branch is not native-qualified. Unsafe lock/temp objects block reads and writes; safe stale temp files are ignored by reads and removed only after successful write preflight. Provider directory contents are never inspected.

Native binding checks reject scripts, unsafe ownership/permissions/ancestry, data-root containment and Larp's own filesystem identity. Unix installation symlinks are resolved through a bounded trusted chain; the resolved target is used. Native file headers prove neither architecture nor provenance. On Windows, Larp creates managed objects with protected ACLs and verifies existing owners, ACLs and reparse points before use. Unsafe paths are refused without changing permissions or copying provider data. Windows support is not advertised.

Use a local filesystem. Advisory locks coordinate Larp writers; NFS/SMB locking is unsupported. Opened metadata handles are validated before data reads. Unix atomic replacement and directory sync depend on filesystem support. Same-user or privileged writers can still race path-based checks and execution; there is no identity-pinned execution or sandbox claim. On WSL, use a Linux path rather than `/mnt/c`.

Checks use synthetic data and native test helpers. Set `LARPCC_TEST_PARENT` to an existing private parent with trusted ancestry; the setting affects test fixtures only. CI provisions a private per-run parent and runs native Linux/macOS/Windows and Ubuntu Rust 1.89 gates.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --features synthetic-launch --target-dir /absolute/private/build-parent/synthetic-target --locked
cargo test --bin larpcc-synthetic --features synthetic-launch --target-dir /absolute/private/build-parent/synthetic-target --locked
cargo +1.89 test --workspace --locked
```

The `synthetic-launch` feature builds separate test-only driver/fake binaries. The legacy transport grammar and invented operation rows do not describe admitted vendor interfaces. Invented rows exercise the same bounded executable SHA-256, snapshot revalidation and metadata admission as ordinary dispatch. Unix run uses direct `execve`; synthetic authentication/status use a short-lived foreground wrapper retaining a per-ID cooperative lock until the direct child is reaped. Registry locks do not span confirmation or child lifetime. These checks do not exclude direct native clients or surviving descendants. Parent SIGKILL or an OS crash may release the cooperative lock while a child survives; human quiescence is required after abnormal exit. No shell fallback or background manager is used. Synthetic helpers are excluded by the explicit ordinary build/install commands above and are not release artifacts. Only fake operations are exercised; no installed provider, credential store or account is accessed. Native operation rows remain empty.

Larp CC is independent and is not affiliated with Anthropic or OpenAI.

New profile names and rename destinations reserve `larp`; existing legacy profiles
with that name remain readable and addressable. Repeating a surface setting is a
no-op: JSON reports `changed:false` and the registry revision stays unchanged.
