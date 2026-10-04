#!/usr/bin/env python3
"""one_at_a_time.py LOCKFILE COMMAND [ARGS...]: run COMMAND while holding an
exclusive lock on LOCKFILE, so commands sharing it run one after another.

Used by shims/cargo. While another command holds the lock it says so once on
stderr and waits. The lock is released when the command exits, however it
exits, because it belongs to this process's open file.
"""
import fcntl
import os
import signal
import subprocess
import sys
import time


def main():
    if len(sys.argv) < 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    lockfile, command = sys.argv[1], sys.argv[2:]
    os.makedirs(os.path.dirname(os.path.abspath(lockfile)), exist_ok=True)
    with open(lockfile, "a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            started = time.monotonic()
            print("cargo: another cargo command in this workspace is running; waiting for it to finish "
                  "(builds run one at a time so they reuse each other's work)", file=sys.stderr, flush=True)
            fcntl.flock(lock, fcntl.LOCK_EX)
            print(f"cargo: waited {time.monotonic() - started:.0f} s; starting", file=sys.stderr, flush=True)
        process = subprocess.Popen(command)
        forward = lambda signum, _frame: process.send_signal(signum)
        for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(sig, forward)
        return process.wait()


if __name__ == "__main__":
    sys.exit(main())
