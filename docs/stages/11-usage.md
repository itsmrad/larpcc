# Stage 11 — usage limits (5-hour, weekly, resets)

Prospective contract, docs only. Import is **OUT OF SCOPE**; fresh profiles only.
No Rust in this planning PR. A later CORE worker implements the CLI against this
contract; app and menu bar consumers follow the rollout rule below.

## Decision record (D14)

On 2026-11-17 the user decided, explicitly and after the lead explained the ban
risk (relayed by the lead): show each profile's 5-hour limit, weekly limit and
reset times in the CLI and the Mac app, with **both** modes. **LOCAL** is the
default. **LIVE** is opt-in in settings and ships enabled-but-opt-in, labelled
"unverified against real accounts" until a user-authorized qualification. This
amends [plan.md](../../plan.md#stage-11-usage-limits-exception-d14) binding
invariants 2 and 3 for this feature only and narrowly amends stage 04's
"never reads history/session contents" rule (LOCAL Codex reader below). Every
other invariant stays binding. Agents never read real tokens or call real
endpoints; LIVE is built and tested against fakes until the user authorizes
real verification (RULES 4a).

**Rollout (user rule 2026-11-21, D16).** This PR specifies the complete CLI
surface. The stage-11 CLI implementation comes right after this plan merges.
The Mac app and the menu bar (MENUBAR lane, branch `stage/11-usage-ui`) build
against this contract and its fixtures only; that branch is not wired to real
CLI data until the lead's explicit go. UI shows only fields the CLI provides.

**Not in this stage.** Account email and plan from the official client's status
command are stage 14 (D25/D26: shown from a fresh status run, never stored); this
stage does not duplicate them. Here, a plan appears only as a **user-entered**
label. No spend, cost, credit balance or token counts anywhere.

## CLI surface

```text
larp cc usage [PROFILE] [--local|--live]                       # command "usage"
larp cc settings usage                                         # "settings.usage"
larp cc settings usage live on --acknowledge-risk              # "settings.usage.live"
larp cc settings usage live off                                # "settings.usage.live"
larp cc settings usage mode local|live [--profile PROFILE]     # "settings.usage.mode"
larp cc settings usage mode inherit --profile PROFILE          # "settings.usage.mode"
larp cc settings usage interval off|SECONDS                    # "settings.usage.interval"
larp cc settings usage claude-statusline on|off                # "settings.usage.claude-statusline"
larp cc profile plan-label PROFILE LABEL                       # "profile.plan-label"
larp cc profile plan-label PROFILE --clear                     # "profile.plan-label"
```

All accept global `--json` and `--expected-revision REV` with cli-v1 framing.
Settings live under `settings usage` because a profile may legally be named
`settings`, `live` or `mode`; `usage [PROFILE]` therefore has no subcommands.

- `usage` without PROFILE reports every **active** profile in ASCII name order
  (tombstoned profiles are omitted). `usage PROFILE` on a tombstone fails
  `recovery_required`, exit 1. Unknown name: `not_found`, exit 1.
- Without a flag each profile uses its effective mode (profile override, else
  installation mode; default `local`). `--local` forces LOCAL for all rows and
  never touches the network. `--live` forces LIVE for all rows and fails
  `live_disabled`, exit 1, before any credential read unless LIVE is enabled and
  the current warning is acknowledged. `--local --live` is `usage`, exit 2.
- `live on` without `--acknowledge-risk` fails `confirmation_required`, exit 2,
  without effects (same rule as a missing `--yes`). It is required on every
  enable. Human mode prints the warning text below before enabling; no prompt.
- `live off` atomically sets the installation mode to `local` and every profile
  override `live` to `inherit`, so a stored `live` mode always implies LIVE is
  enabled. `mode live` while LIVE is off fails `live_disabled`, exit 1.
- `mode inherit` requires `--profile`; `mode local|live` without `--profile` sets
  the installation mode. `interval` accepts `off` or an integer 300–86400.
- `plan-label` takes exactly one of LABEL or `--clear` (else `usage`, exit 2).
  LABEL must match the grammar below or fails `usage`, exit 2, without echo.
- Receipts are the common `changed,profile_id,name,warnings` mutation result.
  `profile.plan-label` and per-profile `settings.usage.mode` carry the profile
  ID and profile name; installation-wide `settings.usage.*` carry null/null.
  `live on` adds warning `live_unverified`. Same value again: `changed:false`.
- A hidden fixed hook entry point (Claude capture, below) is not part of the
  public grammar or help, and has no JSON mode.

### `--expected-revision`, exit codes and JSON framing

`usage` and `settings usage` are registry-backed reads; all other commands here
are registry-backed mutations. REV is compared under the registry lock before
name resolution, any capture read, credential read or request; mismatch is
`conflict`, exit 1, without effects. GUI mutations always pass it. Exit codes are
cli-v1: 0 success, 1 local operational error, 2 usage/confirmation. One envelope
plus LF on stdout, stderr empty, no progress output.

`usage --live --json` is **Larp's own** network IO, not native IO: no child
process, no PTY, no captured provider output. It is therefore not
`json_native_io`. Its envelope is built only from the allowlisted parsed fields.
Per-profile LIVE failures (expired token, timeout, unknown shape) are reported
as row reasons inside an `ok:true` envelope, exit 0. Only whole-command
failures (`live_disabled`, `conflict`, registry errors) are error envelopes.

The cli-v1 rule "no timestamps or nondeterministic keys" is amended for
`usage` only: it carries `generated_at`, `captured_at`, `resets_at` and
`runs_out_at`. `settings.usage` and all receipts stay timestamp-free.

## Persistence

- `usage.json` at the LARPCC_HOME root: strict `version:1`, at most 64 KiB, under
  the registry lock, written with the stage 06 index commit pattern (temporary
  file, fsync, rename, then registry revision bump; guarded relock). Fields:
  `mode`, `live.enabled`, `live.acknowledged_warning` (integer or null),
  `refresh_interval_seconds` (null or 300–86400), `claude_statusline_capture`
  (default true) and `profiles: {"<id>": {mode, plan_label}}`. Unknown fields
  or versions refuse. Entries for IDs that are not active are ignored and pruned
  by the next usage-settings mutation; IDs are never reused, so delete/forget do
  not need to touch this file. No timestamps, tokens or provider data here.
- Claude capture: `profiles/<id>/usage/claude-statusline.json` (below). The
  `usage/` directory is a new Larp-owned sibling of `cli/` and `desktop/`,
  outside every native home, created 0700 by the hook under the validated
  profile container. Whole-profile delete removes it with the folder.
- Claude settings render: `profiles/<id>/usage/claude-settings.json`, a Larp-owned
  render recorded in the profile's owned-file manifest (stage 04 rules: hash,
  identity, drift refuses, never overwrite changed bytes).

