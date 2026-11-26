# Stage 11 — usage limits (5-hour, weekly, resets)

## Decision record (D14, amended by D56)

LOCAL is the default. It reads bounded observations from the profile's own
client data. LIVE is opt-in and asks the profile's bound official client
over stdio. Authentication and provider networking stay inside that client.
D56 supersedes the earlier D14 direct-usage-request exception. The CLI has
no credential reader or direct provider HTTP path.

Production Usage qualification rows remain empty. Real-account qualification
requires the user's G1 sign-in and isolation proof for the exact artifact,
operation, platform and backend. This implementation is tested with synthetic
clients. Fresh profiles only; import is out of scope.

**Rollout (RULES "Feature rollout order", user 2026-11-21, D16).** This PR
specifies the complete CLI surface, and the stage-11 CLI ships first, right
after this plan merges. The Mac app step and then the menu-bar step (MENUBAR
lane, branch `stage/11-usage-ui`) each need a separate lead approval, which
the lead asks the user for, **before that step begins**. No app or menu-bar
work on this feature starts before its approval; when it starts, it consumes
only fields the CLI already provides.

**Not in this stage.** Account email and plan from the official client's status
command are stage 14 (D25/D26: shown from a fresh status run, never stored); this
stage does not duplicate them. Here, a plan is the user-entered label, the
plan the client recorded in LOCAL non-credential data (D29), or the plan in the
Codex LIVE usage response (D32). It is never read from credentials and never
stored. No spend, cost, credit balance or token counts anywhere.

## CLI surface

**Qualification gate.** With no Usage row for this build, explicit LIVE
reads and LIVE settings changes fail `live_disabled`. An effective LIVE mode
produces disabled rows. Both `--local` and forwarded reads use LOCAL.

**Windows MVP limitation.** Usage reads and settings changes, including plan
labels, exit 2 with `unsupported` before accessing profile or registry files.
`settings usage` reports LOCAL mode at revision 0 with no profiles or capture
data. Account status reports `usage: null`.

