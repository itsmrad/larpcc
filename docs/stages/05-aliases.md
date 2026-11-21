# Stage 05 — aliases

Import is **OUT OF SCOPE**; fresh profiles only. Depends on merged stage 04 v3
seed. CORE owns aliases module/shared CLI registration/tests; platform changes
serialized by CORE. JSON shapes: [CLI v1](../contracts/cli-v1.md).

```text
larp cc alias add NAME PROFILE [--desktop] [--shadow]
larp cc alias list
larp cc alias remove NAME
```

Use profile-name grammar with reserved device names and `larp` rejected. Any
valid name maps to either provider and immutable ID, not provider-shaped prefix.
`claude` and `codex` require `--shadow`; never overwrite an existing command,
including with --shadow. Explicit user PATH precedence, no rc/PATH edits.
Artifacts only in `<root>/bin` with private ownership and a root-bound launcher
ABI manifest. Installation prints guidance; tests never touch ~/.local/bin.

A copy of Larp dispatches by its alias basename (.exe stripped on Windows).
The adjacent receipt pins the absolute root, root and `bin` directory
identities, launcher identity and hash, ABI, and immutable profile ID. Alias
dispatch forwards arguments to the selected CLI admission or desktop open.
Arguments such as `cc` and `--json` are vendor arguments in CLI alias mode and
remain subject to the ordinary launch argument policy. Alias invocations never
parse the `cc` grammar. Final admission rechecks the alias mapping revision,
profile ID, directory identities, and receipt digest. Profile rename retains
the ID mapping.

A launcher copied or renamed to the executable basename `larp` and invoked as
`larp` is standard Larp. It uses the ambient `LARPCC_HOME` or OS default and
the same closed admission with empty qualification rows. It ignores adjacent
receipts and alias root pins. Renaming is a user action and provides no privilege
boundary.

Add stages complete files at exclusive private names in `<root>/bin`. The
journal records their identities and hashes before atomic publication without
replacement. Pending receipts cannot dispatch. An interrupted add can roll back
proven artifacts. Unrecorded or changed files leave the journal pending for
manual recovery. Removal and replay derive ownership from the committed mapping
and receipt digest and delete via the checked directory or file handle.
A missing launcher may retire with its owned receipt. Drift leaves any pending
transaction in place. Revision-pinned commands compare the revision before any
recovery and refuse pending work without replaying it.

Alias creation requires Unicode root and source paths. Linux checks the running
image through `/proc/self/exe`. macOS checks the mapped main image's vnode.
Windows creation refuses with `unsafe_path` because reopening the module path
alone cannot prove loaded-image ownership.

Implement native synthetic tests and execute
`cargo test --workspace --locked stage05_` (zero matches fails):
`stage05_dispatch` arbitrary names/providers, rename stability, exact argv[0],
spaces/non-Unicode/Windows quoting and exit; `stage05_collisions` shadow flag,
existing files/PATH collisions/reserved names; `stage05_transaction` interrupted
add/remove/concurrent writers/forged receipt/moved root/drift; `stage05_closed`
ordinary alias still qualification-refuses. Extend existing harness, use only
claude-work/codex-45 in temporary LARPCC_HOME, finally delete aliases/receipts/home.
