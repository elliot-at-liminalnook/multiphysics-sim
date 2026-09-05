from pathlib import Path
import json
from zipfile import ZipFile
import pytest

import robocad.experiments as experiments
from robocad.snapshots import digest


def test_queued_run_captures_binary_and_derivation_sources_before_execution(tmp_path, monkeypatch):
    package = tmp_path/'source'/'robocad'; package.mkdir(parents=True)
    source = package/'experiments.py'; source.write_text('VERSION = 1\n')
    monkeypatch.setattr(experiments, '__file__', str(source))
    executable = tmp_path/'sim-experiment'; executable.write_bytes(b'binary version one')
    manager = experiments.Experiments(root=tmp_path/'runs', binary=executable)
    monkeypatch.setattr(manager, '_run', lambda run_id: None)
    first = manager.create({'system': 'let a = 1;'})
    source.write_text('VERSION = 2\n'); executable.write_bytes(b'binary version two')
    second = manager.create({'system': 'let a = 1;'})
    a, b = first['runner'], second['runner']
    assert Path(a['binary']).read_bytes() == b'binary version one'
    assert Path(b['binary']).read_bytes() == b'binary version two'
    with ZipFile(a['python']) as bundle: assert bundle.read('robocad/experiments.py') == b'VERSION = 1\n'
    with ZipFile(b['python']) as bundle: assert bundle.read('robocad/experiments.py') == b'VERSION = 2\n'
    assert first['provenance']['binary_hash'] == experiments.binary_digest(b'binary version one')
    # A corrupted shared artifact is repaired on the next identical capture.
    Path(b['binary']).write_bytes(b'tampered')
    third = manager.create({'system': 'let a = 1;'})
    assert Path(third['runner']['binary']).read_bytes() == b'binary version two'
    captured = manager.inputs(first['id'])
    assert captured['provenance']['derivation']['sources']['experiments.py'] == digest(b'VERSION = 1\n')
    manager.close()


@pytest.mark.parametrize('changed,diagnostic', [
    ('dependency', 'installed dependency versions changed'),
    ('source', 'Captured derivation sources'),
    ('interpreter', 'Python interpreter version changed'),
    ('binary', 'Captured simulator binary failed its content check'),
])
def test_worker_rejects_changed_runtime_identity_before_invoking_simulator(tmp_path, monkeypatch, changed, diagnostic):
    executable = tmp_path/'sim-experiment'; executable.write_bytes(b'not an executable')
    manager = experiments.Experiments(root=tmp_path/'runs', binary=executable)
    execute = manager._run
    monkeypatch.setattr(manager, '_run', lambda run_id: None)
    try:
        job = manager.create({'system': 'let a = 1;'})
        path = manager.root/job['id']/'input.json'
        spec = json.loads(path.read_text())
        provenance = spec['provenance']
        if changed == 'dependency': provenance['derivation']['packages']['numpy'] = 'changed'
        elif changed == 'source': provenance['derivation']['sources']['physical.py'] = 'changed'
        elif changed == 'interpreter': provenance['python_version'] = 'changed'
        else: Path(job['runner']['binary']).write_bytes(b'changed executable')
        experiments.write_json(path, spec)
        execute(job['id'])
        record = manager.get(job['id'])
        assert record['state'] == 'failed'
        assert diagnostic in record['error']
        assert not (path.parent/'result.json').exists()
    finally:
        manager.close()
