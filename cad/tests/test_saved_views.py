import pytest

from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import KernelError
from robocad.api import ApiServer
from robocad.client import RoboClient


CUT = {'target': [130, 0, 8], 'distance': 330, 'yaw': 90, 'pitch': 5,
       'orthographic': True, 'section': {'enabled': True, 'plane': {
           'origin': [0, 0, 0], 'normal': [0, 1, 0], 'x_axis': [1, 0, 0]}}}


def test_saved_views_roundtrip_undo_and_no_geometry_invalidation(tmp_path):
    doc = Document(); ops = Ops(doc)
    body = ops.box((0,0,0), (2,3,4)); mesh = doc.mesh_of(body)
    doc.results = {'stale': False}
    events = []; doc.listeners.append(lambda event, payload: events.append(event))
    before = doc.revision
    vid = ops.save_view(' Worm cutaway ', CUT)
    assert doc.revision == before + 1
    assert doc.mesh_of(body) is mesh and not doc.results['stale']
    assert events == ['saved_views']
    ops.update_saved_view(vid, name='Worm drive')
    ops.update_saved_view(vid, state={'distance': 500})
    ops.undo()
    assert doc.saved_views[vid]['state']['section']['enabled']
    path = str(tmp_path/'views.rcad'); doc.save(path)
    loaded = Document.load(path)
    assert loaded.saved_views == doc.saved_views
    ops.delete_saved_view(vid)
    assert not doc.saved_views
    ops.undo(); assert doc.saved_views[vid]['name'] == 'Worm drive'
    ops.redo(); assert not doc.saved_views


@pytest.mark.parametrize('state', [
    {'distance': -1}, {'yaw': float('inf')}, {'pitch': 90}, {'target': [1,2]},
    {'section': {'enabled': True}}, {'display_mode': 'bad'}, {'grid': 1},
    {'section': {'enabled': True, 'plane': {'origin':[0,0,0], 'normal':[0,0,0], 'x_axis':[1,0,0]}}},
    {'section': {'enabled': True, 'plane': {'origin':[0,0,0], 'normal':[1,0,0], 'x_axis':[1,0,0]}}},
])
def test_invalid_view_is_atomic(state):
    doc = Document(); ops = Ops(doc)
    with pytest.raises(KernelError): ops.save_view('Invalid', state)
    assert not doc.saved_views and doc.revision == 0


def test_saved_view_rest_crud_and_headless_errors():
    doc = Document()
    server = ApiServer(doc, port=0); server.start()
    client = RoboClient(server.url)
    try:
        row = client.post('/views', {'name':'Cutaway', 'state':CUT}); vid = row['id']
        assert client.get('/views')[0] == row
        client.patch('/views/'+vid, {'name':'Gear cutaway'})
        assert client.get('/views/'+vid)['name'] == 'Gear cutaway'
        with pytest.raises(RuntimeError, match='409'): client.post('/views/'+vid+'/restore')
        with pytest.raises(RuntimeError, match='409'): client.post('/views', {'name':'No desktop'})
        with pytest.raises(RuntimeError, match='422'): client.post('/views', {'name':'', 'state':CUT})
        client.delete('/views/'+vid)
        with pytest.raises(RuntimeError, match='404'): client.get('/views/'+vid)
        client.undo(); assert client.get('/views/'+vid)['name'] == 'Gear cutaway'
    finally:
        server.stop()