```text
larp cc usage [PROFILE] [--local|--live]                       # command "usage"
larp cc settings usage                                         # "settings.usage"
larp cc settings usage live on --acknowledge-risk              # "settings.usage.live"
larp cc settings usage live off                                # "settings.usage.live"
larp cc settings usage mode local|live [--profile PROFILE]     # "settings.usage.mode"
larp cc settings usage mode inherit --profile PROFILE          # "settings.usage.mode"
larp cc settings usage interval off|SECONDS                    # "settings.usage.interval"
larp cc settings usage claude-statusline on|off --profile PROFILE  # "settings.usage.claude-statusline"
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
  `live_disabled`, exit 1, before a client process unless LIVE is enabled,
  the current warning is acknowledged and a Usage qualification row is available. `--local --live` is `usage`, exit 2.
- `live on` without `--acknowledge-risk` fails `confirmation_required`, exit 2,
  without effects (same rule as a missing `--yes`). It is required on every
  enable. The warning appears in enable help. The CLI does not prompt.
- `live off` atomically sets the installation mode to `local` and every profile
  override `live` to `inherit`, so a stored `live` mode always implies LIVE is
  enabled. `mode live` while LIVE is off fails `live_disabled`, exit 1.
- `mode inherit` requires `--profile`; `mode local|live` without `--profile` sets
  the installation mode. `interval` accepts `off` or an integer 300–86400.
- `claude-statusline` is the per-profile opt-in for LOCAL Claude capture
  (default off; D29). It requires `--profile`; a Codex profile fails
  `unsupported`, exit 1. `on` refuses `statusline_exists`, exit 1, without
  effects when the profile already has its own statusLine (check below); Larp
  never overwrites or wraps a user statusLine. `off` always succeeds.
- `plan-label` takes exactly one of LABEL or `--clear` (else `usage`, exit 2).
  LABEL must match the grammar below or fails `usage`, exit 2, without echo.
- Receipts are the common `changed,profile_id,name,warnings` mutation result.
  `profile.plan-label`, `settings.usage.claude-statusline` and per-profile
  `settings.usage.mode` carry the profile ID and profile name; installation-wide
  `settings.usage.mode|live|interval` carry null/null.
  `live on` adds warning `live_unverified`. Same value again: `changed:false`.
- A hidden fixed hook entry point (Claude capture, below) is not part of the
  public grammar or help, and has no JSON mode.

### `--expected-revision`, exit codes and JSON framing

`usage` and `settings usage` are registry-backed reads; all other commands here
are registry-backed mutations. REV is compared under the registry lock before
name resolution, any capture read or client process; mismatch is
`conflict`, exit 1, without effects. GUI mutations always pass it. Exit codes are
cli-v1: 0 success, 1 local operational error, 2 usage/confirmation. One envelope
plus LF on stdout, stderr empty, no progress output.

`usage --live --json` emits a LarpCC envelope from the official client's
allowlisted usage fields. Protocol output stays on private pipes.
Timeout, signed-out and unknown-response failures are per-profile row reasons.
Registry errors, disabled LIVE and admission failures for an explicitly named
LIVE profile use an error envelope. In a profile list, an absent provider row
is `live_disabled` and other admission failures are `live_unavailable`.

The cli-v1 rule "no timestamps or nondeterministic keys" is amended for usage
rows only: standalone `usage` carries `generated_at` (exclusive to it) plus each
row's `captured_at`, `resets_at` and `runs_out_at`, and `status.account.usage`
carries the same row timestamps (`captured_at`, `resets_at`, `runs_out_at`) but
no `generated_at`. `settings.usage` and all receipts stay timestamp-free.

### Account status (stage 14)

Stage 14 merged first, so this PR performs its hand-off: `status.account`'s
`usage` is widened from null to the selected profile's LOCAL usage row (the
`usage_profile` shape, produced by the same reader as `usage PROFILE --local`,
always `mode:"local"`, never LIVE data or a `live_endpoint` plan), or null.
The row is produced at the status command's read clock (its start time),
which is not on the wire: every `reset_since_capture` decision in the row uses
that clock, never a capture time. Fixtures use the documented fixture read
clock `2026-11-21T12:30:00Z`. The human graph keeps stage 14's rendering rules.
Stage 14's `account.plan` and this row's `plan` stay separate.

### Remote hosts (stage 13)

Consistent with [stage 13](13-remote-hosts.md): remote profiles use LOCAL only,
even when the destination enabled LIVE for its own local callers. The reviewed
forwardable usage reads are exactly `usage [PROFILE]` and `settings usage`.
Under the internal `--remote-request` flag the receiver runs `usage` as if
`--local` were given (`requested_mode:"local"`, every row `mode:"local"`, no
`live_endpoint` source or plan). A forwarded `--live` and every usage mutation
(`settings usage …` changes and `profile plan-label`) are refused `unsupported`,
exit 2, before any client process or registry write. Usage
settings, capture and plan labels are machine-scoped like all registry data.
The envelope shape is unchanged; the app binds it to the route it used.

## Persistence

- `usage.json` at the LARPCC_HOME root: strict `version:1`, at most 64 KiB,
  changed only under the registry lock by a **journaled two-phase commit** that
  reuses the stage 04 pending-journal gate. With the registry at revision N:
  (1) write `usage.json.next` (target revision N+1) and the journal
  `{kind:"usage_settings", base:N, target:N+1, sha256}`, each fsynced with its
  directory; (2) advance the registry to N+1 and fsync; (3) rename
  `usage.json.next` over `usage.json` and fsync the directory; (4) remove the
  journal. While a journal exists, every reader and mutator of the registry
  (all `usage`, `settings usage` and launch paths included) runs recovery under
  the lock **before** comparing `--expected-revision` or reading settings:
  registry = N → discard `.next` and journal (old settings stay valid at N);
  registry = N+1 with a `.next` whose digest matches the journal → finish
  steps 3–4; registry = N+1 with no `.next` and a **final** `usage.json` whose
  digest matches the journal (crash after step 3, before step 4) → durably
  retire the journal (remove it, fsync the directory) and continue; anything
  else → `recovery_required`, exit 1, nothing read. So LIVE consent or mode
  can never be visible at a revision it was not committed with. Fields:
  `mode`, `live.enabled`, `live.acknowledged_warning` (integer or null),
  `refresh_interval_seconds` (null or 300–86400) and
  `profiles: {"<id>": {mode, plan_label, claude_statusline}}` (`claude_statusline`
  default false, Claude profiles only). Unknown fields
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
| `source` | `codex_session_log`, `claude_statusline`, `live_endpoint` (existing wire spelling for official-client LIVE), or null when nothing was read |
| `captured_at` | the newest observation time of any window in the row (local clock), or the record time when no window has values; null when nothing was read. Values are "as of last use" in LOCAL. |
| `client_version` | version of the official client that wrote the LOCAL record (`^N.N.N$`), else null |
| `plan` | null, or `{label, source}`: `user` (the user-entered label, wins), `client_local` (Codex `plan_type` from the LOCAL record, D29) or `live_endpoint` (Codex `plan_type` from the LIVE response, D32) |
| `windows.five_hour`, `windows.weekly` | always present window objects, W = 18000 s and 604800 s |
| `windows.extra` | null (this source has no extra windows) or an array of 0–8 extra windows |
| `reset_credits` | always present tri-state object |
| `diagnostics` | safe `code,message` objects, ordered by code then message |

**Window** `{used_percent, resets_at, captured_at, window_seconds, reason, pace}`.
`used_percent` is 0–100 with at most one decimal; `resets_at` is an RFC 3339 UTC
timestamp with whole seconds and `Z`. `captured_at` is **this window's own**
observation time, non-null exactly when `used_percent` is. Windows in one row
can come from different events (Codex buckets are recorded separately), so
pace and the future-record check use the window's own `captured_at`, never
the row's. Expiry is decided by the **read clock**: each window carries its
own reset instant, and a window whose `resets_at` <= `generated_at` reports
`reset_since_capture`, whatever the row or other windows show. `reason` is null **exactly** when both
values are present; otherwise it says why:

| reason | meaning (app text) |
|---|---|
| `not_captured` | no record captured yet for this profile ("Not available yet") |
| `not_reported` | a record exists but the client did not report this window |
| `unsupported` | this source/provider never carries this field |
| `unverified_version` | the record's client version is not in the verified table below |
| `reset_since_capture` | this window's own `resets_at` <= the read clock `generated_at`. `resets_at` keeps that past instant; `used_percent`, `captured_at` and `pace` are null, so no stale percentage is shown (stage 14 renders "Reset due") |
| `reset_time_unknown` | usage known, reset time not reported (`used_percent` kept) |
| `capture_not_installed` | Claude capture is not wired (no qualified Larp launch, capture off, or unsupported OS) |
| `invalid_record` | record malformed, out of range, oversized, unsafe path or from the future |
| `live_disabled` | effective mode is LIVE but consent or Usage qualification is unavailable |
| `sign_in_again` | the official client reports a sign-in or authentication failure |
| `live_unavailable` | client process, protocol, timeout, output cap or unknown response shape |

Value normalization: finite numbers in [0, 100] are rounded half away from zero
to one decimal; (100, 1000] clamp to 100; anything else is `invalid_record`.
Reset instants are integer seconds in [2020-01-01, 2100-01-01) or
`invalid_record`. A window whose `captured_at` is more than 300 s after
`generated_at` is `invalid_record`. LOCAL values never expire except by
`reset_since_capture`; nothing is inferred from other fields.

**Extra windows** add `label` and allow `window_seconds: null` (no pace then).
LOCAL keeps the fixed Larp vocabulary and drops raw provider bucket names and
identifiers. In LIVE, a Claude model-scoped row can use the client's
`display_name` if it contains 1–24 ASCII letters, digits, spaces, periods or
hyphens and has at least one non-space character. Invalid or absent names use
`Other limit N`. The CLI label is `<name> weekly`. The app display convention
is `Weekly · <name>`; app rendering is a separate change. Codex nonstandard
durations use `Other limit N <minutes>m` and drop other bucket identifiers.
Order: label in
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
Every LOCAL row is `unsupported`/`unsupported`. LIVE maps the count from
the same client response:

| LIVE count | `state` / `count` | `expires_state` / `expires_at` |
|---|---|---|
| integer >= 1 | `available` / N | `unsupported` / null (expiry needs another request) |
| 0 | `none` / 0 | `none` / null (general rule) |
| field absent or null in a usable response, or no usable response | `not_recorded` / null | `not_recorded` / null (general rule) |

**Plan** (D29, D32). Precedence: the user-entered `plan_label`
(`source:"user"`) always wins. Otherwise, in a LOCAL row, a plan the official
client recorded in the LOCAL **non-credential** data this stage already reads
(`source:"client_local"`). Otherwise, in a LIVE row, the plan in that profile's
LIVE usage response (`source:"live_endpoint"`). Otherwise null.
Provider plans are held **in memory for the current invocation only**: never
written to `usage.json`, the capture file, a cache or a log, the same
never-store rule as D26 status output. Only the user label
persists. Today only Codex provides one: `rate_limits.plan_type` of the
selected `codex`-bucket `token_count` event. It is used only when it is in the
**LOCAL allowlist**, the 0.169.0 rollout `PlanType` values
(`codex-rs/protocol/src/account.rs:12-48`): `free`, `go`, `plus`, `pro`,
`prolite`, `promax`, `team`, `self_serve_business_prolite`,
`self_serve_business_usage_based`, `business`, `ent26`,
`enterprise_cbp_automation`, `enterprise_cbp_usage_based`, `enterprise`, `edu`,
`edu_plus`, `edu_pro`, `sci`, `sci_plus`. It is shown as recorded.
Claude's statusLine input carries no plan, so Claude shows the user label only.
In LIVE, Codex `planType` comes from the selected app-server snapshot.
The LIVE allowlist is
the LOCAL list plus `guest`, `free_workspace`, `education`, `quorum` and `k12`.
In both enums `unknown` and anything else give a null plan. The Claude LIVE response has no plan field, so Claude
LIVE rows show the user label only. Grammar for every source: ASCII, 1–32
characters, starts with a letter or digit, letters/digits and `. _ + / ( ) -`,
single inner spaces (`plan_label`). No `@`, quotes, control or non-ASCII
characters, so it cannot carry an email. Plans are display-only and never
verified.

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
directories (exact digit grammar) newest first and files matching either form
written by `codex-rs/rollout/src/rollout_file_name.rs:62-74` at
`rust-v0.169.0`: `rollout-<YYYY-MM-DDTHH-MM-SS>-<thread>.jsonl` or, after a
revert, `rollout-<YYYY-MM-DDTHH-MM-SS>-<thread>_<rollout>.jsonl`, where
`<thread>` and `<rollout>` are canonical hyphenated UUIDs (`ThreadId`/`RolloutId`,
`codex-rs/protocol/src/thread_id.rs:16-43`). Both forms are grammar-checked;
`.zst`,
`archived_sessions/` and anything else skipped). No-follow opens; refuse
symlinks, reparse points, hardlinks (link count > 1), non-regular files and any
object whose identity leaves the profile root. Newest files by name first.

- Caps: 62 day-directories, 2,000 directory entries, 8 files, per file the
  first line (≤ 1 MiB) plus the last 4 MiB, 20,000 lines per file, 40 MiB and
  2 s total. Hitting a cap stops scanning with diagnostic `scan_limit`.
- Per line: a streaming JSON deserializer whose target struct names only
  `timestamp`, `type`, `payload.type` and, for `event_msg`/`token_count`, the
  allowlisted `payload.rate_limits` subfields `limit_id`,
  `primary|secondary.{used_percent, window_minutes, resets_at}` and `plan_type`
  (D29). Every other value is skipped unretained (e.g. serde `IgnoredAny`).
  `info`, `credits`, `individual_limit` and conversation items are never
  materialized.
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
  labelled `Other limit N <duration>` (fixed vocabulary above; `limit_name` is
  never read into output), with `window_seconds = window_minutes·60` (null when
  absent). Each window's `captured_at` is its own event line's `timestamp`,
  truncated to seconds; the row `captured_at` is the newest of them. `client_version` is the file's
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
| precedence | above user/project/local, so Larp must check for an existing statusLine first (below) | a project or local statusLine overrides it |
| coverage | only Larp-launched sessions (qualified run rows, aliases) | also manual `CLAUDE_CONFIG_DIR=… claude` sessions |
| tamper | file hash-checked by Larp before every launch | any native rewrite silently changes it |

Recommendation: `--settings` with the owned render. The launch adapter appends
the reviewed flag `--settings <abs path to claude-settings.json>` only through a
qualified native row (stage 08 injection rules). **Native rows are still empty**,
so until a Claude row is qualified every Claude profile reports
`capture_not_installed`; the GUI shows "Not available". Manual sessions outside
Larp are never captured (no native config writes).

**Opt-in, never over a user statusLine (D29).** Capture is off by default and
enabled per profile with `settings usage claude-statusline on --profile P`.
Because `--settings` outranks user/project/local settings, Larp first checks
that no user statusLine exists. The check is a bounded, confined, no-follow,
**key-presence-only** read (≤ 1 MiB, values skipped unretained, nothing
output) of top-level `statusLine` in Claude settings files. It is the only
native-config read in this stage:

- at `on`: the profile's `profiles/<id>/cli/settings.json`; present →
  `statusline_exists`, nothing changed;
- before each injecting launch: that file again, plus `.claude/settings.json`
  and `.claude/settings.local.json` in the launch directory and each parent up
  to the filesystem root. Present → the launch proceeds **without**
  `--settings` (the user's statusLine wins; nothing captured). Managed settings
  are not read; a managed statusLine already wins over flags.

Absence counts as established only when every checked file is either missing
(no-follow open reports not-found) or is a regular, owner-readable file ≤ 1 MiB
that parses as one strict JSON object (no comments, trailing commas,
duplicate top-level keys or trailing bytes) without a top-level `statusLine`.
Every other outcome is **indeterminate**: malformed or unsupported syntax,
non-object JSON, oversize, symlink/reparse point/non-regular file, permission
or IO error, an undeterminable launch directory, or more than 64 parent levels
before the root. Indeterminate **refuses** at enable time
(`statusline_unverifiable`, exit 1, nothing changed) and launches **without**
injection at launch time. The future Claude row qualification must establish
exactly where 2.1.325+ discovers user, project and local settings (launch
directory versus repository root, any other sources) before injection is
enabled. Until then the walk above is the conservative superset, and
injection stays off anyway while rows are empty.

The render is exactly
`{"statusLine":{"type":"command","command":"'<root>/bin/.larp-usage-hook' claude-statusline <ID>","padding":0}}`
and is only passed when the profile opted in and the check passed. The hook
never wraps or chains another command. The path is single-quoted; a root path containing `'`, a
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
5. prints **nothing** to stdout or stderr (D29: it only captures), always exits
   0, has no network client in its code path, never echoes or logs the input.

