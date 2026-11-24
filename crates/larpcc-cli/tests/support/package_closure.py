"""Check the actual source archive and all-features packaged install, never a provider."""
import json
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
for operation in ("run", "login", "logout"):
    env = os.environ.copy()
    env.update(LARPCC_HOME="relative", LARPCC_SYNTHETIC="1")
    result = subprocess.run(
        [str(prefix / "bin/larp"), "cc", operation, "missing"],
        env=env, capture_output=True, timeout=10,
    )
    assert result.returncode == 1 and not result.stdout, result
    assert result.stderr == Path("crates/larpcc-cli/tests/fixtures/qualification-pending.txt").read_bytes(), result
env.update(LARPCC_HOME=str(prefix / "managed"), LARPCC_STAGE14_FAKE_DIGEST="00" * 32)
larp = str(prefix / "bin/larp")
subprocess.run([larp, "cc", "profile", "add", "codex-45", "--provider", "codex"],
               env=env, capture_output=True, timeout=10, check=True)
status = subprocess.run([larp, "cc", "status", "codex-45", "--json"],
                        env=env, capture_output=True, timeout=10, check=True)
assert not status.stderr
data = json.loads(status.stdout)["data"]
assert data["account"] == {"state": "not_qualified", "email": None, "plan": None}
assert data["provider_version"] is None
assert not (prefix / "managed" / "auth-locks").exists()
print("Source archive excludes synthetic helpers; packaged all-features install contains only closed larp.")
