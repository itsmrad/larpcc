# Stage 10 — native SwiftUI CLI consumer

Import is **OUT OF SCOPE**; fresh profiles only. After CLI completion then desktop
interface, MAC owns a native sidebar/settings-style SwiftUI app in apps/macos/**.
No third-party style reference, Tauri dependency or library extraction. CORE owns
any shared CLI changes, contract amendments first. Native app builds serialize
with other Mac builds (one xcodebuild/cargo at a time).

The app invokes an explicitly selected validated Larp binary with argument arrays,
never a shell string. Metadata commands always --json and decode schema:1;
profile list, MCP/skills matrices drive explicit per-profile switches. Fresh
snapshot on revision conflict, no automatic account selection. Launch buttons use
local app.open receipt; native terminal login/run remains a separately reviewed
human path, never captured into GUI JSON. JSON status means local metadata only.
No account identity, quota meter or auth inference from zero exit/receipt.

UI handles unknown schema, empty arrays, absent/unsafe paths, null detected version,
closed qualification, drift/recovery, stale names and ID precision; destructive
confirmation describes folder deletion versus metadata-only forget and retained
keychain grants. Keyboard/VoiceOver, cancellation, bounded process IO and stale
responses covered. No polling, telemetry or secret fields. No publish/signing/new
spending implied. Unsupported operations are shown as unavailable with safe reason.

MAC freezes executable test target in UI seed; native `xcodebuild test` must
exercise fixture-driven decode of every cli-v1.examples.json command, matrix
toggles, rename race and delete warning, bad/oversize JSON and CLI failure;
zero test cases/merely compiling is not acceptance. CI registration belongs to
MAC's macos-app.yml, shared CLI registration to CORE. Synthetic fixture CLI only,
no vendor/account execution, use temporary app data and delete it after testing.
Final independent QA/OCR and lead merge required; native qualification remains a
separate gate even if the GUI builds.
