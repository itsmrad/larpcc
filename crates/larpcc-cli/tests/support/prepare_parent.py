"""CI fixture allocation only; production has no test-root override."""
import os
from pathlib import Path
import stat
import tempfile

home = Path.home()
if os.name == "posix":
    for path in [*reversed(home.parents), home]:
        metadata = path.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid not in (0, os.geteuid()) or metadata.st_mode & 0o022:
            raise RuntimeError(f"untrusted fixture ancestor: {path}")
parent = Path(tempfile.mkdtemp(prefix="larpcc-native-", dir=home))
if os.name == "posix":
    metadata = parent.lstat()
    if metadata.st_uid != os.geteuid() or stat.S_IMODE(metadata.st_mode) != 0o700:
        raise RuntimeError("fixture parent was not created private")
# Native platform tests also validate their actual ancestry; Windows ACL evidence is required.
with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as output:
    output.write(f"LARPCC_TEST_PARENT={parent}\n")
    output.write(f"CARGO_TARGET_DIR={parent / 'default-target'}\n")
print(f"Synthetic fixture parent: {parent}")