## `usage` JSON (normative schema: [cli-v1.schema.json](../contracts/cli-v1.schema.json))

`data = {requested_mode, generated_at, profiles[]}`. `requested_mode` is
`settings|local|live`; `generated_at` is the local clock at the start of the
command. Each row:

| field | meaning |
|---|---|
| `profile_id`, `name`, `provider` | LarpCC identity only. No email, account ID, org or user name. |
| `mode` | effective mode for this row, `local` or `live` |
| `source` | `codex_session_log`, `claude_statusline`, `live_endpoint`, or null when nothing was read |
| `captured_at` | when the values were observed (local clock), or null. Values are "as of last use" in LOCAL. |
| `client_version` | version of the official client that wrote the LOCAL record (`^N.N.N$`), else null |
| `plan` | null, or `{label, source:"user"}`: the user-entered label only |
| `windows.five_hour`, `windows.weekly` | always present window objects, W = 18000 s and 604800 s |
| `windows.extra` | null (this source has no extra windows) or an array of 0–8 extra windows |
| `reset_credits` | always present tri-state object |
| `diagnostics` | safe `code,message` objects, ordered by code then message |

**Window** `{used_percent, resets_at, window_seconds, reason, pace}`.
`used_percent` is 0–100 with at most one decimal; `resets_at` is an RFC 3339 UTC
timestamp with whole seconds and `Z`. `reason` is null **exactly** when both
values are present; otherwise it says why:

| reason | meaning (app text) |
|---|---|
| `not_captured` | no record captured yet for this profile ("Not available yet") |
| `not_reported` | a record exists but the client did not report this window |
| `unsupported` | this source/provider never carries this field |
| `unverified_version` | the record's client version is not in the verified table below |
| `reset_since_capture` | the captured window has reset since (`generated_at >= resets_at`) |
| `reset_time_unknown` | usage known, reset time not reported (`used_percent` kept) |
| `capture_not_installed` | Claude capture is not wired (no qualified Larp launch, capture off, or unsupported OS) |
| `invalid_record` | record malformed, out of range, oversized, unsafe path or from the future |
| `live_disabled` | effective mode is LIVE but the current warning is not acknowledged |
| `sign_in_again` | LIVE credential missing, expired or rejected: "Sign in again in the official client" |
| `live_unavailable` | LIVE network/TLS/timeout/redirect/HTTP error or unknown response shape |

