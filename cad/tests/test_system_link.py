"""A CAD document references a hierarchical system file by path and hash:
linking is undoable, edits to the system file are detected, and the physical
export carries the reference (never a copy of the system)."""
import json
import os
import shutil

import pytest

from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import KernelError, Plane
from robocad.physical import export_physical_model

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
BOARD = os.path.join(ROOT, 'examples', 'systems-builder', 'motor-driver-board', 'board.system.json')


def test_link_detects_changes_is_undoable_and_exports_a_reference(tmp_path):
    system = tmp_path / 'board.system.json'
    shutil.copy(BOARD, system)
    doc = Document()
    doc.path = str(tmp_path / 'robot.rcad')
    ops = Ops(doc)
    assert ops.system_status()['state'] == 'unlinked'
    link = ops.link_system(str(system))
    assert link['path'] == 'board.system.json', 'stored relative to the CAD file'
    assert link['title'] == 'Motor driver board' and link['definitions'] >= 4
    assert ops.system_status()['state'] == 'current'

    data = json.loads(system.read_text())
    data['revision'] += 1
    data['title'] = 'Edited board'
    system.write_text(json.dumps(data))
    st = ops.system_status()
    assert st['state'] == 'changed' and st['now']['title'] == 'Edited board'
    ops.refresh_system_link()
    assert ops.system_status()['state'] == 'current'

    ops.stack.undo()  # back to the original link
    assert ops.system_status()['state'] == 'changed'
    ops.stack.undo()  # before linking
    assert ops.system_status()['state'] == 'unlinked'
    ops.stack.redo()
    assert doc.robot_settings['system']['path'] == 'board.system.json'

    body = ops.box((0, 0, 0), (10, 10, 10))
    ops.doc.nodes[body].name = 'ground'
    out = tmp_path / 'robot.simrobot.json'
    export_physical_model(doc, str(out), flex=False)
    exported = json.loads(out.read_text())
    assert exported['system']['sha256'] == link['sha256']
    assert 'definitions' not in exported['system'] or isinstance(exported['system']['definitions'], int)

    system.unlink()
    assert ops.system_status()['state'] == 'missing'


def test_rejects_files_that_are_not_system_files(tmp_path):
    other = tmp_path / 'not.json'
    other.write_text('{"format": "simrobot"}')
    ops = Ops(Document())
    with pytest.raises(KernelError):
        ops.link_system(str(other))
