"""Bounded synthetic-only terminal confirmation and snapshot race oracle."""
import errno
import os
import pty
import select
import signal
import subprocess
import sys
import termios
import time

mode, larp, driver, digest, operation = sys.argv[1:]
pid, master = pty.fork()
if pid == 0:
    attrs = termios.tcgetattr(0)
    attrs[3] &= ~termios.ECHO
    termios.tcsetattr(0, termios.TCSANOW, attrs)
    os.execve(driver, [driver, 'stage3', operation, 'work', '--provider', 'codex', '--sha256', digest], dict(os.environ))
end = time.monotonic() + 8
output = bytearray()
status = None
try:
    while b'Type yes: ' not in output:
        assert time.monotonic() < end, 'confirmation prompt timeout'
        if select.select([master], [], [], 0.05)[0]:
            output.extend(os.read(master, 4096))
    assert b'ID 1' in output and b'codex' in output and b'profiles/1' in output
    if mode == 'rename':
        subprocess.run([larp, 'cc', 'profile', 'rename', 'work', 'renamed'], env=dict(os.environ), check=True, capture_output=True, timeout=2)
    answers = {'yes': b'yes\n', 'crlf': b'yes\r\n', 'no': b'no\n', 'eof': b'\x04', 'overlong': b'x'*17+b'\n', 'second': b'no\nyes\n', 'rename': b'yes\n'}
    os.write(master, answers[mode])
    while time.monotonic() < end:
        if select.select([master], [], [], 0.02)[0]:
            try:
                output.extend(os.read(master, 4096))
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
        observed, value = os.waitpid(pid, os.WNOHANG)
        if observed:
            status = value
            break
    assert status is not None, 'confirmation completion timeout'
    expected = 0 if mode in ('yes', 'crlf') else 1
    assert os.WIFEXITED(status) and os.WEXITSTATUS(status) == expected, (status, output)
    if mode == 'rename':
        assert b'launch context changed during admission' in output
    elif expected:
        assert b'authentication operation cancelled' in output
    print('confirmation', operation, mode, 'passed')
finally:
    if status is None:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
    os.close(master)