Value normalization: finite numbers in [0, 100] are rounded half away from zero
to one decimal; (100, 1000] clamp to 100; anything else is `invalid_record`.
Reset instants are integer seconds in [2020-01-01, 2100-01-01) or
`invalid_record`. A record whose `captured_at` is more than 300 s after
`generated_at` is `invalid_record`. LOCAL values never expire except by
`reset_since_capture`; nothing is inferred from other fields.

**Extra windows** add `label` and allow `window_seconds: null` (no pace then).
Labels are **Larp-assigned**, ASCII, 1–48 characters, letters/digits and
`. _ + / ( ) -`, single inner spaces, no leading/trailing space
(`usage_extra_label` in the schema). Provider strings that do not fit are
dropped with diagnostic `extra_label_rejected`, never echoed. Order: label in
ASCII order, then `window_seconds` ascending with null last. Labels are unique
(later duplicates dropped, `extra_duplicate`). Cap 8, overflow dropped in order
with `extra_truncated`. `[]` means the source supports extras and reported none;
null means the source cannot report extras.

**Reset credits** `{state, count, expires_state, expires_at}`, never absent:

| state | count | meaning (app text) |
|---|---|---|
| `available` | integer >= 1 | "N resets available" |
| `none` | 0 | the client recorded zero: "No resets available" |
| `not_recorded` | null | the source can carry it but no value is captured yet |
| `unsupported` | null | this source/version never records it: "Reset info not available" |

`expires_state` is `recorded` (then `expires_at` is set), `none` (recorded as no
expiry), `not_recorded` or `unsupported`; for `none|not_recorded|unsupported`
states it equals the state. "Resets remaining" = `count` plus optional expiry.

**Plan label** grammar: ASCII, 1–32 characters, starts with a letter or digit,
letters/digits and `. _ + / ( ) -`, single inner spaces (`plan_label`). No `@`,
quotes, control or non-ASCII characters, so it cannot carry an email. It is
user metadata, never verified, never read from tokens, credential files,
session logs or provider output, and shown with `source:"user"`. D14 LIVE
covers only an in-memory token read for one usage request; it does **not**
cover reading plan or account identity, so LIVE never reads
`subscriptionType`/`rateLimitTier` or `plan_type` (open decision 1).

### Pace (normative; vectors in [usage-pace-vectors.json](../contracts/usage-pace-vectors.json))

Computed per window by the CLI at **T = captured_at** (the instant U was
observed), never at display time. R = `resets_at`, U = `used_percent`,
W = `window_seconds` (18000 five_hour, 604800 weekly; extra windows only when
`window_seconds` is reported). All arithmetic is exact integer arithmetic on
seconds and tenths of a percent (U10 = 10·U).

1. Pace is null when U, R, W or T is null, when the window has a non-null
   reason, when R <= T, or when R − T > W + 300 (clock skew beyond tolerance;
   diagnostic `clock_skew`, values still shown).
2. E = clamp((W − (R − T)) / W, 0, 1). Elapsed seconds e = E·W.
3. `elapsed_percent` = 100·E and `reserve_percent` = 100·E − U, each rounded half
   away from zero to one decimal. Positive is reserve, negative is deficit.
4. If U = 0: `runs_out_at` = R, `lasts_until_reset` = true. Else if E = 0:
   both null (rate unknown). Else X = T + floor((100 − U)·e / U)
   = T + floor((100 − U)/(U/(E·W))); if X >= R then `runs_out_at` = R and
   `lasts_until_reset` = true ("lasts until reset"), else `runs_out_at` = X and
   `lasts_until_reset` = false. U = 100 gives X = T.

Clock skew: R comes from the provider's clock, T and `generated_at` from the
local clock. Up to 300 s of skew is absorbed by the clamp; more disables pace.
`validate.py` contains the reference implementation and checks every vector and
every fixture's pace. Rust tests (`stage11_pace`) must load the same vector file.

## LOCAL Codex: session log reader

**Verified source (Codex 0.169.0, tag `rust-v0.169.0`, commit
`d32063f96c2dff1dc96af839b3952abb6acfeda4`, read via the GitHub API):**

