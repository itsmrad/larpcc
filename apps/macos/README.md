# LarpCC for macOS

Native SwiftUI settings window, macOS 14+, Swift 5.9+. It starts in explicitly
labelled **sample data** mode: changes stay in memory and reset on restart. This
preserves fixture screenshots (`--appearance=light` / `--appearance=dark`, plus
`--page=general|mcp|skills|aliases|desktop|about|profile-<id>` to open a page).
Pages are grouped forms: General, MCP Servers, Skills, Aliases, Desktop Apps,
About, then one page per profile. The appearance is a local view preference in
UserDefaults; nothing else is persisted.

**General › larp executable › Choose…** explicitly selects a trusted local native executable named `larp`.
Live mode clears the samples immediately; failed operations never simulate success.
Selection rejects scripts, symlinks, directories, non-executables and non-Mach-O
files. A bounded SHA-256 fingerprint, file identity and resolved path are rechecked
before every invocation. A schema-1 JSON help response must identify `larp cc`.
These checks detect accidental replacement, not a malicious executable: the user
must trust the selected build. There remains a path replacement race between
recheck and Foundation Process execution; the app does not claim adversarial
filesystem isolation. Selection is session-only and is never discovered via PATH.

The merged CLI contract is prospective: most commands are not implemented yet.
Contract examples and synthetic CLI tests demonstrate the bridge, **not real CLI
integration or provider qualification**. Unsupported or malformed responses show a
safe unavailable state. No fallback accesses provider files, apps, keychain or
network. CLI terminal login/run/logout is unavailable. Desktop open requests go
only through `larp cc app open`, with qualification enforced by Larp. A dispatch
receipt describes only local launched/focused outcome, not auth, readiness or
isolation. Configuration indicators never imply signed-in state.

Every mutation/open uses the displayed coherent snapshot's revision and immutable
ID/name mapping. Matrix and metadata reads also carry that revision. A conflict
refreshes metadata and requires a new action; a stale delete is never retried.
Delete requests folder/alias removal through Larp, rather than metadata-only
forget. Keychain items and remote grants may remain; no logout is performed.
Display paths are strings only and are never used as executable or action paths.
The current UI shows CLI folder metadata; binding versions remain undetected in
presentation and null values are safe. Recovery/drift errors use fixed safe text.

Process IO runs on a background queue, drains both pipes concurrently with
nonblocking reads, caps each stream at 1 MiB and requires empty stderr. A 15-second
deadline and cancellation terminate then kill/reap only the direct child. No
process-tree killing, polling service or automatic persistent tasks are used.
Cancellation may occur after a CLI side effect: refresh before another action.
Raw JSON failures, stderr, arbitrary diagnostic/error messages and secret
references are never displayed or logged. The bundled frozen schema validates
required fields, enums, numeric strings and rejects unknown fields before typed
decoding; the workflow checks its bytes against the merged source contract.

From this directory:

```sh
swift test
swift run LarpCC
./scripts/bundle.sh
open dist/LarpCC.app --args --appearance=light
# Quit before relaunching with --appearance=dark.
```

The unsigned bundle includes SwiftPM's schema resource bundle at the app root.
`LarpCC --validate-contract-resource` validates packaged decoding and exits 0/1
without opening a window or invoking a CLI. Verify relocation with the original
`.build` directory absent. No signing, notarization, installation or artifact publishing is
included. The dedicated macOS workflow runs synthetic tests and release bundling.
Native manual validation still covers keyboard/VoiceOver, selection, search,
catalog switches, cancellation, delete wording, and light/dark appearance.
