"""Capture explicit process-controller source and native artifact bundles."""
import base64
import binascii
import json
import os
from pathlib import Path, PurePosixPath
import sys
from .kernel import KernelError
from .snapshots import digest
from .experiment_config import object_fields


def relative_name(name):
    if not isinstance(name, str) or not name or '\\' in name:
        raise KernelError('Controller artifact names must be relative POSIX paths')
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or str(path) != name or name == '.':
        raise KernelError(f'Invalid controller artifact path {name}')
    return name


def capture(value):
    object_fields(value, ('runtime', 'entry', 'files', 'arguments'), 'controller.process')
    runtime = value.get('runtime')
    if runtime not in ('python', 'native'): raise KernelError('Process runtime must be python or native')
    entry = relative_name(value.get('entry'))
    files = value.get('files')
    if not isinstance(files, dict) or entry not in files: raise KernelError('Process entry must exist in its captured files')
    arguments = value.get('arguments', [])
    if not isinstance(arguments, list) or any(not isinstance(v, str) for v in arguments):
        raise KernelError('Process arguments must be a string array')
    captured = {}
    for name, content in files.items():
        relative_name(name)
        if isinstance(content, str): data = content.encode()
        elif isinstance(content, dict) and set(content) == {'path'}:
            data = Path(content['path']).read_bytes()
        elif isinstance(content, dict) and set(content) in ({'base64'}, {'base64', 'sha256'}):
            try: data = base64.b64decode(content['base64'], validate=True)
            except (binascii.Error, ValueError) as error: raise KernelError(f'Invalid base64 artifact {name}') from error
            if 'sha256' in content and digest(data) != content['sha256']:
                raise KernelError(f'Captured controller artifact failed its content check: {name}')
        else: raise KernelError('Artifact content must be text, {path: ...}, or {base64: ...}')
        captured[name] = {'base64': base64.b64encode(data).decode(), 'sha256': digest(data)}
    # Reject paths where a file would also have to be a directory.
    for name in captured:
        if any(str(parent) in captured for parent in PurePosixPath(name).parents):
            raise KernelError(f'Conflicting controller artifact paths at {name}')
    return {'runtime': runtime, 'entry': entry, 'files': captured, 'arguments': arguments}


def prepare(controller, folder, seed):
    bundle = controller['process']
    root = Path(folder)/'controller'; root.mkdir()
    for name, artifact in bundle['files'].items():
        relative_name(name)
        data = base64.b64decode(artifact['base64'], validate=True)
        if digest(data) != artifact['sha256']: raise KernelError(f'Captured controller artifact failed its content check: {name}')
        path = root/name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(data)
    entry = root/relative_name(bundle['entry'])
    if bundle['runtime'] == 'python':
        # Isolated startup excludes live PYTHONPATH/site-packages. Imported
        # project modules and data must be supplied in the captured bundle.
        bootstrap = "import runpy,sys; root=sys.argv.pop(1);sys.path.insert(0,root);sys.argv=sys.argv[1:];runpy.run_path(sys.argv[0],run_name='__main__')"
        command = [sys.executable, '-I', '-S', '-u', '-c', bootstrap, str(root), str(entry), *bundle['arguments']]
    else:
        if os.name != 'nt': entry.chmod(0o755)
        command = [str(entry), *bundle['arguments']]
    controller['command'] = command
    controller['directory'] = str(root)
    controller['environment'] = {'LANG': 'C', 'LC_ALL': 'C', 'TZ': 'UTC',
        'SIM_PARAMETERS': json.dumps(controller.get('parameters', {}), allow_nan=False), 'SIM_SEED': str(seed)}