`usage` reads the capture file (no-follow, regular, owner-only, ≤ 4 KiB, strict
fields) and maps a missing window to `not_reported`, an unverified
`client_version` to `unverified_version`. Claude statusLine records no reset
credits or extras: `reset_credits` `unsupported`, `extra` null.

## LIVE mode (opt-in, unverified against real accounts)

**Warning (version 2).** “LIVE usage asks the profile's qualified official
client for limits using its stored sign-in. Provider restrictions are possible.
LIVE remains unverified against real accounts.”

`settings usage live on --acknowledge-risk` records warning version 2.
An older acknowledgement leaves stored mode choices intact and reports
`needs_acknowledgement`. Use `settings usage mode live --profile P` to set
a per-profile default. `--local` and remote reads force LOCAL.

LIVE runs only in a foreground usage command. It uses Stage 03 admission for a
distinct Usage operation, including the bound artifact digest, exact argv, home
isolation, per-ID auth guard and revalidation. Launch uses a managed working
directory and the profile's selected CLI home. The child inherits only
`HOME`, `PATH`, `USER`, `LOGNAME`, `TMPDIR` and `LANG` plus its selected
provider home variable. The qualification must cover native config, login
store behavior and any dependencies these settings permit.

- **Codex.** Start `app-server` with `CODEX_HOME` set. Initialize newline-delimited
  JSON-RPC, then call `account/rateLimits/read` once. Prefer
  `rateLimitsByLimitId.codex` and otherwise use `rateLimits` if it represents
  the Codex bucket. Map `windowDurationMins` of 300 and 10080 to `five_hour`
  and `weekly`; other valid durations become extras. Ignore other IDs.
  Read `rateLimitResetCredits.availableCount` from that reply.
