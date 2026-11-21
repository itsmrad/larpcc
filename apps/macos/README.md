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

## Menu bar

LarpCC also adds a menu bar item (General › Menu Bar › Show in menu bar, on by
default). Its template glyph shows the tightest "% left" across enabled profiles
and both windows, one chosen profile's tightest window, or the glyph alone; with no
usage data it is always the glyph alone. The needle drops below 25% and 10% left.

The panel lists one card per enabled profile, tightest first: 5-hour and weekly
bars of percent left with an even-pace tick ("Faster than even pace" when usage runs
faster), "Resets in 2h 13m" countdowns, limit-reset availability (always shown,
also on each profile page as "Limit resets"), when it was updated, and an Open
desktop app button (enabled surfaces, never with sample data). Open in Terminal
stays disabled until larp has a command for it. Orange (below 25%) and red
(below 10%) always come with "Low" / "Almost out" text. ↑/↓ move between cards,
Return opens Settings, Esc closes; ⌘R refreshes, ⌘, opens Settings, ⌘Q quits.

Refresh rules follow the usage source. **Local**: opening the panel re-reads the
data already saved on this Mac (no timer). **Live**: nothing is read on open;
only the panel's Refresh or the opt-in interval reads live usage. "Hide Dock icon
when the window is closed" switches to a menu-bar-only app after the window closes
and restores the Dock icon when the window opens again; it needs the menu bar item.

Screenshots: in debug builds only (`swift run LarpCC --menubar-panel-snapshot=/abs/path.png
--appearance=dark`), the snapshot option renders the panel with sample data at 2x,
honouring `--appearance=light|dark`, and exits. Release builds have no such hook;
capture the live panel there with `screencapture -x -l <windowID>`.
`--page=general` shows the Menu Bar settings, and `--page=profile-1` (2 resets),
`profile-2` (no reset info) or `profile-3` (no resets) show the usage section.

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
