# Stage 14 — Account summary and profile extras

Planning contract for user decisions D24–D27. This PR changes docs and one
Swift test only. It enables no native row and ships no product behaviour.

Rollout follows the standing order: (1) the `larp` CLI and its `--json`
contract, (2) the Mac app, (3) the menu bar. After the CLI ships, the lead
asks the user before the app step, and asks again before the menu-bar step.
The app and menu-bar sections below are later steps and are not approved by
merging this plan. Native account operations still need the separately
reviewed qualification in [Phase 2](#phase-2--mac-qualification-plan).

## Decisions applied

| Source | Rule in this plan |
|---|---|
| D25 + lead ruling | `larp cc status NAME` shows the account (email + plan) **by default**, in human and `--json` output. Email is shown in full in the CLI. `--no-account` gives metadata-only output and never runs the official client. |
| D26 | Email and plan are **never stored**: not in the registry, caches, logs, temp files, crash output or app settings. They exist only in the memory of the status run that produced them. |
| D25 | The app blurs the email until clicked and offers **Hide account emails**. The menu bar never shows emails. These are later steps. |
| D24 | Real-account qualification happens on the user's Mac with new throwaway profiles. The user completes every browser sign-in. Agents never read tokens. |
| D27 | Build provider version display, per-profile display name, per-profile default launch arguments and the usage text graph. No "shadow home". |
| Stage 3 | D25 amends the stage 3 "no account-derived label/capture" rule for the email and plan fields of `status` only. Every other stage 3 admission, guard and evidence rule stays. |

## CLI account summary

### Commands

```text
larp cc status NAME [--json] [--expected-revision REV]
larp cc status NAME --no-account [--json] [--expected-revision REV]
```

`status NAME` (no flag) is the **account status** mode. It runs the qualified
official status operation for that profile, captures the account fields and
prints them with the profile's metadata and local usage.

`--no-account` is the **metadata status** mode. It reads only the registry and
local filesystem metadata, never spawns a provider binary and returns exactly
today's `status` output (JSON shape byte-compatible with the frozen `status`
variant). It is the mode for scripts, the app's background-free metadata
views and any caller that must not cause provider activity.

Why `--no-account` and not `--local`: `--local` already reads as "LOCAL usage
mode" (stage 11) and as "this machine, not `--host`" (stage 13). A flag named
after what it suppresses cannot be confused with either. It is accepted only
by `status`. Elsewhere it is a `usage` error (exit 2). `doctor` stays
metadata-only and does not accept it.

`<alias> status` is **not** an account summary. Alias invocations keep stage 5
argv forwarding to the selected CLI run and its own admission rules. Only the
`larp cc status NAME` entry point requests the summary. That keeps one
capture-capable code path and avoids guessing whether a forwarded `status`
word was meant for Larp or the provider.

Under a stage 13 `--host NAME` route, account status is allowed. The remote
Larp runs its own qualified status and returns `status.account` over the
user's SSH. See [Remote account status](#remote-account-status).

### Human output

```text
codex-q1 (codex 0.98.0)
Signed in as user@example.com · Plus
Session [############--------] 64% left
        Resets in 35m
Weekly  [################----] 80% left
        Resets in 2h 12m
```

Line 1 is the display name (or command name), then the provider and its
exact version in parentheses (`version unknown` when null). Line 2 is one of:

| Account state | Line |
|---|---|
| `signed_in`, email and plan | `Signed in as <email> · <plan>` |
| `signed_in`, one field absent | `Signed in as <email>` or `Signed in · <plan>` |
| `signed_in`, neither field | `Signed in` |
| `signed_out` | `Signed out` |
| `unknown` | `account: unknown` |
| `not_qualified` | `account: not qualified for this provider build` |
| `unavailable` | `account: unavailable` |

The email prints in full. Then comes the usage text graph (below). The stage 3
routing and auth notices go to stderr before the child, as today. Human
`--no-account` prints the metadata status (profile, binding, diagnostics)
with no account line and no usage graph. `larp cc usage` (stage 11) remains
the way to see usage without running a client.

