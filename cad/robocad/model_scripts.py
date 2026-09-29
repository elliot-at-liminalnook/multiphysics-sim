"""Run a repository model script against the live document as one undo step.

A model script is a Python file in the repository that defines::

    def build(ops, params):   # ops: robocad.commands.Ops on a staged copy
        ...                    # returns a JSON-serialisable summary (optional)

``run_script`` stages the script on a snapshot (off the UI thread), records
what it created, and publishes the result with the same ``PublishState`` step
as an atomic edit batch, so a failed script never touches the live document.
Re-running the same script with ``replace`` removes the nodes its previous run
created first, so editing the script (or its parameters) and running it again
updates the model in place. Each run is recorded in
``robot_settings['model_scripts'][path]`` with the script's SHA-256, the
parameters and the created node ids, so a saved document says how it was made.

Only ``.py`` files inside the repository can be run: the REST surface can name
a script, not supply code.
"""
from __future__ import annotations

import hashlib
import io
import json
import time
import traceback
from copy import deepcopy
from pathlib import Path

from .commands import Ops
from .document import Document
from .kernel import KernelError

REPO = Path(__file__).resolve().parents[2]
SETTING = 'model_scripts'


def resolve(path: str) -> tuple[Path, str]:
    """The script file and its repository-relative key; refuses anything outside the repository."""
    if not isinstance(path, str) or not path:
        raise KernelError('A model script run needs a path')
    p, root = Path(path), REPO.resolve()
    full = (p if p.is_absolute() else root / p).resolve()
    if full.suffix != '.py' or not full.is_file():
        raise KernelError(f'{path}: not a Python file')
    try:
        key = full.relative_to(root).as_posix()
    except ValueError:
        raise KernelError(f'{path}: model scripts must live inside the repository ({REPO})') from None
    return full, key


def _script_error(path: Path, error: Exception) -> KernelError:
    frames = [f for f in traceback.extract_tb(error.__traceback__) if Path(f.filename).resolve() == path]
    shown = path.relative_to(REPO.resolve()).as_posix()
    where = f'{shown}:{frames[-1].lineno}' if frames else shown
    line = f' ({frames[-1].line})' if frames and frames[-1].line else ''
    return KernelError(f'{where}: {type(error).__name__}: {error}{line}')


def stage(snapshot, path: str, params: dict | None, replace: bool = True):
    """Run the script on a copy of ``snapshot``; return (staged document, summary)."""
    full, key = resolve(path)
    params = dict(params or {})
    json.dumps(params)  # parameters are recorded; they must be plain JSON
    source = full.read_bytes()
    doc = Document.load(io.BytesIO(snapshot.data)); doc.path = None
    ops = Ops(doc)
    runs = deepcopy(doc.robot_settings.get(SETTING) or {})
    removed = []
    if replace and key in runs:
        previous = [n for n in runs[key].get('nodes', []) if n in doc.nodes]
        tops = [n for n in previous if doc.nodes[n].parent not in previous]
        if tops:
            ops.delete(tops)
            removed = tops
    before = set(doc.nodes)
    namespace = {'__name__': 'robocad_model_script', '__file__': str(full)}
    started = time.perf_counter()
    try:
        exec(compile(source, str(full), 'exec'), namespace)
        build = namespace.get('build')
        if not callable(build):
            raise KernelError(f'{key}: define build(ops, params)')
        result = build(ops, params)
        json.dumps(result)
    except KernelError as error:
        if str(error).startswith(key):
            raise
        raise _script_error(full, error) from error
    except Exception as error:
        raise _script_error(full, error) from error
    created = [n for n in doc.nodes if n not in before]
    runs[key] = {'sha256': hashlib.sha256(source).hexdigest(), 'params': params, 'nodes': created,
                 'seconds': round(time.perf_counter() - started, 3)}
    doc.robot_settings[SETTING] = runs
    return doc, {'script': key, 'sha256': runs[key]['sha256'], 'params': params, 'created': len(created),
                 'removed': removed, 'seconds': runs[key]['seconds'], 'result': result}
