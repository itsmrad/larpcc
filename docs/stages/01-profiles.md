# Stage 1 — Profiles registry and profile commands

Status: **frozen contract**. Developers start only after this document is merged to `main` (call that commit `M0`). Changes follow §10.3. Parent plan: [`plan.md`](../../plan.md).

## 1. Scope

**In scope:**
- A metadata-only profile registry: data root, `registry.json` v1, a lock, atomic replace.
- `larp cc profile add | list | show | rename | forget`, plus `--help` and `--version`.
- An empty, private state directory created for each profile.
- Baseline CI on native Linux, macOS and Windows runners, with an MSRV build.
- A README that describes the integrated behavior.

**Out of scope (later stages):**
- binding or resolving provider executables
- `run`, `login`, `status`, `logout`
- aliases
- Windows ACL creation/verification (a stage 2 entry criterion)
- adopting existing directories, purge/delete commands, migrations
- JSON output, a `--root` flag, interactive prompts, colors, shell completions
- a library crate, a trait for the store

**Hard prohibitions (stage-wide, including tests):**
- Never read, open or stat any provider file or environment variable, such as `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `.credentials.json`, `auth.json`, `~/.claude*` or `~/.codex`. Tests may plant sentinels, but only to prove these files are untouched.
- Never spawn any process from non-test code. `std::process::Command` must not appear under `src/`.
- Never write outside the data root.
- Never recursively delete anything.
- Never install software or use the network beyond `cargo` fetching the pinned dependencies.

## 2. CLI contract

### 2.1 Grammar

```text
larp [-h|--help] [-V|--version]
larp cc [-h|--help]
larp cc profile [-h|--help]
larp cc profile add <NAME> --provider <PROVIDER>
larp cc profile list
larp cc profile show <NAME>
larp cc profile rename <NAME> <NEW_NAME>
larp cc profile forget <NAME> [--yes]
```

**Parser settings:**
- The parser is `clap` derive.
- `larp`, `larp cc` and `larp cc profile` set `arg_required_else_help`: with no subcommand, they print help to stderr and exit 2.
- `disable_help_subcommand = true`: there is no `help` subcommand.
- Only the top-level `larp` has `--version`. It prints `larp 0.1.0` (from `CARGO_PKG_VERSION`) and exits 0.
- Options may appear before or after positionals (clap default). No other global flags.
- `NAME`/`NEW_NAME` parse via `ProfileName::from_str`.
- `PROVIDER` accepts exactly `claude` or `codex`. Help shows `[possible values: claude, codex]`.
- Unknown subcommands exit 2. That includes `run`, `login`, `status`, `logout`, `alias`. Help must not mention unimplemented commands.

### 2.2 Help strings (frozen text; clap renders layout)

| Item | Text |
|---|---|
| `larp` about | `Named Claude Code and Codex account profiles` |
| `cc` about | `Manage Claude Code and Codex profiles` |
| `cc` after-help | `Data directory: $LARPCC_HOME if set, otherwise $XDG_DATA_HOME/larpcc or ~/.local/share/larpcc (Linux), ~/Library/Application Support/larpcc (macOS), %LOCALAPPDATA%\larpcc (Windows).` |
| `profile` about | `Manage profile records (no login, no provider launch)` |
| `add` about | `Register a profile and create its empty state directory` |
| `list` about | `List profiles` |
| `show` about | `Show one profile` |
| `rename` about | `Rename a profile; its ID and state directory do not change` |
| `forget` about | `Remove a profile from the registry; its state directory is kept` |
| `<NAME>` (add) | `Profile name: 1-32 characters from a-z, 0-9, '-', '_', starting with a letter` |
| `<NAME>` (other) | `Profile name` |
| `<NEW_NAME>` | `New profile name` |
| `--provider` | `Provider this profile is for` |
| `--yes` | `Confirm removing the profile from the registry` |

The CLI developer records the rendered help as fixtures (§12.2). After review, those fixtures are the frozen help.

### 2.3 Output (stdout, exit 0)

**Rendering rules:**
- `<dir>` is `Path::display()` of the derived state directory.
- Valid names are printed raw; the character set makes this safe.
- Lines end with `\n`. No trailing whitespace. No color.

**add**
```text
Added profile work (claude, id 1).
State directory: <dir>
```
**list** (sorted by name, byte order)
```text
NAME      PROVIDER  ID  STATE
personal  codex     2   ok
work      claude    1   missing
```
Each column is left-aligned, padded to the widest of its header and values, with two spaces between columns. The last column is not padded. `STATE` is `ok`, `missing` or `unsafe`. If there are no profiles, including when the data root does not exist, the output is the single line `No profiles.`.

**show**
```text
Name:      work
Provider:  claude
ID:        1
State dir: <dir>
State:     ok
```
An unsafe directory shows `State:     unsafe: <reason>` (reasons in §7.2).

**rename**
```text
Renamed profile work to job (id 1).
State directory unchanged: <dir>
```

**forget --yes**
```text
Forgot profile work (id 1).
State directory kept: <dir>
```

### 2.4 Errors (stderr)

**Operational errors** (exit 1): the format is `error: <message>`, then zero or more `hint: <text>` lines. Stdout stays empty. Messages and hints come from `Error`'s `Display` and `hint()` (§7).

**Usage errors** (exit 2) are clap's errors. They print `error: …`, a usage line and the "For more information, try '--help'." line. Invalid name and provider values render as clap's `error: invalid value '<input>' for '<NAME>': <NameError message>`.

**`forget` without `--yes`** is a usage error. If the profile does not exist, the result is `NotFound` with exit 1. If it exists, stderr is:
```text
error: refusing to forget profile work without --yes
hint: the registry entry would be removed; the state directory <dir> is kept and this version cannot attach it to a new profile
```
Exit 2, and nothing is written.

### 2.5 Exit codes and robustness

- **Exit codes.** 0 = success, or help/version. 1 = any `Error`, or a failed stdout write. 2 = a usage error.
- **No panics.** No input or IO failure causes a panic. Non-test code has no `unwrap()`/`expect()` on IO, parsing or environment values.
- **Stdout failures.** A stdout write failure, including `BrokenPipe`, exits 1. A `BrokenPipe` prints nothing. Any other write failure prints `error: cannot write output: <io error>`.

## 3. Data root

`Registry::locate()` resolves the root from the environment using `env::var_os`. It touches no files.
1. If `LARPCC_HOME` is set and non-empty, use it as-is.
2. Otherwise, on Unix other than macOS: use `$XDG_DATA_HOME/larpcc` if `XDG_DATA_HOME` is non-empty and absolute; otherwise `$HOME/.local/share/larpcc` if `HOME` is absolute.
3. On macOS: `$HOME/Library/Application Support/larpcc` if `HOME` is absolute.
4. On Windows: `%LOCALAPPDATA%\larpcc` if `LOCALAPPDATA` is absolute.
5. If nothing applies: `Error::NoDataDir`.

**Validation of the resolved root (from any source, and from `Registry::at`):**
- It must be absolute and contain no `..` component; otherwise `Error::InvalidRoot`.
- The root is not canonicalized. Ancestors may be symlinks (for example macOS `/var`). Only the root and everything below it are checked (§4.3).
- Root paths may contain spaces and Unicode, and on Unix arbitrary non-UTF-8 bytes. This works because no path is ever serialized.

## 4. On-disk format

### 4.1 Layout

```text
<root>/                  dir, created 0700
  registry.json          file 0600, the only state file
  registry.lock          file 0600, empty, created on first write, never replaced or deleted
  registry.json.tmp      file 0600, transient, exists only during or after an interrupted write
  profiles/              dir 0700
    <id>/                dir 0700, created empty by `add`, never written to again in stage 1
