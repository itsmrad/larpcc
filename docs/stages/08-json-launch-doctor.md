# Stage 08 — machine output, launch injection and doctor

Import is **OUT OF SCOPE**; fresh profiles only. Depends on merged v3 seed and
stages 05–07 render contracts. CORE owns shared CLI/main/launch/provider/registry
integration, fixtures and CI. MAC consumes the schema, never edits shared
registration. [CLI JSON v1](../contracts/cli-v1.md) and its schema freeze the ABI;
seed introduces serializer/framing, this slice completes all command variants.

```text
larp cc doctor [PROFILE] [--detect-versions] [--json]
larp cc status PROFILE --json
larp cc run|login|logout PROFILE [existing options]  # human-only native IO
```

Doctor without detection and JSON status report local metadata, surfaces,
render/alias drift, recorded bindings and empty native rows. Never read native
auth/config/session files, keychain, socket/pid contents or account output.
Unknown detected version is null. JSON success for native run/login/logout is
not offered; exit-2 `json_native_io` precedes effects. App JSON receipt is stage 09.

## Explicit version preparation, separate from account qualification

`doctor --detect-versions` is opt-in and foreground, no background cache refresh.
CORE validates an explicitly bound official artifact using existing trusted path,
owner/ACL/identity, bounded hash and revalidation machinery. It does not scan PATH.
A separate reviewed version-probe descriptor pins provider/OS/artifact digest,
exact `--version` argv, isolated-home requirements, parser and bounds. This is
not an ArtifactRow and cannot admit run/login/logout/native status. A missing or
unknown descriptor/source yields detected_version=null and safe diagnostic.
No source/digest is approved by this planning PR. Reliable reviewed static bundle
metadata may provide desktop version without executing the app; otherwise null.
A recorded version label never substitutes for detected version.

Probe uses a fresh private throwaway home in temporary LARPCC_HOME, fixture name
claude-work or codex-45. Construct a clean explicit environment with synthetic
HOME/USERPROFILE/XDG/APPDATA paths and the selected fresh CLAUDE_CONFIG_DIR or
CODEX_HOME; no ambient auth keys or login state. No native auth/status/login/logout,
model requests, MCP server or app open. Retain checked handles, revalidate bytes
before spawn; no shell, stdin closed, no interactive flow. Capture **version text
only**, capped at 4 KiB combined stdout/stderr, timeout 5 seconds. Descriptor
requires exact version grammar and zero exit; no arbitrary output is returned or
logged. Reject unexpected bytes/lines, oversize, errors and timeouts with null
and fixed redacted diagnostic; kill/reap known direct child within 2 seconds.
If cleanup/liveness cannot be proved, fail and report safe recovery requirement.
Use existing synthetic lifecycle primitives rather than a general subprocess API.
No direct provider-network requests by Larp; descriptor proof must exclude native
version-triggered network/auth/global writes. Unknown behavior refuses the probe.
All temporary roots, profile directories and child artifacts removed in finally
on success/failure; cleanup failure is operational failure, never silent pass.
Detected versions are result-local; ordinary read-only doctor does not probe or
implicitly trust cached versions. No provider probe executed in Phase 0.

## Launch injection

Native ROWS remains empty in this delivery, all four ordinary operations retain
early closure before managed IO. No CLI option/environment unlock. Synthetic-only
invented rows exercise same checked snapshot/admission and plan machinery for
`cli` surface paths, skills and deterministic render injection. Amendment of a
future native row needs reviewed exact flags/reference support and new SHA-linked
native qualification, including existing project/admin config, no-daemon/backend,
auth/permission/guard rules and abnormal child limitations. Neither MCP config
files nor alias dispatch loosens policy. No blanket CLI flag allowlist.

Implement/execute native `cargo test --workspace --locked stage08_`:
`stage08_json` all example command variants, parser/help/version, ordering/u64,
1 MiB overflow/exit/stdout/stderr/framing and no raw input echo;
`stage08_doctor` fixture version=1.2.3 detected, no probe without flag, unknown
artifact=null, malformed/oversize/nonzero/timeout output redacted, clean env and
finally cleanup; `stage08_injection` invented rows exact argv/cli homes and
changed render/refs refusal; `stage08_closed` ordinary debug/release/all-features
operations closed and JSON native IO has zero child/root effects. Retain all
stage-3 A01–A08 tests. Zero matches fails. Temporary fixtures only; clean all homes.
