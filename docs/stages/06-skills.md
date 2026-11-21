# Stage 06 — skills store and links

Import/adoption of existing user profiles or global skills is **OUT OF SCOPE**.
Fresh profiles only. Explicit synthetic/source skill packages may be added;
never read ~/.agents/skills or vendor homes. Depends on stage 04; CORE owns share
module, CLI registration and synthetic tests, serializes platform helpers.

```text
larp cc skills add PATH --name NAME
larp cc skills remove NAME
larp cc skills enable|disable NAME PROFILE
larp cc skills list
larp cc skills matrix
larp cc skills sync
```

Store `<root>/shared/skills/<name>/SKILL.md` and safe supporting regular files.
Add copies an explicit package once (not a live external link), after no-follow
validation; reject special files, symlinks/reparse, traversal, hardlinked files,
unsafe ancestry, collisions; limits 1,000 files/16 MiB/16 levels per package.
Sources inside the managed root or known vendor homes refuse `forbidden_source`
before any package contents are read. This comparison is bidirectional: a source
cannot equal, contain or be inside a known vendor home, managed root or profile
directory. Directory identities alone identify excluded vendor homes; permissive
vendor modes do not authorize reads or block unrelated packages. Only directory
targets found by the default namespace scan are homes; config-file aliases are
ignored. Configured but unidentifiable homes still fail closed.

Each source is one explicit package with a root `SKILL.md` and portable supporting
regular files. All dot entries are refused (including `.credentials.json`, `.env*`
and `.git/**`), as are case-insensitive `auth.json`, `*.pem` and `id_*` names at any
depth. A denied name refuses the entire add with `unsafe_path` before its bytes
are read; nothing is copied. No dot entries are needed or accepted by this
restricted package format. Reopened source identity must match the directory
already authorized. No scripts execute. SHA-256 digest is over sorted relative paths+file lengths+
bytes, unambiguous length-prefixed encoding; store immutable, no overwrite.
Remove refuses enabled references until disabled. Names use profile grammar.
New skills default enabled for all active CLI profiles; new profiles inherit all
stored skills. Explicit disables persist by ID, independent of rename.

Under same registry lock, separate skills index version=1 holds revision,
name/digest and effective per-ID switches. Per-profile links into cli/skills
point only to verified internal store entries. Stage 06 uses owned digest-copies
on Windows; junction creation is outside this stage.
No fallback over occupied destinations. Existing directories, links to other
sources and changed copies refuse `drift`; never replace real directories.
Disable/removal only removes proven owned link/copy, no traversal of target.
`sync` journals plan/apply with manifest expected revision, rollback/resume rules
from stage 04. A missing owned profile artifact is absent: sync recreates it, and disable/removal
drops its receipt through the journal. Occupied or changed artifacts still refuse
drift. Profile hooks validate only their affected entries. Read-only list/matrix
never repairs. Skills writes acquire lifecycle guards in ascending profile ID
order before reacquiring the registry lock and rechecking the revision; contention
refuses without profile writes. Catalog-only changes need no lifecycle guard. Global/project/provider
skills may remain visible; disabling here does not hide those scopes. Desktop
chat skills are account-side and outside local sharing.

[JSON](../contracts/cli-v1.md) list and matrix expose digest/switch/state, not
package contents. Implement and execute
`cargo test --workspace --locked stage06_` on each native OS:
`stage06_store` safe package/digest/limits; `stage06_links` enable/disable and new
profile, Windows owned-copy mode and drift/removal; `stage06_drift` occupied directory,
changed copy/link, traversal/reparse/hardlink/outside sentinel;
`stage06_crash` concurrent toggles/delete and each transaction interruption.
Zero matched tests fails. All fixtures temporary, names claude-work/codex-45,
finally remove store, links, copies, profile and root; no existing global skills.
