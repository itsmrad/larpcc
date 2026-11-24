"""Run Stage 14 acceptance tests and require both real gate and fake coverage."""
import re
import subprocess
import sys

from cargo_test_environment import require_private_test_environment

require_private_test_environment('stage14_check.py')

result = subprocess.run(
    ["cargo", *sys.argv[1:], "test", "--workspace", "--locked",
     "--features", "synthetic-launch", "stage14_"],
    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
)
print(result.stdout, end="")
if result.returncode:
    sys.exit(result.returncode)
for test in ("closed_status", "status_modes", "admission", "parser",
             "never_store", "json_contract", "human"):
    if not re.search(rf"^test \S*stage14_{test}\S* \.\.\. ok$", result.stdout, re.M):
        sys.exit(f"Stage 14 acceptance did not run {test}")