- `codex-rs/protocol/src/protocol.rs:2645-2703`: `TokenCountEvent {info,
  rate_limits: Option<RateLimitSnapshot>}`; `RateLimitSnapshot {limit_id,
  limit_name, normal_model_slug?, primary, secondary, credits, individual_limit,
  spend_control_reached, plan_type, rate_limit_reached_type}`;
  `RateLimitWindow {used_percent: f64 /* 0-100 */, window_minutes: Option<i64>,
  resets_at: Option<i64> /* unix seconds */}`.
- `codex-rs/rollout/src/policy.rs:134-140`: `TokenCount` is persisted in every
  history mode. `codex-rs/history/src/rollout_payload.rs:36-38,75-77` and
  `codex-rs/rollout/src/recorder.rs:2041-2061`: each line is
  `{"timestamp":"YYYY-MM-DDTHH:MM:SS.mmmZ","ordinal"?:N,"type":"event_msg",
  "payload":{"type":"token_count","info":…,"rate_limits":…}}`, timestamp UTC.
- `codex-rs/rollout/src/recorder.rs:1684-1706` and `codex-rs/rollout/src/rollout_file_name.rs:62-74`: files are
  `CODEX_HOME/sessions/YYYY/MM/DD/rollout-YYYY-MM-DDTHH-MM-SS-<thread>.jsonl`
  (local-time directories); `codex-rs/rollout/src/compression.rs:26,46-53`: rollouts older than 7
  days may become `.jsonl.zst`.
- `codex-rs/tui/src/chatwidget/rate_limits.rs:103-146`: the official label comes
  from `window_minutes`, not position: 300 min ±5% is "5h", 10080 ±5% "weekly"
  (also daily 1440, monthly 43200, annual 525600).
- `codex-rs/core/src/state/session.rs:400-411,479-499` (traced by a helper, not
  re-read): one latest snapshot per session, not keyed by bucket; a missing
  `limit_id` defaults to `"codex"`. A persisted snapshot can therefore belong to
  another bucket; filter by `limit_id`.
- `protocol.rs:3485-3510`, `codex-rs/rollout/src/recorder.rs:987`: the first line `session_meta`
  carries `cli_version` but also cwd, base instructions and creator account IDs.

**Reader contract.** CODEX_HOME is the derived `profiles/<id>/cli`. Under the
retained validated profile handle, enumerate only `sessions/YYYY/MM/DD`
directories (exact digit grammar) newest first and files matching
`rollout-<YYYY-MM-DDTHH-MM-SS>-<id>.jsonl` (grammar-checked; `.zst`,
`archived_sessions/` and anything else skipped). No-follow opens; refuse
symlinks, reparse points, hardlinks (link count > 1), non-regular files and any
object whose identity leaves the profile root. Newest files by name first.

- Caps: 62 day-directories, 2,000 directory entries, 8 files, per file the
  first line (≤ 1 MiB) plus the last 4 MiB, 20,000 lines per file, 40 MiB and
  2 s total. Hitting a cap stops scanning with diagnostic `scan_limit`.
- Per line: a streaming JSON deserializer whose target struct names only
  `timestamp`, `type`, `payload.type` and, for `event_msg`/`token_count`, the
  allowlisted `payload.rate_limits` subfields `limit_id`, `limit_name`,
  `primary|secondary.{used_percent, window_minutes, resets_at}`. Every other
  value is skipped unretained (e.g. serde `IgnoredAny`). `info`, `credits`,
  `individual_limit`, `plan_type` and conversation items are never materialized.
  From `session_meta` only `cli_version` is extracted the same way. A partial
  final line (no LF) is ignored.
- Parser errors are mapped to fixed codes; the error value is **never**
  formatted, because deserializer messages can quote input. Nothing from a log
  is logged, cached, written or printed except the normalized output fields.
- Selection: the newest `token_count` with non-null `rate_limits` per
  `limit_id` (null = `codex`). Bucket `codex`: a window whose
  `window_minutes` is 300 ±5% fills `five_hour`, 10080 ±5% fills `weekly`
  (position ignored); other codex windows become extras labelled
  `Codex <5h|daily|weekly|monthly|annual|Nm>`. Other buckets become extras
  labelled `<limit_name or limit_id> <duration>` after grammar check, with
  `window_seconds = window_minutes·60` (null when absent). `captured_at` is that
  line's `timestamp` truncated to seconds. `client_version` is the file's
  `session_meta.cli_version`; only verified versions (0.169.0) are interpreted,
  others report `unverified_version` with null values.
- Codex windows have no reset-credit data in the log: `reset_credits` is
  `unsupported`. `extra` is `[]` when only the codex bucket was seen.
