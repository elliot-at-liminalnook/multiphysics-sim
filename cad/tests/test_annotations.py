"""Annotations persist, follow transforms, survive deletion, and share API/UI undo."""
import pytest
from robocad.document import Document
from robocad.commands import Ops
from robocad.kernel import KernelError
from robocad.api import ApiServer
from robocad.client import RoboClient


def model():
    d = Document()
    o = Ops(d)
    b = o.box((0,0,0), (10,20,30))
    return d,o,b


def test_thread_reply_edit_delete_save_and_undo(tmp_path):
    d,o,b = model()
    tid = o.create_thread(b, [10,10,15], "Check the wall", "Elliot")
    cid = o.add_comment(tid, "I can measure that", "Codex")
    o.update_comment(cid, "Measured: 2 mm")
    assert o.thread(tid)["comments"][1]["body"] == "Measured: 2 mm"
    o.undo()
    assert o.thread(tid)["comments"][1]["body"] == "I can measure that"
    o.redo()
    o.update_thread(tid, status="resolved")
    path = str(tmp_path / 'notes.rcad')
    d.save(path)
    loaded = Ops(Document.load(path))
    assert loaded.thread(tid)["anchor_status"] == "attached"
    assert loaded.thread(tid)["status"] == "resolved"
    assert loaded.thread(tid)["comments"][1]["author"] == "Codex"
    o.delete_comment(cid)
    assert len(o.thread(tid)["comments"]) == 1
    with pytest.raises(KernelError): o.delete_comment(o.thread(tid)["comments"][0]["id"])
    o.delete_thread(tid)
    assert not o.threads()
    o.undo()
    assert len(o.threads()) == 1


def test_translation_rotation_and_undo_move_pin():
    d,o,b = model()
    t = o.create_thread(b,[10,10,15],"Side face")
    o.transform([b],translation=(5,0,0))
    assert o.thread(t)["anchor"]["point"] == [15,10,15]
    assert o.thread(t)["anchor_status"] == "attached"
    o.transform([b],axis=(0,0,1),angle_deg=90,center=(0,0,0))
    assert o.thread(t)["anchor"]["point"] == pytest.approx([-10,15,15])
    o.undo(); o.undo()
    assert o.thread(t)["anchor"]["point"] == [10,10,15]
    assert o.thread(t)["anchor_status"] == "attached"


def test_geometry_change_and_deleted_part_keep_discussion():
    d,o,b = model()
    t = o.create_thread(b,[10,10,15],"Keep this face")
    face = next(f for f in d.kernel.faces(d.nodes[b].body) if f.normal[0] > .9)
    o.push_pull(b,face,2)
    assert o.thread(t)["anchor_status"] == "needs_review"
    o.undo()
    assert o.thread(t)["anchor_status"] == "attached"
    o.delete([b])
    assert o.thread(t)["anchor_status"] == "missing"
    assert o.thread(t)["comments"][0]["body"] == "Keep this face"
    o.undo()
    assert o.thread(t)["anchor_status"] == "attached"


def test_notes_do_not_invalidate_geometry():
    d,o,b = model()
    mesh = d.mesh_of(b)
    o.create_thread(b,[1,2,3],"Only a comment")
    assert d.mesh_of(b) is mesh


@pytest.mark.parametrize('point,body', [([float('nan'),0,0],'x'),([0,0],'x'),([0,0,0],'  ')])
def test_validation_does_not_add_partial_threads(point,body):
    d,o,b = model()
    with pytest.raises(KernelError): o.create_thread(b,point,body)
    assert not d.annotations


def test_rest_crud_errors_and_shared_undo():
    d,o,b = model()
    server = ApiServer(d, o, port=0).start()
    c = RoboClient(server.url)
    try:
        t = c.create_thread(b,[10,10,15],"Please check",author="Elliot")
        reply = c.reply(t['id'],"On it")
        assert reply['author'] == 'Codex'
        c.edit_comment(reply['id'],"Done")
        assert c.get('/comments/'+reply['id'])['body'] == 'Done'
        c.update_thread(t['id'],status='resolved')
        assert not c.threads(status='open')
        assert len(c.threads(node_id=b,status='resolved')) == 1
        c.delete_comment(reply['id'])
        c.undo()
        assert c.get('/comments/'+reply['id'])['body'] == 'Done'
        c.delete_thread(t['id'])
        assert not c.threads()
        c.undo()
        assert len(o.threads()) == 1
        with pytest.raises(RuntimeError,match='404'): c.reply('missing','x')
        with pytest.raises(RuntimeError,match='422'): c.create_thread(b,[0,0,0],'')
        with pytest.raises(RuntimeError,match='422'): c.update_thread(t['id'],status='bogus')
        with pytest.raises(RuntimeError,match='405'): c.put('/threads',{})
    finally:
        server.stop()


def test_part_references_links_persist_rename_delete_filter_and_undo(tmp_path):
    d,o,b=model(); other=o.box((20,0,0),(2,3,4))
    tid=o.create_thread(b,[0,0,0],f'The [small block](part:{other}) mates here.',part_refs=[{'node_id':other,'label':'Mating block','description':'The small block on the right'}])
    assert o.threads(node_id=other)[0]['id']==tid
    o.rename(other,'New CAD name')
    assert o.thread(tid)['linked_parts'][0]['name']=='New CAD name'
    assert o.thread(tid)['linked_parts'][0]['label']=='Mating block'
    path=str(tmp_path/'linked.rcad');d.save(path)
    assert Ops(Document.load(path)).thread(tid)['linked_parts']==o.thread(tid)['linked_parts']
    o.delete([other])
    assert not o.thread(tid)['linked_parts'][0]['available']
    o.undo(); assert o.thread(tid)['linked_parts'][0]['available']
    before=d.revision
    with pytest.raises(KernelError):o.update_thread(tid,part_refs=['missing'])
    assert d.revision==before


def test_inline_part_links_alone_are_discoverable_and_legacy_anchor_survives():
    d,o,b=model(); other=o.box((30,0,0),(3,3,3))
    tid=o.create_thread(b,[0,0,0],'Original')
    o.add_comment(tid,f'Look at [this block](part:{other}).')
    assert {r['node_id'] for r in o.thread(tid)['linked_parts']}=={b,other}
    assert o.threads(node_id=other)[0]['id']==tid

def test_unchanged_body_annotation_refresh_reuses_fingerprint(monkeypatch):
    d,o,b = model()
    t = o.create_thread(b, [10,10,15], 'Side face')
    original = d.kernel.mass_properties
    calls = []
    def measured(body):
        calls.append(body)
        return original(body)
    monkeypatch.setattr(d.kernel, 'mass_properties', measured)
    assert o.thread(t)['anchor_status'] == 'attached'
    o.threads(); o.add_comment(t, 'A reply'); o.threads()
    assert not calls
    o.transform([b], translation=(2,0,0))
    assert calls
    assert o.thread(t)['anchor_status'] == 'attached'
    o.undo(); o.undo()
    assert o.thread(t)['anchor_status'] == 'attached'
