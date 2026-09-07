"""Drive lost motion has explicit provenance, independent of bearing inference."""
import copy
import pytest
from robocad.commands import Ops
from robocad.document import Document
from robocad.physical import inspect_joint_physics, validate_drive_backlash


def test_drive_override_is_validated_before_undoable_mutation():
    doc = Document()
    ops = Ops(doc)
    a = ops.box((0, 0, 0), (10, 10, 10), name='base')
    b = ops.box((0, 0, 10), (10, 10, 20), name='arm')
    jid = ops.add_joint('revolute', a, b, (5, 5, 10), (0, 0, 1), name='pivot')
    initial = inspect_joint_physics(doc, jid)
    assert initial['drive_backlash']['width_rad'] is None
    assert initial['clearance'] > 0
    authored = {'width_rad': 0.012, 'provenance': 'measured', 'reference': 'reversal.csv', 'uncertainty_rad': 0.002}
    ops.set_joint_physics(jid, drive_backlash=authored)
    result = inspect_joint_physics(doc, jid)
    assert result['drive_backlash'] == authored
    assert result['clearance'] == initial['clearance']
    assert result['bearing_clearance_angle_rad'] == initial['bearing_clearance_angle_rad']
    before = copy.deepcopy(doc.nodes[jid].robot)
    for bad in [dict(authored, width_rad=-1), dict(authored, width_rad=True), dict(authored, width_rad=float('nan')),
                dict(authored, reference=''), dict(authored, uncertainty_rad=-1), dict(authored, provenance='unmeasured')]:
        with pytest.raises(Exception, match='drive_backlash'):
            ops.set_joint_physics(jid, drive_backlash=bad)
        assert doc.nodes[jid].robot == before
    ops.stack.undo()
    assert inspect_joint_physics(doc, jid)['drive_backlash']['width_rad'] is None
    ops.stack.redo()
    assert inspect_joint_physics(doc, jid)['drive_backlash'] == authored
    # Compatibility setter expresses an explicit estimate, not geometry.
    ops.set_joint_physics(jid, backlash=0.03)
    result = inspect_joint_physics(doc, jid)
    assert result['drive_backlash']['width_rad'] == 0.03
    assert result['drive_backlash']['provenance'] == 'estimated'


def test_unknown_is_distinct_from_an_explicit_zero():
    unknown = {'width_rad': None, 'provenance': 'unmeasured', 'reference': 'not measured'}
    assert validate_drive_backlash(unknown) == unknown
    assert validate_drive_backlash(dict(unknown, width_rad=0, provenance='estimated'))['width_rad'] == 0
    for bad in [dict(unknown, width_rad=0), dict(unknown, provenance='measured'), dict(unknown, unrecognized=1)]:
        with pytest.raises(Exception, match='drive_backlash'):
            validate_drive_backlash(bad)


def test_rest_edit_supersedes_old_fit_and_undo_restores_its_evidence():
    from robocad.api import ApiServer
    from robocad.client import RoboClient
    doc = Document(); ops = Ops(doc)
    a = ops.box((0, 0, 0), (10, 10, 10))
    b = ops.box((0, 0, 10), (10, 10, 20))
    jid = ops.add_joint('revolute', a, b, (5, 5, 10), (0, 0, 1), name='pivot')
    doc.robot_settings['identification'] = {'pivot': {'backlash': .02, 'source_log': 'reversal.csv'}}
    assert inspect_joint_physics(doc, jid)['drive_backlash']['provenance'] == 'derived'
    server = ApiServer(doc, port=0).start()
    try:
        client = RoboClient(server.url)
        value = {'width_rad': .01, 'provenance': 'measured', 'reference': 'new-reversal.csv'}
        assert client.set_joint_physics(jid, drive_backlash=value)['drive_backlash'] == value
        assert inspect_joint_physics(doc, jid)['drive_backlash'] == value
        assert doc.robot_settings['identification']['pivot']['superseded_backlash'][0]['width_rad'] == .02
        client.post('/undo', {})
        assert inspect_joint_physics(doc, jid)['drive_backlash']['width_rad'] == .02
        assert doc.robot_settings['identification']['pivot']['source_log'] == 'reversal.csv'
    finally:
        server.stop()