- No rows exist yet when the user has never run Codex in that profile:
  `not_captured`. A resumed session written by a different Codex version than
  its `session_meta` says is a known limitation (threat review).

## LOCAL Claude: statusLine capture hook

**Verified source (Claude Code 2.1.325):** npm `@anthropic-ai/claude-code@2.1.325`
(sha256 `e842def1f55378eeb736f43f7e172a7eb0fe767b9ae4522a9ce717fd541ee27f`) and
`@anthropic-ai/claude-code-linux-x64@2.1.325` (sha256
`e8735d37521f49c31d65c3dcd8c0c8babe6d1f5f72c2066346b3ae88d32ac31d`, embedded
`VERSION 2.1.325`, `GIT_SHA a360e7297ecfcddeae9ec99fb3c93153ed86aca4`), inspected
as untrusted data with `strings` only, never executed; plus
<https://code.claude.com/docs/en/statusline> and
<https://code.claude.com/docs/en/settings> (read 2026-11-21).

- The statusLine command receives JSON on stdin with optional
  `rate_limits.five_hour` and `rate_limits.seven_day`, each
  `{used_percentage: number 0-100, resets_at: number unix seconds}`. The builder
  computes `used_percentage = min(round(utilization·1000)/10, 100)` and includes
  a window only while its `resets_at` has not passed; `rate_limits` appears only
  for claude.ai subscribers after the first API response. A gateway-only
  `spend_limit` (USD) exists and is ignored. There are no reset-credit, plan or
  extra-window fields. The same input also carries `session_id`,
  `transcript_path`, `cwd`, `workspace.repo`, `model`, `cost`, `context_window`,
  `version` and more: all hostile/private to the hook except the allowlist.
- Settings: `statusLine: {type:"command", command: string (run in a shell),
  padding?, refreshInterval? (>= 1 s), hideVimModeIndicator?}`. It runs at
  session start, on assistant messages, resets, timers etc., debounced 300 ms,
  and an in-flight run is cancelled by a newer one. It is skipped when
  `disableAllHooks` is true or workspace trust is not accepted; managed-only
  policy uses only the managed `statusLine`. The command's stderr goes to
  Claude Code's debug log.
- `--settings <file-or-json>` exists in 2.1.325. Docs: it applies "above your
  user, project, and local files and below managed settings"; binary source
  order `userSettings, projectSettings, localSettings, flagSettings,
  policySettings`.

**Install options compared.**

| | Larp-owned file via `--settings` (recommended) | owned block in profile `settings.json` |
|---|---|---|
| native file writes | none; the render lives in `profiles/<id>/usage/` | merges into a file Claude Code itself rewrites (`/config`, permissions, model) |
| stage 04 rules | ordinary owned render + manifest | needs native config parse/merge, which stage 04 forbids; constant drift refusals |
| precedence | above user/project/local, so a repo's `.claude/settings.json` cannot replace the hook | a project or local statusLine overrides it |
| coverage | only Larp-launched sessions (qualified run rows, aliases) | also manual `CLAUDE_CONFIG_DIR=… claude` sessions |
| tamper | file hash-checked by Larp before every launch | any native rewrite silently changes it |

Recommendation: `--settings` with the owned render. The launch adapter appends
the reviewed flag `--settings <abs path to claude-settings.json>` only through a
qualified native row (stage 08 injection rules). **Native rows are still empty**,
so until a Claude row is qualified every Claude profile reports
`capture_not_installed`; the GUI shows "Not available". Manual sessions outside
Larp are never captured (no native config writes).

The render is exactly
`{"statusLine":{"type":"command","command":"'<root>/bin/.larp-usage-hook' claude-statusline <ID>","padding":0}}`
when `claude_statusline_capture` is on (else no `--settings` flag). It
replaces the user's own statusLine for Larp-launched sessions; turning capture
off restores it. The path is single-quoted; a root path containing `'`, a
newline, a control or non-ASCII character makes capture unavailable
(`capture_not_installed`, diagnostic `unsafe_path`). Windows is
`capture_not_installed` (diagnostic `unsupported_os`) until the statusLine shell
on Windows is verified.

**Hook executable.** `<root>/bin/.larp-usage-hook` is a root-pinned copy of Larp
(stage 05 receipt rules: root-bound receipt, hash/identity validated, never a
vendor binary). The leading dot keeps it outside the alias name grammar. It
dispatches by argv[0] basename to the hook alone, so no other Larp grammar is
reachable. It:

