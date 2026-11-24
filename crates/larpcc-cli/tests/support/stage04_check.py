"""Run the native seed acceptance filter and reject vacuous zero-test success."""
import os
import re
import subprocess
import sys
from pathlib import Path

test_parent = os.environ.get('LARPCC_TEST_PARENT', '')
is_ci = os.environ.get('GITHUB_ACTIONS') == 'true'
vendor_vars = ('CODEX_HOME', 'CLAUDE_CONFIG_DIR', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME',
               'ANTHROPIC_API_KEY', 'OPENAI_API_KEY')
wrapped = (Path(os.environ.get('HOME', '')).name.startswith('.larpcc-cargo-home.')
           and all(name not in os.environ for name in vendor_vars))
if not os.path.isdir(test_parent) or not (is_ci or wrapped):
    print('stage04_check.py requires an existing LARPCC_TEST_PARENT and GitHub Actions or larpcc-cargo.',
          file=sys.stderr)
    sys.exit(2)

stage = os.environ.get('LARPCC_STAGE_FILTER', 'stage04_')
assert stage in ('stage04_', 'stage06_', 'stage07_')
result = subprocess.run(['cargo', *sys.argv[1:], 'test', '--workspace', '--locked', stage],
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
print(result.stdout, end='')
if result.returncode:
    sys.exit(result.returncode)
counts = re.findall(r'test result: ok\. ([0-9]+) passed;', result.stdout)
if not counts or sum(map(int, counts)) == 0:
    sys.exit(f'{stage} acceptance matched zero tests')
