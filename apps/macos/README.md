# LarpCC for macOS

Native SwiftUI settings window, requiring macOS 14 or newer and Swift 5.9 or newer.
This development build **always uses sample mode**. All changes live in memory and
reset when the app restarts. It does not execute the CLI, access provider files,
launch providers, inspect authentication, install aliases, or make network calls.
Green means a sample configuration is ready, never that an account is signed in.
Authentication is always unknown. Launch actions refuse with qualification pending.
Delete removes only the sample record and assignments, never files on disk.

From this directory:

```sh
swift test
swift run LarpCC
./scripts/bundle.sh
open dist/LarpCC.app
```

The script builds an unsigned release bundle under `dist/`. No signing,
notarization, installation, artifact publishing, or provider binaries are included.
The dedicated macOS workflow builds and runs synthetic tests. A native manual
run is still required for layout, keyboard navigation, VoiceOver, and light/dark
appearance, including search, matrix scrolling, toggles and delete confirmation.

`LarpClient` is the only data/action boundary. `FixtureLarpClient` uses an isolated
in-memory snapshot. Its presentation structs are deliberately not Codable and
are not a proposed wire schema. The draft CLI contract at planning commit
`b9f7b9a0b4779515c1e2c203273241b6025cbbec` informs sample semantics only.
The CLI surface switch is explicit; desktop has a separate switch. Neither
qualifies launch. Display paths are placeholders and never action arguments.

A real process client is deferred until the CLI contract is merged and frozen.
It must use fixed structured `larp --json` argv, async bounded IO (at most 1 MiB),
schema-first validation, canonical u64 string IDs and fresh ID/name targeting.
It must reject unsupported schemas and malformed envelopes with safe errors,
never expose raw provider output, and keep command-unavailable and
qualification-pending states usable. Only the CLI may mutate managed data or
remove a profile folder. No direct filesystem or provider fallback is allowed.
