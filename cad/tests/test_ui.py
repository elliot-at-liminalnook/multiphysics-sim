"""UI smoke tests, offscreen: the window builds, commands are registered
with keys, the palette finds and flags conflicts, the numeric bar parses
units, tools run through the same Ops, undo works from the UI."""

import os

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtWidgets import QApplication  # noqa: E402
from PySide6.QtCore import QCoreApplication, QEvent  # noqa: E402

from robocad.ui.app import MainWindow  # noqa: E402
from robocad.ui.tools import NumericField, PrimitiveTool, SelectTool, TransformTool  # noqa: E402
from robocad.ui.widgets import CommandPalette, NumericBar  # noqa: E402


def test_desktop_autosave_during_edits_uses_gui_thread_and_keeps_dirty_state(qapp, tmp_path, monkeypatch):
    import threading
    import time
    from PySide6.QtCore import Qt
    from PySide6.QtTest import QTest
    from robocad.document import Document
    from robocad.commands import Ops
    monkeypatch.setattr(MainWindow, 'start_api', lambda self: None)
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (20, 20, 20))
    path = str(tmp_path/'live.rcad'); doc.save(path)
    window = MainWindow(doc); window.show()
    events = []; main_thread = threading.get_ident()
    doc.listeners.append(lambda event, payload: events.append((event, threading.get_ident())) if event in ('saved','autosaved') else None)
    try:
        window._start_autosave(.02)
        for i in range(3):
            ops.rename(body, 'Edited '+str(i))
            QTest.mouseClick(window.viewport, Qt.LeftButton, pos=window.viewport.rect().center())
            QTest.qWait(50)
            deadline = time.monotonic() + 3
            while window._autosave_last_revision != (id(doc), doc.revision) and time.monotonic() < deadline:
                qapp.processEvents()
                time.sleep(.005)
        assert len(events) >= 3 and all(event == 'autosaved' and thread == main_thread for event, thread in events)
        assert doc.path == path and doc.dirty and doc._autosave_thread is None
        assert 'live.rcad' in window.windowTitle() and '*' in window.windowTitle()
        recovered = Document.load(doc.autosave_path())
        assert recovered.nodes[body].name == 'Edited 2'
        assert Document.load(path).nodes[body].name == 'Box'
    finally:
        doc.dirty = False; window.close(); window.deleteLater()
        QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)


@pytest.fixture(scope="module")
def qapp():
    return QApplication.instance() or QApplication([])


@pytest.fixture
def win(qapp, monkeypatch):
    # These tests exercise widgets, not the HTTP server. Dispose each window
    # and its timers/listeners before Qt reuses any accessibility object IDs.
    monkeypatch.setattr(MainWindow, 'start_api', lambda self: None)
    w = MainWindow()
    yield w
    w.doc.dirty = False
    w.close()
    w.deleteLater()
    QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)


def test_commands_and_keymap(win):
    assert len(win.commands) > 120
    assert "Ctrl+Z" in win.commands["edit.undo"]["keys"]
    assert win.commands["tool.push_pull"]["keys"] == ["D"]


