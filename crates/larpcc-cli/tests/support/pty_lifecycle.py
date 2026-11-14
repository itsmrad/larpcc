"""Bounded real PTY job-control harness; the provider itself is always the native fake."""
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import resource
import subprocess
import select
import signal
import struct
import sys
import termios
import time

record = Path(os.environ["LARPCC_FAKE_RECORD"])
deadline = time.monotonic() + 8
final_signal = signal.SIGQUIT if os.environ.get("LARPCC_PTY_FINAL_SIGNAL") == "QUIT" else signal.SIGINT
report_read, report_write = os.pipe()
control_read, control_write = os.pipe()
harness, master = pty.fork()
if harness == 0:
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.close(report_read)
    os.close(control_write)
    signal.signal(signal.SIGTTOU, signal.SIG_IGN)
    start_read, start_write = os.pipe()
    child = os.fork()
    if child == 0:
        os.close(start_write)
        os.setpgid(0, 0)
        if os.environ.get("LARPCC_PTY_DELAY_FOREGROUND"):
            os.write(report_write, b"gated")
        if os.read(start_read, 2) != b"fg":
            os._exit(6)
        os.close(start_read)
        os.execve(sys.argv[1], sys.argv[1:], dict(os.environ))
    os.close(start_read)
    os.setpgid(child, child)
    if os.environ.get("LARPCC_PTY_DELAY_FOREGROUND"):
        if os.read(control_read, 2) != b"go":
            os._exit(7)
    os.tcsetpgrp(0, child)
    # Entry must imply foreground installation, not merely that setpgid completed.
    os.write(start_write, b"fg")
    os.close(start_write)
    pid, status = os.waitpid(child, os.WUNTRACED)
    if not os.WIFSTOPPED(status) or os.WSTOPSIG(status) != signal.SIGTSTP:
        os._exit(3)
    os.write(report_write, b"stopped")
    if os.read(control_read, 2) != b"fg":
        os._exit(4)
    os.tcsetpgrp(0, child)
    os.killpg(child, signal.SIGCONT)
    os.write(report_write, b"resumed")
    _, status = os.waitpid(child, 0)
    os._exit(0 if os.WIFSIGNALED(status) and os.WTERMSIG(status) == final_signal else 5)
os.close(report_write)
os.close(control_read)

def wait_for(predicate):
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.01)
    raise TimeoutError("PTY condition exceeded eight seconds")

def report(expected):
    remaining = deadline - time.monotonic()
    if remaining <= 0 or not select.select([report_read], [], [], remaining)[0]:
        raise TimeoutError("PTY report timed out")
    assert os.read(report_read, len(expected)) == expected

complete = False
try:
    def ready():
        try:
            value = json.loads(record.read_text())
            return value if value else False
        except (FileNotFoundError, json.JSONDecodeError):
            return False
    if os.environ.get("LARPCC_PTY_DELAY_FOREGROUND"):
        report(b"gated")
        assert not record.exists(), "fake entered before foreground installation"
        os.write(control_write, b"go")
    wait_for(ready)
    value = json.loads(record.read_text())
    assert all(value[key] for key in ("stdin_terminal", "stdout_terminal", "stderr_terminal"))
    if os.environ.get("LARPCC_PTY_AUTH"):
        assert value["pid"] != value["unix"]["pgrp"]
    else:
        assert value["pid"] == value["unix"]["pgrp"]
    assert os.tcgetpgrp(master) == value["unix"]["pgrp"]
    # Real terminal resize, suspend character, foreground continuation, then interrupt character.
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 41, 103, 0, 0))
    def resized():
        try:
            size = json.loads(record.with_suffix(".resize").read_text())
            return size == {"rows": 41, "columns": 103}
        except (FileNotFoundError, json.JSONDecodeError):
            return False
    wait_for(resized)
    os.write(master, b"\x1a")
    report(b"stopped")
    if os.environ.get("LARPCC_PTY_AUTH"):
        other = subprocess.run(sys.argv[1:], env=dict(os.environ), capture_output=True, timeout=2)
        assert other.returncode == 1 and b'profile authentication operation is busy' in other.stderr
    os.write(control_write, b"fg")
    report(b"resumed")
    os.write(master, b"\x1c" if final_signal == signal.SIGQUIT else b"\x03")
    result = None
    def exited():
        global result
        pid, status = os.waitpid(harness, os.WNOHANG)
        if pid:
            result = status
            return True
        return False
    wait_for(exited)
    complete = True
    assert os.WIFEXITED(result) and os.WEXITSTATUS(result) == 0, result
    print("real PTY stdin/out/err, foreground group, Ctrl-Z/fg, resize and Ctrl-C passed")
finally:
    if not complete:
        try:
            os.kill(harness, signal.SIGKILL)
        except ProcessLookupError:
            pass
        cleanup = time.monotonic() + 2
        while time.monotonic() < cleanup:
            try:
                if os.waitpid(harness, os.WNOHANG)[0]:
                    break
            except ChildProcessError:
                break
            time.sleep(0.01)
    for descriptor in (master, report_read, control_write):
        os.close(descriptor)
