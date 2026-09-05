import zipfile
import io
import json
from robocad.document import Document
from robocad.commands import Ops
from robocad.snapshots import capture


def test_capture_unsaved_state_is_repeatable_and_does_not_save(tmp_path):
    doc=Document(); ops=Ops(doc)
    part=ops.box((0,0,0),(10,20,30))
    before=(doc.path,doc.dirty,doc.revision,len(ops.stack.undo_stack))
    first=capture(doc); second=capture(doc)
    assert first.data==second.data
    assert first.physical_hash==second.physical_hash
    assert (doc.path,doc.dirty,doc.revision,len(ops.stack.undo_stack))==before
    path=tmp_path/'snapshot.rcad'; path.write_bytes(first.data)
    loaded=Document.load(str(path))
    assert loaded.document_id==doc.document_id and loaded.revision==doc.revision
    assert loaded.kernel.mass_properties(loaded.nodes[part].body).volume==6000
    assert capture(loaded).physical_hash == first.physical_hash


def test_review_edits_reuse_physical_identity_and_geometry_changes_do_not():
    doc=Document(); ops=Ops(doc); part=ops.box((0,0,0),(10,20,30))
    first=capture(doc)
    ops.create_thread(part,[10,5,5],'Check clearance')
    second=capture(doc)
    assert second.revision>first.revision
    assert second.archive_hash!=first.archive_hash
    assert second.physical_hash==first.physical_hash
    ops.transform([part],scale=1.1)
    third=capture(doc)
    assert third.physical_hash!=first.physical_hash
    ops.undo()
    assert capture(doc).physical_hash==first.physical_hash


def test_loaded_results_are_not_included_in_captured_inputs():
    doc=Document(); ops=Ops(doc); part=ops.box((0,0,0),(10,20,30))
    doc.results={'trace':{'t':[0,1]},'stale':False}
    doc.nodes[part].results={'peak_current_a':1}
    snapshot=capture(doc)
    with zipfile.ZipFile(io.BytesIO(snapshot.data)) as archive:
        m=json.loads(archive.read('manifest.json'))
    assert m['results'] is None and 'results' not in m['nodes'][0]
    ops.transform([part],scale=1.1)
    assert doc.results['stale']