1. accepts argv exactly `claude-statusline <ID>` and requires
   `CLAUDE_CONFIG_DIR` to equal the derived `<root>/profiles/<ID>/cli`
   (otherwise it exits 0 without writing; inherited environment is to be
   confirmed by the future qualification);
2. reads stdin with a 256 KiB cap and 2 s timeout (oversize or timeout: no
   write);
3. deserializes only `version` and `rate_limits.{five_hour,seven_day}.
   {used_percentage,resets_at}`, skipping every other value unretained;
4. writes nothing when `rate_limits` is absent (session start), so a new session
   does not erase the last capture; otherwise validates ranges and atomically
   replaces `profiles/<ID>/usage/claude-statusline.json` (0600 temporary file
   with exclusive no-follow create in the same 0700 directory, fsync, rename;
   stale own temporaries older than 60 s removed by exact pattern). It rewrites
   at most once per 60 s unless values change. Content (≤ 1 KiB):
   `{"version":1,"captured_at":"…Z","client_version":"2.1.325","five_hour":{"used_percent":42.0,"resets_at":1763740800}|null,"weekly":{…}|null}`;
5. prints at most one line built only from validated numbers
   (`5h 42% | wk 13%`), writes **nothing** to stderr, always exits 0, has no
   network client in its code path, never echoes or logs the input.

`usage` reads the capture file (no-follow, regular, owner-only, ≤ 4 KiB, strict
fields) and maps a missing window to `not_reported`, an unverified
`client_version` to `unverified_version`. Claude statusLine records no reset
credits or extras: `reset_credits` `unsupported`, `extra` null.

## LIVE mode (opt-in, unverified against real accounts)

**Warning text (version 1, fixed; CLI help, `live on` output and the GUI sheet):**
"LIVE usage reads this profile's sign-in token from its local credential store
and sends it to the provider's usage endpoint. This may break the provider's
terms of service, and LarpCC can't rule out account restrictions. LIVE is
unverified against real accounts. LOCAL mode, the default, never reads tokens
and makes no network requests."

Acknowledgement is stored as `live.acknowledged_warning = 1` in `usage.json` by
`settings usage live on --acknowledge-risk` (the GUI passes the flag only after
the user confirms the sheet). Changing the text bumps `current_warning`; rows
then report `live_disabled` until re-acknowledged. Per-profile opt-in uses
`settings usage mode live --profile P`.

**Refresh and invariant 5.** LIVE requests happen only inside a foreground
`larp cc usage` invocation: a user action, or the GUI's optional interval
(`refresh_interval_seconds`, 300–86400 s, LIVE only) while a LarpCC usage view
(window or open menu bar panel) is visible. The timer stops when the view
closes. Larp starts no daemon, LaunchAgent, scheduled task, login item or
background refresh, and the CLI has no watch/loop mode. LOCAL refreshes only on
view open or explicit refresh. Usage never selects or switches profiles
(invariant 4).

**One request per profile.** Profiles are processed sequentially. For each:
take the per-ID lifecycle guard non-blocking (busy: `live_unavailable`,
diagnostic `busy`), so delete cannot run concurrently; the registry lock is not
held across IO. Read the credential, send one request, parse, drop. Per request:
5 s connect, 10 s total; whole command budget 60 s (unreached rows
`live_unavailable`, diagnostic `live_budget`). No retries, no token refresh, no
PTY, no child process.

**Credential sources** (field names from public source/binary; never read by agents):

| provider / OS | location | read |
|---|---|---|
| Claude, Linux/Windows | `profiles/<id>/cli/.credentials.json` → `claudeAiOauth.accessToken` (2.1.325 binary: `storagePath = join(configDir, ".credentials.json")`) | `accessToken` only; `refreshToken`, `subscriptionType`, `rateLimitTier` never materialized |
| Claude, macOS | Keychain generic password, service `Claude Code-credentials-<first 8 hex of sha256(NFC config dir)>` when `CLAUDE_CONFIG_DIR` is set, account `$USER` (binary function building `"Claude Code" + suffix + "-" + hash8`; the `-credentials` argument and JSON payload are **unverified**) | same JSON field. The OS asks the user to allow Larp access; "Always Allow" adds Larp to the item ACL (residual risk) |
| Codex, all OS (file store) | `profiles/<id>/cli/auth.json` → `tokens.access_token`, `tokens.account_id` (`codex-rs/login/src/auth/storage.rs:51-76,167-169`, `codex-rs/login/src/token_data.rs:10-33`) | those two only; `id_token`, `refresh_token`, `OPENAI_API_KEY` never materialized |
| Codex, keyring store | service `Codex Auth`, account `cli|<16 hex of sha256(CODEX_HOME)>` (`codex-rs/login/src/auth/storage.rs:302-321`) | **not supported in v1**: Larp does not read config.toml to learn the store mode; a missing auth.json is `sign_in_again` |