def test_autosave_worker_keeps_ui_live_and_snapshots_edits(win, qapp, monkeypatch, tmp_path):
    import threading
    import time
    from PySide6.QtTest import QTest
    from robocad import document
    body = win.ops.box((0, 0, 0), (10, 20, 30), name='Before')
    win.doc.save(str(tmp_path / 'robot.rcad'))
    win.ops.rename(body, 'Captured')
    started = threading.Event(); release = threading.Event(); writers = []
    original_write = document.write_archive
    def slow_write(path, entries):
        writers.append(threading.get_ident()); started.set()
        assert release.wait(5)
        original_write(path, entries)
    monkeypatch.setattr(document, 'write_archive', slow_write)
    def forbid(*args, **kwargs):
        pytest.fail('Metadata autosave must reuse unchanged B-reps')
    monkeypatch.setattr(win.doc.kernel, 'serialize', forbid)
    notices = []
    win.doc.listeners.append(lambda event, payload: notices.append(threading.get_ident()) if event == 'autosaved' else None)
    try:
        win._autosave()
        assert started.wait(1)
        win.ops.rename(body, 'Edited while saving')
        QTest.qWait(50)
        assert win.doc.nodes[body].name == 'Edited while saving'
        assert win._autosave_pending is not None and not notices
    finally:
        release.set()
    deadline = time.monotonic() + 5
    while win._autosave_pending and time.monotonic() < deadline:
        QTest.qWait(20)
    assert writers == [writers[0]] and writers[0] != threading.get_ident()
    assert notices == [threading.get_ident()]
    recovered = document.Document.load(win.doc.autosave_path())
    assert recovered.nodes[body].name == 'Captured'
    assert win.doc.dirty and win.doc.path == str(tmp_path / 'robot.rcad')
    win._autosave()
    while win._autosave_pending and time.monotonic() < deadline:
        QTest.qWait(20)
    assert document.Document.load(win.doc.autosave_path()).nodes[body].name == 'Edited while saving'


def test_outliner_fit_uses_clicked_row_and_nested_geometry(win, qapp):
    import numpy as np
    from PySide6.QtCore import Qt, QTimer
    from PySide6.QtWidgets import QMenu
    a = win.ops.box((0, 0, 0), (10, 10, 10))
    b = win.ops.box((100, 0, 0), (20, 20, 20))
    group = win.ops.group([b], 'Drive')
    outer = win.ops.group([group], 'Robot')
    win.outliner.refresh()
    tree = win.outliner.tree
    tree.topLevelItem(0).setSelected(True)
    target = tree.topLevelItem(1)
    def choose_fit():
        menu = qapp.activePopupWidget()
        assert isinstance(menu, QMenu)
        actions = menu.actions()
        next(a for a in actions if a.text() == 'Fit in view').trigger()
        menu.close()
    QTimer.singleShot(0, choose_fit)
    win.outliner._menu(tree.visualItemRect(target).center())
    assert win.viewport.selection.nodes() == [outer]
    assert np.allclose(win.viewport.camera.target, (110, 10, 10))
    # Fit must synchronize geometry created since the last rendered frame.
    win.ops.box((1000, 0, 0), (10, 10, 10))
    win.outliner._fit([a])
    assert np.allclose(win.viewport.camera.target, (5, 5, 5))


def test_outliner_preserves_collapse_and_searches_nested_parts(win):
    from PySide6.QtCore import Qt
    b = win.ops.box((0, 0, 0), (10, 10, 10), name='Left wheel')
    inner = win.ops.group([b], 'Drive')
    outer = win.ops.group([inner], 'Robot')
    win.outliner.refresh()
    tree = win.outliner.tree
    tree.topLevelItem(0).setExpanded(False)
    win.ops.rename(b, 'Front left wheel')
    win.outliner.refresh()
    assert not tree.topLevelItem(0).isExpanded()
    win.outliner.search.setText('front left')
    root = tree.topLevelItem(0)
    assert root.data(0, Qt.UserRole) == outer and root.isExpanded()
    assert root.child(0).child(0).data(0, Qt.UserRole) == b
    win.outliner.search.clear()
    assert not tree.topLevelItem(0).isExpanded()
    # Inline rename preserves the first word, with no icon text in the editor.
    tree.topLevelItem(0).child(0).child(0).setText(0, 'Rear left wheel')
    assert win.doc.nodes[b].name == 'Rear left wheel'


def test_outliner_move_menu_moves_selection_and_undoes(win):
    a = win.ops.box((0, 0, 0), (10, 10, 10))
    b = win.ops.box((20, 0, 0), (10, 10, 10))
    group = win.ops.group([], 'Drive')
    win.outliner.refresh()
    for i in (0, 1):
        win.outliner.tree.topLevelItem(i).setSelected(True)
    menu = win.outliner._context_menu()
    actions = menu.actions()
    move = next(a.menu() for a in actions if a.text() == 'Move to group')
    destinations = move.actions()
    next(a for a in destinations if a.text() == 'Drive').trigger()
    assert win.doc.nodes[group].children == [a, b]
    win.ops.undo()
    assert win.doc.nodes[a].parent is None and win.doc.nodes[b].parent is None