```

`registry.lock` is separate from `registry.json` because `rename` swaps the inode of `registry.json`. A lock taken on the replaced file would stop excluding writers.

### 4.2 `registry.json` schema version 1

```json
{
  "version": 1,
  "revision": 2,
  "next_id": 3,
  "profiles": [
    {
      "id": 1,
      "name": "work",
      "provider": "claude"
    },
    {
      "id": 2,
      "name": "personal",
      "provider": "codex"
    }
  ]
}
```

**Writing:** `serde_json::to_writer_pretty` (2-space indentation), field order as shown, a trailing `\n`, profiles sorted by ascending `id`.

**Reading** fails as `Corrupt` unless every rule holds:
1. The file is a regular file of at most 1 MiB.
2. It is valid JSON with a top-level object. There are no trailing bytes and no BOM.
3. `version` is present and is an unsigned integer. A value greater than 1 is `NewerSchema` (checked **before** strict parsing, so newer files with new fields report `NewerSchema`). A value of 0 is `Corrupt`.
4. Strict parsing (`deny_unknown_fields`, duplicate keys rejected) finds `revision: u64`, `next_id: u64 >= 1` and a `profiles` array.
5. Each profile has `id: u64 >= 1`, a `name` that passes `ProfileName::parse`, and `provider` of `"claude"` or `"codex"`.
6. IDs are strictly ascending, and every ID is less than `next_id`.
7. Names are unique.

**Defaults and counters:**
- A missing file means the empty registry `{version 1, revision 0, next_id 1, profiles []}`, held in memory only.
- `revision` increases by exactly 1 on every successful mutation.
- `next_id` never decreases.
- Counter overflow is `Error::Exhausted`.

**Other notes:**
- Whitespace and CRLF differences are valid JSON and are accepted.
- The registry contains no paths, timestamps, account identities or credentials.
- Stage 2 and stage 4 introduce versions 2 and 3, with read-upgrade tests. Version 1 never gains fields.

### 4.3 Safety checks

**Unix** checks every path below. A failed check is `Unsafe` (exit 1). Per-profile directories are the exception: they get a status instead (§4.4).
- **Root:** `symlink_metadata` is not a symlink and is a directory; `uid == geteuid()`; `mode & 0o077 == 0`.
- **`registry.json`, `registry.lock`:** not a symlink; a regular file; owner and mode as for the root.
- **`profiles/`:** same as the root.
- **Foreign-directory rule (writes only):** if the root exists and is non-empty, it must contain at least one of `registry.json`, `registry.lock` or `profiles`. This stops `LARPCC_HOME=$HOME` from filling a home directory.

Read operations check the root, `registry.json` and `profiles/` if they exist. They never create anything. A missing root or registry means an empty list.

**Windows:**
- `symlink_metadata().file_type().is_symlink()` rejects symlinks and junctions (`std` treats name-surrogate reparse points as symlinks).
- Directory and file types are checked.
- Owner, mode and ACL checks are deferred to stage 2. That is documented, not silent.

### 4.4 Per-profile directory status

`list`, `show`, and the results of mutations report `DirStatus`:
- `Ok`
- `Missing`, which includes a missing `profiles/`
- `Unsafe(reason)` with a §7.2 reason: a symlink, not a directory, wrong owner, mode, or (Unix only) the same `(st_dev, st_ino)` as another profile's directory.

A status is informational. `rename` and `forget` still work on such profiles.

## 5. Persistence algorithms

### 5.1 Read (`list`, `get`)

No lock is taken and nothing is created.
1. Check the root (§4.3). A missing root means empty.
2. Check and read `registry.json`. A missing file means empty.
3. Validate (§4.2).
4. Compute `DirStatus` for every entry.

Readers always see a complete old or new file, because writers replace it atomically.

### 5.2 Write transaction (`add`, `rename`, `forget`)

1. **Root.** Create it if missing, using `DirBuilder::recursive(true)` with mode 0o700 on Unix. Then check it, including the foreign-directory rule.
2. **Lock.** If `registry.lock` is missing, create it with `create_new` and mode 0o600 (`AlreadyExists` is fine). Check it (§4.3), then open it read/write without truncating. Acquire the lock with `File::try_lock` in a loop: sleep 25 ms between tries, deadline 10 s, then `Busy`.
3. **Load** the registry under the lock (§5.1 steps 1–3). Any error aborts before a write.
4. **Validate the operation.**
   - `add`: if the name exists, `NameTaken`.
   - `rename`: the source must exist (`NotFound`), and the target must not (`NameTaken`; this includes renaming to the same name).
   - `forget`: the name must exist (`NotFound`).
5. **`add` only — create the directory.**
   - Ensure `profiles/` exists: `create_dir` with mode 0o700 if missing, then check it.
   - Start with `id = next_id` and try `DirBuilder::new().mode(0o700).create(profiles/<id>)`, which is not recursive.
   - On `AlreadyExists`, the directory is an orphan: increment `id` and retry, up to 1000 attempts; after that, `Unsafe{profiles, "contains too many unregistered directories"}`. Orphans are never reused, opened or deleted.
   - Set `next_id = id + 1`.
6. **Mutate** in memory and set `revision += 1`, using checked arithmetic.
7. **Write the temp file.** Remove a stale `registry.json.tmp`; `remove_file` removes a symlink itself and never its target, and `NotFound` is fine. Then `create_new` with mode 0o600, `write_all` the serialized bytes, `sync_all`, and close.
8. **Replace.** `rename(registry.json.tmp, registry.json)`. On Windows, retry `PermissionDenied` up to 10 times with 100 ms sleeps, for a reader holding the file open.
9. **Sync the directory.** On Unix, open the root directory and `sync_all` it. Windows has no equivalent; this is documented.
10. **Release** the lock (drop the file) and return the entry.

**Rollback:**
- If step 7 or 8 fails during `add`: best-effort `remove_dir(profiles/<id>)` (non-recursive; that directory was created empty in this same operation), then best-effort removal of the temp file. Return the original error.
- After step 8 succeeds, nothing is rolled back. If step 9 fails, return `Io` with the hint `the change was applied but may not be durable; run 'larp cc profile list'`.

`get`, `list` and the write operations all compute `DirStatus` against every profile, so identity duplicates are detected.

### 5.3 Crash and interruption states

There is no signal handling. Ctrl-C or SIGKILL counts as a crash.

| Interrupted after | Disk state | Next operation |
|---|---|---|
| lock creation | empty `registry.lock` | normal; the OS releases the lock when the process dies |
| step 5 | orphan empty `profiles/<id>` | `add` skips that ID; the orphan is never touched |
| part of step 7 | partial `registry.json.tmp` | readers ignore it; the next writer removes it |
| step 8, before step 9 | new registry; power loss may revert to old | both are valid |

**Invariant:** every record's directory existed when its record was committed.

### 5.4 Concurrency guarantees and limits

**Guarantees:**
- Mutations are serialized among Larp processes by an OS advisory lock: `flock` on Unix, `LockFileEx` on Windows. `std::fs::File::lock` is stable since Rust 1.89.
- Updates are never lost.
- Readers never see a partial file.
- A same-name race has exactly one winner. The losers create no directories, because name validation happens before step 5.

**Limits** (documented in the README at integration):
- Advisory locks do not bind non-Larp programs.
- Locks over network filesystems (NFS/SMB) are unsupported.
- A process running as the same user can swap files between Larp's checks and its use of them (TOCTOU). Larp detects static unsafe states, not an active same-user attacker.
- On WSL, roots under `/mnt/c` usually fail mode checks; use a Linux path.

## 6. Domain types and Rust interface (frozen)

The crate stays bin-only (`larpcc-cli`, binary `larp`). Module tree:

```text
src/main.rs            mod cli; mod registry; fn main() -> ExitCode { cli::run() }
src/cli/mod.rs         clap definitions, dispatch, exit-code mapping
src/cli/render.rs      stdout/stderr formatting (§2.3, §2.4)
src/registry/mod.rs    Registry and re-exports (below)
src/registry/model.rs  Provider, ProfileId, ProfileName, Profile, DirStatus, ProfileEntry
src/registry/error.rs  Error, NameError
src/registry/paths.rs  root resolution, safety checks, DirStatus computation
src/registry/store.rs  JSON load/validate/serialize, lock, atomic write, transactions
src/registry/tests.rs  registry unit tests (#[cfg(test)])
```

```rust
// registry/model.rs — complete in the seed commit
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider { Claude, Codex }
impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Codex];
    pub fn as_str(self) -> &'static str;            // "claude" | "codex"
}
impl fmt::Display for Provider;                     // as_str
impl FromStr for Provider { type Err = (); }        // exact lowercase match only

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileId(u64);                          // invariant >= 1; never reused
impl ProfileId { pub fn get(self) -> u64; }
impl fmt::Display for ProfileId;                    // decimal; also the directory name

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileName(String);                     // validated (rules below)
impl ProfileName {
    pub fn parse(s: &str) -> Result<ProfileName, NameError>;
    pub fn as_str(&self) -> &str;
}
impl FromStr for ProfileName { type Err = NameError; }   // = parse
impl fmt::Display for ProfileName;
impl<'de> Deserialize<'de> for ProfileName;         // via parse; failure -> serde error -> Corrupt

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile { pub id: ProfileId, pub name: ProfileName, pub provider: Provider }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirStatus { Ok, Missing, Unsafe(String) }  // reason from §7.2

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileEntry { pub profile: Profile, pub state_dir: PathBuf, pub status: DirStatus }
```

```rust
// registry/mod.rs — signatures frozen; seed bodies are stubs (§10.1)
pub use error::{Error, NameError};
pub use model::{DirStatus, Profile, ProfileEntry, ProfileId, ProfileName, Provider};

