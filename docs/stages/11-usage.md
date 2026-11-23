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

### Account status (stage 14)

Stage 14 merged first, so this PR performs its hand-off: `status.account`'s
`usage` is widened from null to the selected profile's LOCAL usage row (the
`usage_profile` shape, produced by the same reader as `usage PROFILE --local`,
always `mode:"local"`, never LIVE data or a `live_endpoint` plan), or null.
The human graph keeps stage 14's rendering rules. Stage 14's `account.plan`
and this row's `plan` stay separate.

### Remote hosts (stage 13)

Consistent with [stage 13](13-remote-hosts.md): remote profiles use LOCAL only,
even when the destination enabled LIVE for its own local callers. The reviewed
forwardable usage reads are exactly `usage [PROFILE]` and `settings usage`.
Under the internal `--remote-request` flag the receiver runs `usage` as if
`--local` were given (`requested_mode:"local"`, every row `mode:"local"`, no
`live_endpoint` source or plan). A forwarded `--live` and every usage mutation
(`settings usage …` changes and `profile plan-label`) are refused `unsupported`,
exit 2, before any credential read, provider request or registry write. Usage
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
| `source` | `codex_session_log`, `claude_statusline`, `live_endpoint`, or null when nothing was read |
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
| `live_disabled` | effective mode is LIVE but the current warning is not acknowledged |
| `sign_in_again` | LIVE credential missing, expired or rejected: "Sign in again in the official client" |
| `live_unavailable` | LIVE network/TLS/timeout/redirect/HTTP error or unknown response shape |

Value normalization: finite numbers in [0, 100] are rounded half away from zero
to one decimal; (100, 1000] clamp to 100; anything else is `invalid_record`.
Reset instants are integer seconds in [2020-01-01, 2100-01-01) or
`invalid_record`. A window whose `captured_at` is more than 300 s after
`generated_at` is `invalid_record`. LOCAL values never expire except by
`reset_since_capture`; nothing is inferred from other fields.

**Extra windows** add `label` and allow `window_seconds: null` (no pace then).
Labels come only from a **fixed Larp vocabulary** (`usage_extra_label` in the
schema): a bucket word, `Codex`, `Opus`, `Sonnet`, `OAuth apps` or
`Other limit N`, optionally followed by a duration word, `5h`, `daily`,
`weekly`, `monthly`, `annual`, `<minutes>m` or `secondary`. Provider bucket
names and identifiers (`limit_name`, `limit_id`, `metered_feature`) are
**never** output, not even in forwarded LOCAL output: they may name an
organization or a private model. Reviewed buckets map to fixed words (Codex
`codex` → `Codex`; Claude LIVE `seven_day_opus|sonnet|oauth_apps` → `Opus`,
`Sonnet`, `OAuth apps` with `weekly`). Every other bucket is `Other limit N`,
with N = 1, 2, … assigned by ascending byte order of the raw identifier within
this one invocation. The raw identifier is used only for that ordering and is
then dropped. Order: label in
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
Per source (validator-checked): every LOCAL row and every Claude row is
`unsupported`/`unsupported` (no LOCAL source records reset credits). A Codex
LIVE row maps its count as follows (no second request, D29):

| Codex LIVE `available_count` | `state` / `count` | `expires_state` / `expires_at` |
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
never-store rule as D14 tokens and D26 status output. Only the user label
persists. Today only Codex provides one: `rate_limits.plan_type` of the
selected `codex`-bucket `token_count` event. It is used only when it is in the
**LOCAL allowlist**, the 0.169.0 rollout `PlanType` values
(`codex-rs/protocol/src/account.rs:12-48`): `free`, `go`, `plus`, `pro`,
`prolite`, `promax`, `team`, `self_serve_business_prolite`,
`self_serve_business_usage_based`, `business`, `ent26`,
`enterprise_cbp_automation`, `enterprise_cbp_usage_based`, `enterprise`, `edu`,
`edu_plus`, `edu_pro`, `sci`, `sci_plus`. It is shown as recorded.
Claude's statusLine input carries no plan, so Claude shows the user label only.
Plans are **never** read from tokens or credential files (Claude
`subscriptionType`/`rateLimitTier`, Codex `id_token` claims). In LIVE, only the
Codex response's top-level `plan_type` is used (D32, within D14 because it is
part of the one usage response, LIVE enabled only), checked against the
separate **LIVE allowlist**, the backend `PlanType` values
(`codex-rs/codex-backend-openapi-models/src/models/rate_limit_status_payload.rs:97-148`):
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

**Warning text (version 1, fixed; CLI help, `live on` output and the GUI sheet):**
"LIVE usage reads this profile's sign-in token from its local credential store
and sends it to the provider's usage endpoint. This may break the provider's
terms of service, and LarpCC can't rule out account restrictions. LIVE is
unverified against real accounts. LOCAL mode, the default, never reads tokens
and makes no network requests."
On macOS the sheet adds: "For Claude profiles, macOS will ask whether larp may
read the Claude Code sign-in item in your Keychain. Allow lets it read once.
Always Allow lets larp read that item from now on without asking. Deny keeps
LIVE from working for that profile. LarpCC never chooses for you."