- **Claude.** Use `CLAUDE_CONFIG_DIR` and `--print --input-format stream-json
  --output-format stream-json --verbose`. Initialize the control stream and
  send one `get_usage` request with `skip_behaviors`. No user prompt or model
  request is sent. The control interface is unstable and must be pinned by the
  qualification row. Use `rate_limits.five_hour` and `rate_limits.seven_day`
  utilization (0–100) and the numeric entries in `rate_limits.model_scoped`
  for weekly extras. Read `rate_limit_reset_credits.available_count` if
  supplied by the same response; otherwise reset credits are `not_recorded`.

Clients run sequentially with a 10-second probe budget and a 60-second command
probe budget, and at most 1 MiB of stdout per client. The subprocess session
is killed and the direct child reaped on completion or failure. Stderr is
discarded. The CLI emits a fixed result reason for a signed-out client
(`sign_in_again`) or an unusable response (`live_unavailable`). No protocol
frames are emitted, persisted or logged. There is one usage request per
profile and no retry, background task, or separate request for credits or expiry.

## Verified vs unverified fields

| client / mode | field | status |
|---|---|---|
| Codex 0.169.0 LOCAL | `rate_limits.primary/secondary.{used_percent, window_minutes, resets_at}`, `limit_id`, `limit_name`, line `timestamp`, `session_meta.cli_version`, file layout | verified from source |
| Codex 0.169.0 LOCAL | `plan_type` in the same record (`PlanType` values, `account.rs:9-49`) | verified; shown as `client_local` plan unless a user label is set (D29) |
| Codex 0.169.0 LOCAL | reset credits | not in the record: `unsupported` |
| Claude 2.1.325 LOCAL | `rate_limits.five_hour/seven_day.{used_percentage, resets_at}`, `version`, `--settings`, statusLine schema | verified from binary + docs |
| Claude 2.1.325 LOCAL | reset credits, plan, extra windows | not passed to statusLine: `unsupported` (plan = user label only) |
| Claude 2.1.325 LOCAL | project settings lookup path (launch dir vs repository root) | unverified, so the existing-statusLine check walks the launch directory and all parents |
| Claude 2.1.325 LOCAL | statusLine child inherits `CLAUDE_CONFIG_DIR`; Windows shell | unverified (hook refuses / Windows unsupported) |

