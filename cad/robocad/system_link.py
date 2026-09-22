"""Link a CAD document to a hierarchical system file (`sim.system/1`).

The system file owns circuit and subsystem topology; CAD owns geometry and
physical parts. The CAD document stores a durable reference (path relative
to the .rcad when possible, SHA-256 of the file, title and revision), never a
copy, and reports when the linked file has changed since it was linked.
"""
import hashlib
import json
import os
import time

from .kernel import KernelError

SCHEMA = 'sim.system/1'


def read(path: str) -> dict:
    """Validate the file's schema and summarize it."""
    try:
        with open(path, 'rb') as f:
            data = f.read()
    except OSError as e:
        raise KernelError(f'Cannot read system file: {e}') from e
    try:
        doc = json.loads(data)
    except ValueError as e:
        raise KernelError(f'{os.path.basename(path)} is not JSON: {e}') from e
    if not isinstance(doc, dict) or doc.get('schema') != SCHEMA:
        raise KernelError(f'{os.path.basename(path)} is not a {SCHEMA} system file')
    definitions = doc.get('definitions') or {}
    root = definitions.get(doc.get('root'), {})
    return {
        'sha256': hashlib.sha256(data).hexdigest(),
        'title': doc.get('title', ''),
        'revision': int(doc.get('revision', 0)),
        'definitions': len(definitions),
        'instances': len(root.get('instances', {})),
    }


def stored_path(doc_path, system_path: str) -> str:
    """Relative to the CAD file when both share a tree, else absolute."""
    system_path = os.path.abspath(system_path)
    if doc_path:
        try:
            rel = os.path.relpath(system_path, os.path.dirname(os.path.abspath(doc_path)))
            if not rel.startswith('..' + os.sep + '..' + os.sep + '..'):
                return rel
        except ValueError:
            pass
    return system_path


def resolve(doc_path, stored: str) -> str:
    if os.path.isabs(stored) or not doc_path:
        return stored
    return os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(doc_path)), stored))


def make_link(doc_path, system_path: str) -> dict:
    summary = read(system_path)
    return {'path': stored_path(doc_path, system_path), **summary, 'linked': time.strftime('%Y-%m-%dT%H:%M:%S')}


def status(doc_path, link) -> dict:
    """'current', 'changed' (file edited since linking) or 'missing'."""
    if not link:
        return {'state': 'unlinked'}
    path = resolve(doc_path, link['path'])
    try:
        now = read(path)
    except KernelError as e:
        return {'state': 'missing', 'path': path, 'error': str(e), 'link': link}
    state = 'current' if now['sha256'] == link.get('sha256') else 'changed'
    return {'state': state, 'path': path, 'link': link, 'now': now}
