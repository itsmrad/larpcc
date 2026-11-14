"""Check the actual source archive and all-features packaged install, never a provider."""
import os
from pathlib import Path
import subprocess
import tarfile
import tomllib

package = Path(os.environ["CARGO_TARGET_DIR"]) / "package"
name = "larpcc-cli-0.1.0"
with tarfile.open(package / f"{name}.crate") as archive:
    members = archive.getnames()
    assert not any(path.startswith(f"{name}/tests/") for path in members), members
    manifest = tomllib.loads(archive.extractfile(f"{name}/Cargo.toml").read().decode())
    assert [target["name"] for target in manifest["bin"]] == ["larp"], manifest["bin"]
prefix = Path(os.environ["LARPCC_TEST_PARENT"]) / "package-install"
assert [path.name for path in (prefix / "bin").iterdir()] == ["larp"]
for operation in ("run", "login", "status", "logout"):
    env = os.environ.copy()
    env.update(LARPCC_HOME="relative", LARPCC_SYNTHETIC="1")
    result = subprocess.run(
        [str(prefix / "bin/larp"), "cc", operation, "missing"],
        env=env, capture_output=True, timeout=10,
    )
    assert result.returncode == 1 and not result.stdout, result
    assert result.stderr == Path("crates/larpcc-cli/tests/fixtures/qualification-pending.txt").read_bytes(), result
print("Source archive excludes synthetic helpers; packaged all-features install contains only closed larp.")
