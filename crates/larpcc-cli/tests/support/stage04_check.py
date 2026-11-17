"""Run the native seed acceptance filter and reject vacuous zero-test success."""
import re
import subprocess
import sys

result = subprocess.run(['cargo', *sys.argv[1:], 'test', '--workspace', '--locked', 'stage04_'],
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
print(result.stdout, end='')
if result.returncode:
    sys.exit(result.returncode)
counts = re.findall(r'test result: ok\. ([0-9]+) passed;', result.stdout)
if not counts or sum(map(int, counts)) == 0:
    sys.exit('stage04_ acceptance matched zero tests')