def test_joint_selection_is_bounded_and_cache_tracks_edits(win, monkeypatch):
    import time
    from robocad import physical
    from PySide6.QtWidgets import QLineEdit
    base = win.ops.box((0, 0, 0), (100, 80, 6))
    arm = win.ops.box((0, 0, 6), (10, 10, 80))
    joint = win.ops.add_joint('revolute', base, arm, (5, 5, 6))
    for i in range(40):
        part = win.ops.box((i * 3, 20, 6), (2, 2, 5))
        win.ops.connect_fixed(base, part)
    calls = []
    infer = physical.inspect_joint_physics
    def inspect(doc, nid):
        calls.append(nid)
        return infer(doc, nid)
    def forbid(*args, **kwargs):
        pytest.fail('Selection must not export collision data or validate the whole robot')
    monkeypatch.setattr(physical, 'inspect_joint_physics', inspect)
    monkeypatch.setattr(physical, 'collision_block', forbid)
    monkeypatch.setattr(win.robot_panel, 'refresh', forbid)
    win.viewport.selection.set_nodes([joint])
    start = time.perf_counter()
    win.selection_changed(None)
    assert time.perf_counter() - start < .5
    win.selection_changed(None, from_outliner=True)
    assert calls == [joint]
    assert float(win.properties.findChild(QLineEdit, 'flex_patch_radius').text()) > 0
    win.ops.set_joint_physics(joint, flex_patch_radius=.008)
    win.properties.refresh()
    assert calls == [joint, joint]
    assert float(win.properties.findChild(QLineEdit, 'flex_patch_radius').text()) == pytest.approx(8)
    win.ops.undo(); win.properties.refresh()
    assert len(calls) == 3
    assert float(win.properties.findChild(QLineEdit, 'flex_patch_radius').text()) != 8


def test_palette_search_and_conflicts(win):
    p = CommandPalette(win.commands, win)
    p.refresh("fillet")
    labels = [p.list.item(i).text() for i in range(p.list.count())]
    assert any("Fillet" in t for t in labels)
    win.commands["tool.box"]["keys"] = ["Ctrl+Z"]  # make a conflict
    p.refresh("box")
    assert any("conflicts with" in p.list.item(i).text() for i in range(p.list.count()))


def test_numeric_bar_units(qapp):
    bar = NumericBar()
    bar.set_fields([NumericField("width", 10.0), NumericField("angle", 30.0, angle=True)])
    bar.edits[0].setText("1in + 2mm")
    bar.edits[1].setText("(pi/2) rad")
    vals = bar.values()
    assert vals[0] == pytest.approx(27.4) and vals[1] == pytest.approx(90.0)
    bar.edits[0].setText("1/0")
    assert bar.values() is None


def test_primitive_tool_commit_and_undo(win):
    tool = PrimitiveTool(win.ctx, "box")
    win.set_tool(tool)
    tool.commit([20.0, 10.0, 5.0])
    assert len(win.doc.bodies()) == 1
    assert win.doc.kernel.mass_properties(win.doc.bodies()[0].body).volume == pytest.approx(1000)
    win.commands["edit.undo"]["run"]()
    assert len(win.doc.bodies()) == 0


def test_transform_tool_numeric(win):
    b = win.ops.box((0, 0, 0), (10, 10, 10))
    win.viewport.selection.items.append((b, "body", 0))
    tool = TransformTool(win.ctx, "move")
    win.set_tool(tool)
    tool.commit([5.0, 0.0, 0.0])
    assert win.doc.kernel.mass_properties(win.doc.nodes[b].body).centroid[0] == pytest.approx(10.0)


