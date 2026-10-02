"""Unix descriptor-relative parity I/O; path swaps cannot redirect publication.

Only headless parity uses this helper. This is not a general CAD sandbox.
"""
from contextlib import contextmanager
from pathlib import Path
import os
import stat
import uuid

MAX_FILE = 64 * 1024 * 1024


@contextmanager
def directory(path):
    if os.name != 'posix' or not hasattr(os, 'O_NOFOLLOW'):
        raise ValueError('Parity owned I/O requires Unix no-follow directory capabilities')
    absolute = Path(os.path.abspath(path))
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for part in absolute.parts[1:]:
            if part in ('', '.', '..'):
                raise ValueError('Unsafe directory component')
            try:
                next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            except OSError as error:
                raise ValueError('Unsafe or missing directory component: ' + part) from error
            os.close(fd)
            fd = next_fd
        yield fd
    finally:
        os.close(fd)


def read_regular(path, limit=MAX_FILE):
    path = Path(path)
    if not path.name or path.name in ('.', '..'):
        raise ValueError('Require regular named file')
    with directory(path.parent) as parent:
        fd = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=parent)
        with os.fdopen(fd, 'rb') as stream:
            metadata = os.fstat(stream.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
                raise ValueError('Require bounded regular file')
            value = stream.read(limit + 1)
            if len(value) > limit:
                raise ValueError('File grew past read bound')
            return value


def publish_new(path, value):
    import json
    path = Path(path)
    if not path.name or path.name in ('.', '..'):
        raise ValueError('Require named owned output')
    with directory(path.parent) as parent:
        temporary = '.parity-' + uuid.uuid4().hex
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=parent)
        try:
            with os.fdopen(fd, 'w', encoding='utf-8') as stream:
                json.dump(value, stream, sort_keys=True, allow_nan=False)
                stream.flush()
                os.fsync(stream.fileno())
            os.link(temporary, path.name, src_dir_fd=parent, dst_dir_fd=parent, follow_symlinks=False)
            os.fsync(parent)
        finally:
            os.unlink(temporary, dir_fd=parent)