Both human modes exit 0 for every completed observation, including
`signed_out`, `unknown`, `not_qualified` and `unavailable`. Native exit codes
are provider-specific and are interpreted only by the parser. Scripts that
need the state use `--json` (lead ruling, 2026-11-21). Pre-spawn, admission, conflict,
busy and cleanup failures are fixed local errors (exit 1, or 2 for usage).
A real process cancellation (INT/TERM/HUP) is propagated after the child is
reaped, with no account line printed.

### JSON and compatibility

The account mode is a **new result variant** of schema 1 with its own
discriminator, `command:"status.account"`. Metadata mode keeps
`command:"status"` and the frozen data shape, unchanged. The full
amendment is in [cli-v1.md §Account summary](../contracts/cli-v1.md#account-summary).

```json
{"schema":1,"command":"status.account","ok":true,"revision":"7",
 "data":{"profile":{...},"provider_version":"0.98.0",
         "account":{"state":"signed_in","email":"user@example.com","plan":"Plus"},
         "usage":null,"diagnostics":[]}}
```

Why a variant and not a schema 2 bump: schema 2 would force every consumer
to re-version every command, although only one command changes. A distinct
dotted discriminator keeps all other envelopes unchanged, lets strict
consumers fail closed on exactly this command, and lets the old metadata
shape stay reachable under its old name. Why not add fields to `status`:
the app's strict decoder rejects unknown fields, so a widened `status` would
break every existing caller with no way to ask for the old shape.

Compatibility matrix:

| Caller | CLI | Result |
|---|---|---|
| Script/app using `status NAME --json` | old | `status` (metadata). Unchanged. |
| Script/app using `status NAME --json` | new | `status.account`. An old strict decoder rejects the unknown variant (fails closed). The official client ran if a row admitted. Callers that must not cause provider activity switch to `--no-account`. |
| New caller using `--no-account --json` | old | `parse` error, exit 2 (unknown option). The app shows "update larp". |
| New caller using `--no-account --json` | new | `status`, byte-compatible with today. |

The current Mac app never calls `status`, so no shipped app silently starts
running a provider client. The frozen app copy of the schema includes the
new variant, and the app test proves it is still refused (no decoder support)
until the later app step adds it.

JSON account mode emits exactly one envelope on stdout and nothing on
stderr. The stage 3 human notices are not printed in JSON mode. A JSON
consumer that runs account status shows its own fixed notice. `status.account`
errors use the same discriminator. `run|login|logout --json` keep failing
`json_native_io`. The cli-v1 rule "`status --json` never invokes native
status" and "no implementation may capture native stdout to manufacture a
JSON account result" are amended for `status.account` only: native output is
read solely by the exact row's parser and is never forwarded.

`usage` is `null` in this amendment. The stage 11 contract owns the usage
window shape. Whichever of stage 11 and stage 14 merges second widens
`usage` to stage 11's per-profile local summary (or null). Until then the
human graph has no JSON counterpart.

Profile extras JSON (display name, launch arguments) is **not** frozen here.
Adding fields to `profile` changes every profile-bearing envelope, so it lands
as its own amendment with the registry v4 bump in the profile-extras
implementation PR.

### Admission and dispatch

Account mode first resolves the profile and checks `--expected-revision`
through the ordinary metadata path, exactly as `--no-account` does. Then it
uses the existing stage 3 admission and per-ID auth guard, in this order:
closed-table gate, checked home, exact artifact SHA-256 and pre-dispatch
revalidation.

1. **Closed table** (no Status row compiled in at all, which is today) →
   `not_qualified`, `provider_version` null, exit 0 and **zero provider
   executions**: no status child, no `--version` probe, no binary read beyond
   the metadata path. The default command stays usable.
2. No CLI binding, CLI surface disabled or unsafe CLI home → `unavailable`,
   no spawn.
3. Table not empty, but no exact Status row for provider, OS/arch, build,
   digest and argv → `not_qualified`, no status spawn. There is no parser
   fallback and no guess from another version.
4. Auth guard busy → `busy` error (exit 1), no account fields. Metadata mode
   is unaffected.
5. Otherwise spawn the row's literal argv. Candidates are Claude
   `["auth","status"]` (plus `"--json"` only if that exact build qualifies it)
   and Codex `["login","status"]`. Use the same selected CLI home as
   login/run/logout via `CLAUDE_CONFIG_DIR` or `CODEX_HOME`, with no shell
   and no fallback home.

Status replaces stage 3's terminal pass-through for this operation with a
**captured** spawn: stdin is the null device, stdout and stderr are pipes,
and the child gets no terminal. The row pins the non-TTY output shape.
Drain both pipes concurrently with a 16 KiB cap on each and a 10 s deadline,
unless a stricter reviewed row applies. On overflow, timeout or cancellation,
use the reviewed direct-child cancel-and-reap path before releasing the guard.
Keep the guard until reap, then revalidate the profile snapshot before
publishing. A stale snapshot fails `conflict` and publishes no account data.

LarpCC makes no network calls. The official child may contact its provider
as established in that row's evidence.

Provider version in the result is the qualified row's exact version. The
only fallback is case 3 above: the table is not empty, no Status row admits,
and an approved stage 08 version-only descriptor matches the exact artifact.
Then account mode runs that one bounded `--version` probe in a fresh
isolated home and reports its version. In every other case
`provider_version` is null and nothing runs. `--no-account` never probes.

### Remote account status

This is implemented with [stage 13](13-remote-hosts.md) (#20), not before.
Until then `--host` is unknown to the CLI. Every stage 13 rule applies
unchanged: routing and `--expected-host-revision`, the fixed SSH argv and
remote command, the version probe and receiver allowlist, `--remote-request`,
empty remote stderr, the 1 MiB per-stream caps, untrusted remote output with
fixed local error text, and the reserved `ssh_*`/`remote_*` codes.

- **Trigger.** Only an explicit user action: a `larp cc --host H status NAME`
  invocation, or the app's explicit account open/Refresh on a remote profile.
  App launch, restore, selection of a machine, window or menu-bar open and
  wake never run it. No polling.
- **Who decides.** The remote Larp runs its own admission, auth guard, exact
  Status row and parser against its own binaries and homes. The local side
  never checks, unlocks or overrides remote rows. Remote `not_qualified`,
  `unavailable`, `unknown` and `signed_out` show locally as returned.
- **Distinct opt-in.** Under `--remote-request`, an unmarked `status NAME`
  is **always metadata-only** on every receiver release, and returns
  `status`. Account mode needs the internal flag `--remote-account`, which
  the receiver checks before any registry IO, guard or spawn. It is valid
  only together with `--json --remote-request`; anywhere else it is `usage`,
  exit 2. A receiver that does not implement this stage rejects the unknown
  flag before effects, which the router reports as `remote_incompatible`.
  So `--no-account` sends plain `status NAME` to every receiver, and the
  router sends `--remote-account` only to versions on the stage 14 receiver
  allowlist. If the remote is upgraded or downgraded between the version
  probe and the command login, a metadata request can never become an
  account request, and an account request either runs on a receiver that
  understands the flag or is refused before effects.
- **Expected identity.** The router takes the expected profile ID and
  revision from the same metadata snapshot the request came from: the app's
  displayed remote snapshot, or, for a direct CLI caller, the
  `--expected-revision` and name it typed. It adds no extra remote read. It
  sends `--expected-revision REV` and the internal
  `--expected-profile-id ID` (also valid only with `--remote-request`). The
  receiver checks the revision under its registry lock before name
  resolution, then checks that NAME resolves to ID, all before the auth
  guard and any spawn. A mismatch is `conflict`, exit 1, with no child. The
  app always sends both. A direct CLI caller may omit them. The receiver then
  resolves NAME at call time, and the router still requires the reply's
  profile name to equal NAME. The router rejects a reply whose profile ID or
  name differs from what was sent, before display, as `remote_output`.
- **Reply.** One schema-1 `status.account` envelope (or an allowed error),
  with the same shape as local and no host field.
- **Deadlines.** See the table below.
- **Validation.** Apply the full schema and semantic checks of the local
  variant, including the email and plan forms. Anything else is
  `remote_output` and no account data is shown. Re-encode the validated
  values. Remote diagnostics get fixed local text. Remote email and plan
  are inert display text, never evidence that a local profile or another
  host is signed in.
- **Never store on both ends (D26).** The remote receiver keeps the values
  only in its request memory and logs nothing, as for a local call. The local
  router and app neither log, cache, persist nor attach them to diagnostics.
  The SSH child's stdout is drained into memory and dropped after
  validation. The app keys the result by route and profile ID and drops it on
  route change, refresh, mismatch or close.
- **Human output** is rendered locally from the validated envelope and adds
  the host name to the header line (`codex-q1 on build-box (codex 0.98.0)`).
- **Usage** stays stage 11 LOCAL-only remotely; `usage` remains null.

Deadlines for account status, the same numbers as
[stage 13's deadline table](13-remote-hosts.md#deadlines) (#20). Each SSH
child deadline is at least the remote op budget plus OpenSSH's 4 s
`ConnectTimeout` plus 2 s of margin. Reads and mutations keep their separate
stage 13 budgets.

| Operation | Op budget | SSH child deadline | App-bridge deadline |
|---|---:|---:|---:|
| Local account status (no SSH) | 10 s | – | 10 s plus local spawn and reap |
| Remote compatibility probe (stage 13) | 5 s | 11 s | part of the row below |
| Remote account status (`--host`) | 10 s | 16 s (10 + 4 + 2) | 33 s |

The 33 s app-bridge budget is 16 s for the account child, 11 s for the
compatibility probe, 2 s for local `ssh -V`, 2 s for failure cleanup, and
2 s for bridge overhead and envelope encoding. It uses the existing metadata
snapshot and adds no read. This replaces the earlier 12 s and 22 s draft
numbers, which did not allow for the 4 s `ConnectTimeout`.

The email crosses the user's own encrypted SSH connection to their own
machine. LarpCC adds no other transport.

### Parser (exact-version, fail-closed)

Each Status row names one parser for its exact release/build, digest,
platform and argv. The row pins the stream, encoding, complete shape, exact
exit/result pairs and which fields may be absent, with source and native
evidence. The parser extracts only:

- signed-in yes/no,
- account email, only from an explicitly sourced field or line,
- plan/subscription label, only from an explicitly sourced field or line.

Every other byte is discarded and never forwarded, logged or kept. An auth
method or backend name alone is not a plan. Nothing absent is filled in from a
credential file, keychain, session file or environment value. LarpCC never
reads token or credential files, ever.

Result rules:

- Unknown shape, duplicate keys/lines, invalid UTF-8, control bytes, invalid
  field values, an unrecognized exit/stream pair or any ambiguity → `unknown`.
- Child timeout, overflow or unexpected exit → `unknown`.
- Only the row's exact signed-out result → `signed_out`.
- A signed-in shape may omit a field only when that exact shape is in the row.
  For example, if the Codex build proves only signed-in state, both fields
  are null.
- Email must match the contract's printable-ASCII `local@domain` form (at most
  64 + 253 bytes). Plan must be 1–64 printable ASCII with no leading or
  trailing space. Otherwise → `unknown`.
- `unknown`, `signed_out`, `not_qualified` and `unavailable` always carry
  null email and plan.

A successful observation says what the client reported at that moment. It
does not certify grant validity, remote validity or isolation.

### Never store (D26)

LarpCC holds email and plan only in the memory of the request that produced
them. Buffers are released on success, refusal, timeout and cancellation.
No registry field, cache, log, temp file, telemetry, crash attachment, debug
formatting, help text or shell completion ever contains them. A
user-chosen display name may not contain `@`, so a label cannot become a
stored email. Application-level discard cannot prevent OS memory dumps,
terminal scrollback, screenshots or explicit user redirection, and the
shipped docs must say so.

## Profile extras

None of these needs a real account.

- **Provider version.** Use the official exact-path `--version` through the
  reviewed, bounded stage 08 version-only probe in a fresh isolated home. Only
  approved probe descriptors are extended. It works signed out. Results are
  cached in process memory, keyed by provider, resolved executable path,
  checked identity and current SHA-256, and rechecked on lookup, replacement
  and rebind. Stage 3 forbids persisting digests, so there is no on-disk
  version cache. A failed or unsupported probe reports unknown.
- **Display name.** `profile display-name NAME LABEL` sets a label.
  `profile display-name NAME --clear` removes it. 1–64 printable ASCII, no
  leading/trailing space, no `@`. The optional registry field `display_name`
  belongs to the immutable profile ID. Labels can repeat. An absent label
  shows the command name. Rename changes the command name only, and keeps
  aliases and the label on the ID. Never accept a provider-derived label.
- **Default launch arguments.** `profile launch-args NAME -- ARGS...` sets the
  whole vector. `profile launch-args NAME --clear` clears it. Stored as
  optional `launch_args`. At most 16 nonempty UTF-8 tokens of at most 128 bytes,
  no control characters, no `--` token. Only literal flags or bounded enum
  values from the exact reviewed interactive-run row grammar are accepted. At
  freeze that grammar admits only the empty vector, so the feature is inert
  until a run row widens it. Never prompts, free values, credentials, env
  assignments, paths, backend/auth overrides or isolation-affecting options.
  Invalid edits are refused without echo or partial write. `profile show`
  displays the vector with argument boundaries intact. `run NAME` uses the
  saved vector. `run NAME -- ARGS...` replaces it for that invocation. A bare
  `--` suppresses it. Each run validates the selected vector against the
  current exact row, including after rebind. Stale defaults that no longer
  qualify refuse until cleared or replaced. Defaults never apply to native
  auth, desktop or alias forwarding.
- **Registry.** One strict schema bump (v3 → v4) coordinated by CORE.
  Migrate v3 records under the existing registry lock and atomic writer, with
  null label and empty arguments. Metadata only: no profile-home reads or
  moves. Malformed or future schemas are refused without rewriting. IDs,
  aliases, surfaces and tombstones are preserved. Revision bumps once per real
  edit, `--expected-revision` applies, and clear/no-change is idempotent.
  The registry never holds an account result.

### Usage text graph

Account mode calls the **same** stage 11 per-profile LOCAL reader that
`larp cc usage PROFILE --local` uses, once, for the selected ID, and projects
its `windows.five_hour` (Session) and `windows.weekly` (Weekly). It starts no
LIVE request, refresh or separate scan, even if LIVE is enabled for that
profile. Before stage 11 ships, or when neither window is usable, print
`usage: not available`.

Each window follows its stage 11 `reason`:

| Stage 11 window | Rendering |
|---|---|
| `reason` null | Bar and `N% left`, then `Resets in …` |
| `reset_time_unknown` | Bar and `N% left`, then `Reset time unavailable` |
| `reset_since_capture` | `Session reset since last use` (padded label, no bar, no percentage, because the captured value is stale), then `Reset due` |
| any other reason | `Session: not available`, no reset line |

- Order: Session, then Weekly.
- Remaining = 100 − `used_percent`. `N` is floor(remaining), so the CLI
  never overstates what is left. Bar: 20 cells, filled = floor(remaining ×
  20 / 100). Labels padded to 7 columns, then a space, the bar in brackets, a
  space and `N% left`.
- Glyphs: `█` and `░` on a UTF-8 terminal with known cell widths, otherwise
  `#` and `-`.
- Colour: only the filled cells, only on a TTY with `NO_COLOR` unset and
  `TERM` not `dumb`. Pipes and unknown terminals get ASCII and no colour.
- Narrow: under 48 columns, omit the bar and keep label and percentage.
- Resets on their own line, indented 8 columns. `Resets in …` is
  `resets_at` minus stage 11's `generated_at`, rounded up to whole minutes
  (`Nh Mm` from one hour, `Nd Nh` from one day).
- Plan: the status output's plan is only the status-derived `account.plan`.
  Stage 11's plan (user label, `client_local` or `live_endpoint`, with its
  D29/D32 precedence) belongs to `usage` and is never copied into
  `account.plan` or the `Signed in` line.
- JSON never carries rendered bars, colour or relative time strings.

## Later app and menu-bar steps

These need the lead's explicit go, which needs the user's go, per step.
UI may only show data the CLI already provides.

**App (step 2).** Run `status NAME --json` only when the user explicitly
opens the selected profile's account view or presses Refresh. Never on
launch, selection restore, wake or a timer. Use `--expected-revision` and
check that the returned profile ID matches the request. Cancel or ignore
in-flight results on selection or window change. Clear account fields on
refresh, mismatch and close. All other views use `--no-account`.

- The email renders **blurred**. Clicking it reveals it, and clicking again
  re-blurs it (like the T3 app). It re-blurs when the window closes or the app
  relaunches. Revealed state is never persisted.
- While blurred, accessibility values, tooltips, selection/copy, drag and
  context menus must not expose the text.
- **Hide account emails** (Settings) removes the email and its reveal
  control and clears any revealed value at once. Only this boolean is
  persisted. The plan label still shows.
- The app never persists the account result, never background-refreshes it,
  and shows absent fields as unavailable.
- Profile-extras editing uses the existing CLI mutation precondition.
- This step amends stage 10's "No account identity, quota meter or auth
  inference" for this account view only.

**Menu bar (step 3).** The menu bar never runs account status and never shows
an email. Whether it may show a plan label is deferred to this step; the lead
asks the user then. It uses profile labels and stage 11 usage/reset data the CLI
already provides, and renders unknown resets as unavailable. It never caches
or reuses an account result from the app.

## Phase 2 — Mac qualification plan

This is a **plan only**. During this PR nothing runs on the Mac, nobody signs
in, and no agent touches `~/.claude*`, `~/.codex*`, `~/.codex-1`, `~/.codex-2`,
wrappers or default app data. Execution is a separate session that the lead
authorizes after merging this plan, with the user present.

Standing limits for that session: the user completes every browser sign-in.
Agents never read credential contents, list keychain items, call a provider
API or record an email. Existing installs stay unmodified.

### Gate G0 — credential store isolation (before the first native call)

No native operation may read, refresh or overwrite the user's existing
sign-in. That includes **status**: stage 3 treats status as a native auth
operation that can read or refresh a grant, and a throwaway home alone does
not prove the client avoids a shared Keychain item or a fallback location.
So G0 applies to each native operation (status, login, run, logout) before
it first runs on the user's Mac. For each one, the exact build's evidence
(official docs, release notes or source for that build, never a keychain
listing) must establish its effective credential backend, its lookup key,
the precedence between backends and every fallback. Without that proof the
operation does not run and its row stays closed. In particular:

- Claude Code with `CLAUDE_CONFIG_DIR` set stores its credential in a location
  keyed to that config dir (a per-dir keychain item or a file inside the
  home), not in the shared default item.
- Codex with `CODEX_HOME` set stores `auth.json` inside that home, or a
  keyring entry keyed to that home. If the store mode is configurable, the
  row pins the mode and where it comes from.

If any backend, key, precedence or fallback is shared, unkeyed or unknown for
an operation, **that operation is blocked** and its row stays closed. A
signed-out status is not exempt. G0 evidence is cited sources only.

**Every PII-capable observation runs only in the user's unrecorded
terminal.** That is every account-mode `status` (including those expected to
return `Signed out`), `login`, `run` and `logout`. The agent runs only
metadata commands (`--no-account`, `profile add`, `provider bind`,
`profile delete`) and the stage 08 `--version` probes in fresh probe homes.
The user reports result categories and field presence to the agent, never
values.

### Steps

1. **Nominate and pin the binaries.** The user runs, and reports to the
   agent, the resolved path of the official Claude Code install and of
   `/opt/homebrew/bin/codex`. Validate the trusted path chain and final
   native executable with stage 3 checks. Record install format, macOS
   version/arch, exact Larp candidate head, resolved native path, checked
   identity, byte size and SHA-256 (verified handle, 512 MiB actual-byte
   limit, pre/post identity and dispatch recheck). Link the bytes to the
   official release. Run each official `--version` once in a fresh signed-out
   probe home and record the exact build. An unsafe shim or an unexplained
   digest, build, architecture or load dependency keeps the rows closed.
2. **Confined state.** Create a private temporary `LARPCC_HOME` and a
   disposable repository. Create `claude-q1` and `codex-q1` with
   `profile add NAME --provider PROVIDER` and bind the nominated paths with
   `provider bind`. Use a fresh browser profile/window chosen by the user.
   Quiesce other sessions of the same provider. The ordinary CLI stays
   closed until a reviewed candidate build has the proposed rows compiled
   in. No override or public bypass.
3. **Run the script, one provider at a time, on that candidate:**

   | # | Command | Who | Expect |
   |---|---|---|---|
   | 1 | `larp cc status NAME --no-account` | agent | metadata only, no child |
   | 2 | `larp cc status NAME` | user's terminal, not recorded (G0 status passed) | `Signed out` |
   | 3 | `larp cc login NAME` | user confirms, completes browser sign-in | native exit 0 |
   | 4 | `larp cc status NAME` | user's terminal, not recorded | `Signed in as … · …` |
   | 5 | `larp cc status NAME --json` | user's terminal, not recorded | `status.account`, `signed_in` |
   | 6 | `larp cc run NAME` | user, interactive, exits at once | normal exit, no model request |
   | 7 | `larp cc status NAME` | user's terminal, not recorded | still signed in after run |
   | 8 | `larp cc logout NAME` | user confirms | native exit 0 |
   | 9 | `larp cc status NAME` | user's terminal, not recorded | `Signed out` |
   | 10 | `larp cc profile delete NAME --yes` | agent | profile removed |

   Budget per provider: those calls once each, no retries. Status 10 s each,
   login 5 min, run and logout 2 min each, whole session 30 min. Freeze exact
   times, allowed provider/browser network actions and state effects in the
   approved sheet beforehand. Steps 2–9 run in the user's terminal, and
   the email appears only there. The agent learns result categories and
   field presence from the user, not values. A step whose G0 proof is
   missing is skipped and recorded as blocked.
4. **Evidence, with no secrets.** For every invocation record: operation,
   exit/signal, parser result category, and `email: present|absent`,
   `plan: present|absent`. Never record the email, plan value, transcript,
   screenshot, auth code, token, URL, argv/env dump or raw output.

   Touched paths use a method without fs_usage or dtrace by default: a
   before/after inventory of paths, kinds, sizes and mtimes inside the
   disposable root only (no file content, no symlink traversal). This proves
   observed changes inside the root. It cannot prove which paths were read or
   that nothing outside was touched, and the evidence says so. Outside-access
   evidence stays **pending** unless the user opts into a reviewed per-process
   observer scoped to the candidate and its direct child, which emits only
   approved disposable paths plus one boolean "outside approved paths". Raw
   traces are never persisted. Use invented fixtures to reproduce the schema.
5. **Stop, clean up, assess.** Stop on wrong account, unexpected backend,
   outside access, changed artifact, sensitive spill or failure to reap.
   Keep only safe diagnostics and block the affected rows. After sign-out and
   quiescence, delete each throwaway profile with confirmed `profile delete`,
   then remove the temporary root and repository, inside the approved cleanup
   scope only. Never improvise a logout on a shared backend. Record cleanup
   status. Two profiles (one per provider) do not prove same-provider A/B
   independence, natural refresh, remote grant validity or global
   invariance. Those need their own approved sheets.

### Rows to add to the compiled table

The later implementation PR (after the lead merges this plan) adds only
complete, approved rows. Each row records: exact Larp head, OS/arch,
provider build, resolved native artifact and SHA-256, install kind and
provenance, operation, literal ordered argv, interface/backend/fallback,
credential store mode (G0), policy revision, denied env names, native path
rules (kind, absence/error, purpose), evidence references, and, for Status,
the parser ID and its exit/shape matrix. Unknown cells come only from
qualification evidence. A version probe or a successful status does not
authorize any other operation.

| macOS candidate | Operation | Native argv to review | Depends on |
|---|---|---|---|
| Claude Code (nominated path) | Status | `auth status` (+ `--json` if qualified) | step 1, G0 (status) |
| Claude Code | Login | `auth login` | G0 (login), Status |
| Claude Code | Run | interactive root, no forwarded args, exact fixed options | G0 (run), Login |
| Claude Code | Logout | `auth logout` | G0 (logout), Login |
| Codex `/opt/homebrew/bin/codex` | Status | `login status` | step 1, G0 (status) |
| Codex | Login | `login` | G0 (login), Status |
| Codex | Run | interactive root, no forwarded args, exact fixed options | G0 (run), Login |
| Codex | Logout | `logout` | G0 (logout), Login |

### Tests

Synthetic tests use invented rows and release-independent fixtures. The
shipped table stays closed wherever admission, namespace, parser or
lifecycle evidence is incomplete.

| Test | Required proof |
|---|---|
| `stage14_status_modes` | Default runs account mode; `--no-account` never spawns (fake binary counts zero execs) and emits the byte-compatible `status` envelope; `--no-account` refused on other commands. |
| `stage14_admission` | Closed table → `not_qualified`, null version, exit 0 and zero provider executions (no status child, no `--version` probe); nonempty table without a matching row → at most the approved version probe; no binding/disabled/unsafe home → `unavailable`; exact SHA/OS/build/argv; revision drift before dispatch and after reap → `conflict`; auth guard held until reap; busy → `busy`; no fallback home. |
| `stage14_parser` | Each invented row's signed-in, partial and signed-out shapes; `unknown` on wrong version, shape, exit/stream pair, encoding, duplicates, control bytes, invalid email/plan, overflow and timeout. |
| `stage14_never_store` | Sentinel PII and sentinel credentials in fake output never appear in stdout of other commands, stderr, errors, registry, any file under the root, temp dirs or crash output; unrelated fields from the fake never appear anywhere. |
| `stage14_json_contract` | Fixtures in cli-v1 examples; one LF-terminated object, empty stderr; errors use `status.account`; `run/login/logout --json` still `json_native_io`; `doctor` unchanged. |
| `stage14_human` | Full email, each state line, partial fields, display name header, exit 0 per completed state, cancellation propagation. |
| `stage14_extras` | v3 → v4 metadata migration; label/arguments kept on rename; `@` refused; expected-revision and atomic failure; version cache invalidation on replacement; argv override, bare `--`, stale-default refusal. |
| `stage14_usage_render` | Exactly one call to the stage 11 LOCAL reader and no LIVE request even with LIVE enabled; each stage 11 `reason`, including `reset_since_capture` with no percentage and `Reset due`; stage 11 plan never in `account.plan`; 0% and 100%, floor rounding, reset text, narrow, non-TTY, `NO_COLOR`, ASCII, no live call. |
| Mac live evidence | Per-operation identity/digest, G0 citation, path scope, exit/result and cleanup on the exact head; unresolved stage 3 dependencies listed one by one. |
| `stage14_remote` (with stage 13) | Fake SSH and fake remote Larp: explicit-action only; remote states pass through; an unmarked `status` under `--remote-request` never spawns on any receiver; `--remote-account` and `--expected-profile-id` refused outside `--remote-request`; stage-13-only remote gives `remote_incompatible` for account mode and plain metadata `status` for `--no-account`; **version change between probe and dispatch** (upgrade: metadata request stays metadata with zero spawns; downgrade: account request refused before effects); **rename + name reuse** (snapshot rev 7 has ID 2 as `codex-45`, then ID 2 is renamed and ID 3 takes the name: stale revision → `conflict`, revision omitted but `--expected-profile-id 2` → `conflict`, both with zero spawns; a reply for ID 3 → `remote_output`); wrong discriminator, bad email form and stderr → `remote_output`; stream cap → `too_large`; 16 s child and 33 s bridge deadlines → `remote_timeout`; no account data in any failure; sentinel email absent from local and remote logs, files and diagnostics. |
| Later UI acceptance | CLI contract shipped first; explicit open/refresh only; stale-result guard; blur, reveal, re-blur on close/relaunch, hide setting, accessibility; menu bar never shows an email. |

## Resolved rulings (lead, 2026-11-21; the user may still veto)

1. **Remote account status:** allowed, as specified in
   [Remote account status](#remote-account-status), implemented with stage 13.
2. **Plan label in the menu bar:** deferred to the menu-bar step. For now the
   menu bar never runs account status.
3. **Human exit code:** 0 for every completed observation, including signed
   out. Scripts read `account.state` from `--json`.
4. **G0** and the separate registry v4 profile-extras amendment are approved.

Review fixes (lead, 2026-11-23): G0 covers every native operation including
the first status call, and PII-capable observations run only in the user's
terminal. Remote account mode needs the receiver-checked `--remote-account`
opt-in. Expected ID and revision come from the metadata snapshot. A closed
table means zero provider executions. Deadlines match stage 13 exactly. The
usage graph reuses the stage 11 LOCAL reader.