pub struct Registry { root: PathBuf }
impl Registry {
    pub fn locate() -> Result<Registry, Error>;          // §3; no filesystem access
    pub fn at(root: PathBuf) -> Result<Registry, Error>; // §3 validation; no filesystem access
    pub fn root(&self) -> &Path;
    pub fn list(&self) -> Result<Vec<ProfileEntry>, Error>;              // sorted by name; read-only
    pub fn get(&self, name: &ProfileName) -> Result<ProfileEntry, Error>; // read-only
    pub fn add(&self, name: ProfileName, provider: Provider) -> Result<ProfileEntry, Error>;
    pub fn rename(&self, from: &ProfileName, to: ProfileName) -> Result<ProfileEntry, Error>;
    pub fn forget(&self, name: &ProfileName) -> Result<ProfileEntry, Error>; // returns removed entry
}
```

**Name rules (`ProfileName::parse`):**
- ASCII only, matching `^[a-z][a-z0-9_-]{0,31}$`.
- Not one of the Windows device names `con prn aux nul com1`–`com9` `lpt1`–`lpt9`.
- Uppercase is rejected, not folded, so case-fold collisions cannot happen.
- Names never become paths; IDs do. Path traversal through names is impossible by construction.

**Implementation latitude:** internal items, such as a lock timeout parameter or an environment-lookup closure for testing root resolution, are registry-private and not frozen. Tests never mutate the process environment (`set_var` is `unsafe` in edition 2024 and races).

## 7. Error catalog (frozen)

### 7.1 `Error` (exit 1) and `NameError` (exit 2)

`Error` implements `Display` (message), `std::error::Error`, and `pub fn hint(&self) -> Option<String>`. `<p>` is `Path::display()`.

| Variant | Message | Hint |
|---|---|---|
| `NoDataDir` | `cannot determine the data directory` | `set LARPCC_HOME to an absolute path` |
| `InvalidRoot { path, reason }` | `invalid data directory <p>: <reason>` (`must be an absolute path` \| `must not contain '..'`) | `set LARPCC_HOME to an absolute path` |
| `Unsafe { path, reason }` | `unsafe data path <p>: <reason>` | mode reason: `run: chmod 700 <p>` (dir) or `chmod 600 <p>` (file); otherwise `fix or move it, or set LARPCC_HOME to another directory` |
| `Corrupt { path, reason }` | `registry <p> is invalid: <reason>` | `Larp CC did not modify it; move it aside to start an empty registry (state directories under <root>/profiles are not touched)` |
| `NewerSchema { path, found }` | `registry <p> uses schema version <found>; this Larp CC supports version 1` | `upgrade Larp CC; the file was not modified` |
| `NotFound { name }` | `no profile named <name>` | `run 'larp cc profile list'` |
| `NameTaken { name }` | `a profile named <name> already exists` | none |
| `Busy { path }` | `registry is locked by another Larp CC process (<p>)` | `retry when the other command finishes` |
| `Exhausted` | `registry counters are exhausted` | none |
| `Io { action, path, source }` | `cannot <action> <p>: <source>` | none, except §5.2 step 9 |

- **`Io` actions:** `create directory`, `inspect`, `open`, `read`, `write`, `lock`, `rename`, `sync`, `remove`.
- **`Corrupt` reasons** are human-readable and not frozen. Tests assert the message prefix and the exit code.
- **`NameError`:** `Invalid { input }` displays `invalid profile name <input:?>: use 1-32 characters from a-z, 0-9, '-' and '_', starting with a letter`. `Reserved { input }` displays `invalid profile name <input:?>: reserved on Windows`. Debug formatting escapes control characters, so they never reach the terminal raw.

### 7.2 Unsafe reasons (frozen strings)

- `is a symbolic link or reparse point`
- `is not a directory`
- `is not a regular file`
- `is owned by uid <actual>, expected <euid>`
- `has mode <mode:04o>; group and other access must be removed`
- `shares a filesystem identity with profile <id>`
- `is not empty and is not a Larp CC data directory`
- `contains too many unregistered directories`

## 8. Cross-platform boundaries

| Concern | Linux / macOS (`cfg(unix)`) | Windows (`cfg(windows)`) |
|---|---|---|
| Default root | XDG / `~/.local/share` (Linux); `~/Library/Application Support` (macOS) | `%LOCALAPPDATA%\larpcc` (not roaming) |
| Private creation | modes 0700/0600 via `DirBuilderExt`/`OpenOptionsExt` | inherited ACL of `%LOCALAPPDATA%`; explicit ACL is a stage 2 entry criterion |
| Verification | owner (`libc::geteuid`), mode, symlink, type, `(dev, ino)` duplicates | reparse point (symlink/junction) and type only |
| Lock | `File::lock`/`try_lock` (`flock`) | `File::lock`/`try_lock` (`LockFileEx`) |
| Replace | `rename` (atomic on one filesystem) plus directory `fsync` | `rename` (`MoveFileExW` replace) with `PermissionDenied` retry; no directory sync |
| Paths | non-UTF-8 supported (Linux tests; macOS filesystems reject it) | UTF-16 via `OsString`; Unicode supported |

Only `cfg(unix)` and `cfg(windows)` branches are allowed, with no other OS-specific code. Help-fixture comparisons normalize CRLF. Windows code is verified only by Windows CI (§9.2). There is no local Windows target.

## 9. Dependencies and toolchain

### 9.1 Accepted (exact lock versions resolved offline on AWS, 2026-11-11)

| Crate | Use | Why not std |
|---|---|---|
| `clap 4.6.7`, `default-features = false`, features `std, derive, help, usage, error-context` | argument parsing, help, usage errors, exit 2 | correct help/usage/OsString handling for nested subcommands is a large hand-written surface; color and suggestions are off to cut dependencies |
| `serde 1.0.229` (`derive`), `serde_json 1.0.151` | strict registry (de)serialization | strict unknown/duplicate-field rejection and escaping; a hand-written JSON codec is a correctness risk |
| `libc 0.2.189`, `cfg(unix)` only | `geteuid()` for the ownership check | std exposes file uid but not the process euid; one `unsafe` call with a `SAFETY` comment |
| `tempfile 3.27.0` (dev only) | temporary roots in tests | auto-cleanup temp directories; never used in non-test code |

**Rejected:**
- `fs2`/`fs4`/`fd-lock`: std `File::lock`.
- `dirs`/`directories`: three environment variables suffice.
- `anyhow`/`thiserror`: one small enum.
- `uuid`/`getrandom`: monotonic IDs.
- `assert_cmd`/`insta`: std `Command` plus fixture files.
- `toml`: no gain over JSON.
- runtime `tempfile`: a fixed temp name under the lock is enough.

The scratch probe compiled this set offline with Rust 1.99.1 and 1.89, and resolved 30 packages.

**Manifest changes (seed):**
- workspace `[workspace.package] rust-version = "1.89"`, inherited by the crate
- crate `[dependencies]` as above
- `Cargo.lock` regenerated with `cargo generate-lockfile --offline` and committed

`clippy::incompatible_msrv` then flags std APIs newer than 1.89.

### 9.2 CI (owned by the CLI slice)

`.github/workflows/ci.yml` runs on `pull_request` and on `push` to `main`, with `permissions: contents: read` and no secrets.
- **Runners:** standard GitHub-hosted runners only (`ubuntu-latest`, `macos-latest`, `windows-latest`). No paid runners.
- **Toolchain:** the runner's preinstalled `rustup`; no third-party toolchain action.
- **Actions:** `actions/checkout` pinned by full commit SHA, resolved by the implementer and checked by the reviewer.
- **Matrix:** all three OSes run `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`. Ubuntu also runs `cargo fmt --all -- --check`.
- **MSRV job (Ubuntu):** `rustup toolchain install 1.89 --profile minimal`, then `cargo +1.89 test --workspace --locked`.

## 10. Ownership

### 10.1 Contract seed (registry developer, first commit on `stage/1-registry`)

The seed commit is titled `Add stage 1 contract seed`. Exactly these files:
- `Cargo.toml` (adds `rust-version`), `crates/larpcc-cli/Cargo.toml` (dependencies), `Cargo.lock`
- `src/main.rs` in its final shape: `mod cli; #[allow(dead_code)] mod registry; fn main() -> ExitCode { cli::run() }`
- `src/cli/mod.rs` with `pub fn run() -> ExitCode`. It prints the current placeholder line and returns success, so behavior and the README stay true.
- `src/registry/model.rs` and `src/registry/error.rs`, complete per §6 and §7
- `src/registry/mod.rs` with the §6 signatures. Each body is `{ let _ = (<every parameter>); unimplemented!("stage 1 registry") }`. The `let _` keeps unused-parameter warnings from failing `-D warnings`; this was verified in a scratch crate. `root()` is implemented for real.

