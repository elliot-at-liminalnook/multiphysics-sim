"""Process leases and atomic run records shared by editors and workers."""
import json
import os
from pathlib import Path
import time
import uuid
from .snapshots import canonical

TERMINAL = {'completed', 'failed', 'cancelled'}


class Lease:
    def __init__(self, file): self.file = file

    @classmethod
    def acquire(cls, path, blocking=False):
        file = Path(path).open('a+b')
        try:
            if os.name == 'nt':
                import msvcrt
                if file.tell() == 0: file.write(b'\0'); file.flush()
                file.seek(0)
                msvcrt.locking(file.fileno(), msvcrt.LK_LOCK if blocking else msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(file.fileno(), fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
        except (BlockingIOError, PermissionError):
            file.close(); return None
        return cls(file)

    def close(self):
        # Closing releases this reference. A worker inherits the same open file
        # description, keeping its lease alive if the editor crashes.
        self.file.close()


def update(folder, **fields):
    folder = Path(folder)
    lease = Lease.acquire(folder/'record.lock', blocking=True)
    try:
        record = json.loads((folder/'run.json').read_text())
        if record['state'] in TERMINAL: return record
        if record['state'] == 'cancelling' and fields.get('state') not in (None, 'cancelling', 'cancelled'):
            return record
        record.update(fields); record['updated_at'] = time.time()
        temporary = folder/('run.'+uuid.uuid4().hex+'.tmp')
        temporary.write_bytes(canonical(record)); os.replace(temporary, folder/'run.json')
        return record
    finally: lease.close()


def observe(folder):
    folder = Path(folder)
    record = json.loads((folder/'run.json').read_text())
    if record['state'] in TERMINAL: return record
    lease = Lease.acquire(folder/'owner.lock')
    if lease is None: return record
    try:
        return update(folder, state='failed', stage='interrupted',
                      error='The editor and worker exited before finalizing this run. Captured inputs and partial outputs are retained; restore inputs to start a new run.')
    finally: lease.close()
