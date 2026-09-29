import pytest

from robocad import model_scripts
from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.document import Document

SCRIPT = '''
def build(ops, params):
    size = params.get("size", 10)
    a = ops.box((0, 0, 0), (size, size, size), name="Block")
    b = ops.cylinder((0, 0, size), (0, 0, 1), 2, 5, name="Peg")
    return {"block": a, "peg": b}
'''


def test_model_script_runs_as_one_undo_step_and_replaces_its_previous_run(tmp_path, monkeypatch):
    monkeypatch.setattr(model_scripts, 'REPO', tmp_path)
    (tmp_path/'model.py').write_text(SCRIPT)
    (tmp_path/'broken.py').write_text('def build(ops, params):\n    ops.box((0, 0, 0), (1, 1, 1))\n    raise ValueError("bad size")\n')
    doc = Document(); server = ApiServer(doc, port=0); server.start()
    client = RoboClient(server.url)
    try:
        first = client.script('model.py', {'size': 10})
        assert first['created'] == 2 and first['script'] == 'model.py'
        assert doc.robot_settings['model_scripts']['model.py']['params'] == {'size': 10}
        second = client.script('model.py', {'size': 20})
        assert sorted(second['removed']) == sorted(first['result'].values())
        names = sorted(n.name for n in doc.nodes.values())
        assert names == ['Block', 'Peg']  # replaced, not duplicated
        assert doc.kernel.mass_properties(doc.nodes[second['result']['block']].body).volume == pytest.approx(8000)
        client.undo()  # back to the first run in one step
        assert set(doc.nodes) == set(first['result'].values())
        revision = doc.revision
        with pytest.raises(RuntimeError, match=r'broken.py:3: ValueError: bad size'):
            client.script('broken.py')
        assert doc.revision == revision and set(doc.nodes) == set(first['result'].values())
        outside = tmp_path.parent/f'{tmp_path.name}-outside.py'; outside.write_text(SCRIPT)
        with pytest.raises(RuntimeError, match='inside the repository'):
            client.script(str(outside))
    finally:
        server.stop()