**Requirements:** the seed must pass fmt, clippy `-D warnings` and build with `--locked --offline`. Push it immediately and report the seed SHA `S` to the lead.

### 10.2 Slices

| Slice | Branch (parent) | Owns exclusively after the seed |
|---|---|---|
| Registry/core | `stage/1-registry` (`M0`) | `src/registry/**`, the manifests and `Cargo.lock` (changes only via §10.3) |
| CLI/help/fixtures | `stage/1-cli` (`S`) | `src/main.rs`, `src/cli/**`, `tests/**` (incl. `tests/common/`, `tests/fixtures/`), `.github/workflows/ci.yml`, `README.md` |

- **CLI developer:** removes `#[allow(dead_code)]` when wiring, and updates the README last, to describe exactly the integrated behavior: commands, data root, limits from §5.4, Windows ACL deferral.
- **Registry developer:** never edits CLI-owned files.
- **Neither** edits `plan.md`, this document, `AGENTS.md` or `CLAUDE.md`, except as §10.3 allows.

### 10.3 Contract-change rule

The frozen surface is §2, §3, §4, §6, §7, §9 and the seed file set. To change it:
1. The developer stops the dependent work.
2. The developer sends the lead a request of at most 10 lines, with an artifact giving the exact diff and the reason.
3. Only the lead approves.
4. The registry developer commits the approved change on `stage/1-registry`, with the matching edit to this document in the same commit.
5. The CLI developer rebases.