def test_live_dimensions_listed(win):
    b = win.ops.box((0, 0, 0), (10, 20, 30))
    faces = win.doc.kernel.faces(win.doc.nodes[b].body)
    pair = [f for f in faces if abs(f.normal[0]) > .99]
    win.viewport.selection.items = [(b, 'face', f.index) for f in pair]
    dims = win.live_dimensions()
    labels = [d[0] for d in dims]
    assert 'Distance' in labels
    setter = next(d[3] for d in dims if d[0] == 'Distance')
    setter(14.0)
    assert win.doc.kernel.mass_properties(win.doc.nodes[b].body).size[0] == pytest.approx(14.0)


def test_body_selection_never_runs_exact_geometry(win, monkeypatch):
    b = win.ops.box((0, 0, 0), (10, 20, 30))
    win.viewport.sync()
    def forbid(*args, **kwargs):
        pytest.fail('A body click must not run exact geometry queries')
    for method in ('mass_properties', 'faces', 'edges', 'tessellate', 'serialize'):
        monkeypatch.setattr(win.doc.kernel, method, forbid)
    win.viewport.selection.set_nodes([b])
    win.selection_changed(None, from_outliner=True)
    assert 'Display size ≈' in win.properties.facts.text()
    assert win.properties.measure.isEnabled()
    win.viewport.selection.set_nodes(['deleted-node'])
    win.properties.refresh()
    assert win.properties.facts.text() == 'Nothing selected.'


def test_exact_measurements_are_async_and_cancel_stale_results(win, qapp):
    import time
    from PySide6.QtTest import QTest
    from PySide6.QtCore import QTimer
    b = win.ops.box((0, 0, 0), (10, 20, 30))
    win.viewport.selection.set_nodes([b])
    win.properties.refresh()
    ticks = []
    timer = QTimer(); timer.timeout.connect(lambda: ticks.append(1)); timer.start(10)
    win.properties.measure.click()
    deadline = time.monotonic() + 15
    while win.properties._measurement_process is not None and time.monotonic() < deadline:
        QTest.qWait(20)
    timer.stop()
    assert len(ticks) > 2
    assert 'volume 6.000 cm³' in win.properties.facts.text()
    assert 'area 22.00 cm²' in win.properties.facts.text()
    win.properties.measure.click()
    win.viewport.selection.clear(); win.properties.refresh()
    QTest.qWait(100)
    assert win.properties._measurement_process is None
    assert win.properties.facts.text() == 'Nothing selected.'
    win.viewport.selection.set_nodes([b]); win.properties.refresh()
    win.properties.measure.click()
    win.ops.rename(b, 'Edited during calculation')
    win.properties.refresh()
    QTest.qWait(100)
    assert win.properties._measurement_result is None


def test_camera_math(win):
    cam = win.viewport.camera
    cam.set_view("front")
    o, d = cam.ray(400, 300, 800, 600)
    assert d[1] == pytest.approx(1.0, abs=1e-6)  # looking along +Y from the front
    p = cam.project(cam.target, 800, 600)
    assert p[0] == pytest.approx(400, abs=1) and p[1] == pytest.approx(300, abs=1)
    cam.opposite()
    _, d2 = cam.ray(400, 300, 800, 600)
    assert d2[1] == pytest.approx(-1.0, abs=1e-6)


def test_annotation_ui_create_reply_resolve(win, qapp):
    from robocad.ui.comments import AnnotateTool
    from PySide6.QtCore import Qt
    b = win.ops.box((0,0,0),(10,10,10))
    win.set_tool(AnnotateTool(win.ctx))
    assert win.viewport.cursor().shape() == Qt.CrossCursor
    assert win.viewport.selection_mode == 'face'
    win.comments.begin(b,(10,5,5))
    assert isinstance(win.tool, SelectTool)
    assert win.viewport.selection_mode == 'body'
    win.comments.editor.setPlainText('Check the hole clearance')
    win.comments.submit()
    tid = win.comments.current_id()
    assert win.doc.annotations[tid]['comments'][0]['body'] == 'Check the hole clearance'
    win.comments.editor.setPlainText('Allow 0.2 mm')
    win.comments.submit()
    assert len(win.doc.annotations[tid]['comments']) == 2
    win.comments.resolve()
    qapp.processEvents()
    assert win.doc.annotations[tid]['status'] == 'resolved'
    win.ops.undo()
    assert win.doc.annotations[tid]['status'] == 'open'


