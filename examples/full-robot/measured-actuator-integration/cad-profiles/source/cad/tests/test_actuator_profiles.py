"""The real CAD command, native resolver, archive and REST boundary agree."""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess

import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import KernelError
from robocad.physical import export_physical_model

ROOT = Path(__file__).resolve().parents[2]
TOOL = Path(os.environ.get('ROBOCAD_ACTUATOR_PROFILE_TOOL', str(ROOT / 'target/debug/sim-actuator-profiles')))
FAMILY = ROOT / 'examples/full-robot/measured-actuator-integration/cad-profiles/hx30hm-provisional-family.json'


@pytest.fixture
def authored(monkeypatch):
    assert TOOL.is_file(), 'Build sim-actuator-profiles before running CAD integration acceptance'
    monkeypatch.setenv('ROBOCAD_ACTUATOR_PROFILE_TOOL', str(TOOL))
    doc = Document()
    ops = Ops(doc)
    base = ops.box((-20, -20, 0), (40, 40, 10), name='base')
    link = ops.box((-5, -5, 10), (10, 10, 80), name='link')
    ops.set_ground(base)
    motor = ops.add_motor('hx30hm', (0, 0, 10), (0, 1, 0), mount_on=base, cut_mount=False)
    joint = ops.add_joint('revolute', base, link, (0, 0, 10), (0, 1, 0), name='axis')
    ops.attach_motor(joint, motor)
    profiles = {'version': 1, 'families': {'hx30hm': json.loads(FAMILY.read_text())},
                'bindings': {motor: {'family': 'hx30hm', 'version': 1,
                                     'physical_unit': None, 'deviations': {}}}}
    return doc, ops, profiles


def test_archive_export_and_native_resolution(authored, tmp_path):
    doc, ops, profiles = authored
    expected = deepcopy(profiles)
    returned = ops.set_actuator_profiles(profiles)
    returned['families'].clear()
    profiles['families']['hx30hm']['motor']['resistance']['value'] = 99
    assert doc.robot_settings['actuator_profiles'] == expected
    assert ops.undo() == 'Edit actuator profiles'
    assert 'actuator_profiles' not in doc.robot_settings
    assert ops.redo() == 'Edit actuator profiles'
    doc.save(str(tmp_path / 'profile.rcad'))
    restored = Document.load(str(tmp_path / 'profile.rcad'))
    assert restored.robot_settings['actuator_profiles'] == expected
    model = export_physical_model(restored, flex=False)
    assert model['actuator_profiles'] == expected
    receipt = subprocess.run([str(TOOL)], input=json.dumps(model), text=True,
                             capture_output=True, check=True)
    resolved = json.loads(receipt.stdout)
    motor = next(iter(expected['bindings']))
    for group in ('motor', 'driver'):
        assert resolved['resolved'][motor][group] == {
            k: p['value'] for k, p in expected['families']['hx30hm'][group].items()}
    assert resolved['calibrated'] is False


def test_invalid_edits_are_atomic_and_export_catches_bypass(authored):
    doc, ops, profiles = authored
    ops.set_actuator_profiles(profiles)
    original = deepcopy(doc.robot_settings)
    revision = doc.revision
    invalid = deepcopy(profiles)
    invalid['families']['hx30hm']['motor']['resistance']['unit'] = 'A'
    with pytest.raises(KernelError, match='unit'):
        ops.set_actuator_profiles(invalid)
    assert doc.robot_settings == original and doc.revision == revision
    invalid = deepcopy(profiles)
    invalid['bindings']['missing-motor'] = invalid['bindings'].pop(next(iter(invalid['bindings'])))
    with pytest.raises(KernelError, match='CAD motor'):
        ops.set_actuator_profiles(invalid)
    assert doc.robot_settings == original and doc.revision == revision
    # Legacy generic settings cannot evade validation at export.
    doc.robot_settings['actuator_profiles'] = invalid
    with pytest.raises(KernelError):
        export_physical_model(doc, flex=False)


def test_rest_uses_same_validation_and_explicit_removal(authored):
    doc, _, profiles = authored
    server = ApiServer(doc, port=0).start()
    try:
        client = RoboClient(server.url)
        assert client.post('/actuator-profiles', {'profiles': profiles}) == profiles
        assert client.get('/actuator-profiles') == profiles
        with pytest.raises(RuntimeError):
            client.post('/actuator-profiles', {})
        assert client.get('/actuator-profiles') == profiles
        invalid = deepcopy(profiles)
        invalid['families']['hx30hm']['motor']['resistance']['unit'] = 'A'
        with pytest.raises(RuntimeError):
            client.post('/actuator-profiles', {'profiles': invalid})
        assert client.get('/actuator-profiles') == profiles
        assert client.post('/actuator-profiles', {'profiles': None}) is None
        assert client.undo()['undone'] == 'Edit actuator profiles'
        assert client.get('/actuator-profiles') == profiles
    finally:
        server.stop()
