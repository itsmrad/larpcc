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

A copy of Larp (never vendor binary) dispatches by argv[0] basename (.exe stripped
on Windows) through a root-pinned receipt, not ambient LARPCC_HOME or PATH.
Receipt version/ABI/root/profile ID/alias hash validated; moving/copying launcher
or substituting root refuses. Standard larp invocation alone parses `cc` grammar;
alias invocation forwards argv to selected CLI run or desktop open, including
literal `--json` as vendor args for CLI alias (not JSON mode). Preserve stage-3
argument/admission restrictions; alias is no unlock. Root pinning may use an
adjacent validated receipt and immutable absolute root, no executable patching.
Revalidate mapping snapshot before dispatch. Name change retains ID mapping.

Add is lock+journal+exclusive artifact creation+hash/identity validation+registry
commit; crash resumes or refuses on drift. Do not leave a usable unregistered
launcher: pending receipts cannot dispatch. Remove validates fixed install path,
hash/identity and receipt before unlink; changed/missing ownership refuses drift,
never delete by marker alone. Cleanup and mapping removal are journaled together.
Missing artifact may retire mapping only when its absence and remaining receipt
ownership are safely established. No arbitrary absolute manifest paths.

Implement native synthetic tests and execute
`cargo test --workspace --locked stage05_` (zero matches fails):
`stage05_dispatch` arbitrary names/providers, rename stability, exact argv[0],
spaces/non-Unicode/Windows quoting and exit; `stage05_collisions` shadow flag,
existing files/PATH collisions/reserved names; `stage05_transaction` interrupted
add/remove/concurrent writers/forged receipt/moved root/drift; `stage05_closed`
ordinary alias still qualification-refuses. Extend existing harness, use only
claude-work/codex-45 in temporary LARPCC_HOME, finally delete aliases/receipts/home.