def test_editor_focus_disables_model_shortcuts(win):
    win._focus_changed(win.viewport, win.comments.editor)
    assert not win.commands['tool.annotate']['action'].isEnabled()
    assert not win.commands['edit.delete']['action'].isEnabled()
    win._focus_changed(win.comments.editor, win.viewport)
    assert win.commands['tool.annotate']['action'].isEnabled()
    assert win.commands['edit.delete']['action'].isEnabled()


def test_hover_cannot_replace_click_and_mode_change_cancels_pick(win):
    vp = win.viewport
    click, hover = lambda r: None, lambda r: None
    vp.request_pick(5,6,click)
    vp.request_hover(7,8,hover)
    assert vp._pick_request[2] is click
    assert vp._hover_request[2] is hover
    win.set_tool(SelectTool(win.ctx))
    assert vp._pick_request is None and vp._hover_request is None


def test_empty_geometry_does_not_recurse(win):
    from robocad.document import Node
    n = Node(win.doc.new_id(),'body','Empty body')
    win.doc.add(n)
    win.viewport.sync()
    assert n.id not in win.viewport.items
    assert not win.viewport.dirty_nodes


def test_annotation_draft_stays_on_its_thread(win):
    b = win.ops.box((0, 0, 0), (10, 10, 10))
    a = win.ops.create_thread(b, (10, 5, 5), "First")
    other = win.ops.create_thread(b, (5, 10, 5), "Second")
    win.comments.select(a)
    win.comments.editor.setPlainText("Reply to first")
    win.comments.select(other)
    win.comments.begin(b, (5, 5, 10))
    assert win.comments.current_id() == a
    assert win.comments.pending is None
    assert not win.comments.threads.isEnabled()
    win.comments.submit()
    assert win.doc.annotations[a]["comments"][-1]["body"] == "Reply to first"
    assert len(win.doc.annotations[other]["comments"]) == 1
    assert win.comments.threads.isEnabled()


def test_annotation_recalls_trackball_view(win):
    import numpy as np
    b = win.ops.box((0, 0, 0), (10, 10, 10))
    camera = win.viewport.camera
    camera.sync_trackball()
    camera.mode = "trackball"
    camera.orbit(20, 10)
    original = camera.view().copy()
    win.comments.begin(b, (10, 5, 5))
    win.comments.editor.setPlainText("This view")
    win.comments.submit()
    camera.set_view("front")
    win.comments.focus_thread()
    assert camera.mode == "trackball"
    assert np.allclose(camera.view(), original)


def test_pose_preview_restores_cached_geometry_and_leaves_document_untouched(win):
    import numpy as np
    b = win.ops.box((10,0,0),(10,2,2))
    jid = win.ops.add_joint('revolute',None,b,(0,0,0),lower=-2,upper=2)
    win.viewport.sync()
    original = win.viewport.items[b]
    body = win.doc.nodes[b].body
    history = len(win.ops.stack.undo_stack)
    win.doc.dirty = False
    win.pose_panel.enter()
    win.pose_panel.value.setValue(90)
    assert win.pose_panel.positions[jid] == pytest.approx(np.pi/2)
    assert not np.allclose(win.viewport.items[b].vertices,original.vertices)
    assert win.doc.nodes[b].body is body
    assert not win.doc.dirty
    assert len(win.ops.stack.undo_stack)==history
    win.pose_panel.stop()
    assert win.viewport.items[b] is original
    assert win.properties.isEnabled()
    win.ops.set_joint(jid,lower=0,upper=.123)
    win.pose_panel.enter()
    win.pose_panel.slider.setValue(1000)
    assert win.pose_panel.positions[jid] <= .123  # rounded degree display cannot exceed radian limits
    win.pose_panel.stop()