Any other client version: every field `unverified_version` until a reviewed
amendment adds it to this table.

## Executable synthetic acceptance (implementation obligations)

`cargo test --workspace --locked stage11_` on native Linux/macOS/Windows and
MSRV; zero matching tests fails. Temporary LARPCC_HOME and names
`claude-work`/`codex-45` only; synthetic logs, captures and fake servers only;
every fixture deleted in finally. No vendor binary or real account.

| test | discriminating assertions |
|---|---|
| `stage11_json` | every new example envelope byte-for-byte; `status.account.usage` judged at the status read clock (a window reset between capture and read is `reset_since_capture`, never a stale percentage); a failed Codex LIVE row is `not_recorded`; ordering, framing, exit codes, empty stderr, `--expected-revision` conflict before reads, tombstone/unknown profile, `--local --live` usage error |
| `stage11_pace` | loads `docs/contracts/usage-pace-vectors.json`; every vector exact; skew/reset/unknown cases |
| `stage11_local_codex` | synthetic rollouts in both filename forms (`<thread>` and `<thread>_<rollout>`), malformed names skipped: 5h/weekly by `window_minutes` not position, buckets observed at different times keep their own window `captured_at` and pace (vector `extra_observed_two_hours_before_row`), no false `clock_skew`, a row mixing an expired window (own reset before the read clock → `reset_since_capture`, no percentage) with current windows, a grammar-valid private sentinel `limit_name`/`limit_id` never appears in stdout/stderr/files or forwarded output (label `Other limit N`), LOCAL plan allowlist, `plan_type` → `client_local` plan unless a user label overrides, unknown plan value → null, non-codex bucket → extra, newest per bucket, unverified version, `.zst`/archived skipped, symlink/hardlink/outside-root/oversize/line-cap/file-cap refusal, partial line ignored, conversation sentinel strings never appear in stdout/stderr/files |
| `stage11_statusline_check` | negative fixtures each yield refuse-at-enable and no-injection-at-launch: malformed JSON, JSON with comments or trailing comma, duplicate top-level keys, non-object top level, trailing bytes, oversize, symlink, FIFO/non-regular, unreadable file, unreadable parent, undeterminable launch dir, 65 parent levels; a `statusLine` only in a parent's `.claude/settings.local.json` blocks injection; missing files everywhere establishes absence |
| `stage11_local_claude_hook` | hook argv/env checks, 256 KiB cap, absent `rate_limits` keeps last capture, hostile input (deep nesting, huge strings, wrong types, NaN), atomic replace under kill, empty stdout and stderr, sentinel input fields never written; render file content/quoting/manifest drift refusal; opt-in default off; `statusline_exists` on enable; existing profile/project/local statusLine (including in a parent directory) → no `--settings` injection; settings values never read or output; `capture_not_installed` while rows are empty |
| `stage11_live_official_client` | synthetic Codex app-server and Claude control stream, no prompt, consent and qualification refusal before spawn, bucket selection and duration mapping, model labels, reset-credit counts, signed-out and unknown responses, output cap, timeout, child reap and environment scrubbing |
| `stage11_settings` | journaled commit: kill between each write and step (`.next`, journal, registry bump, rename), including exactly after the rename and before journal removal (recovery retires the journal and reads the new settings at N+1, never `recovery_required`), then every reader sees settings exactly at the committed revision and a stale `--expected-revision` fails `conflict`; `needs_acknowledgement` after a warning bump: `settings usage` succeeds and reports it, `usage` rows `live_disabled` and `usage --live` fail with zero official-client processes; ack required on every enable, `live off` resets modes, mode/interval/capture/plan-label receipts, claude-statusline `--profile` required and Codex `unsupported`, label grammar refusals without echo, `changed:false`, registry revision bumps, pruning |
| `stage11_closed` | default and `--local` make zero network connections (transport fake asserts unused); no background process or scheduler; hook binary has no network path; real hosts unreachable from tests; with `--remote-request`, `usage` is forced LOCAL even when LIVE is enabled, and `--live` and every usage mutation are `unsupported` before credential reads, requests or writes |

## Decided (D29, lead 2026-11-21; the user may veto)

1. **Client-recorded plan: shown.** From LOCAL non-credential data (Codex
   `plan_type` in rollout events; D32 adds the Codex LIVE response); the user's
   `plan_label` overrides it. Never from tokens or credential files. Claude has
   no plan source.
2. **statusLine hook: opt-in per profile.** It preserves an existing user
   statusLine and prints nothing.

## Decided (D32, lead 2026-11-23)

1. **Existing-statusLine detection** is the key-presence-only check above
   (profile `settings.json` at enable; launch directory and parents' `.claude`
   settings at launch). Values are never read. Confirmed.
2. **LIVE Codex `plan_type` may be shown** (source `live_endpoint`), in memory
   only, never stored, cached or logged, LIVE enabled only; the user label
   overrides it. This replaces the earlier "discarded" reading of D29.

No open decisions remain in this plan.
