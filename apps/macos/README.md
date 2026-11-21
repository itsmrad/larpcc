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

Every `larp cc` mutation in the CLI v1 contract has native UI: New Profile (⌘N
or + in the sidebar), Rename, surfaces, Forget, Delete, Recover, Migrate, app and
provider binding, aliases (with `--desktop` / `--shadow`), MCP servers (command or
https URL, environment-variable references only, never secret values), skills
(a chosen folder) and the per-profile switches. Input is validated like the CLI
before any process runs. A larp build that lacks a command answers with a `parse`
envelope, shown as "not supported yet"; other error codes map to fixed safe text.
Sample mode applies every action to the in-memory fixture.

Every mutation/open uses the displayed coherent snapshot's revision and immutable
ID/name mapping. Matrix and metadata reads also carry that revision. A conflict
refreshes metadata and requires a new action; a stale delete is never retried.
Delete requests folder/alias removal through Larp, rather than metadata-only
forget. Keychain items and remote grants may remain; no logout is performed.
Display paths are strings only and are never used as executable or action paths.
The current UI shows CLI folder metadata; binding versions remain undetected in
presentation and null values are safe. Recovery/drift errors use fixed safe text.

Process IO runs on a background queue, drains both pipes concurrently with
nonblocking reads, caps each stream at 1 MiB and requires empty stderr. Reads have a
15-second deadline; changes have none, because stopping one mid-transaction
leaves an unknown outcome (Cancel warns first). A deadline or cancellation terminates then kill/reap only the direct child. No
process-tree killing, polling service or automatic persistent tasks are used.
Cancellation may occur after a CLI side effect: refresh before another action.
Raw JSON failures, stderr, arbitrary diagnostic/error messages and secret
references are never displayed or logged. The bundled frozen schema validates
required fields, enums, numeric strings and rejects unknown fields before typed
decoding; the workflow checks its bytes against the merged source contract.

## Menu bar

The item is on by default under General › Menu Bar. It shows the session
percentage left for Codex and Claude as up to two stacked template lines. Each
provider defaults to the most constrained enabled profile with session data.
Pick a profile per provider in Settings, or choose “Icon only”. The item uses a
gauge when neither provider has a session percentage to show.

The native menu panel opens to the last selected tab. Overview lists enabled
profiles by the tightest limit. Codex and Claude filter that list and appear
when they have enabled profiles. Each section shows session and weekly usage
when available, with the percentage left, reset countdown, an even-pace tick
and a projection based on average usage so far. Limit reset credits always
appear and use the same wording as the profile page. The desktop action in the
footer follows the highlighted profile and requires an enabled desktop surface
and a real CLI configuration.

Use ↑/↓ to move between sections and footer actions. Use ←/→ or Tab to switch
tabs. Return opens Settings for a profile section or activates a footer row.
Esc closes the panel. The footer reads “Refresh ⌘R” in both Local and Live
modes. ⌘R refreshes, ⌘, opens Settings, and ⌘Q quits.

In **Local** mode, opening the panel re-reads saved usage. In **Live** mode,
only Refresh or the existing opt-in interval reads usage. The panel uses
`TimelineView(.everyMinute)` to redraw countdowns, update ages and pace
indicators from loaded data. “Hide Dock icon when the window is closed”
switches to a menu-bar-only app while Settings is closed and needs the menu
bar item to be shown.

In debug builds, render a sample panel at 2x and exit:

```sh
swift run LarpCC --menubar-panel-snapshot=/abs/path.png --menubar-panel-tab=overview --appearance=dark
```

The snapshot accepts `overview`, `codex` or `claude` and `light` or `dark`.
Release builds exclude the snapshot hook. Capture the live panel there with
`screencapture -x -l <windowID>`.

### Native Mac checklist

- **A. Status item** Confirm Codex above Claude in sample mode, then verify the
  per-provider pickers, one-provider layout and Icon only. Check the item against
  both light and dark menu bars.
- **B. Panel** Check all four sample profiles in Overview and the filtered
  provider tabs. Confirm reserve, deficit, on-pace, all three reset-credit
  states, and the profile with no saved usage. Check the full-width hover
  highlight, disabled sample desktop action, and overflow on a small screen.
  Confirm the footer reads “Refresh ⌘R” in both Local and Live modes.
- **C. Refresh** Reopen in Local mode and check the update age. Use ⌘R to
  re-read usage.
- **D. Keyboard and VoiceOver** Move through sections and footer rows, switch
  tabs, activate with Return, and dismiss with Esc. Check one spoken summary
  per profile and the selected state on tabs.
- **E. Settings** Close Settings, confirm the item stays, then reopen Settings
  with the footer action or ⌘,.
- **F. Dock** Enable Dock hiding in Settings and verify the icon disappears on
  close and returns on reopen. Restore the original setting afterwards.
- **G. Visibility** Turn off “Show in menu bar” and verify the Dock icon remains.
  Restore the original setting afterwards.
- **H. Profile usage** Confirm the reset wording on the profile pages and
  smoke-test opening and cancelling the New Profile sheet with ⌘N.
- **I. Appearance** Capture Overview, a provider tab and the status item in both
  light and dark. Check text and bar contrast, including on the highlight.

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