No silent drift: reviewers compare the final code with this document. Unfrozen detail, such as `Corrupt` reasons and private helpers, needs no request.

### 10.4 Integration sequence

1. `M0`: this planning PR is merged. The lead starts the registry thread.
2. The seed `S` is pushed. The lead starts the CLI thread on `stage/1-cli` from `S`.
3. Both slices work in parallel. On the CLI branch, storage-dependent end-to-end tests can fail until it is rebased onto a registry head. That is the only allowed red, and only before integration.
4. The registry slice is done when it is green (fmt, clippy, unit tests, no stubs). It opens draft PR-R: `stage/1-registry` → `main`.
5. The CLI developer rebases `stage/1-cli` onto PR-R's head and opens draft PR-C: `stage/1-cli` → `stage/1-registry`. PR-C's head `H` is the integrated head and the subject of all final evidence.
6. Fix cycles: each finding is fixed by the owner of the file. A registry fix means re-pushing PR-R, after which PR-C rebases. Every fix gets a new `H` and fresh evidence.
7. The lead merges PR-R and then PR-C (retargeted to `main`). Use merge commits; squashing PR-R breaks the stack. Main's final tree must equal `H`'s tree.

## 11. Implementation notes (non-negotiable)

- No `unsafe` except the single `geteuid` call.
- At final head, `src/` contains no `unimplemented!`, `todo!`, `dbg!`, `allow(dead_code)` or `Command`.
- The `State` column comes from `DirStatus`. The CLI derives nothing from the filesystem; all filesystem logic lives in `registry`.
- One registry load per command. `list` and `show` do O(n) work with n ≤ (1 MiB of JSON). Sort with `sort_by` on name.
- `show` and `forget` without `--yes` use `get`.