Acknowledgement is stored as `live.acknowledged_warning = 1` in `usage.json` by
`settings usage live on --acknowledge-risk` (the GUI passes the flag only after
the user confirms the sheet). Changing the text bumps `current_warning` in a
new Larp release. Stored LIVE choices (enabled, modes) are kept. `settings
usage` then reports `live.state:"needs_acknowledgement"` (otherwise `on`, or
`off` when disabled) and succeeds. In that state every effective-LIVE row
reports `live_disabled` and `usage --live` fails `live_disabled`, both with
zero credential reads and zero requests, until `live on --acknowledge-risk`
records the current version. Per-profile opt-in uses
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
| Claude, macOS | Keychain generic password, service `Claude Code-credentials-<first 8 hex of sha256(NFC config dir)>` when `CLAUDE_CONFIG_DIR` is set, account `$USER` (binary function building `"Claude Code" + suffix + "-" + hash8`; the `-credentials` argument and JSON payload are **unverified**) | same JSON field, with a plain read query. macOS shows its own access prompt (D29: explained at enable time). Larp never requests, sets or edits the item's ACL or "Always Allow"; Deny → `live_unavailable`, diagnostic `keychain_denied`. Each refresh may prompt again unless the user chose Always Allow |
| Codex, all OS (file store) | `profiles/<id>/cli/auth.json` → `tokens.access_token`, `tokens.account_id` (`codex-rs/login/src/auth/storage.rs:51-76,167-169`, `codex-rs/login/src/token_data.rs:10-33`) | those two only, LIVE only. `account_id` is inside D14 (D29) under the token's rules: in memory, per request, sent only as the `chatgpt-account-id` header to the pinned host, never stored or output. `id_token`, `refresh_token` and `OPENAI_API_KEY` are never materialized |
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
| headers | `Authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20` (constant present; its use on this call **unverified**), `Accept: application/json`, `User-Agent: larpcc/<version>` | `Authorization: Bearer <token>`, `chatgpt-account-id: <account_id>` (`codex-rs/http-client/src/openai_headers.rs:3`, `codex-rs/backend-client/src/client.rs:388-392`; inside D14 per D29), `Accept`, `User-Agent: larpcc/<version>` |
| response used | `five_hour`, `seven_day` → windows; `seven_day_opus`, `seven_day_sonnet`, `seven_day_oauth_apps` → extras "Opus weekly", "Sonnet weekly", "OAuth apps weekly" (W 604800). Each `{utilization: number 0-100 \| null, resets_at: ISO 8601 \| null}` (binary response parser; units from the SDK schema description) | `rate_limit.primary_window`/`secondary_window` `{used_percent: i32, limit_window_seconds, reset_after_seconds, reset_at}` mapped by window length ±5%; `additional_rate_limits[] {limit_name, metered_feature, rate_limit}` → extras labelled `Other limit N` (names never output); `rate_limit_reset_credits.available_count` → `reset_credits`; `plan_type` → `plan` `live_endpoint`, held in memory only (D32) (`codex-rs/codex-backend-openapi-models/src/models/*.rs`, `codex-rs/backend-client/src/types.rs:22-75`) |
| ignored | `extra_usage`, `cinder_cove`, `wattle_ember`, `limits[]`, all dollar fields | `credits`, `spend_control`, `account_id`, `user_id`, `rate_limit_upsell`, `rate_limit_reached_type` |
| reset credits | `unsupported` (only an experimental `cedar_ember=1` variant has them; D29 keeps it unsupported) | count from the summary; expiry `unsupported` (needs a second request, which D29 rules out) |

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
| Codex 0.169.0 LOCAL | `plan_type` in the same record (`PlanType` values, `account.rs:9-49`) | verified; shown as `client_local` plan unless a user label is set (D29) |
| Codex 0.169.0 LOCAL | reset credits | not in the record: `unsupported` |
| Codex LIVE | `/wham/usage` request, headers, response structs, `rate_limit_reset_credits.available_count` | from source; unverified against a live response |
| Codex LIVE | `plan_type` (`RateLimitStatusPayload`, `rate_limit_status_payload.rs:16-53`) | from source; unverified against a live response; shown as `live_endpoint` plan unless a user label is set, in memory only (D32) |
| Codex LIVE | reset-credit expiry | not available in v1 |
| Claude 2.1.325 LOCAL | `rate_limits.five_hour/seven_day.{used_percentage, resets_at}`, `version`, `--settings`, statusLine schema | verified from binary + docs |
| Claude 2.1.325 LOCAL | reset credits, plan, extra windows | not passed to statusLine: `unsupported` (plan = user label only) |
| Claude 2.1.325 LOCAL | project settings lookup path (launch dir vs repository root) | unverified, so the existing-statusLine check walks the launch directory and all parents |
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
| `stage11_local_codex` | synthetic rollouts in both filename forms (`<thread>` and `<thread>_<rollout>`), malformed names skipped: 5h/weekly by `window_minutes` not position, buckets observed at different times keep their own window `captured_at` and pace (vector `extra_observed_two_hours_before_row`), no false `clock_skew`, a row mixing an expired window (own reset before the read clock → `reset_since_capture`, no percentage) with current windows, a grammar-valid private sentinel `limit_name`/`limit_id` never appears in stdout/stderr/files or forwarded output (label `Other limit N`), LOCAL plan allowlist, `plan_type` → `client_local` plan unless a user label overrides, unknown plan value → null, non-codex bucket → extra, newest per bucket, unverified version, `.zst`/archived skipped, symlink/hardlink/outside-root/oversize/line-cap/file-cap refusal, partial line ignored, conversation sentinel strings never appear in stdout/stderr/files |
| `stage11_statusline_check` | negative fixtures each yield refuse-at-enable and no-injection-at-launch: malformed JSON, JSON with comments or trailing comma, duplicate top-level keys, non-object top level, trailing bytes, oversize, symlink, FIFO/non-regular, unreadable file, unreadable parent, undeterminable launch dir, 65 parent levels; a `statusLine` only in a parent's `.claude/settings.local.json` blocks injection; missing files everywhere establishes absence |
| `stage11_local_claude_hook` | hook argv/env checks, 256 KiB cap, absent `rate_limits` keeps last capture, hostile input (deep nesting, huge strings, wrong types, NaN), atomic replace under kill, empty stdout and stderr, sentinel input fields never written; render file content/quoting/manifest drift refusal; opt-in default off; `statusline_exists` on enable; existing profile/project/local statusLine (including in a parent directory) → no `--settings` injection; settings values never read or output; `capture_not_installed` while rows are empty |
| `stage11_live_fake` | fake HTTPS server: success mapping for both providers, LIVE plan allowlist (e.g. `free_workspace` shown in LIVE only, refused as `client_local`), `available_count` ≥ 1 / 0 / absent → `available`+`unsupported` / `none`+`none` / `not_recorded`+`not_recorded`, `additional_rate_limits` with a sentinel `limit_name`/`metered_feature` → `Other limit N` with zero sentinel bytes in output, Codex `plan_type` → `live_endpoint` plan, user label override, unknown plan value → null, plan bytes absent from every file under LARPCC_HOME after the run, 401 → `sign_in_again`, 3xx not followed, proxy env ignored, oversize/slow/unknown shape → `live_unavailable`, exactly one request per profile, no retry/refresh, `live_disabled` before any credential read, budget |
| `stage11_secrets` | fake tokens `fake-token-…` and fake account IDs appear in zero bytes of stdout, stderr, any file under LARPCC_HOME, child argv/env, panic output and error envelopes (including injected parser/TLS/IO failures) |
| `stage11_settings` | journaled commit: kill between each write and step (`.next`, journal, registry bump, rename), including exactly after the rename and before journal removal (recovery retires the journal and reads the new settings at N+1, never `recovery_required`), then every reader sees settings exactly at the committed revision and a stale `--expected-revision` fails `conflict`; `needs_acknowledgement` after a warning bump: `settings usage` succeeds and reports it, `usage` rows `live_disabled` and `usage --live` fail with zero credential reads and zero requests; ack required on every enable, `live off` resets modes, mode/interval/capture/plan-label receipts, claude-statusline `--profile` required and Codex `unsupported`, label grammar refusals without echo, `changed:false`, registry revision bumps, pruning |
| `stage11_closed` | default and `--local` make zero network connections (transport fake asserts unused); no background process or scheduler; hook binary has no network path; real hosts unreachable from tests; with `--remote-request`, `usage` is forced LOCAL even when LIVE is enabled, and `--live` and every usage mutation are `unsupported` before credential reads, requests or writes |