Credential files are opened no-follow under the validated profile handle,
regular, owner-only, link count 1, ≤ 64 KiB, into a zeroizing buffer; the token
lives in a zeroizing type without Debug/Display and is dropped right after the
request. It is never stored, logged, cached, printed, put in argv/env, or sent
anywhere but its pinned endpoint.

**Endpoints** (sources cited; shapes unverified against live responses):

| | Claude | Codex |
|---|---|---|
| request | `GET https://api.anthropic.com/api/oauth/usage` (2.1.325: `BASE_API_URL`, usage paths, 5 s timeout) | `GET https://chatgpt.com/backend-api/wham/usage` (`codex-rs/backend-client/src/client/rate_limit_resets.rs:69-82`, `codex-rs/model-provider/src/chatgpt_base_url.rs:6`) |
| headers | `Authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20` (constant present; its use on this call **unverified**), `Accept: application/json`, `User-Agent: larpcc/<version>` | `Authorization: Bearer <token>`, `chatgpt-account-id: <account_id>` (`codex-rs/http-client/src/openai_headers.rs:3`, `codex-rs/backend-client/src/client.rs:388-392`; open decision 3), `Accept`, `User-Agent: larpcc/<version>` |
| response used | `five_hour`, `seven_day` → windows; `seven_day_opus`, `seven_day_sonnet`, `seven_day_oauth_apps` → extras "Opus weekly", "Sonnet weekly", "OAuth apps weekly" (W 604800). Each `{utilization: number 0-100 \| null, resets_at: ISO 8601 \| null}` (binary response parser; units from the SDK schema description) | `rate_limit.primary_window`/`secondary_window` `{used_percent: i32, limit_window_seconds, reset_after_seconds, reset_at}` mapped by window length ±5%; `additional_rate_limits[] {limit_name, metered_feature, rate_limit}` → extras; `rate_limit_reset_credits.available_count` → `reset_credits` (`codex-rs/codex-backend-openapi-models/src/models/*.rs`, `codex-rs/backend-client/src/types.rs:22-75`) |
| ignored | `extra_usage`, `cinder_cove`, `wattle_ember`, `limits[]`, all dollar fields | `plan_type`, `credits`, `spend_control`, `account_id`, `user_id`, `rate_limit_upsell`, `rate_limit_reached_type` |
| reset credits | `unsupported` (only an experimental `cedar_ember=1` variant has them; open decision 2) | count from the summary; expiry `unsupported` (needs a second endpoint; open decision 4) |

**Network rules.** HTTPS only to the two fixed host/path pairs; TLS 1.2+ with
certificate and hostname verification; proxy environment variables
(`HTTP(S)_PROXY`, `ALL_PROXY`, `NO_PROXY`) ignored; redirects never followed (any
3xx is `live_unavailable`); no cookies; response body capped at 64 KiB and
read with the same allowlisted deserializer (unknown fields skipped, missing or
mistyped required fields = unknown shape). 401/403 or a missing/expired
credential is `sign_in_again`; everything else that fails is `live_unavailable`.
Raw status lines, headers, bodies and TLS errors are never output.

**Test seams (fakes only).** `CredentialSource` trait (production: file /
Keychain readers; tests: in-memory fixtures like `fake-token-claude-0001`),
`UsageTransport` trait with an injectable base URL, trust root and clock, used
by tests against a local HTTPS test server with a generated test CA. Production
builds hard-code the pinned hosts; the override exists only in test builds
(`cfg(test)` or a test-only feature never enabled in release). Real-account
verification waits for a later explicit user authorization.

## Verified vs unverified fields