## 12. Tests

All tests are hermetic:
- They use `tempfile` roots and an explicit `LARPCC_HOME` (or `Registry::at`).
- They never touch the real home or installed providers.
- They make no network calls.
- They skip permission-denial assertions when `geteuid() == 0`.

### 12.1 Registry unit tests (`src/registry/tests.rs`, registry slice)

| ID | Case |
|---|---|
| R01 | Name table. Valid: `a`, `a1`, `work-1`, `w_x`, a 32-char name. Invalid: `""`, 33 chars, `A`, `Work`, `1a`, `-a`, `_a`, `a b`, `a/b`, `a\b`, `.`, `..`, `../x`, `é`, full-width `ａ`, `a\n`, `a\u{1b}`. Reserved: `con nul aux prn com1 com9 lpt1 lpt9`; `com0` and `console` are valid. |
| R02 | `Provider` parse/display round-trip; `Claude` and `CLAUDE` rejected |
| R03 | Serialization is byte-exact to the §4.2 example; parse then re-serialize is identical |
| R04 | Each §4.2 rule violated: not JSON, empty file, truncated, BOM, trailing data, top-level array, missing/`0`/string `version`, unknown field (top level and profile), duplicate key, duplicate ID, unsorted IDs, `id: 0`, `id >= next_id`, `id` as string `"../x"`, invalid or duplicate name, unknown provider, over 1 MiB. Each gives `Corrupt`; a following `add` also fails and the file bytes are unchanged |
| R05 | `version: 2` with unknown fields gives `NewerSchema`, and the file is unchanged after a failed `add` |
| R06 | Missing root: `list` is empty, `get` is `NotFound`, the root is not created |
| R07 | First `add`: root, `profiles` and `<id>` are 0700; registry and lock are 0600; `revision 1`, `next_id 2`, `id 1` |
| R08 | add/rename/forget semantics: ID and directory stable across rename; rename to self gives `NameTaken`; forget keeps the directory; re-adding a forgotten name gives a new ID; `next_id` is monotonic; `revision` counts mutations |
| R09 | Pre-existing `profiles/1` containing a file: `add` gets ID 2, and `profiles/1` is byte-identical afterwards |
| R10 | Stale garbage temp file, and a temp file that is a symlink to an outside file: `add` succeeds, the outside file is unchanged, no temp file remains |
| R11 | Unix `Unsafe`: root symlink, root is a file, root 0755, registry 0644, registry symlink, lock symlink, `profiles` symlink, `profiles` 0755. Writes fail; reads fail for root, registry and `profiles`; nothing changes |
| R12 | `DirStatus`: missing gives `Missing`; mode 0755, a symlink, or a regular file gives `Unsafe` with the §7.2 reason. Directories cannot be hard-linked, so duplicate-`(dev, ino)` detection is tested through the registry-private pure helper with synthetic identities. `rename`/`forget` still work on such profiles |
| R13 | Foreign directory (root holds only `notes.txt`): `add` gives `Unsafe`; `list` is empty; nothing is written |
| R14 | Threads: 16 distinct adds give 16 profiles, unique IDs, `revision 16`. 8 same-name adds give 1 `Ok` and 7 `NameTaken`, with exactly one new directory |
| R15 | The test holds `registry.lock`; `add` with a 200 ms timeout gives `Busy`; the registry is unchanged |
| R16 | Root resolution through an injected environment: precedence, empty values ignored, relative `XDG_DATA_HOME` ignored, relative or `..` `LARPCC_HOME` gives `InvalidRoot`, missing `HOME` gives `NoDataDir` |
| R17 | Linux: a non-UTF-8 root works, and the registry bytes contain no path |
| R18 | `next_id = u64::MAX` or `revision = u64::MAX`: `add` gives `Exhausted` and nothing changes |
| R19 | Unix: after one successful `add` (so the lock and `profiles/` exist), chmod the root to 0500, which still passes the check. A second `add` creates `profiles/2`, then fails creating the temp file and returns `Io`. `profiles/2` is gone (rollback) and the registry bytes are unchanged |
| R20 | Display and `hint()` strings for every `Error`/`NameError` variant equal §7 |