def test_reference_panel_aligns_and_starts_sketch(win,tmp_path):
    from PIL import Image
    import numpy as np
    from robocad.ui.tools import SketchTool
    p = tmp_path/'reference.png'
    Image.new('RGB',(200,100),'white').save(p)
    nid = win.references.add_paths([p])[0]
    win.references.plane.setCurrentIndex(1)
    win.references.width.setValue(250)
    win.references.commit()
    win.references.sketch()
    assert isinstance(win.tool,SketchTool)
    assert win.viewport.camera.orthographic
    assert np.allclose(win.viewport.camera.direction(),(0,-1,0))
    assert win.doc.nodes[nid].image['width']==250


def test_capture_restores_camera_and_section_even_on_failure(win, monkeypatch):
    from robocad.api import Service
    from robocad.kernel import Plane
    service = Service(win.doc, app=win)
    vp = win.viewport
    original = vp.camera
    vp.section_enabled = True
    vp.section_plane = Plane.xy()
    old_plane = vp.section_plane
    old_grid = vp.show_grid
    def screenshot():
        assert vp.camera is not original
        assert vp.camera.yaw == 42
        assert vp.section_plane.normal == (1, 0, 0)
        assert not vp.show_grid
        return b'png'
    monkeypatch.setattr(service, 'screenshot', screenshot)
    request = {'view': {'yaw': 42, 'grid': False, 'section': {'plane': 'yz'}}}
    assert service.capture(request) == b'png'
    assert vp.camera is original and vp.section_plane is old_plane
    assert vp.section_enabled and vp.show_grid == old_grid
    def fail():
        raise RuntimeError('capture failed')
    monkeypatch.setattr(service, 'screenshot', fail)
    with pytest.raises(RuntimeError, match='capture failed'):
        service.capture(request)
    assert vp.camera is original and vp.section_plane is old_plane
    assert vp.section_enabled and vp.show_grid == old_grid


@pytest.mark.parametrize('view', [{'distance': 0}, {'target': [1, 2]}, {'yaw': float('nan')}, {'build_plate': [5, 5]}, {'fov': 180}])
def test_capture_invalid_view_does_not_mutate(win, view):
    from robocad.api import Service, ApiError
    service = Service(win.doc, app=win)
    before = service.view()
    with pytest.raises(ApiError):
        service.capture({'view': view})
    assert service.view() == before


def test_interactive_section_does_not_call_kernel(win, monkeypatch):
    from robocad import analysis
    from robocad.kernel import Plane
    from robocad.ui import viewport
    def forbidden(*args):
        raise AssertionError('exact section invoked by redraw')
    monkeypatch.setattr(analysis, 'section_outline', forbidden)
    for name in ('glDisable', 'glColor3f', 'glLineWidth', 'glEnableClientState', 'glDisableClientState', 'glEnable', 'glBlendFunc', 'glColor4f', 'glBegin', 'glVertex3f', 'glEnd'):
        monkeypatch.setattr(viewport.GL, name, lambda *args: None)
    win.viewport.section_plane = Plane.xy()
    win.viewport._draw_section_outline()


