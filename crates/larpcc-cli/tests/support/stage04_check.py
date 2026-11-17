"""Run the native seed acceptance filter and reject vacuous zero-test success."""
import os
import re
import subprocess
import sys

stage = os.environ.get('LARPCC_STAGE_FILTER', 'stage04_')
assert stage in ('stage04_', 'stage06_')
result = subprocess.run(['cargo', *sys.argv[1:], 'test', '--workspace', '--locked', stage],
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
print(result.stdout, end='')
if result.returncode:
    sys.exit(result.returncode)
counts = re.findall(r'test result: ok\. ([0-9]+) passed;', result.stdout)
if not counts or sum(map(int, counts)) == 0:
    sys.exit(f'{stage} acceptance matched zero tests')