### 12.2 End-to-end tests (`tests/`, CLI slice)

The helper `tests/common/mod.rs` runs `env!("CARGO_BIN_EXE_larp")`. It sets a fresh `LARPCC_HOME` and points `HOME`, `XDG_DATA_HOME`, `LOCALAPPDATA`, `APPDATA`, `CLAUDE_CONFIG_DIR` and `CODEX_HOME` at separate sentinel directories. The sentinels contain planted `.credentials.json`, `auth.json`, `.claude.json` and `config.toml`. Fixtures live in `tests/fixtures/help/*.txt` and `tests/fixtures/registry/{valid,invalid,newer}/*.json`.

| ID | Case |
|---|---|
| C01 | Help fixtures for `larp`, `cc`, `profile` and each of the 5 subcommands with `--help`: exit 0, exact stdout (CRLF normalized), no `run/login/status/logout/alias` text |
| C02 | `larp --version` prints `larp 0.1.0\n`, exit 0. `-V` gives the same |
| C03 | Exit 2, stdout empty, stderr starts `error:`, for: `larp`; `larp cc`; `larp cc profile`; `larp cc run x`; `larp cc login x`; `larp cc alias`; `add`; `add x`; `add x --provider gpt`; `add Bad --provider claude`; `add 'a b' …`; `add é …`; `add con …`; `show`; `rename a`; `--foo`; extra positional; `forget a` (existing) without `--yes`, whose stderr matches §2.4 and which writes nothing |
| C04 | Lifecycle with exact stdout: add `work` claude, add `personal` codex, list, show, rename `work`→`job` (same ID and directory), `forget personal --yes`, list, re-add `personal` (ID 3; `profiles/2` still exists) |
| C05 | Exit 1 with exact `error:`/`hint:` lines: show missing, rename missing, rename to a taken name, add a taken name, forget missing (with and without `--yes`) |
| C06 | Every invalid fixture: `list` and `add` exit 1 with the prefix `error: registry`, and the file bytes are unchanged. The newer fixture gives the §7 schema message. Valid fixtures, including CRLF and reformatted JSON, list correctly |
| C07 | Read-only commands (help, version, list, show, all C03 cases) with a non-existent `LARPCC_HOME`: the root is never created |
| C08 | No out-of-root writes: snapshot every sentinel tree and the temp parent (paths, sizes, SHA-256, modes) before and after C04; they are identical. The root contains only the §4.1 entries |
| C09 | No provider execution (Unix): `PATH` starts with a directory whose executable `claude`/`codex` scripts create a marker file; after C04, no marker exists |
| C10 | Multi-process: 16 concurrent distinct adds all exit 0 (16 listed, `revision 16`). 8 concurrent same-name adds give one 0 and seven 1, with one directory. 8 adds alongside a `list` loop: every `list` exits 0 |
| C11 | The test holds the `File::lock` on `registry.lock` for 500 ms while an `add` runs; the `add` exits 0 only after the lock is released |
| C12 | Kill recovery, 30 iterations with fixed-seed delays of 0–20 ms: spawn `add`, then `Child::kill` (SIGKILL / TerminateProcess). After each, `list` exits 0, every record's directory exists, and a following `add` succeeds. Runs on all OSes |
| C13 | Root containing spaces and Unicode (`dir with space/ünï cödé`): the lifecycle passes and `show` prints the path. Linux: a non-UTF-8 `LARPCC_HOME` passes the lifecycle |
| C14 | `LARPCC_HOME` relative, or with `..`, gives exit 1 with the §7 message. With `LARPCC_HOME` unset, the per-OS default under the sentinel `HOME`/`XDG_DATA_HOME`/`LOCALAPPDATA` is used, as shown by `show` |
| C15 | Unix, not root: root 0755 gives exit 1 with a `chmod 700` hint; a read-only root (0500) makes `add` exit 1 with no new files |
| C16 | Linux: stdout set to `/dev/full` makes `list` exit 1 with no `panicked` in stderr |
| C17 | Windows: a junction root (`cmd /C mklink /J`) gives exit 1 with `is a symbolic link or reparse point` |