## Decided (D29, lead 2026-11-21; the user may veto)

1. **Client-recorded plan: shown.** From LOCAL non-credential data (Codex
   `plan_type` in rollout events; D32 adds the Codex LIVE response); the user's
   `plan_label` overrides it. Never from tokens or credential files. Claude has
   no plan source.
2. **Claude reset credits** via the experimental `cedar_ember` variant:
   `unsupported`.
3. **Codex `chatgpt-account-id` header: inside D14.** Read only when LIVE is
   enabled, in memory, per request, never stored or output.
4. **No second Codex LIVE request** for reset-credit expiry: expiry
   `unsupported`.
5. **macOS Keychain:** the prompt is explained at enable time; Larp never
   requests or sets "Always Allow" or edits the item's ACL.
6. **statusLine hook: opt-in per profile.** It never overwrites or wraps an
   existing user statusLine (`statusline_exists` on enable, no injection at
   launch), and it prints nothing.
7. **Honest User-Agent** `larpcc/<version>`; imitating an official client is
   refused.

## Decided (D32, lead 2026-11-23)

1. **Existing-statusLine detection** is the key-presence-only check above
   (profile `settings.json` at enable; launch directory and parents' `.claude`
   settings at launch). Values are never read. Confirmed.
2. **LIVE Codex `plan_type` may be shown** (source `live_endpoint`), in memory
   only, never stored, cached or logged, LIVE enabled only; the user label
   overrides it. This replaces the earlier "discarded" reading of D29.

No open decisions remain in this plan.
