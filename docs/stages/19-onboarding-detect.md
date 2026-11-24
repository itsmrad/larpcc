# Stage 19 — Onboarding detection (D40)

`larp cc provider detect [--provider claude|codex] [--json]` is a local,
read-only onboarding command. macOS and Linux search for candidate CLI
installations. macOS also checks the two vendors' desktop bundles. Windows
returns a fixed `unsupported` error before registry or candidate lookup.

## Plan

1. Read stored binding paths using the registry snapshot and revision
   precondition. Build a bounded list from the bindings, conventional install
   locations and each absolute PATH entry. Guard every discovery and symlink
   resolution step before filesystem access.
2. Reuse the binding trust checks, executable evidence and exact Stage 08
   descriptor allowlist. Keep candidate probe execution closed. Keep
   executable and desktop observations independent of account state and
   launch admission.
3. Freeze the `provider.detect` read envelope in [CLI JSON v1](../contracts/cli-v1.md)
   and check it on synthetic trees and all CI platforms. The app can consume
   this command in its separately approved onboarding step.

## Search and results

Check the stored binding first, then conventional locations, then each PATH
directory in order. Search `~/.local/bin`, Claude's native
`~/.local/share/claude/versions/<version>` executable or version directory,
Homebrew bin/Cellar/Caskroom under `/opt/homebrew` and `/usr/local`, known
nvm, Volta, fnm, Bun and npm global layouts, and the official command spellings
in standalone bin locations. Enumerate just the immediate names at each
version level. Use only absolute paths and never invoke a package manager.

Install layout identifies provenance candidates. A layout or a basename alone
does not establish that an executable is official. Binding trust checks include
the symlink chain, owner, permissions, ACLs, executable type and identity.
Unavailable or untrusted digests are null, with a fixed bindability state.
Qualification requires an exact provider, OS, architecture and digest match in
the native artifact rows. Version approval uses the separate Stage 08
allowlist and requires the same provider, OS, architecture and digest.
Without a match, `version` is `"unapproved"` and `version_diagnostic` is
`"version_unapproved"`. A match returns `version:null` and
`"version_probe_unavailable"`. Detection never executes a candidate or
returns a parsed version. Both tables start empty. Deduplicate per provider
by resolved device/inode. Prefer the stored
binding when several spellings name the same file. Sort by bound, qualified,
install kind (`native`, `homebrew`, `npm`, `bun`, `unknown`), then provider and
path. Binding remains the separate, explicit `provider bind` command.

On macOS check `Claude.app` and `ChatGPT.app` under `/Applications` and
`~/Applications`. Read bounded bundle identifier and version metadata using
held directory descriptors. Both code requirements are `unavailable` in
detection. Claude lacks a reviewed requirement. The existing ChatGPT static
check and executable code-directory hash require pathname-based Security APIs,
so detection leaves them unavailable. Explicit desktop bind retains its
existing static check and flags. Detection makes no network requests.

The success envelope uses `command:"provider.detect"` and the observed
registry revision, including `"0"` on a fresh installation. Its data has
`executables`, `desktop_apps`, `skipped` and `limited`. Each executable exposes
`provider`, `path`, `resolved_path`, `identity` (Unix device and inode as
decimal strings), `size` (decimal string), `install_kind`, `bindable`, nullable
`bind_reason` (the bind validator's reason when available),
`qualified`, `bound`, nullable `sha256` and `cdhash`, `version`, and
`version_diagnostic`. A desktop row contains `provider`, `path`,
`resolved_path`, nullable `bundle_id` and `version`, and `code_requirement`
(`passed`, `failed` or `unavailable`). Desktop order is system before home,
then provider. Linux has an empty desktop array.
Skipped rows contain `provider`, the nominated `path` and the fixed reason
`inside provider home`. They sort by provider and path. The resolved
destination is omitted.

## Safety and acceptance

Never descend into a provider home, a configured `CLAUDE_CONFIG_DIR` or
`CODEX_HOME`, or vendor app-data directories. Exclude these paths before
inspection, including during symlink resolution. A bounded scan can
return partial results with `limited:true`. The discovery budget allows 48
candidate spellings, 160 search directories, 2,048 entries in version
directories, 40 links per lookup, and 10 seconds of elapsed time between
filesystem operations. Use no installation or real account in
tests. Cover a forbidden symlink destination, a link loop, inaccessible
directories, duplicate spellings, a fake executable under a writable parent,
the revision precondition and the fixed Windows response.

Detection keeps the observed executable descriptor for evidence and rechecks
the nominated spelling through the same guarded component walk. It uses the
bind validator's owner, D41 directory mode, ACL, native-header, executable bit,
self-image and managed-root rules. Unlike explicit bind, it refuses evidence
when the spelling changes during the scan, and its macOS ACL check uses
descriptors. A low-level failure is reported without a filesystem error path.
The exclusions apply to configured spellings and symlink targets reached by
the walk. An opaque filesystem alias such as a separate mount can expose the
same directory without either spelling. Discovery does not open protected
roots to build an identity map. The registry snapshot retains the existing
path-based registry checks after detection's guarded root preflight.

The Stage 08 runner for explicit bindings reopens the executable by pathname.
On macOS that runner has no descriptor-based execution path. On Linux,
descriptor execution pins file identity but permits writes to the same inode.
Detect would need an immutable executable snapshot and a checked throwaway
workspace for candidate execution. Stage 08's explicit-binding probe retains
its existing behavior on both platforms.