### 12.3 Independent tester checklist (lead-arranged; integrated head `H` only)

| ID | Check |
|---|---|
| T01 | Fresh clone at `H`: fmt, clippy `-D warnings`, `cargo test --locked --offline`, release build, `cargo +1.89 test`; CI green on all three OSes for exactly `H` |
| T02 | Linux `strace -f -e trace=%file,execve,%process` over the C04 lifecycle: the only `execve` is `larp`; no file access outside `LARPCC_HOME` except loader, libc, locale, `/proc/self`, `/dev`; none under the sentinel `HOME`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME` |
| T03 | Hand-edited registries (trailing comma, BOM, CRLF, tabs, reordered keys, `"version": 1.0`) behave per §4.2 |
| T04 | Permission matrix under umask 077, 022 and 002; group-readable root is rejected; registry mode 0400 still allows reads and writes, because writes replace the file through the directory, and leaves the new file at 0600 |
| T05 | Stress: 64 concurrent mixed add/rename/forget/list processes × 10 runs, then a 200-iteration kill loop. Invariants: valid registry, unique IDs and names, `revision` equals the successful mutations, every record has its directory, orphans untouched |
| T06 | Hostile names (RTL override, confusables, 10 KB, ANSI escapes, `%s`, `$(…)`): exit 2, stderr contains no raw control bytes |
| T07 | README fidelity: every README command run at `H` behaves and prints as written |
| T08 | Static greps at `H` for the §1 prohibitions and §11 items; `cargo tree` direct dependencies equal §9.1 |
| T09 | Record (no claims): release binary size and `list` cold-start time |

## 13. Definition of done (merge evidence)

The lead merges only when **all** of the following hold for the same integrated head `H`:
1. PR-R and PR-C are open as drafts, from distinct worktrees, with no AI or co-author trailers.
2. Every R and C test passes. Local Linux evidence plus CI on Linux, macOS, Windows and MSRV 1.89, all for `H`.
3. fmt and clippy `-D warnings` are clean with `--locked`, and the §11 greps come back empty.
4. The README describes exactly `H`'s behavior, and every README command has been verified at `H` (T07).
5. An independent tester report covers T01–T09 with pass/fail per item, at `H`.
6. An OCR **delegate** review report at `H`: OCR file/rule selection, per-file coverage of every changed file including skips with reasons, and reviewer findings covering simplification, correctness, data structures, performance and security. Each finding is fixed or accepted by the lead in writing. It is not reported as "OCR passed".
7. Any commit after the evidence invalidates it, so steps 2–6 are re-run on the new `H`.
8. Frozen-surface changes, if any, were approved under §10.3 and are reflected in this document.

**Evidence artifacts:** each role writes `/tmp/larpcc-stage1-<role>-<short H>.md` with a handoff of at most 10 lines. No credentials or account data.
