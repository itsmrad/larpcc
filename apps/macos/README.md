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

The item is on by default under General › Menu Bar. For Codex and Claude it
shows a ring of the session (5-hour) usage left, with the provider mark inside
and the percentage beside it. Each provider defaults to the most constrained
enabled profile with session data. Pick a profile per provider in Settings, or
choose “Icon only”. The item uses a gauge when neither provider has a session
percentage to show.

The panel lists enabled profiles as cards, grouped by provider, tightest limit
first. Each card shows a ring of the session left, the profile name, when the
usage was saved and its source, and one line per usage window. Each line has
the percentage left, a bar and the reset countdown. Orange and red appear only
below 25% and 10%. A profile without saved usage reads “No usage saved yet”.
Every card always shows the limit reset credits (“2 resets available”, “No
resets available” or “Reset info not available”), and VoiceOver reads them.
Click a card or press Space to show the pace, “Open Claude.app” or “Open
ChatGPT.app” (needs an enabled desktop surface and a real CLI configuration)
and “Show in Settings”, which opens that profile’s page once metadata is
loaded and drops the request if the profile is gone.

Pace is worked out at the moment the usage was saved, so an older sample
never drifts. Once a window’s reset time has passed, the card says “Out of
date — refresh” and the pace line does too. The panel shows only its own
feedback (a failed desktop launch reads “Couldn’t open Claude.app.”) and the
usage error. Other notices stay in the Settings window.

The panel is drawn like the Settings window, so the two read as one app. Its
background is the same translucent window material as the Settings page (the
wallpaper tints it; Reduce Transparency makes it opaque). Each card is drawn
like a Settings form section: a faint fill with 12 pt corners, with its rows
(name and freshness, the meters, the reset credits) split by inset
separators. Provider names are bold section titles, rows use the Settings
fonts, and the footer items are plain menu items with the accent highlight.
A selected card gets an outline, never a tint. Increase Contrast adds a firm
border. The panel is 360 pt wide and fits its content up
to 80% of the visible height of its own screen, then the cards scroll. The
header, notices and footer are measured, and even the first layout is held to
that height. The window resizes as cards expand.

Use ↑/↓ to move between cards, card actions and footer rows. Return opens
the highlighted card’s profile in Settings or activates the highlighted action.
VoiceOver offers “Show in Settings” directly on each card. Mouse and keyboard
share one selection. It stays when the pointer leaves a row, so Return still
acts on it, and it clears when its row goes away. Esc closes the panel, and the
next click on the item opens it again.
⌘R refreshes, even with the Settings window open; ⌘, opens Settings; and ⌘Q
quits. Nothing is highlighted or expanded when the panel opens.

In **Local** mode, opening the panel re-reads saved usage. In **Live** mode,
only Refresh or the existing opt-in interval reads usage. The panel uses
`TimelineView(.everyMinute)` to redraw countdowns, update ages and pace
from loaded data. “Hide Dock icon when the window is closed”
switches to a menu-bar-only app while Settings is closed and needs the menu
bar item to be shown.

In debug builds, render a sample panel at 2x and exit:

```sh
swift run LarpCC --menubar-panel-snapshot=/abs/path.png --appearance=dark \
  --menubar-panel-expanded=1 --menubar-status-snapshot=/abs/status.png
```

`--menubar-panel-expanded=<profile id>` opens one card,
`--menubar-panel-selected=<profile id>` draws one card selected,
`--menubar-status-snapshot` also writes the status item on a light and a dark
bar, and `--menubar-panel-accessibility` renders with Increase Contrast and
Reduce Transparency. `--menubar-screen-height=<pt>` stands in for a short
display (in the snapshot and in live debug runs), and `--menubar-notice=<text>`
adds a notice, so overflow can be checked offscreen: the footer always stays
whole, the header comes next, and the cards get what is left. Snapshots cannot
draw the window material, so they use the Settings page colour sampled on
macOS 26. Release builds exclude the snapshot hook. Capture the live panel there
with `screencapture -x -o -l <windowID>`. Link debug builds the way
`scripts/bundle.sh` does (`-Xlinker -platform_version …`); otherwise macOS
draws the app in its compatibility style with small window corners.

### Native Mac checklist

- **A. Status item** Confirm the Codex ring before the Claude ring in sample
  mode, then verify the per-provider pickers, one-provider layout and Icon
  only. Check the item against both light and dark menu bars.
- **B. Panel** Check all four sample cards. Confirm the colour only on low
  limits, the card with no saved usage, all three reset-credit states on the
  collapsed cards, and that the bars line up across cards. Expand each card.
  Confirm reserve, deficit and on-pace, the disabled sample desktop action,
  and that the window grows and shrinks with the card. Check overflow with
  `--menubar-screen-height=360` and a long `--menubar-notice`, and on a small
  screen: the footer must stay whole.
- **C. Refresh** Reopen in Local mode and check the update age. Use ⌘R to
  re-read usage, including with the Settings window open and LarpCC
  frontmost. The panel must stay open.
- **D. Keyboard and VoiceOver** Move through cards, card actions and footer
  rows. Expand a card with Space. With a card highlighted, press Return and
  confirm its profile opens in Settings, including when sidebar search was active.
  Hover a card, move into the gutter and press Return: the hovered card’s
  profile opens. Repeat using the card’s “Show in Settings” VoiceOver action and
  check one spoken summary per card, including its reset credits. After Esc,
  confirm one click reopens the panel.
- **E. Settings** Close Settings, confirm the item stays, then reopen Settings
  with the footer action, ⌘, or a card’s “Show in Settings”.
- **F. Dock** Enable Dock hiding in Settings and verify the icon disappears on
  close and returns on reopen. Restore the original setting afterwards.
- **G. Visibility** Turn off “Show in menu bar” and verify the Dock icon remains.
  Restore the original setting afterwards.
- **H. Profile usage** Confirm the reset wording on the profile pages and
  smoke-test opening and cancelling the New Profile sheet with ⌘N.
- **I. Appearance** Capture the panel (collapsed and with one card open) and
  the status item in light and dark. Compare the panel corner with the
  previous release. Check Increase Contrast and Reduce Transparency with the
  accessibility snapshot or the system settings.

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
