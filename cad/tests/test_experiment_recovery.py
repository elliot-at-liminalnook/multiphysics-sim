import json
from pathlib import Path
import pytest
from robocad.experiments import Experiments
from robocad.kernel import KernelError


def test_observers_refresh_live_records_without_cancelling_another_editor(tmp_path, monkeypatch):
    binary = tmp_path/'runner'; binary.write_bytes(b'test runner')
    owner = Experiments(root=tmp_path/'runs', binary=binary)
    monkeypatch.setattr(owner, '_run', lambda run_id: None)
    job = owner.create({'system': 'let a = 1;'})
    observer = Experiments(root=tmp_path/'runs', binary=binary)
    assert observer.get(job['id'])['state'] == 'queued'
    with pytest.raises(KernelError, match='another live editor'):
        observer.cancel(job['id'])
    observer.close()
    assert owner.get(job['id'])['state'] == 'queued'
    owner._update(job['id'], state='running', fraction=.4)
    assert observer.get(job['id'])['fraction'] == .4
    owner.cancel(job['id'])
    assert observer.get(job['id'])['state'] == 'cancelled'


def test_abandoned_queued_run_becomes_failed_and_retains_inputs(tmp_path, monkeypatch):
    binary = tmp_path/'runner'; binary.write_bytes(b'test runner')
    owner = Experiments(root=tmp_path/'runs', binary=binary)
    monkeypatch.setattr(owner, '_run', lambda run_id: None)
    job = owner.create({'system': 'let a = 1;'})
    # Equivalent to OS cleanup after the owning process exits: no lease holder.
    owner.leases.pop(job['id']).close()
    observer = Experiments(root=tmp_path/'runs', binary=binary)
    recovered = observer.get(job['id'])
    assert recovered['state'] == 'failed' and recovered['stage'] == 'interrupted'
    assert 'restore inputs' in recovered['error']
    assert observer.inputs(job['id'])['system']['files']['system.rhai'] == 'let a = 1;'
    assert json.loads((Path(job['directory'])/'run.json').read_text())['state'] == 'failed'
