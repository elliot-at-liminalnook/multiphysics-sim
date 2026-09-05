import pytest

from robocad.commands import Ops
from robocad.document import Document, write_archive


def test_loaded_geometry_cache_tracks_geometry_edits_and_undo(tmp_path, monkeypatch):
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (10, 10, 10))
    path = str(tmp_path / 'robot.rcad'); doc.save(path)
    doc = Document.load(path); ops = Ops(doc)
    original = doc.kernel.serialize; calls = []
    def serialize(value):
        calls.append(value)
        return original(value)
    monkeypatch.setattr(doc.kernel, 'serialize', serialize)
    ops.rename(body, 'Renamed'); doc.save(path)
    assert not calls
    ops.transform([body], translation=(10, 0, 0)); doc.save(path)
    assert len(calls) == 1
    moved = Document.load(path)
    assert moved.kernel.mass_properties(moved.nodes[body].body).centroid[0] == pytest.approx(15)
    ops.undo(); doc.save(path)
    restored = Document.load(path)
    assert restored.kernel.mass_properties(restored.nodes[body].body).centroid[0] == pytest.approx(5)


def test_failed_archive_write_preserves_previous_save(tmp_path, monkeypatch):
    import zipfile
    path = tmp_path / 'robot.rcad'
    write_archive(path, [('manifest.json', b'original')])
    before = path.read_bytes()
    def fail(*args, **kwargs):
        raise OSError('disk full')
    monkeypatch.setattr(zipfile.ZipFile, 'writestr', fail)
    with pytest.raises(OSError, match='disk full'):
        write_archive(path, [('manifest.json', b'new')])
    assert path.read_bytes() == before
    assert list(tmp_path.iterdir()) == [path]