| client / mode | field | status |
|---|---|---|
| Codex 0.169.0 LOCAL | `rate_limits.primary/secondary.{used_percent, window_minutes, resets_at}`, `limit_id`, `limit_name`, line `timestamp`, `session_meta.cli_version`, file layout | verified from source |
| Codex 0.169.0 LOCAL | `plan_type` in the same record | verified present; **not used** (open decision 1) |
| Codex 0.169.0 LOCAL | reset credits | not in the record: `unsupported` |
| Codex LIVE | `/wham/usage` request, headers, response structs, `rate_limit_reset_credits.available_count` | from source; unverified against a live response |
| Codex LIVE | reset-credit expiry | not available in v1 |
| Claude 2.1.325 LOCAL | `rate_limits.five_hour/seven_day.{used_percentage, resets_at}`, `version`, `--settings`, statusLine schema | verified from binary + docs |
| Claude 2.1.325 LOCAL | reset credits, plan, extra windows | not passed to statusLine: `unsupported` |
| Claude 2.1.325 LOCAL | statusLine child inherits `CLAUDE_CONFIG_DIR`; Windows shell | unverified (hook refuses / Windows unsupported) |
| Claude LIVE | `/api/oauth/usage` path, host, response keys `{utilization, resets_at}` | from binary; unverified against a live response |
| Claude LIVE | `anthropic-beta` header use, macOS Keychain service suffix/payload, utilization units | unverified |
| Claude LIVE | reset credits (`cedar_ember`) | not available in v1 |

Any other client version: every field `unverified_version` until a reviewed
amendment adds it to this table.

## Executable synthetic acceptance (implementation obligations)

`cargo test --workspace --locked stage11_` on native Linux/macOS/Windows and
MSRV; zero matching tests fails. Temporary LARPCC_HOME and names
`claude-work`/`codex-45` only; synthetic logs, captures and fake servers only;
every fixture deleted in finally. No vendor binary or real account.

| test | discriminating assertions |
|---|---|
| `stage11_json` | every new example envelope byte-for-byte; ordering, framing, exit codes, empty stderr, `--expected-revision` conflict before reads, tombstone/unknown profile, `--local --live` usage error |
| `stage11_pace` | loads `docs/contracts/usage-pace-vectors.json`; every vector exact; skew/reset/unknown cases |
| `stage11_local_codex` | synthetic rollouts: 5h/weekly by `window_minutes` not position, non-codex bucket → extra, newest per bucket, unverified version, `.zst`/archived skipped, symlink/hardlink/outside-root/oversize/line-cap/file-cap refusal, partial line ignored, conversation sentinel strings never appear in stdout/stderr/files |
| `stage11_local_claude_hook` | hook argv/env checks, 256 KiB cap, absent `rate_limits` keeps last capture, hostile input (deep nesting, huge strings, wrong types, NaN), atomic replace under kill, no stderr, sentinel input fields never written; render file content/quoting/manifest drift refusal; `capture_not_installed` while rows are empty |
| `stage11_live_fake` | fake HTTPS server: success mapping for both providers, 401 → `sign_in_again`, 3xx not followed, proxy env ignored, oversize/slow/unknown shape → `live_unavailable`, exactly one request per profile, no retry/refresh, `live_disabled` before any credential read, budget |
| `stage11_secrets` | fake tokens `fake-token-…` and fake account IDs appear in zero bytes of stdout, stderr, any file under LARPCC_HOME, child argv/env, panic output and error envelopes (including injected parser/TLS/IO failures) |
| `stage11_settings` | ack required on every enable, `live off` resets modes, mode/interval/capture/plan-label receipts, label grammar refusals without echo, `changed:false`, registry revision bumps, pruning |
| `stage11_closed` | default and `--local` make zero network connections (transport fake asserts unused); no background process or scheduler; hook binary has no network path; real hosts unreachable from tests |

## Open decisions (lead/user)

1. **Plan from client records.** Codex writes `plan_type` in the same local
   rate-limit record (and the LIVE response); Claude's credential file has
   `subscriptionType`. Showing either as `plan.source:"client_record"` would be
   account inference beyond D14. Default: not included; user label only.
2. **Claude reset credits.** Only the experimental `?cedar_ember=1` usage
   variant returns them. Default: `unsupported`.
3. **Codex `chatgpt-account-id` header.** The official client sends it; using it
   means reading `tokens.account_id` in memory (never output). Confirm it is
   inside D14, or omit it (requests may then fail as `live_unavailable`).
4. **Codex reset-credit expiry** needs a second endpoint per refresh. Default:
   one request per profile, expiry `unsupported`.
5. **macOS Keychain prompt** for Claude LIVE: the user must allow Larp access to
   the Claude Code item; "Always Allow" persists Larp in its ACL.
6. **Hook replaces the user's statusLine** in Larp-launched Claude sessions with
   a usage line. Alternative: print nothing. Toggle: `claude-statusline off`.
7. **User-Agent.** Honest `larpcc/<version>` (default) vs imitating the official
   client, which this plan refuses.