def test_organizing_and_coloring_reuses_display_mesh(win, monkeypatch):
    from robocad.ui import viewport
    node = win.ops.box((0,0,0),(10,10,10))
    win.viewport.sync()
    mesh = win.doc.mesh_of(node)
    vertices = win.viewport.items[node].vertices
    def forbidden(*args):
        raise AssertionError('display mesh rebuilt for metadata edit')
    monkeypatch.setattr(viewport, '_display_edges', forbidden)
    win.ops.rename(node, 'Renamed')
    win.ops.set_color([node], (.2,.3,.4))
    group = win.ops.group([node], 'Assembly')
    win.viewport.sync()
    assert win.doc.mesh_of(node) is mesh
    assert win.viewport.items[node].vertices is vertices
    assert win.viewport.items[node].color == (.2,.3,.4)
    win.ops.undo(); win.ops.undo(); win.ops.undo()
    win.viewport.sync()
    assert win.viewport.items[node].vertices is vertices
    assert win.doc.nodes[node].name == 'Box'


def test_saved_views_panel_saves_restores_replaces_deletes_and_undoes(win, monkeypatch):
    from robocad.kernel import Plane
    from robocad.saved_views import capture_view
    vp = win.viewport; panel = win.saved_views_panel
    vp.camera.yaw = 42; vp.camera.orthographic = True
    vp.section_enabled = True; vp.section_plane = Plane.yz()
    vp.show_grid = False
    before = capture_view(vp)
    panel.name.setText('Worm cutaway'); panel.save_current()
    vid = panel.current_id()
    assert win.doc.saved_views[vid]['name'] == 'Worm cutaway'
    assert panel.views.count() == 1 and 'Cutaway' in panel.views.item(0).text()
    vp.camera.yaw = -30; vp.section_enabled = False; vp.show_grid = True
    revision = win.doc.revision
    panel.restore()
    assert capture_view(vp) == before and win.doc.revision == revision
    vp.camera.yaw = 12; panel.replace_current()
    assert win.doc.saved_views[vid]['state']['yaw'] == 12
    win.ops.undo(); assert win.doc.saved_views[vid]['state']['yaw'] == 42
    panel.delete(); assert panel.views.count() == 0
    win.ops.undo(); assert panel.views.count() == 1
    assert 'view.saved_views' in win.commands


def test_saved_view_restore_trackball_and_disable_section_without_rebuild(win, monkeypatch):
    from robocad.saved_views import restore_view, capture_view
    from robocad.kernel import Plane
    import numpy as np
    vp = win.viewport
    vp.camera.mode = 'trackball'
    vp.camera.rot = np.array([[0,-1,0],[1,0,0],[0,0,1]],dtype=float)
    before = capture_view(vp)
    vp.section_enabled = True; vp.section_plane = Plane.yz()
    vp.camera.mode = 'turntable'
    dirty = set(vp.dirty_nodes)
    def forbidden(*args): raise AssertionError('saved view triggered geometry work')
    monkeypatch.setattr(vp, 'sync', forbidden)
    restore_view(win,before)
    assert capture_view(vp) == before
    assert vp.dirty_nodes == dirty


def test_saved_view_gui_api_captures_live_state_and_restores(win):
    from robocad.api import Service
    from robocad.saved_views import capture_view
    from robocad.kernel import Plane
    service = Service(win.doc, app=win)
    vp = win.viewport
    vp.section_enabled = True; vp.section_plane = Plane.yz()
    before = capture_view(vp)
    row = service.saved_view_request('POST', ['views'], {'name':'From desktop'})
    assert row['state'] == before
    vp.section_enabled = False; vp.camera.distance *= 2
    revision = win.doc.revision
    restored = service.saved_view_request('POST', ['views', row['id'], 'restore'], {})
    assert restored['restored'] == row['id']
    assert capture_view(vp) == before and win.doc.revision == revision
    assert win.saved_views_panel.current_id() == row['id']


