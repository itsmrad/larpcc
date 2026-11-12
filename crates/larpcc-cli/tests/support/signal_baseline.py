"""Compare only native synthetic children, under an explicitly supplied synthetic environment."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

driver, fake, base = sys.argv[1:]

def inherited_signals():
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    signal.pthread_sigmask(signal.SIG_BLOCK, [signal.SIGTERM])

records = []
for suffix, args in [("direct", [fake, "prompt"]), ("launch", [driver, "cc", "run", "claudea", "--", "prompt"])]:
    path = Path(base + "." + suffix)
    env = dict(os.environ)
    env["LARPCC_FAKE_RECORD"] = str(path)
    child = subprocess.Popen(args, env=env, preexec_fn=inherited_signals, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        out, err = child.communicate(timeout=8)
        assert child.returncode == 0 and not out and not err, (child.returncode, err)
    except BaseException:
        child.kill()
        child.communicate(timeout=2)
        raise
    records.append(json.loads(path.read_text())["unix"]["signals"])
assert records[0] == records[1]
by_signal = {entry["signal"]: entry for entry in records[1]}
assert by_signal[signal.SIGINT]["disposition"] == "ignored"
assert by_signal[signal.SIGTERM]["blocked"]
print("native inherited ignored disposition and blocked mask match direct baseline")