def test_comment_link_isolates_part_and_returns_without_mutating_document(win):
    from robocad.saved_views import capture_view
    from robocad.kernel import Plane
    from PySide6.QtCore import Qt
    a=win.ops.box((0,0,0),(10,10,10));b=win.ops.box((20,0,0),(3,4,5))
    tid=win.ops.create_thread(a,[0,0,0],f'Compare [small block](part:{b}) here.',part_refs=[{'node_id':b,'label':'Small block'}])
    win.comments.select(tid);vp=win.viewport
    assert '(part:' not in win.comments.threads.currentItem().text()
    vp.sync();vp.section_enabled=True;vp.section_plane=Plane.yz()
    vp.selection.set_nodes([a]);old_selection=list(vp.selection.items)
    before=capture_view(vp); revision=win.doc.revision; visibility={i:n.visible for i,n in win.doc.nodes.items()}
    label=win.comments.messages.itemWidget(win.comments.messages.item(0))
    assert f'href="part:{b}"' in label.text()
    label.linkActivated.emit('part:'+b)
    assert vp.inspection_ids=={b} and vp.is_visible(b) and not vp.is_visible(a)
    assert not vp.section_enabled and vp.selection.nodes()==[b]
    assert win.doc.revision==revision and {i:n.visible for i,n in win.doc.nodes.items()}==visibility
    assert win.comments.return_button.isEnabled()
    win.comments.end_inspection()
    assert vp.inspection_ids is None and capture_view(vp)==before
    assert vp.selection.items==old_selection and not win.comments.return_button.isEnabled()


def test_part_links_escape_html_and_never_render_external_links(win):
    from robocad.ui.comments import comment_html
    a=win.ops.box((0,0,0),(1,1,1))
    out=comment_html(f'<img src=x> [<b>block</b>](part:{a}) [site](https://evil.example)',win.doc)
    assert '<img' not in out and '&lt;img' in out and '&lt;b&gt;' in out
    assert 'href="https:' not in out
    assert 'part deleted' in comment_html('[old](part:missing)',win.doc)


def test_insert_part_link_and_show_api(win):
    from robocad.api import Service
    a=win.ops.box((0,0,0),(2,2,2));b=win.ops.box((5,0,0),(2,2,2))
    tid=win.ops.create_thread(a,[0,0,0],'Discuss')
    win.comments.select(tid);win.viewport.selection.set_nodes([b])
    win.comments.insert_part_link()
    assert f'](part:{b})' in win.comments.editor.toPlainText()
    win.comments.submit()
    assert b in [r['node_id'] for r in win.ops.thread(tid)['linked_parts']]
    service=Service(win.doc,app=win)
    service.annotation_request('POST',['threads',tid,'show'],{}, {'mode':'parts','node_id':b})
    assert win.viewport.inspection_ids=={b}
    service.annotation_request('POST',['threads',tid,'show'],{}, {'mode':'back'})
    assert win.viewport.inspection_ids is None

def test_selection_sync_preserves_existing_outliner_items(win):
 from PySide6.QtCore import Qt
 a=win.ops.box((0,0,0),(2,2,2));b=win.ops.box((3,0,0),(2,2,2));win.outliner.refresh()
 item=win.outliner.tree.topLevelItem(0)
 win.viewport.selection.set_nodes([a]);win.outliner.sync_selection()
 assert win.outliner.tree.topLevelItem(0) is item
 assert [it.data(0,Qt.UserRole) for it in win.outliner.tree.selectedItems()]==[a]
 win.viewport.selection.set_nodes([b]);win.outliner.sync_selection()
 assert win.outliner.tree.topLevelItem(0) is item
 assert [it.data(0,Qt.UserRole) for it in win.outliner.tree.selectedItems()]==[b]

def test_large_assembly_joint_selection_defers_geometry(win,monkeypatch):
 from types import SimpleNamespace
 from robocad import physical
 a=win.ops.box((0,0,0),(2,2,2));b=win.ops.box((3,0,0),(2,2,2))
 j=win.ops.add_joint('revolute',a,b,(2,0,0))
 win.viewport.items={'large':SimpleNamespace(indices=SimpleNamespace(size=150001))}
 def forbid(*a,**kw):pytest.fail('Large-assembly selection must not infer geometry')
 monkeypatch.setattr(physical,'inspect_joint_physics',forbid)
 result=win.properties._joint_physics(win.doc.nodes[j])
 assert 'deferred' in result['source']
